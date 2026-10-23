//! Review files of real PRs (spec 0004 FR-1..FR-3): which files a PR
//! changes, and their contents for local diffs.
//!
//! Endpoints, verified live with an org-scoped PAT (2026-09-30):
//! - `…/pullRequests/{id}/iterations` → one entry per push, each with
//!   `sourceRefCommit` and `commonRefCommit` (the merge base).
//! - `…/iterations/{it}/changes?$compareTo=0&$top=…&$skip=…` → the latest
//!   iteration against the merge base: `changeEntries[]` with `item.objectId`
//!   (source blob), `item.originalObjectId` (merge-base blob, edits only),
//!   `changeTrackingId`, and `nextSkip` while more pages follow. Object ids
//!   come upper-case; they are stored lower-case. No file sizes.
//! - `…/blobs/{objectId}?$format=octetstream` → raw bytes
//!   (`application/octet-stream`; `$format=text` answers the same; without
//!   `$format` the route answers 400). The bytes hash to the object id
//!   (`sha1("blob {len}\0" + bytes)`), which is checked before caching.
//!
//! Costs seen in `X-RateLimit-Cost`: changes 0.007–0.048 TSTU; iterations
//! and blobs carried no cost header (booked with the hints below).
//!
//! Rules:
//! - A PR's file list is refetched only when its `last_source_commit` moved
//!   past the commit the stored list describes ([`files_due`]): opening a PR
//!   whose list is current costs zero requests.
//! - A blob already in the store's [`BlobCache`] is never requested again
//!   (AC-4); concurrent asks for one id share a single request.
//! - At most [`MAX_BLOBS_IN_FLIGHT`] blob requests at a time (FR-3), all
//!   through the shared governor at the caller's tier.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use adoken_model::{PrFileChange, PullRequest};
use adoken_store::read::{self, FileSync};
use adoken_store::{BlobCache, Store, write};
use tokio::sync::{OnceCell, Semaphore};
use url::Url;

use crate::Tier;
use crate::attribution::{self, Origin, UserAction};
use crate::clock::Clock;
use crate::engine::{API_VERSION, SyncError};
use crate::governor::Governor;
use crate::transport::{ApiRequest, Transport};
use crate::wire::{self, IterationChanges, WireIteration};

/// FR-3: blob requests in flight at once.
pub const MAX_BLOBS_IN_FLIGHT: usize = 4;
/// Changes per page: the API's maximum (spec 0004 §5; the default is 100).
pub const CHANGES_PAGE: u32 = 2_000;
/// A 100k-file PR is 50 pages; stop there rather than loop on a bad `nextSkip`.
const MAX_CHANGE_PAGES: usize = 50;
/// Booked when a response carries no `X-RateLimit-Cost` (none seen live on
/// iterations or blobs; changes measured up to 0.048).
pub const ITERATIONS_COST: f64 = 0.005;
pub const CHANGES_COST: f64 = 0.05;
pub const BLOB_COST: f64 = 0.002;

/// Where a PR's repository lives (URL parts of every call here).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoRef {
    pub project_id: String,
    pub repo_id: String,
}

impl RepoRef {
    pub fn of(pr: &PullRequest) -> Self {
        RepoRef {
            project_id: pr.project_id.clone(),
            repo_id: pr.repo_id.clone(),
        }
    }
}

/// The stored file list is missing or describes an older push.
pub fn files_due(pr: &PullRequest, synced: Option<&FileSync>) -> bool {
    match (synced, pr.last_source_commit.as_deref()) {
        (None, _) => true,
        (Some(s), Some(head)) => !s.source_commit.eq_ignore_ascii_case(head),
        (Some(_), None) => false,
    }
}

/// What [`PrFiles::sync_files`] stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilesSynced {
    pub iteration: u32,
    pub files: usize,
    /// The file list differs from what the store had.
    pub changed: bool,
}

type BlobResult = Result<(), Arc<SyncError>>;

/// Fetches file lists and blobs through the shared governor. `Send + Sync`;
/// share it behind an `Arc` and call it from Tokio tasks.
pub struct PrFiles {
    store: Store,
    transport: Arc<dyn Transport>,
    gov: Arc<Governor>,
    clock: Arc<dyn Clock>,
    org_url: Url,
    slots: Semaphore,
    /// Blob requests in progress, by lower-case id (single flight).
    inflight: Mutex<HashMap<String, Arc<OnceCell<BlobResult>>>>,
}

impl PrFiles {
    pub fn new(
        store: Store,
        transport: Arc<dyn Transport>,
        gov: Arc<Governor>,
        clock: Arc<dyn Clock>,
        org_url: Url,
    ) -> Self {
        let mut org_url = org_url;
        if !org_url.path().ends_with('/') {
            org_url.set_path(&format!("{}/", org_url.path()));
        }
        PrFiles {
            store,
            transport,
            gov,
            clock,
            org_url,
            slots: Semaphore::new(MAX_BLOBS_IN_FLIGHT),
            inflight: Mutex::new(HashMap::new()),
        }
    }

    pub fn blobs(&self) -> &BlobCache {
        self.store.blobs()
    }

    /// Why a request at user priority would wait right now (the review's
    /// loading copy).
    pub fn hold(&self) -> Option<crate::governor::Hold> {
        self.gov.hold()
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> Url {
        let mut url = self.org_url.join(path).expect("relative ADO path");
        {
            let mut q = url.query_pairs_mut();
            for (k, v) in query {
                q.append_pair(k, v);
            }
            q.append_pair("api-version", API_VERSION);
        }
        url
    }

    async fn db<R: Send + 'static>(
        &self,
        f: impl FnOnce(&Store) -> adoken_store::Result<R> + Send + 'static,
    ) -> Result<R, SyncError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| SyncError::Decode {
                what: "store task".into(),
                message: e.to_string(),
            })?
            .map_err(SyncError::from)
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        tier: Tier,
        req: ApiRequest,
    ) -> Result<T, SyncError> {
        let what = req.url().path().to_owned();
        let resp = self.gov.send(self.transport.as_ref(), tier, req).await?;
        serde_json::from_slice(&resp.body).map_err(|e| SyncError::Decode {
            what,
            message: e.to_string(),
        })
    }

    /// The stored list is missing or older than the PR's head (blocking read).
    pub fn is_due(&self, pr: &PullRequest) -> adoken_store::Result<bool> {
        let synced = self.store.with_conn(|c| read::pr_file_sync(c, pr.id))?;
        Ok(files_due(pr, synced.as_ref()))
    }

    /// FR-1: the latest iteration's changes against the merge base, every
    /// page, stored as `pr_file` rows with the iterations and the
    /// [`FileSync`] marker, in one transaction.
    pub async fn sync_files(&self, tier: Tier, pr: &PullRequest) -> Result<FilesSynced, SyncError> {
        attribution::or_origin(
            Origin::You(UserAction::OpenPr),
            self.sync_files_inner(tier, pr),
        )
        .await
    }

    async fn sync_files_inner(
        &self,
        tier: Tier,
        pr: &PullRequest,
    ) -> Result<FilesSynced, SyncError> {
        let base = format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/iterations",
            pr.project_id, pr.repo_id, pr.id
        );
        let iterations: wire::List<WireIteration> = self
            .fetch_json(
                tier,
                // Spec 0017 FR-4: the same request carries each push's
                // commits, so Today counts commits at no extra request.
                ApiRequest::get(self.url(&base, &[("includeCommits", "true")]))
                    .with_cost_hint(ITERATIONS_COST),
            )
            .await?;
        let Some(latest) = iterations.value.iter().max_by_key(|it| it.id).cloned() else {
            return Err(SyncError::Missing("pull request iterations"));
        };
        let mut entries = Vec::new();
        let mut skip = 0u32;
        for _ in 0..MAX_CHANGE_PAGES {
            let top = CHANGES_PAGE.to_string();
            let skip_s = skip.to_string();
            let page: IterationChanges = self
                .fetch_json(
                    tier,
                    ApiRequest::get(self.url(
                        &format!("{base}/{}/changes", latest.id),
                        &[("$top", &top), ("$skip", &skip_s), ("$compareTo", "0")],
                    ))
                    .with_cost_hint(CHANGES_COST),
                )
                .await?;
            let n = page.change_entries.len() as u32;
            entries.extend(page.change_entries);
            // `nextSkip` while more pages follow; a server that ignores
            // `$top` and pages by its default of 100 is followed the same way.
            if n == 0 || page.next_skip <= skip {
                break;
            }
            skip = page.next_skip;
        }
        let files: Vec<PrFileChange> = entries
            .into_iter()
            .filter_map(|e| {
                let mut f = e.into_model()?;
                lower(&mut f.old_content);
                lower(&mut f.new_content);
                Some(f)
            })
            .collect();
        let synced = FileSync {
            iteration: latest.id,
            source_commit: latest
                .source_ref_commit
                .as_ref()
                .map(|c| c.commit_id.to_ascii_lowercase())
                .or_else(|| pr.last_source_commit.clone())
                .unwrap_or_default(),
            base_commit: latest
                .common_ref_commit
                .as_ref()
                .map(|c| c.commit_id.to_ascii_lowercase()),
            synced_ms: self.clock.now_ms(),
        };
        let iterations = wire::iterations_into_model(iterations.value);
        let pr_id = pr.id;
        let count = files.len();
        let iteration = synced.iteration;
        let changed = self
            .db(move |s| {
                s.write(|tx| {
                    let before = read::pr_files(tx, pr_id)?;
                    let changed = before != files;
                    write::put_pr_iterations(tx, pr_id, &iterations)?;
                    if changed {
                        write::put_pr_files(tx, pr_id, &files)?;
                    }
                    write::put_pr_file_sync(tx, pr_id, &synced)?;
                    Ok(changed)
                })
            })
            .await?;
        Ok(FilesSynced {
            iteration,
            files: count,
            changed,
        })
    }

    /// FR-2: make every blob in `ids` available in the cache, fetching the
    /// missing ones (at most [`MAX_BLOBS_IN_FLIGHT`] at a time, one request
    /// per id however many callers ask). The first failure is returned; the
    /// blobs that did arrive stay cached.
    pub async fn ensure_blobs(
        self: &Arc<Self>,
        tier: Tier,
        repo: &RepoRef,
        ids: &[String],
    ) -> BlobResult {
        // Requests a caller didn't attribute are the review opening a PR
        // (spec 0016 FR-18).
        let jobs = ids.iter().map(|id| self.ensure_blob(tier, repo, id));
        attribution::or_origin(Origin::You(UserAction::OpenPr), async {
            futures::future::join_all(jobs).await.into_iter().collect()
        })
        .await
    }

    async fn ensure_blob(self: &Arc<Self>, tier: Tier, repo: &RepoRef, id: &str) -> BlobResult {
        let id = id.to_ascii_lowercase();
        if !adoken_store::is_object_id(&id) {
            return Err(Arc::new(SyncError::Decode {
                what: "blob id".into(),
                message: format!("not an object id: {id}"),
            }));
        }
        let cell = {
            let mut inflight = self.inflight.lock().unwrap_or_else(|e| e.into_inner());
            inflight.entry(id.clone()).or_default().clone()
        };
        let result = cell
            .get_or_init(|| self.fetch_blob(tier, repo, &id))
            .await
            .clone();
        self.inflight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        result
    }

    async fn fetch_blob(&self, tier: Tier, repo: &RepoRef, id: &str) -> BlobResult {
        let cache = self.store.blobs().clone();
        let probe = id.to_owned();
        if spawn_blocking(move || Ok(cache.contains(&probe))).await? {
            return Ok(());
        }
        let _slot = self.slots.acquire().await.expect("semaphore open");
        let url = self.url(
            &format!(
                "{}/_apis/git/repositories/{}/blobs/{id}",
                repo.project_id, repo.repo_id
            ),
            &[("$format", "octetstream")],
        );
        let resp = self
            .gov
            .send(
                self.transport.as_ref(),
                tier,
                ApiRequest::get(url).with_cost_hint(BLOB_COST),
            )
            .await
            .map_err(|e| Arc::new(SyncError::from(e)))?;
        if id.len() == 40 && git_blob_id(&resp.body) != id {
            return Err(Arc::new(SyncError::Decode {
                what: format!("blob {id}"),
                message: "content doesn't match its object id".into(),
            }));
        }
        let cache = self.store.blobs().clone();
        let id = id.to_owned();
        spawn_blocking(move || cache.put(&id, &resp.body)).await
    }
}

async fn spawn_blocking<R: Send + 'static>(
    f: impl FnOnce() -> std::io::Result<R> + Send + 'static,
) -> Result<R, Arc<SyncError>> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()))
        .map_err(|message| {
            Arc::new(SyncError::Decode {
                what: "blob cache".into(),
                message,
            })
        })
}

fn lower(id: &mut Option<String>) {
    if let Some(id) = id {
        id.make_ascii_lowercase();
    }
}

/// Git's object id of a blob with these bytes.
pub fn git_blob_id(bytes: &[u8]) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.digest().to_string()
}
