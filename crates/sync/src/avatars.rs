//! Profile pictures (spec 0012): whether and when to ask Azure DevOps for a
//! person's avatar, and the on-disk cache. The app decodes and paints.
//!
//! Endpoint, verified live with an org-scoped PAT (2026-09-30):
//! `GET {org}/_api/_common/identityImage?id={identity-guid}` → `image/png`
//! (44 × 44, the default size) with `ETag: 1-<guid>` for someone who
//! uploaded a picture, and `304` to `If-None-Match`. Azure DevOps' defaults
//! come back as `200` too: a generated group glyph (PNG, no `ETag`) or a
//! generic silhouette (`image/svg+xml`) for unknown or service identities.
//! The `_links.avatar.href` / build `imageUrl` form
//! (`_apis/GraphProfile/MemberAvatars/{descriptor}`) answers `401` to a PAT
//! and is never called (FR-10).
//!
//! Rules:
//! - Groups and teams (container identities) are never fetched.
//! - Without the `vso.identity` scope nothing is fetched (FR-9).
//! - A cached picture younger than [`REVALIDATE_MS`] costs zero requests;
//!   older ones are revalidated with a conditional GET (FR-6).
//! - Defaults ([`classify`]), 4xx and bad bodies are negative-cached for
//!   [`NEGATIVE_MS`]; 5xx are retried after [`RETRY_MS`], at most
//!   [`MAX_ATTEMPTS`] times a day (FR-7).
//! - A 401/403 from the avatar route stops avatars for this fetcher (the
//!   token's lifetime) without touching sync's "token rejected" latch.
//! - Offline, throttled, token rejected, rate pressure or the avatar budget:
//!   nothing is recorded, a stale picture keeps showing, the caller asks
//!   again later.
//! - Requests go through the shared [`Governor`] (tier `Cold`) and the
//!   ledgered transport, at most [`MAX_IN_FLIGHT`] at a time, only while the
//!   pressure is `Normal`, and within [`MAX_COST_PER_WINDOW`] TSTU per
//!   [`WINDOW_MS`] (FR-4, FR-5).
//!
//! Cache: `<org dir>/avatars/<id>.png` (bytes as served) and `<id>.json`
//! (state, validators, last check), directory `0700`, files `0600`. It sits
//! beside the org's store, so deleting the org's data removes it (FR-15).
//! The metadata is not in the store: store commits wake every view to
//! reload, and avatar bookkeeping must not.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use adoken_model::Millis;
use adoken_model::time::{HOUR, MINUTE};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, watch};
use url::Url;

use crate::Tier;
use crate::clock::Clock;
use crate::engine::{Phase, SyncStatus};
use crate::governor::{GovError, Governor, Pressure};
use crate::transport::{ApiRequest, ApiResponse, Transport, Validators};
use crate::wire::is_guid;

/// A cached picture is revalidated at most once per this long.
pub const REVALIDATE_MS: Millis = 24 * HOUR;
/// Defaults and permanent failures are not re-asked for this long.
pub const NEGATIVE_MS: Millis = 24 * HOUR;
/// Wait after the 1st and 2nd transient failure; the 3rd waits a day.
pub const RETRY_MS: [Millis; 2] = [5 * MINUTE, 30 * MINUTE];
pub const MAX_ATTEMPTS: u32 = 3;
/// Avatar requests in flight (of the governor's 3 background slots per
/// host; the 4th is the user's): sync always keeps at least one.
pub const MAX_IN_FLIGHT: usize = 2;
/// Avatar TSTU per sliding [`WINDOW_MS`] (of the governor's 50).
pub const MAX_COST_PER_WINDOW: f64 = 1.0;
pub const WINDOW_MS: Millis = 5 * MINUTE;
/// Larger bodies are refused (a 44 px avatar is ~5 KiB).
pub const MAX_BYTES: usize = 1 << 20;
/// Booked when a response carries no `X-RateLimit-Cost`: 60 requests per
/// window fill the budget (measured cost: 0.001–0.02 TSTU).
const COST_HINT: f64 = MAX_COST_PER_WINDOW / 60.0;
/// The scope that grants avatars (onboarding: "Show teammates' names and
/// avatars").
pub const SCOPE: &str = "vso.identity";
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";
const ROUTE: &str = "_api/_common/identityImage";

/// The identity-image URL on the org's own host for identity `id`, or
/// `None` when there is nothing to fetch (groups, malformed ids). Captured
/// `imageUrl`s are never requested (FR-10): the token only goes to the org.
pub fn avatar_url(org_url: &Url, id: &str, is_group: bool) -> Option<Url> {
    if is_group || !is_guid(id) {
        return None;
    }
    let mut url = org_url.join(ROUTE).ok()?;
    url.query_pairs_mut().append_pair("id", id);
    Some(url)
}

/// What a `200` from the identity-image route is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A picture the person uploaded.
    Picture,
    /// Azure DevOps' generated default: keep initials.
    Placeholder,
    /// Not an image we paint (wrong type, empty, too large).
    Invalid,
}

/// Placeholder detection from what the live endpoint returns: the generic
/// silhouette is SVG; generated defaults (group glyphs) carry no `ETag`
/// because there is no stored image behind them. Only PNG is painted (the
/// endpoint serves PNG).
pub fn classify(resp: &ApiResponse) -> Kind {
    let content_type = resp
        .content_type
        .as_deref()
        .map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    if content_type == "image/svg+xml" {
        return Kind::Placeholder;
    }
    if resp.body.is_empty() || resp.body.len() > MAX_BYTES || !resp.body.starts_with(PNG_MAGIC) {
        return Kind::Invalid;
    }
    if resp.validators.etag.is_none() {
        return Kind::Placeholder;
    }
    Kind::Picture
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Picture,
    Placeholder,
    Failed,
}

/// `<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub state: State,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    pub checked_ms: Millis,
    /// Transient failures in a row ([`State::Failed`] only).
    #[serde(default)]
    pub attempts: u32,
}

impl Entry {
    /// When this entry may be asked about again.
    fn due_ms(&self) -> Millis {
        let wait = match self.state {
            State::Picture => REVALIDATE_MS,
            State::Placeholder => NEGATIVE_MS,
            State::Failed => match self.attempts {
                1 => RETRY_MS[0],
                2 => RETRY_MS[1],
                _ => NEGATIVE_MS,
            },
        };
        self.checked_ms + wait
    }
}

/// The on-disk cache. Blocking: call from a blocking pool / background
/// executor.
#[derive(Clone, Debug)]
pub struct AvatarCache {
    dir: PathBuf,
}

impl AvatarCache {
    /// Cache for the org whose store lives in `org_dir`.
    pub fn new(org_dir: &Path) -> Self {
        AvatarCache {
            dir: org_dir.join("avatars"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `<dir>/<id>.png`; `None` unless `id` is a GUID (no path tricks).
    pub fn picture_path(&self, id: &str) -> Option<PathBuf> {
        is_guid(id).then(|| self.dir.join(format!("{}.png", id.to_ascii_lowercase())))
    }

    fn entry_path(&self, id: &str) -> Option<PathBuf> {
        is_guid(id).then(|| self.dir.join(format!("{}.json", id.to_ascii_lowercase())))
    }

    pub fn entry(&self, id: &str) -> Option<Entry> {
        let bytes = std::fs::read(self.entry_path(id)?).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// A picture that is on disk right now (any age).
    pub fn cached_picture(&self, id: &str) -> Option<PathBuf> {
        let entry = self.entry(id)?;
        let path = self.picture_path(id)?;
        (entry.state == State::Picture && path.is_file()).then_some(path)
    }

    fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        restrict(&self.dir, 0o700)
    }

    fn put_entry(&self, id: &str, entry: &Entry) {
        let Some(path) = self.entry_path(id) else {
            return;
        };
        let saved = self
            .ensure_dir()
            .and_then(|()| write_atomic(&path, &serde_json::to_vec(entry).expect("entry json")));
        if let Err(e) = saved {
            log::warn!(target: "adoken_sync::avatars", "avatar cache: {e}");
        }
    }

    fn put_picture(&self, id: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let path = self
            .picture_path(id)
            .ok_or_else(|| std::io::Error::other("not an identity id"))?;
        self.ensure_dir()?;
        write_atomic(&path, bytes)?;
        Ok(path)
    }

    fn drop_picture(&self, id: &str) {
        if let Some(path) = self.picture_path(id) {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes)?;
    restrict(&partial, 0o600)?;
    std::fs::rename(&partial, path)
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// The outcome for one person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// Paint this file (PNG).
    Picture(PathBuf),
    /// Initials, settled until [`REVALIDATE_MS`]-scale timers pass: group,
    /// no scope, a default, a failure, or avatars refused for this token.
    Initials,
    /// Initials for now: offline, throttled, rate pressure or over budget.
    /// Ask again later.
    Later,
}

/// What [`AvatarFetcher::resolve`] will do, decided from the cache alone.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Plan {
    Done(Resolved),
    Fetch {
        url: Url,
        /// The cached picture to serve if the request can't go out.
        stale: Option<PathBuf>,
        validators: Validators,
        attempts: u32,
    },
}

/// Fetches avatars lazily through the shared governor. `Send + Sync`; share
/// it behind an `Arc` and call [`Self::resolve`] from Tokio tasks.
pub struct AvatarFetcher {
    cache: AvatarCache,
    transport: Arc<dyn Transport>,
    gov: Arc<Governor>,
    clock: Arc<dyn Clock>,
    org_url: Url,
    /// The token carries [`SCOPE`].
    enabled: bool,
    /// The avatar route answered 401/403: no more avatar requests with this
    /// token (FR-9).
    refused: AtomicBool,
    /// Sync's status: no requests while offline or with the token rejected.
    sync: Option<watch::Receiver<SyncStatus>>,
    slots: Semaphore,
    /// (sent at, TSTU) of avatar requests in the last [`WINDOW_MS`].
    spent: Mutex<VecDeque<(Millis, f64)>>,
}

impl AvatarFetcher {
    /// `transport` should be an [`crate::transport::OptionalAdoTransport`]
    /// (wrapped in the sync ledger) so a refusal can't reject the token.
    pub fn new(
        cache: AvatarCache,
        transport: Arc<dyn Transport>,
        gov: Arc<Governor>,
        clock: Arc<dyn Clock>,
        org_url: Url,
        granted_scopes: &[String],
    ) -> Self {
        let mut org_url = org_url;
        if !org_url.path().ends_with('/') {
            org_url.set_path(&format!("{}/", org_url.path()));
        }
        AvatarFetcher {
            cache,
            transport,
            gov,
            clock,
            org_url,
            enabled: granted_scopes.iter().any(|s| s == SCOPE),
            refused: AtomicBool::new(false),
            sync: None,
            slots: Semaphore::new(MAX_IN_FLIGHT),
            spent: Mutex::new(VecDeque::new()),
        }
    }

    /// Follow sync's status ([`crate::Engine::status`]): offline or token
    /// rejected means no avatar requests.
    pub fn with_sync_status(mut self, status: watch::Receiver<SyncStatus>) -> Self {
        self.sync = Some(status);
        self
    }

    pub fn cache(&self) -> &AvatarCache {
        &self.cache
    }

    /// The token may fetch avatars at all (scope granted, not refused).
    pub fn is_enabled(&self) -> bool {
        self.enabled && !self.refused.load(Ordering::Acquire)
    }

    fn plan(&self, id: &str, is_group: bool, now: Millis) -> Plan {
        if !self.is_enabled() {
            return Plan::Done(Resolved::Initials);
        }
        let Some(url) = avatar_url(&self.org_url, id, is_group) else {
            return Plan::Done(Resolved::Initials);
        };
        let entry = self.cache.entry(id);
        let stale = self.cache.cached_picture(id);
        match (entry, stale) {
            (Some(e), stale) if now < e.due_ms() => Plan::Done(match stale {
                Some(path) => Resolved::Picture(path),
                None => Resolved::Initials,
            }),
            (Some(e), Some(stale)) => Plan::Fetch {
                url,
                stale: Some(stale),
                validators: Validators {
                    etag: e.etag,
                    last_modified: e.last_modified,
                },
                attempts: 0,
            },
            (e, _) => Plan::Fetch {
                url,
                stale: None,
                validators: Validators::default(),
                // A day after the third failure the count starts over.
                attempts: e
                    .filter(|e| e.state == State::Failed && e.attempts < MAX_ATTEMPTS)
                    .map_or(0, |e| e.attempts),
            },
        }
    }

    /// Requests may go out now: online, token accepted, no throttle window,
    /// no rate pressure.
    fn may_send(&self, now: Millis) -> bool {
        let sync_ok = self
            .sync
            .as_ref()
            .is_none_or(|s| !matches!(s.borrow().phase, Phase::Offline | Phase::Unauthorized));
        sync_ok
            && !self.gov.is_unauthorized()
            && self.gov.pressure() == Pressure::Normal
            && self.gov.snapshot().paused_until.is_none_or(|t| t <= now)
    }

    /// Room left in the avatar budget at `now`.
    fn within_budget(&self, now: Millis) -> bool {
        let mut spent = self.spent.lock().unwrap_or_else(|e| e.into_inner());
        while spent.front().is_some_and(|(at, _)| *at <= now - WINDOW_MS) {
            spent.pop_front();
        }
        spent.iter().map(|(_, c)| c).sum::<f64>() < MAX_COST_PER_WINDOW
    }

    fn book(&self, at: Millis, cost: f64) {
        self.spent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back((at, cost));
    }

    /// Avatar TSTU spent in the last [`WINDOW_MS`].
    pub fn window_cost(&self) -> f64 {
        let since = self.clock.now_ms() - WINDOW_MS;
        self.spent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|(at, _)| *at > since)
            .map(|(_, c)| c)
            .sum()
    }

    /// Decide from the cache, fetch or revalidate if due, update the cache.
    /// Waits only for its own request slot.
    pub async fn resolve(self: &Arc<Self>, id: String, is_group: bool) -> Resolved {
        let now = self.clock.now_ms();
        let planned = {
            let this = self.clone();
            let id = id.clone();
            tokio::task::spawn_blocking(move || this.plan(&id, is_group, now)).await
        };
        let (url, stale, validators, attempts) = match planned {
            Ok(Plan::Done(done)) => return done,
            Ok(Plan::Fetch {
                url,
                stale,
                validators,
                attempts,
            }) => (url, stale, validators, attempts),
            Err(_) => return Resolved::Later,
        };
        let later = || stale.clone().map_or(Resolved::Later, Resolved::Picture);
        let Ok(_slot) = self.slots.acquire().await else {
            return later();
        };
        let sent_at = self.clock.now_ms();
        if !self.may_send(sent_at) || !self.within_budget(sent_at) {
            return later();
        }
        let req = ApiRequest::get(url)
            .with_validators(validators)
            .with_cost_hint(COST_HINT)
            .with_origin(crate::attribution::Origin::Avatars);
        let result = self
            .gov
            .send_optional(self.transport.as_ref(), Tier::Cold, req)
            .await;
        let cost = match &result {
            Ok(resp) => resp.rate.cost.unwrap_or(COST_HINT),
            // The request reached the server.
            Err(GovError::Status { .. }) => COST_HINT,
            Err(_) => 0.0,
        };
        self.book(sent_at, cost);
        let done = self.clock.now_ms();
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.settle(&id, result, stale, attempts, done))
            .await
            .unwrap_or(Resolved::Later)
    }

    /// Blocking: record the response in the cache.
    fn settle(
        &self,
        id: &str,
        result: Result<ApiResponse, GovError>,
        stale: Option<PathBuf>,
        attempts: u32,
        now: Millis,
    ) -> Resolved {
        let cache = &self.cache;
        let negative = |state: State, attempts: u32| {
            cache.drop_picture(id);
            cache.put_entry(
                id,
                &Entry {
                    state,
                    etag: None,
                    last_modified: None,
                    checked_ms: now,
                    attempts,
                },
            );
            Resolved::Initials
        };
        // Keep what we have and look again after the usual day.
        let keep = |path: PathBuf| {
            if let Some(mut entry) = cache.entry(id) {
                entry.checked_ms = now;
                cache.put_entry(id, &entry);
            }
            Resolved::Picture(path)
        };
        match result {
            Ok(resp) if resp.status == 304 => match stale {
                Some(path) => keep(path),
                // 304 without a copy to keep: ask plainly next time.
                None => Resolved::Later,
            },
            Ok(resp) => match classify(&resp) {
                Kind::Picture => match cache.put_picture(id, &resp.body) {
                    Ok(path) => {
                        cache.put_entry(
                            id,
                            &Entry {
                                state: State::Picture,
                                etag: resp.validators.etag,
                                last_modified: resp.validators.last_modified,
                                checked_ms: now,
                                attempts: 0,
                            },
                        );
                        Resolved::Picture(path)
                    }
                    Err(e) => {
                        log::warn!(target: "adoken_sync::avatars", "avatar cache: {e}");
                        stale.map_or(Resolved::Later, Resolved::Picture)
                    }
                },
                Kind::Placeholder => negative(State::Placeholder, 0),
                Kind::Invalid => negative(State::Failed, MAX_ATTEMPTS),
            },
            // The avatar route refuses this token (FR-9).
            Err(GovError::Status {
                status: 401 | 403 | 203 | 300..=399,
                ..
            }) => {
                if !self.refused.swap(true, Ordering::AcqRel) {
                    log::info!(
                        target: "adoken_sync::avatars",
                        "avatars unavailable: the avatar endpoint refused the token"
                    );
                }
                Resolved::Initials
            }
            Err(GovError::Status { status, .. }) => {
                let transient = status >= 500;
                match stale {
                    Some(path) => keep(path),
                    None if transient => negative(State::Failed, attempts + 1),
                    None => negative(State::Failed, MAX_ATTEMPTS),
                }
            }
            // Throttled after retries: a transient failure.
            Err(GovError::Throttled { .. }) if stale.is_none() => {
                negative(State::Failed, attempts + 1)
            }
            Err(_) => stale.map_or(Resolved::Later, Resolved::Picture),
        }
    }
}

#[cfg(test)]
mod tests;
