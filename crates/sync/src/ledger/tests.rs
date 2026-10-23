use std::collections::{BTreeSet, VecDeque};

use parking_lot::Mutex as PlMutex;

use adoken_model::time::SECOND;
use url::Url;

use super::*;
use crate::attribution::{self, Attribution, UserAction};
use crate::cadence::Lane;
use crate::clock::ManualClock;
use crate::governor::{Governor, Hold, MAX_REQUESTS_PER_WINDOW};

const T0: Millis = 1_790_769_600_000;

/// Answers from a script; an empty script never answers (in flight).
#[derive(Default)]
struct Script {
    replies: PlMutex<VecDeque<ApiResponse>>,
}

impl Script {
    fn reply(&self, status: u16, headers: &[(&str, &str)], body: &str) {
        let mut h = reqwest::header::HeaderMap::new();
        for (k, v) in headers {
            h.insert(
                reqwest::header::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        self.replies.lock().push_back(ApiResponse {
            status,
            rate: RateHeaders::parse(&h),
            body: body.as_bytes().to_vec(),
            content_type: Some("application/json".into()),
            validators: Default::default(),
        });
    }
}

impl Transport for Script {
    fn send(&self, _req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        let next = self.replies.lock().pop_front();
        Box::pin(async move {
            match next {
                Some(resp) => Ok(resp),
                None => futures::future::pending().await,
            }
        })
    }
}

fn get(path: &str) -> ApiRequest {
    ApiRequest::get(Url::parse(&format!("https://dev.azure.com{path}?api-version=7.1")).unwrap())
}

fn sync_origin(lane: Lane) -> Origin {
    Origin::Sync {
        lane,
        pinned: false,
        sync_now: false,
    }
}

struct Rig {
    clock: Arc<ManualClock>,
    log: Arc<NetLog>,
    gov: Governor,
}

fn rig() -> Rig {
    let clock = Arc::new(ManualClock::new(T0));
    Rig {
        log: Arc::new(NetLog::new(clock.clone())),
        gov: Governor::new(clock.clone()),
        clock,
    }
}

/// A completed entry for ring/totals fixtures.
fn done(seq: u64, at: Millis, status: u16) -> Entry {
    Entry {
        seq,
        at,
        method: "GET",
        write: false,
        host: "dev.azure.com".into(),
        route: "/{org}/{project}/_apis/build/builds".into(),
        purpose: "Builds",
        origin: sync_origin(Lane::DoneRuns),
        pass: None,
        subject: None,
        tier: Some(Tier::Warm),
        attempt: 1,
        queued_ms: 0,
        elapsed_ms: Some(80),
        status: Status::Http(status),
        bytes: Some(100),
        rate: RateHeaders::default(),
        error: None,
        optional: false,
        booked_tstu: 0.01,
    }
}

/// AC-B1: every field, and the three transports in one send order.
#[tokio::test]
async fn transports_share_one_log_in_send_order_with_every_field() {
    let r = rig();
    let main = Script::default();
    let writes = Script::default();
    let avatars = Script::default();
    main.reply(
        200,
        &[
            ("X-RateLimit-Cost", "0.004"),
            ("x-vss-activityid", "6f1c0e2a-0000-0000-0000-00000000d941"),
        ],
        r#"{"value":[]}"#,
    );
    writes.reply(200, &[], r#"{"vote":10}"#);
    avatars.reply(200, &[], "png");
    let (main, writes, avatars) = (
        Recording::new(main, r.log.clone()),
        Recording::new(writes, r.log.clone()),
        Recording::new(avatars, r.log.clone()),
    );
    attribution::scope(
        Attribution::new(sync_origin(Lane::AllPrs)),
        r.gov
            .send(&main, Tier::Warm, get("/contoso/p1/_apis/git/pullrequests")),
    )
    .await
    .unwrap();
    r.clock.advance(SECOND);
    attribution::scope(
        Attribution::new(Origin::You(UserAction::Vote)),
        r.gov.send(
            &writes,
            Tier::User,
            ApiRequest::put(
                Url::parse(
                    "https://dev.azure.com/contoso/p1/_apis/git/repositories/r1/pullRequests/9/reviewers/me",
                )
                .unwrap(),
                serde_json::json!({ "vote": 10 }),
            ),
        ),
    )
    .await
    .unwrap();
    r.gov
        .send_optional(
            &avatars,
            Tier::Cold,
            get("/contoso/_api/_common/identityImage").with_origin(Origin::Avatars),
        )
        .await
        .unwrap();

    let e = r.log.entries();
    assert_eq!(e.iter().map(|e| e.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(e[0].at, T0);
    assert_eq!(e[1].at, T0 + SECOND);
    assert_eq!(e[0].method, "GET");
    assert_eq!(e[0].host, "dev.azure.com");
    assert_eq!(e[0].route, "/{org}/{project}/_apis/git/pullrequests");
    assert_eq!(e[0].purpose, "Active PRs");
    assert_eq!(e[0].origin.label(), "All active pull requests");
    assert_eq!(e[0].tier, Some(Tier::Warm));
    assert_eq!(e[0].attempt, 1);
    assert_eq!(e[0].queued_ms, 0);
    assert!(e[0].elapsed_ms.is_some());
    assert_eq!(e[0].status, Status::Http(200));
    assert_eq!(e[0].bytes, Some(12));
    assert_eq!(e[0].rate.cost, Some(0.004));
    assert_eq!(
        e[0].rate.activity_id.as_deref(),
        Some("6f1c0e2a-0000-0000-0000-00000000d941")
    );
    assert_eq!((e[1].purpose, e[1].write), ("Vote", true));
    assert_eq!(e[1].origin.label(), "You: vote");
    assert_eq!(e[1].subject, Some(Subject::Pr(9)));
    assert_eq!((e[2].purpose, e[2].optional), ("Avatar", true));
    assert_eq!(e[2].origin, Origin::Avatars);
}

/// AC-B5: each governor retry is its own entry; the retry's queue wait is
/// the `Retry-After`, and a budget hold shows up as queue wait.
#[tokio::test]
async fn retries_are_attempts_and_holds_are_queue_wait() {
    let r = rig();
    let script = Script::default();
    script.reply(429, &[("Retry-After", "2")], "");
    script.reply(200, &[], "{}");
    let t = Recording::new(script, r.log.clone());
    attribution::scope(
        Attribution::new(sync_origin(Lane::MyPrs)),
        r.gov.send(&t, Tier::Hot, get("/o/_apis/connectionData")),
    )
    .await
    .unwrap();
    let e = r.log.entries();
    assert_eq!(e.len(), 2);
    assert_eq!((e[0].attempt, e[1].attempt), (1, 2));
    assert_eq!(e[0].class(), StatusClass::Warning);
    assert!(e[1].queued_ms >= 2_000, "{}", e[1].queued_ms);

    // Spend the 5-minute budget, then one more request waits for it.
    let r = rig();
    let script = Script::default();
    for _ in 0..=MAX_REQUESTS_PER_WINDOW {
        script.reply(200, &[], "{}");
    }
    let t = Recording::new(script, r.log.clone());
    let origin = Attribution::new(sync_origin(Lane::Index));
    for _ in 0..MAX_REQUESTS_PER_WINDOW {
        attribution::scope(
            origin.clone(),
            r.gov.send(&t, Tier::Cold, get("/o/_apis/projects")),
        )
        .await
        .unwrap();
    }
    let Some(Hold::Budget { until }) = r.gov.hold() else {
        panic!("budget spent");
    };
    let held = (until - r.clock.now_ms()) as u64;
    attribution::scope(origin, r.gov.send(&t, Tier::Cold, get("/o/_apis/projects")))
        .await
        .unwrap();
    assert_eq!(r.log.entries().last().unwrap().queued_ms, held);
}

/// AC-B6: requests whose futures are dropped complete as Canceled and the
/// in-flight count returns to 0.
#[tokio::test]
async fn dropped_requests_are_canceled_and_leave_no_flight() {
    let r = rig();
    let t = Recording::new(Script::default(), r.log.clone());
    let mut held: Vec<_> = (0..3)
        .map(|_| {
            Box::pin(attribution::scope(
                Attribution::new(sync_origin(Lane::MyPrs)),
                t.send(get("/o/p/_apis/git/repositories/r/pullRequests/1/threads")),
            ))
        })
        .collect();
    for f in &mut held {
        assert!(futures::poll!(f.as_mut()).is_pending());
    }
    assert_eq!(r.log.in_flight(), 3);
    assert_eq!(r.log.summary(T0).in_flight, 3);
    drop(held);
    assert_eq!(r.log.in_flight(), 0);
    assert!(
        r.log
            .entries()
            .iter()
            .all(|e| e.status == Status::Canceled && e.class() == StatusClass::Quiet)
    );
}

/// AC-B8: 10,001 requests keep the newest 10,000; since-launch counts all.
#[test]
fn the_ring_keeps_ten_thousand_and_launch_totals_stay_exact() {
    let clock = Arc::new(ManualClock::new(T0));
    let entries: Vec<Entry> = (1..=MAX_ENTRIES as u64 + 1)
        .map(|seq| done(seq, T0, 200))
        .collect();
    let log = NetLog::from_entries(clock, entries);
    assert_eq!(log.len(), MAX_ENTRIES);
    assert_eq!(log.first_seq(), 2);
    assert_eq!(log.last_seq(), MAX_ENTRIES as u64 + 1);
    let launch = log.totals(Group::Endpoint, Window::SinceLaunch, T0);
    assert_eq!(launch.requests, MAX_ENTRIES + 1);
    let hour = log.totals(Group::Endpoint, Window::LastHour, T0 + 1);
    assert_eq!(hour.requests, MAX_ENTRIES);
    // Incremental pull: only what's newer than what a view holds.
    assert_eq!(log.entries_since(MAX_ENTRIES as u64 - 1).len(), 2);
    assert_eq!(log.entries_since(0).len(), MAX_ENTRIES);
}

/// AC-B8: last-hour totals per tier equal the governor's ledger, and the
/// 5-minute TSTU equals its window cost.
#[tokio::test]
async fn tier_totals_equal_the_governor_ledger() {
    let r = rig();
    let script = Script::default();
    for (i, cost) in ["0.01", "0.02", "0.03", "0.04"].iter().enumerate() {
        let status = if i == 2 { 404 } else { 200 };
        script.reply(status, &[("X-RateLimit-Cost", cost)], "{}");
    }
    script.reply(200, &[], "{}");
    let t = Recording::new(script, r.log.clone());
    let send = |tier, path| {
        attribution::scope(
            Attribution::new(sync_origin(Lane::WorkItems)),
            r.gov.send(&t, tier, get(path)),
        )
    };
    send(Tier::Hot, "/o/p/_apis/build/builds").await.unwrap();
    send(Tier::Warm, "/o/p/_apis/build/builds").await.unwrap();
    send(Tier::Warm, "/o/p/_apis/build/builds/7")
        .await
        .unwrap_err();
    r.clock.advance(10 * MINUTE);
    send(Tier::Cold, "/o/_apis/projects").await.unwrap();
    // Booked with its estimate, like the governor does.
    attribution::scope(
        Attribution::new(sync_origin(Lane::Index)),
        r.gov
            .send(&t, Tier::Cold, get("/o/_apis/projects").with_cost_hint(0.5)),
    )
    .await
    .unwrap();
    let now = r.clock.now_ms();
    let totals = r.log.totals(Group::Tier, Window::LastHour, now);
    let mut ours: Vec<(Tier, usize, f64)> = totals
        .rows
        .iter()
        .map(|row| match row.key {
            TotalsKey::Tier(t) => (t, row.requests, row.tstu),
            _ => unreachable!(),
        })
        .collect();
    let mut gov = r.gov.snapshot().ledger;
    let key = |t: &(Tier, usize, f64)| format!("{:?}", t.0);
    ours.sort_by_key(key);
    gov.sort_by_key(key);
    assert_eq!(ours.len(), gov.len());
    for (a, b) in ours.iter().zip(&gov) {
        assert_eq!((a.0, a.1), (b.0, b.1));
        assert!((a.2 - b.2).abs() < 1e-9, "{a:?} vs {b:?}");
    }
    assert!((totals.last_5min_tstu - r.gov.window_cost()).abs() < 1e-9);
    assert!((totals.share_of_limit_pct - 0.54 / 200.0 * 100.0).abs() < 1e-9);
}

/// FR-20 status classes.
#[test]
fn status_classes_follow_fr20() {
    let class = |status: Status, rate: RateHeaders, kind: Option<ErrorKind>| {
        let mut e = done(1, T0, 200);
        e.status = status;
        e.rate = rate;
        e.error = kind.map(|kind| ErrorInfo {
            kind,
            code: None,
            message: None,
        });
        e.class()
    };
    let none = RateHeaders::default;
    use StatusClass::*;
    assert_eq!(class(Status::Http(200), none(), None), Success);
    assert_eq!(class(Status::Http(304), none(), None), Success);
    assert_eq!(class(Status::Http(404), none(), None), Warning);
    assert_eq!(class(Status::Http(429), none(), None), Warning);
    let delayed = RateHeaders {
        delay_ms: Some(1_500),
        ..none()
    };
    assert_eq!(class(Status::Http(200), delayed, None), Warning);
    for code in [401, 403, 203, 500, 503] {
        assert_eq!(class(Status::Http(code), none(), None), Danger, "{code}");
    }
    assert_eq!(
        class(
            Status::Failed { offline: true },
            none(),
            Some(ErrorKind::Offline)
        ),
        Danger
    );
    assert_eq!(class(Status::Canceled, none(), None), Quiet);
    assert_eq!(class(Status::InFlight, none(), None), Quiet);
}

/// FR-21 / Q-10: background avatar failures don't count for the indicator;
/// user-caused ones do. Unseen counts split danger and warning.
#[test]
fn indicator_counts_unseen_failures_but_not_background_avatars() {
    let clock = Arc::new(ManualClock::new(T0));
    // Aged out: 6 minutes ago.
    let old = done(1, T0 - 6 * MINUTE, 503);
    let mut avatar = done(2, T0, 404);
    avatar.optional = true;
    avatar.origin = Origin::Avatars;
    let mut link = done(3, T0, 401);
    link.optional = true;
    link.origin = Origin::You(UserAction::LinkWorkItem);
    let warn = done(4, T0, 404);
    let log = NetLog::from_entries(clock, vec![old, avatar, link, warn, done(5, T0, 200)]);
    let summary = log.summary(T0);
    assert_eq!(summary.unseen(0), (1, 1));
    assert_eq!(summary.unseen(3), (0, 1));
    assert_eq!(summary.unseen(4), (0, 0));
    assert_eq!(summary.last_completed.map(|e| e.seq), Some(5));
}

/// FR-17's export fields; the in-app subject and Azure DevOps' message
/// are never among them.
const EXPORT_FIELDS: [&str; 16] = [
    "seq",
    "at",
    "method",
    "host",
    "route",
    "purpose",
    "origin",
    "pass",
    "attempt",
    "queued_ms",
    "elapsed_ms",
    "status",
    "bytes",
    "rate",
    "activity_id",
    "error",
];

/// AC-B10: header line, then exactly the FR-17 export fields per entry.
#[test]
fn export_writes_a_header_then_only_export_fields() {
    let clock = Arc::new(ManualClock::new(T0));
    let mut e = done(1, T0, 404);
    e.subject = Some(Subject::Pr(4821));
    e.error = Some(ErrorInfo {
        kind: ErrorKind::Http,
        code: Some("TF401019".into()),
        message: Some("The Git repository with name canary-repo does not exist".into()),
    });
    let log = NetLog::from_entries(clock, vec![e, done(2, T0, 200)]);
    let mut out = Vec::new();
    export_jsonl(&log.entries(), &mut out, "0.1.0", T0).unwrap();
    let text = String::from_utf8(out).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0]["adoken_network_log"], 1);
    assert_eq!(lines[0]["app_version"], "0.1.0");
    assert_eq!(lines[0]["exported_at"], "2026-09-30T12:00:00Z");
    assert_eq!(lines[0]["redaction"], REDACTION_NOTE);
    let expected: BTreeSet<&str> = EXPORT_FIELDS.into_iter().collect();
    for line in &lines[1..] {
        let keys: BTreeSet<&str> = line
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, expected);
    }
    assert_eq!(
        lines[1]["error"],
        json!({ "kind": "http", "code": "TF401019" })
    );
    assert!(!text.contains("4821") && !text.contains("canary-repo"));
    assert_eq!(copy_line(&log.entries()[1]), lines[2].to_string());
}

/// AC-B2 at the recorder: names in paths, queries, bodies and error
/// messages never reach the host, route, purpose, origin, the export or
/// the `ADOKEN_SYNC_LOG` file; only the in-app error detail keeps Azure
/// DevOps' message.
#[tokio::test]
async fn canaries_never_reach_the_log_the_export_or_the_sink() {
    let canaries = [
        "canaryorg",
        "CanaryProject",
        "canary-repo",
        "canary@example.com",
        "canary-pat-0000",
        "canary-body",
        "CanaryOther",
    ];
    let dir = tempfile::tempdir().unwrap();
    let sink = dir.path().join("sync.log");
    let r = rig();
    r.log.set_sink(std::fs::File::create(&sink).unwrap());
    let script = Script::default();
    for _ in 0..5 {
        script.reply(200, &[], r#"{"author":"canary@example.com"}"#);
    }
    script.reply(
        404,
        &[],
        r#"{"message":"TF200016: The project CanaryProject does not exist. See https://canaryorg.visualstudio.com/x"}"#,
    );
    script.reply(200, &[], "{}");
    let t = Recording::new(script, r.log.clone());
    let base = "https://dev.azure.com/canaryorg";
    let url = |p: &str| Url::parse(&format!("{base}/{p}")).unwrap();
    let mut user =
        url("CanaryProject/_apis/git/pullrequests?searchCriteria.creatorId=canary@example.com");
    user.set_username("canary-pat-0000").unwrap();
    let requests = vec![
        ApiRequest::get(user),
        ApiRequest::get(url("_apis/projects/CanaryOther")),
        ApiRequest::put(
            url("CanaryProject/_apis/git/repositories/canary-repo/pullRequests/7/reviewers/me"),
            json!({ "vote": 10 }),
        ),
        ApiRequest::post(
            url("CanaryProject/_apis/git/repositories/canary-repo/pullRequests/7/threads"),
            json!({ "comments": [{ "content": "canary-body" }] }),
        ),
        ApiRequest::get(url("_api/_common/identityImage?id=canary@example.com")),
        ApiRequest::get(url(
            "CanaryProject/_apis/git/repositories/canary-repo/blobs/95d09f2b10159347eece71399a7e2e907ea3df4f",
        )),
        ApiRequest::get(
            Url::parse("https://canaryorg.visualstudio.com/CanaryProject/_apis/build/builds")
                .unwrap(),
        ),
    ];
    for req in requests {
        let _ = attribution::scope(
            Attribution::new(Origin::You(UserAction::Comment)),
            r.gov.send(&t, Tier::User, req),
        )
        .await;
    }
    let entries = r.log.entries();
    assert_eq!(entries.len(), 7);
    assert_eq!(entries[1].route, "/{org}/_apis/projects/{project}");
    assert!(
        entries[5]
            .error
            .as_ref()
            .unwrap()
            .message
            .as_deref()
            .unwrap()
            .contains("CanaryProject")
    );
    assert!(
        !entries[5]
            .error
            .as_ref()
            .unwrap()
            .message
            .as_deref()
            .unwrap()
            .contains("://")
    );
    let mut export = Vec::new();
    export_jsonl(&entries, &mut export, "0.1.0", T0).unwrap();
    let export = String::from_utf8(export).unwrap();
    let sink = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(sink.lines().count(), 7);
    for e in &entries {
        let visible = format!("{} {} {} {}", e.host, e.route, e.purpose, e.origin.label());
        for c in canaries {
            assert!(!visible.contains(c), "{c} in {visible}");
        }
    }
    for c in canaries {
        assert!(!export.contains(c), "{c} in the export");
        assert!(!sink.contains(c), "{c} in ADOKEN_SYNC_LOG");
    }
}

/// FR-18: a request nothing attributed is a bug in debug builds.
#[tokio::test]
#[should_panic(expected = "unattributed request")]
async fn unattributed_requests_assert_in_debug() {
    let r = rig();
    let script = Script::default();
    script.reply(200, &[], "{}");
    let t = Recording::new(script, r.log.clone());
    let _ = r.gov.send(&t, Tier::Hot, get("/o/_apis/projects")).await;
}

/// FR-19: a route outside the table is a bug in debug builds.
#[tokio::test]
#[should_panic(expected = "outside the route table")]
async fn unknown_routes_assert_in_debug() {
    let r = rig();
    let script = Script::default();
    script.reply(200, &[], "{}");
    let t = Recording::new(script, r.log.clone());
    let _ = r
        .gov
        .send(
            &t,
            Tier::Hot,
            get("/o/p/_apis/git/repositories/r/refs/heads/main").with_origin(Origin::Startup),
        )
        .await;
}
