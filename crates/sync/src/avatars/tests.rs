use std::collections::VecDeque;

use futures::future::BoxFuture;
use parking_lot::Mutex;

use super::*;
use crate::clock::ManualClock;
use crate::transport::{RateHeaders, TransportError};

const ORG: &str = "https://dev.azure.com/contoso/";
const DANA: &str = "6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d";
const SAM: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
const KIM: &str = "1f2e3d4c-5b6a-4978-8a9b-0c1d2e3f4a5b";
const T0: Millis = 1_790_000_000_000;
const ETAG: &str = "1-00000000-0000-0000-e004-ec63f61edf08";

/// Answers requests from a script; panics on an unexpected request.
#[derive(Default)]
struct Script {
    replies: Mutex<VecDeque<Result<ApiResponse, TransportError>>>,
    seen: Mutex<Vec<ApiRequest>>,
}

impl Script {
    fn reply(&self, r: Result<ApiResponse, TransportError>) {
        self.replies.lock().push_back(r);
    }
    fn requests(&self) -> usize {
        self.seen.lock().len()
    }
    fn last(&self) -> ApiRequest {
        self.seen.lock().last().cloned().expect("a request")
    }
}

impl Transport for Script {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            self.seen.lock().push(req.clone());
            self.replies
                .lock()
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected request {}", req.url()))
        })
    }
}

fn response(status: u16, content_type: &str, body: &[u8], etag: Option<&str>) -> ApiResponse {
    ApiResponse {
        status,
        rate: RateHeaders::default(),
        body: body.to_vec(),
        content_type: Some(content_type.to_owned()),
        validators: Validators {
            etag: etag.map(str::to_owned),
            last_modified: None,
        },
    }
}

fn png_bytes() -> Vec<u8> {
    let mut b = PNG_MAGIC.to_vec();
    b.extend_from_slice(b"rest-of-a-png");
    b
}

/// A user's uploaded picture, as the live endpoint serves it.
fn picture() -> ApiResponse {
    response(200, "image/png", &png_bytes(), Some(ETAG))
}

/// Unknown and service identities: the generic silhouette.
fn silhouette() -> ApiResponse {
    response(200, "image/svg+xml", b"<?xml version=\"1.0\"?><svg/>", None)
}

fn status(code: u16) -> ApiResponse {
    response(code, "text/html", b"<html>", None)
}

struct Rig {
    _dir: tempfile::TempDir,
    script: Arc<Script>,
    clock: Arc<ManualClock>,
    gov: Arc<Governor>,
    fetcher: Arc<AvatarFetcher>,
    sync: Option<watch::Sender<SyncStatus>>,
}

impl Rig {
    async fn get(&self, id: &str) -> Resolved {
        self.fetcher.resolve(id.to_owned(), false).await
    }
    async fn picture(&self, id: &str) -> PathBuf {
        match self.get(id).await {
            Resolved::Picture(path) => path,
            other => panic!("expected a picture, got {other:?}"),
        }
    }
}

fn build(scopes: &[&str], follow_sync: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let script = Arc::new(Script::default());
    let clock = Arc::new(ManualClock::new(T0));
    let gov = Arc::new(Governor::new(clock.clone()));
    let scopes: Vec<String> = scopes.iter().map(|s| (*s).to_owned()).collect();
    let mut fetcher = AvatarFetcher::new(
        AvatarCache::new(dir.path()),
        script.clone(),
        gov.clone(),
        clock.clone(),
        Url::parse(ORG).unwrap(),
        &scopes,
    );
    let mut sync = None;
    if follow_sync {
        let (tx, rx) = watch::channel(SyncStatus {
            phase: Phase::Idle,
            last_ok_hot: None,
            last_ok_warm: None,
            last_ok_cold: None,
        });
        fetcher = fetcher.with_sync_status(rx);
        sync = Some(tx);
    }
    Rig {
        _dir: dir,
        script,
        clock,
        gov,
        fetcher: Arc::new(fetcher),
        sync,
    }
}

fn rig() -> Rig {
    build(&["vso.code_write", SCOPE, "vso.project"], true)
}

// ── URL ────────────────────────────────────────────────────────────────

#[test]
fn builds_the_identity_route_on_the_org_host() {
    let org = Url::parse(ORG).unwrap();
    assert_eq!(
        avatar_url(&org, DANA, false).unwrap().as_str(),
        format!("https://dev.azure.com/contoso/_api/_common/identityImage?id={DANA}")
    );
    let legacy = Url::parse("https://contoso.visualstudio.com/").unwrap();
    assert_eq!(
        avatar_url(&legacy, DANA, false).unwrap().as_str(),
        format!("https://contoso.visualstudio.com/_api/_common/identityImage?id={DANA}")
    );
}

#[test]
fn groups_and_malformed_ids_have_no_avatar_url() {
    let org = Url::parse(ORG).unwrap();
    assert_eq!(avatar_url(&org, DANA, true), None);
    assert_eq!(avatar_url(&org, "../../etc/passwd", false), None);
    assert_eq!(avatar_url(&org, "dana@example.com", false), None);
}

// ── placeholder detection ──────────────────────────────────────────────

#[test]
fn classifies_what_the_live_endpoint_returns() {
    assert_eq!(classify(&picture()), Kind::Picture);
    assert_eq!(classify(&silhouette()), Kind::Placeholder);
    // Generated defaults (group glyph): a PNG with no stored image, no ETag.
    let generated = response(200, "image/png", &png_bytes(), None);
    assert_eq!(classify(&generated), Kind::Placeholder);
    // Not something we paint.
    let html = response(200, "text/html", b"<html>", Some("x"));
    assert_eq!(classify(&html), Kind::Invalid);
    assert_eq!(
        classify(&response(200, "image/png", b"", Some("x"))),
        Kind::Invalid
    );
    let mut huge = png_bytes();
    huge.resize(MAX_BYTES + 1, 0);
    assert_eq!(
        classify(&response(200, "image/png", &huge, Some("x"))),
        Kind::Invalid
    );
}

// ── fetch and cache ────────────────────────────────────────────────────

#[tokio::test]
async fn fetches_once_then_serves_the_cache_with_zero_requests() {
    let r = rig();
    r.script.reply(Ok(picture()));
    let path = r.picture(DANA).await;
    assert_eq!(std::fs::read(&path).unwrap(), png_bytes());
    assert_eq!(r.script.requests(), 1);
    assert_eq!(
        r.script.last().url().as_str(),
        format!("{ORG}_api/_common/identityImage?id={DANA}")
    );
    assert_eq!(r.script.last().validators(), &Validators::default());

    r.clock.advance(REVALIDATE_MS - 1);
    assert_eq!(r.get(DANA).await, Resolved::Picture(path));
    assert_eq!(r.script.requests(), 1, "a fresh cache hit costs nothing");
}

#[cfg(unix)]
#[tokio::test]
async fn cache_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let r = rig();
    r.script.reply(Ok(picture()));
    let path = r.picture(DANA).await;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(r.fetcher.cache().dir()), 0o700);
}

#[tokio::test]
async fn revalidates_after_a_day_with_the_etag() {
    let r = rig();
    r.script.reply(Ok(picture()));
    let first = r.get(DANA).await;
    r.clock.advance(REVALIDATE_MS);

    r.script.reply(Ok(response(304, "image/png", b"", None)));
    assert_eq!(r.get(DANA).await, first);
    assert_eq!(r.script.requests(), 2);
    assert_eq!(r.script.last().validators().etag.as_deref(), Some(ETAG));
    // The 304 restarts the day.
    r.clock.advance(REVALIDATE_MS - 1);
    assert_eq!(r.get(DANA).await, first);
    assert_eq!(r.script.requests(), 2);
}

#[tokio::test]
async fn a_changed_picture_replaces_the_cached_one() {
    let r = rig();
    r.script.reply(Ok(picture()));
    r.get(DANA).await;
    r.clock.advance(REVALIDATE_MS);
    let mut new = png_bytes();
    new.extend_from_slice(b"v2");
    r.script
        .reply(Ok(response(200, "image/png", &new, Some("2-new"))));
    let path = r.picture(DANA).await;
    assert_eq!(std::fs::read(path).unwrap(), new);
    assert_eq!(
        r.fetcher.cache().entry(DANA).unwrap().etag.as_deref(),
        Some("2-new")
    );
}

#[tokio::test]
async fn placeholders_are_negative_cached_for_a_day() {
    let r = rig();
    r.script.reply(Ok(silhouette()));
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    r.clock.advance(NEGATIVE_MS - 1);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.script.requests(), 1, "no second ask within a day");

    // A day later they may have uploaded one.
    r.clock.advance(1);
    r.script.reply(Ok(picture()));
    r.picture(DANA).await;
    assert_eq!(r.script.requests(), 2);
}

#[tokio::test]
async fn a_picture_removed_upstream_becomes_initials() {
    let r = rig();
    r.script.reply(Ok(picture()));
    let path = r.picture(DANA).await;
    r.clock.advance(REVALIDATE_MS);
    r.script
        .reply(Ok(response(200, "image/png", &png_bytes(), None)));
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert!(!path.exists());
}

#[tokio::test]
async fn not_found_and_bad_bodies_are_negative_cached() {
    let r = rig();
    r.script.reply(Ok(status(404)));
    r.script
        .reply(Ok(response(200, "text/html", b"<html>", Some("x"))));
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.get(SAM).await, Resolved::Initials);
    r.clock.advance(NEGATIVE_MS - 1);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.get(SAM).await, Resolved::Initials);
    assert_eq!(r.script.requests(), 2);
}

#[tokio::test]
async fn server_errors_are_retried_at_most_three_times_a_day() {
    let r = rig();
    for _ in 0..MAX_ATTEMPTS {
        r.script.reply(Ok(status(500)));
    }
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.get(DANA).await, Resolved::Initials, "backs off");
    r.clock.advance(RETRY_MS[0]);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    r.clock.advance(RETRY_MS[1]);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.script.requests(), 3);
    // The third failure waits a day.
    r.clock.advance(NEGATIVE_MS - 1);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert_eq!(r.script.requests(), 3);
    r.clock.advance(1);
    r.script.reply(Ok(picture()));
    r.picture(DANA).await;
}

#[tokio::test]
async fn offline_is_not_remembered() {
    let r = rig();
    r.script
        .reply(Err(TransportError::Offline("no route".into())));
    assert_eq!(r.get(DANA).await, Resolved::Later);
    assert_eq!(r.fetcher.cache().entry(DANA), None);
    r.script.reply(Ok(picture()));
    r.picture(DANA).await;
}

#[tokio::test]
async fn failed_revalidation_keeps_the_stale_picture() {
    let r = rig();
    r.script.reply(Ok(picture()));
    let first = r.get(DANA).await;
    r.clock.advance(REVALIDATE_MS);
    r.script
        .reply(Err(TransportError::Offline("no route".into())));
    assert_eq!(r.get(DANA).await, first);
    r.script.reply(Ok(status(500)));
    assert_eq!(r.get(DANA).await, first);
}

// ── never asked ────────────────────────────────────────────────────────

#[tokio::test]
async fn without_the_identity_scope_nothing_is_requested() {
    let r = build(&["vso.code_write", "vso.project", "vso.work"], false);
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert!(!r.fetcher.is_enabled());
    assert_eq!(r.script.requests(), 0);
}

#[tokio::test]
async fn groups_are_never_requested() {
    let r = rig();
    assert_eq!(
        r.fetcher.resolve(DANA.to_owned(), true).await,
        Resolved::Initials
    );
    assert_eq!(r.script.requests(), 0);
}

#[tokio::test]
async fn a_refusing_avatar_route_stops_avatars_but_not_sync() {
    let r = rig();
    r.script.reply(Ok(status(401)));
    assert_eq!(r.get(DANA).await, Resolved::Initials);
    assert!(!r.gov.is_unauthorized(), "sync keeps its token");
    assert!(!r.fetcher.is_enabled());
    assert_eq!(r.get(SAM).await, Resolved::Initials);
    assert_eq!(r.script.requests(), 1);
}

#[tokio::test]
async fn offline_or_rejected_sync_means_no_requests() {
    let r = rig();
    let tx = r.sync.as_ref().unwrap();
    tx.send_modify(|s| s.phase = Phase::Offline);
    assert_eq!(r.get(DANA).await, Resolved::Later);
    tx.send_modify(|s| s.phase = Phase::Unauthorized);
    assert_eq!(r.get(DANA).await, Resolved::Later);
    assert_eq!(r.script.requests(), 0);
    tx.send_modify(|s| s.phase = Phase::Idle);
    r.script.reply(Ok(picture()));
    r.picture(DANA).await;
}

#[tokio::test]
async fn rate_pressure_pauses_avatars() {
    let r = rig();
    // Sync saw 40% of the limit left: warm/cold slow down, avatars stop.
    let mut pressured = picture();
    pressured.rate = RateHeaders {
        limit: Some(200.0),
        remaining: Some(80.0),
        reset_s: Some((T0 + HOUR) / 1000),
        ..RateHeaders::default()
    };
    r.script.reply(Ok(pressured));
    r.picture(DANA).await;
    assert_eq!(r.gov.pressure(), Pressure::Slow);
    assert_eq!(r.get(SAM).await, Resolved::Later);
    assert_eq!(r.script.requests(), 1);
}

#[tokio::test]
async fn the_avatar_budget_caps_tstu_per_window() {
    let r = rig();
    let costly = || {
        let mut resp = picture();
        resp.rate.cost = Some(0.6);
        resp
    };
    r.script.reply(Ok(costly()));
    r.script.reply(Ok(costly()));
    r.picture(DANA).await;
    r.picture(SAM).await;
    assert!((r.fetcher.window_cost() - 1.2).abs() < 1e-9);
    assert_eq!(r.get(KIM).await, Resolved::Later);
    assert_eq!(r.script.requests(), 2);
    // The window slides.
    r.clock.advance(WINDOW_MS);
    r.script.reply(Ok(picture()));
    r.picture(KIM).await;
}

#[tokio::test]
async fn without_cost_headers_sixty_requests_fill_the_window() {
    let r = rig();
    let ids: Vec<String> = (0..61)
        .map(|n| format!("00000000-0000-4000-8000-{n:012}"))
        .collect();
    for _ in 0..60 {
        r.script.reply(Ok(silhouette()));
    }
    for id in &ids[..60] {
        assert_eq!(r.get(id).await, Resolved::Initials);
    }
    assert_eq!(r.get(&ids[60]).await, Resolved::Later);
    assert_eq!(r.script.requests(), 60);
    assert!(r.fetcher.window_cost() <= MAX_COST_PER_WINDOW + 1e-9);
}
