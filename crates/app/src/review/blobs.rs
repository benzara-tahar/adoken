//! Where a review's file contents come from (spec 0004 FR-1..FR-3): the
//! store's content-addressed cache, backed for a signed-in org by
//! [`PrFiles`] (the sync governor at user priority), or an in-memory map
//! for gallery stories. Reads block (background executor); fetches run on
//! Tokio and never on the UI thread.

use std::collections::HashMap;
use std::sync::Arc;

use adoken_store::Store;
use adoken_sync::Tier;
use adoken_sync::pr_files::{PrFiles, RepoRef};
use gpui::{App, Global, Task};
use gpui_tokio::Tokio;
use tokio::sync::watch;

/// Reads a blob's text if it is available locally.
type Reader = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// A review's blob source. Cheap to clone.
#[derive(Clone)]
pub struct Blobs {
    read: Reader,
    remote: Option<Remote>,
}

#[derive(Clone)]
struct Remote {
    files: Arc<PrFiles>,
    repo: RepoRef,
}

impl Blobs {
    /// Gallery stories: every blob in memory, nothing to fetch.
    pub fn memory(blobs: HashMap<String, String>) -> Self {
        let blobs = Arc::new(blobs);
        Blobs {
            read: Arc::new(move |id| blobs.get(id).cloned()),
            remote: None,
        }
    }

    /// The store's blob cache; misses are fetched through `files` (a
    /// signed-in org) or stay missing (the demo org has every blob).
    pub fn store(store: &Store, remote: Option<(Arc<PrFiles>, RepoRef)>) -> Self {
        let cache = store.blobs().clone();
        Blobs {
            read: Arc::new(move |id| cache.text(id).ok().flatten()),
            remote: remote.map(|(files, repo)| Remote { files, repo }),
        }
    }

    /// Blocking.
    pub fn read(&self, id: &str) -> Option<String> {
        (self.read)(id)
    }

    /// Misses can be fetched.
    pub fn is_remote(&self) -> bool {
        self.remote.is_some()
    }

    /// The file-list / blob fetcher, for a signed-in org.
    pub fn files(&self) -> Option<Arc<PrFiles>> {
        self.remote.as_ref().map(|r| r.files.clone())
    }

    /// Why a fetch sent now would wait (the diff's loading copy); `None`
    /// without a remote.
    pub fn hold(&self) -> Option<adoken_sync::Hold> {
        self.remote.as_ref().and_then(|r| r.files.hold())
    }

    /// Of `ids`, those a fetch would request (blocking: stats the cache).
    pub fn missing(&self, ids: &[String]) -> Vec<String> {
        match &self.remote {
            Some(remote) => {
                let cache = remote.files.blobs();
                ids.iter()
                    .filter(|id| !cache.contains(id))
                    .cloned()
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// Fetch `ids` into the cache at user priority (FR-3: the fetcher keeps
    /// at most four in flight). Resolves off the UI thread.
    pub fn fetch(&self, ids: Vec<String>, cx: &App) -> Task<Result<(), String>> {
        let Some(remote) = self.remote.clone() else {
            return Task::ready(Ok(()));
        };
        let job = Tokio::spawn(cx, async move {
            remote
                .files
                .ensure_blobs(Tier::User, &remote.repo, &ids)
                .await
                .map_err(|e| e.to_string())
        });
        cx.background_executor()
            .spawn(async move { job.await.unwrap_or_else(|e| Err(e.to_string())) })
    }
}

/// The signed-in org's review file source, set by the shell once sync has
/// built its engine; absent for the demo org.
pub struct ReviewFiles(pub watch::Receiver<Option<Arc<PrFiles>>>);

impl Global for ReviewFiles {}

pub fn set_files(files: watch::Receiver<Option<Arc<PrFiles>>>, cx: &mut App) {
    cx.set_global(ReviewFiles(files));
}

/// The fetcher, once sync has one.
pub fn files(cx: &App) -> Option<Arc<PrFiles>> {
    cx.try_global::<ReviewFiles>()
        .and_then(|f| f.0.borrow().clone())
}

/// Sync `pr`'s file list at user priority (spec 0004 FR-1). Resolves off
/// the UI thread with whether the stored list changed.
pub fn sync_files(
    files: Arc<PrFiles>,
    pr: adoken_model::PullRequest,
    cx: &App,
) -> Task<Result<bool, String>> {
    let job = Tokio::spawn(cx, async move {
        files
            .sync_files(Tier::User, &pr)
            .await
            .map(|s| s.changed)
            .map_err(|e| e.to_string())
    });
    cx.background_executor()
        .spawn(async move { job.await.unwrap_or_else(|e| Err(e.to_string())) })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use adoken_model::{ChangeType, MergeStatus, PrDetail, PrFileChange, PrStatus, PullRequest};
    use adoken_store::Store;
    use adoken_sync::pr_files::{PrFiles, RepoRef, git_blob_id};
    use adoken_sync::transport::{ApiResponse, RateHeaders, Transport, TransportError};
    use adoken_sync::{ApiRequest, Governor, ManualClock, Tier};
    use futures::future::BoxFuture;
    use parking_lot::Mutex;

    use super::Blobs;
    use crate::review::data::{self, Collapsed, LoadedPr};

    /// Serves blob bytes by id; records every requested id.
    struct Mock {
        blobs: HashMap<String, Vec<u8>>,
        asked: Mutex<Vec<String>>,
    }

    impl Transport for Mock {
        fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
            let id = req
                .url()
                .path()
                .rsplit('/')
                .next()
                .unwrap_or_default()
                .to_owned();
            self.asked.lock().push(id.clone());
            let body = self.blobs.get(&id).cloned();
            Box::pin(async move {
                Ok(ApiResponse {
                    status: if body.is_some() { 200 } else { 404 },
                    rate: RateHeaders::default(),
                    body: body.unwrap_or_default(),
                    content_type: Some("application/octet-stream".into()),
                    validators: Default::default(),
                })
            })
        }
    }

    fn pr() -> PullRequest {
        PullRequest {
            id: 7,
            repo_id: "r".into(),
            project_id: "p".into(),
            title: "Retry".into(),
            description: String::new(),
            author: "ana".into(),
            reviewers: Vec::new(),
            status: PrStatus::Active,
            is_draft: false,
            merge_status: MergeStatus::Succeeded,
            source_branch: "refs/heads/retry".into(),
            target_branch: "refs/heads/main".into(),
            last_source_commit: None,
            created: 0,
            updated: 0,
            closed: None,
            iterations: 1,
            last_push: None,
        }
    }

    /// What opening the review loads for `selected` ([`data::load_plan`] +
    /// the view's fetch-then-diff), without a window.
    async fn open(loaded: &mut LoadedPr, blobs: &Blobs, files: &Arc<PrFiles>, selected: usize) {
        for file in data::load_plan(loaded, selected) {
            expand(loaded, blobs, files, file, false).await;
        }
    }

    async fn expand(
        loaded: &mut LoadedPr,
        blobs: &Blobs,
        files: &Arc<PrFiles>,
        file: usize,
        force: bool,
    ) {
        let job = loaded.job(file);
        let missing = blobs.missing(&job.blob_ids());
        files
            .ensure_blobs(Tier::User, &RepoRef::of(loaded.pr()), &missing)
            .await
            .unwrap();
        let outcome = job.run(&|id| blobs.read(id), force);
        data::apply_diff(loaded, file, outcome);
    }

    fn build(store: &Store, files: &[PrFileChange]) -> LoadedPr {
        let blob = |id: &str| store.blob(id).ok().flatten();
        data::build(
            data::Source {
                detail: PrDetail {
                    pr: pr(),
                    iterations: Vec::new(),
                    files: files.to_vec(),
                    threads: Vec::new(),
                },
                people: Vec::new(),
                me: "me".into(),
                now: 0,
                viewed: HashSet::new(),
                read_only: true,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            1,
        )
    }

    /// AC-12: generated and binary files cost zero blob requests until
    /// "Load anyway", then exactly their two; AC-4: a re-open over the
    /// same store directory costs zero.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn collapsed_files_cost_nothing_until_expanded_and_reopen_costs_nothing() {
        let texts = [
            ("a_old", "fn a() {}\n"),
            ("a_new", "fn a() -> u8 { 1 }\n"),
            ("lock_old", "# lock v1\n"),
            ("lock_new", "# lock v2\n"),
            ("png_old", "\u{89}PNG\0old"),
            ("png_new", "\u{89}PNG\0new"),
        ];
        let ids: HashMap<&str, String> = texts
            .iter()
            .map(|(k, t)| (*k, git_blob_id(t.as_bytes())))
            .collect();
        let change = |path: &str, old: &str, new: &str| PrFileChange {
            path: path.into(),
            original_path: None,
            change_type: ChangeType::Edit,
            old_content: Some(ids[old].clone()),
            new_content: Some(ids[new].clone()),
            change_tracking_id: None,
        };
        let changes = [
            change("/Cargo.lock", "lock_old", "lock_new"),
            change("/assets/logo.png", "png_old", "png_new"),
            change("/src/a.rs", "a_old", "a_new"),
        ];
        let mock = Arc::new(Mock {
            blobs: texts
                .iter()
                .map(|(k, t)| (ids[k].clone(), t.as_bytes().to_vec()))
                .collect(),
            asked: Mutex::new(Vec::new()),
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("adoken.sqlite");
        let fetcher = |store: &Store| {
            let clock = Arc::new(ManualClock::new(1_000));
            Arc::new(PrFiles::new(
                store.clone(),
                mock.clone(),
                Arc::new(Governor::new(clock.clone())),
                clock,
                url::Url::parse("https://dev.azure.com/contoso/").unwrap(),
            ))
        };
        let asked = || mock.asked.lock().clone();

        let store = Store::open(&path).unwrap();
        let files = fetcher(&store);
        let blobs = Blobs::store(&store, Some((files.clone(), RepoRef::of(&pr()))));
        let mut loaded = build(&store, &changes);
        let at = |p: &str| loaded.files.iter().position(|f| f.path == p).unwrap();
        let (lock, png, a) = (at("Cargo.lock"), at("assets/logo.png"), at("src/a.rs"));
        assert_eq!(loaded.files[lock].collapsed, Some(Collapsed::Generated));
        assert_eq!(loaded.files[png].collapsed, Some(Collapsed::Binary));
        assert!(loaded.files[a].diff.is_none(), "nothing cached yet");

        // Open on each file in turn: only src/a.rs's two blobs are asked for.
        for file in [lock, png, a] {
            open(&mut loaded, &blobs, &files, file).await;
        }
        assert_eq!(
            asked().len(),
            2,
            "only the visible text file's blobs are fetched"
        );
        assert!(loaded.files[a].diff.is_some());

        // "Load anyway" on the lock file: its two blobs, once.
        expand(&mut loaded, &blobs, &files, lock, true).await;
        assert_eq!(asked().len(), 4);
        assert!(loaded.files[lock].diff.is_some());

        // Re-open over the same directory: every diff from the cache.
        drop(store);
        let store = Store::open(&path).unwrap();
        let files = fetcher(&store);
        let blobs = Blobs::store(&store, Some((files.clone(), RepoRef::of(&pr()))));
        let mut again = build(&store, &changes);
        for file in [a, lock] {
            open(&mut again, &blobs, &files, file).await;
        }
        expand(&mut again, &blobs, &files, lock, true).await;
        assert!(again.files[a].diff.is_some());
        assert!(again.files[lock].diff.is_some());
        assert_eq!(asked().len(), 4, "AC-4: zero requests on re-open");
    }
}
