//! Synthetic activity data for gallery stories, the smoke run and tests
//! (activity.md §10: no real org data). Times sit around the demo
//! dataset's fixed "now" so stories line up with the demo store.

use std::sync::Arc;

use adoken_model::{Millis, Subject};
use adoken_sync::Tier;
use adoken_sync::activity::{
    ChangeWord, Changes, FailReason, KindChanges, LaneActivity, Outcome, PassRecord, PinsRow,
    Progress, ProgressUnit, SyncInputs, Tracker, Trigger,
};
use adoken_sync::attribution::{Origin, PassRef, UserAction};
use adoken_sync::cadence::{Lane, Lanes, Pause, Prefs};
use adoken_sync::ledger::{Entry, ErrorInfo, ErrorKind, NetLog, Status};
use adoken_sync::transport::RateHeaders;
use adoken_sync::{GovSnapshot, Hold, ManualClock};

use crate::sync::ActivityHub;

/// The demo dataset's "now".
pub const NOW: Millis = adoken_demo::DEMO_NOW;
const SECOND: Millis = 1_000;
const MINUTE: Millis = 60 * SECOND;

/// A completed GET by a sync lane, 200 unless `status` says otherwise.
pub fn entry(seq: u64, at: Millis, status: Status, origin: Origin) -> Entry {
    let done = !matches!(status, Status::InFlight);
    let has_body = matches!(status, Status::Http(_));
    Entry {
        seq,
        at,
        method: "GET",
        write: false,
        host: "dev.azure.com".into(),
        route: "/{org}/{project}/_apis/wit/workitemsbatch".into(),
        purpose: "Work items batch",
        origin,
        pass: Some(PassRef { id: 1, started: at }),
        subject: None,
        tier: Some(Tier::Warm),
        attempt: 1,
        queued_ms: 0,
        elapsed_ms: done.then_some(84),
        status,
        bytes: has_body.then_some(4_096),
        rate: RateHeaders {
            cost: done.then_some(0.004),
            ..RateHeaders::default()
        },
        booked_tstu: if done { 0.004 } else { 0.0 },
        error: None,
        optional: false,
    }
}

pub fn lane_origin() -> Origin {
    Origin::Sync {
        lane: Lane::WorkItems,
        pinned: false,
        sync_now: false,
    }
}

/// A background avatar fetch on the optional transport (Q-10).
pub fn avatar_entry(seq: u64, at: Millis, status: Status) -> Entry {
    Entry {
        route: "/{org}/_api/_common/identityImage".into(),
        purpose: "Avatar",
        tier: Some(Tier::Cold),
        optional: true,
        ..entry(seq, at, status, Origin::Avatars)
    }
}

/// The routes a session hits, with their purpose, origin and tier.
fn shape(i: u64) -> (&'static str, &'static str, &'static str, Origin, Tier) {
    let sync = |lane| Origin::Sync {
        lane,
        pinned: false,
        sync_now: false,
    };
    match i % 9 {
        0 => (
            "GET",
            "/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads",
            "PR threads",
            sync(Lane::MyPrs),
            Tier::Hot,
        ),
        1 => (
            "GET",
            "/{org}/{project}/_apis/git/pullrequests",
            "Active pull requests",
            sync(Lane::AllPrs),
            Tier::Warm,
        ),
        2 => (
            "POST",
            "/{org}/{project}/_apis/wit/wiql",
            "Work items query",
            sync(Lane::WorkItems),
            Tier::Warm,
        ),
        3 => (
            "POST",
            "/{org}/{project}/_apis/wit/workitemsbatch",
            "Work items batch",
            sync(Lane::WorkItems),
            Tier::Warm,
        ),
        4 => (
            "GET",
            "/{org}/{project}/_apis/build/builds",
            "Pipeline runs",
            sync(Lane::LiveRuns),
            Tier::Hot,
        ),
        5 => (
            "GET",
            "/{org}/{project}/_apis/build/builds/{n}/timeline",
            "Build timeline",
            sync(Lane::LiveRuns),
            Tier::Hot,
        ),
        6 => (
            "GET",
            "/{org}/{project}/_apis/git/repositories/{repo}/blobs/{sha}",
            "File contents",
            Origin::You(UserAction::OpenPr),
            Tier::User,
        ),
        7 => (
            "POST",
            "/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/reviewers/{id}",
            "Vote",
            Origin::You(UserAction::Vote),
            Tier::User,
        ),
        _ => (
            "GET",
            "/{org}/_apis/projects",
            "Projects",
            sync(Lane::Index),
            Tier::Cold,
        ),
    }
}

/// `n` completed requests ending at `now`, one every 1.1 s, with exactly
/// `failures` warning/danger entries spread evenly (503, 429, 404, 401).
pub fn ledger(n: usize, failures: usize, now: Millis) -> Vec<Entry> {
    let step = n.checked_div(failures).unwrap_or(usize::MAX);
    (0..n as u64)
        .map(|i| {
            let (method, route, purpose, origin, tier) = shape(i);
            // One failure per `step` entries, shifted so they land on
            // different routes and origins.
            let failing = failures > 0
                && (i as usize) / step < failures
                && (i as usize) % step == step / 2 + ((i as usize) / step) % 9;
            let status = if failing {
                match (i as usize / step) % 4 {
                    0 => Status::Http(503),
                    1 => Status::Http(429),
                    2 => Status::Http(404),
                    _ => Status::Http(401),
                }
            } else if i % 13 == 4 {
                Status::Http(304)
            } else {
                Status::Http(200)
            };
            let at = now - (n as Millis - i as Millis) * 1_100;
            let mut e = entry(i + 1, at, status, origin);
            e.method = method;
            e.write = method == "POST" && purpose == "Vote";
            e.route = route.into();
            e.purpose = purpose;
            e.tier = Some(tier);
            e.elapsed_ms = Some(40 + (i * 37) % 900);
            e.queued_ms = if i % 17 == 0 { 120 } else { 0 };
            e.bytes = Some(512 + (i * 811) % 40_000);
            e.rate.cost = Some(0.001 + ((i % 7) as f64) * 0.001);
            e.booked_tstu = e.rate.cost.unwrap_or_default();
            if let Status::Http(code) = e.status
                && code >= 400
            {
                e.bytes = Some(180);
                e.error = Some(ErrorInfo {
                    kind: ErrorKind::Http,
                    code: (code == 503).then(|| "TF400733".to_owned()),
                    message: Some(format!("Azure DevOps returned {code}.")),
                });
                if code == 429 {
                    e.rate.retry_after_ms = Some(30_000);
                    e.rate.resource = Some("Core".into());
                }
            }
            e
        })
        .collect()
}

/// The live-tail story's mix (activity.md §10 `activity-network`): 200s,
/// a 304, one in flight, a 429, a 503, an offline failure, a canceled one.
pub fn live_mix(now: Millis) -> Vec<Entry> {
    let mut entries = ledger(60, 0, now - 5 * SECOND);
    let next = entries.len() as u64 + 1;
    let tail = [
        Status::Http(304),
        Status::Http(429),
        Status::Http(503),
        Status::Failed { offline: true },
        Status::Canceled,
        Status::Http(200),
        Status::InFlight,
    ];
    for (k, status) in tail.into_iter().enumerate() {
        let seq = next + k as u64;
        let (method, route, purpose, origin, tier) = shape(seq + 2);
        let mut e = entry(seq, now - (6 - k as Millis) * 600, status, origin);
        e.method = method;
        e.route = route.into();
        e.purpose = purpose;
        e.tier = Some(tier);
        match status {
            Status::Http(429) => {
                e.rate.retry_after_ms = Some(40_000);
                e.rate.resource = Some("Core".into());
                e.error = Some(ErrorInfo {
                    kind: ErrorKind::Http,
                    code: None,
                    message: None,
                });
            }
            Status::Http(503) => {
                e.queued_ms = 120;
                e.elapsed_ms = Some(1_200);
                e.bytes = None;
                e.rate = RateHeaders {
                    cost: None,
                    limit: Some(200.0),
                    remaining: Some(182.0),
                    reset_s: Some((NOW / 1000) + 240),
                    retry_after_ms: Some(30_000),
                    ..RateHeaders::default()
                };
                e.rate.activity_id = Some("6f1c0e2a-4b8e-4f1e-9a51-1c2d3e4f9d41".into());
                e.error = Some(ErrorInfo {
                    kind: ErrorKind::Http,
                    code: Some("TF400733".into()),
                    message: Some("The service is temporarily unavailable.".into()),
                });
                e.attempt = 2;
            }
            Status::Failed { .. } => {
                e.bytes = None;
                e.rate.cost = None;
                e.error = Some(ErrorInfo {
                    kind: ErrorKind::Offline,
                    code: None,
                    message: None,
                });
            }
            Status::Canceled => {
                e.bytes = None;
                e.rate.cost = None;
                e.elapsed_ms = Some(310);
            }
            _ => {}
        }
        entries.push(e);
    }
    entries
}

pub fn net_log(entries: Vec<Entry>) -> Arc<NetLog> {
    Arc::new(NetLog::from_entries(
        Arc::new(ManualClock::new(NOW)),
        entries,
    ))
}

// ── sync activity ───────────────────────────────────────────────────────

fn changes_of(prs: &[i64], work_items: &[i64], runs: &[i64]) -> Changes {
    let kind = |ids: &[i64], word: fn(usize) -> ChangeWord| KindChanges {
        updated: ids.len() as u32,
        ids: ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, word(i)))
            .collect(),
        ..KindChanges::default()
    };
    Changes {
        prs: kind(prs, |i| match i {
            0 => ChangeWord::NewComment,
            1 => ChangeWord::VoteChanged,
            _ => ChangeWord::Pushed,
        }),
        work_items: kind(work_items, |i| match i {
            0 => ChangeWord::State("Active".into()),
            _ => ChangeWord::Updated,
        }),
        runs: KindChanges {
            removed: runs.len() as u32,
            ids: runs
                .iter()
                .map(|id| {
                    (
                        *id,
                        ChangeWord::Finished(adoken_model::RunResult::Succeeded),
                    )
                })
                .collect(),
            ..KindChanges::default()
        },
        ..Changes::default()
    }
}

fn lanes(list: &[Lane]) -> Lanes {
    let mut lanes = Lanes::NONE;
    for lane in list {
        lanes.insert(*lane);
    }
    lanes
}

fn pass(
    id: u64,
    started: Millis,
    lanes_: &[Lane],
    trigger: Trigger,
    changes: Changes,
) -> PassRecord {
    PassRecord {
        id,
        started,
        ended: Some(started + 1_100),
        lanes: lanes(lanes_),
        trigger,
        outcome: Outcome::Ok,
        requests: 9,
        tstu: 0.004,
        events: 0,
        changes,
    }
}

/// Ids of the demo dataset's first PRs, work items and runs (their titles
/// resolve against the demo store).
pub fn demo_ids() -> (Vec<i64>, Vec<i64>, Vec<i64>) {
    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    (
        ds.prs.iter().take(3).map(|p| p.id).collect(),
        ds.work_items.iter().take(2).map(|w| w.id).collect(),
        ds.runs.iter().take(1).map(|r| r.id).collect(),
    )
}

/// A pass in progress (pinned to the top of Recent passes).
pub fn running_pass(now: Millis) -> PassRecord {
    PassRecord {
        ended: None,
        outcome: Outcome::Running,
        requests: 6,
        ..pass(
            11,
            now - 2 * SECOND,
            &[Lane::AllPrs, Lane::WorkItems],
            Trigger::Scheduled,
            Changes::default(),
        )
    }
}

/// Recent passes (activity.md §10 `activity-sync`): one with changes (3
/// PRs, 2 work items, 1 run), four no-change passes to fold, a "You: Sync
/// #id" pass and a failed one.
pub fn passes(now: Millis) -> Vec<PassRecord> {
    let (prs, work_items, runs) = demo_ids();
    let mut list = vec![pass(
        10,
        now - 20 * SECOND,
        &[Lane::MyPrs, Lane::WorkItems, Lane::LiveRuns],
        Trigger::Scheduled,
        changes_of(&prs, &work_items, &runs),
    )];
    for k in 0..4 {
        list.push(pass(
            9 - k,
            now - (2 + k as Millis) * MINUTE,
            &[Lane::AllPrs],
            Trigger::Scheduled,
            Changes::default(),
        ));
    }
    list.push(pass(
        5,
        now - 7 * MINUTE,
        &[Lane::WorkItems],
        Trigger::ItemSync(work_items[0]),
        changes_of(&[], &work_items[..1], &[]),
    ));
    let mut failed = pass(
        4,
        now - 9 * MINUTE,
        &[Lane::WorkItems],
        Trigger::Scheduled,
        Changes::default(),
    );
    failed.outcome = Outcome::Failed(FailReason::Status {
        status: 503,
        code: Some("TF400733".into()),
        message: None,
    });
    failed.requests = 2;
    list.push(failed);
    list
}

/// Each lane synced recently, next passes planned.
pub fn lanes_ok(now: Millis) -> [LaneActivity; 6] {
    std::array::from_fn(|ix| LaneActivity {
        last_ok: Some(now - (12 + 20 * ix as Millis) * SECOND),
        next_due: Some(now + (40 + 10 * ix as Millis) * SECOND),
        ..LaneActivity::default()
    })
}

pub fn pins_row(now: Millis) -> PinsRow {
    PinsRow {
        pins: 4,
        unavailable: 1,
        last_full_refresh: Some(now - MINUTE),
    }
}

/// `rows()` inputs for one story state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncScene {
    Idle,
    Syncing,
    First,
    Throttled,
    Failed,
    Paused,
    MissingScope,
    Offline,
}

pub fn inputs(scene: SyncScene, now: Millis) -> SyncInputs {
    let mut inputs = SyncInputs {
        lanes: lanes_ok(now),
        prefs: Prefs::default(),
        online: true,
        foreground: true,
        pins: pins_row(now),
        ..SyncInputs::default()
    };
    match scene {
        SyncScene::Idle => {}
        SyncScene::Syncing => {
            let lane = &mut inputs.lanes[Lane::AllPrs.index()];
            lane.running = true;
            lane.progress = Some(Progress {
                done: 3,
                total: Some(12),
                unit: ProgressUnit::Projects,
            });
        }
        SyncScene::First => {
            for (ix, lane) in inputs.lanes.iter_mut().enumerate() {
                *lane = LaneActivity {
                    next_due: Some(now + 10 * SECOND),
                    running: ix == Lane::MyPrs.index(),
                    progress: (ix == Lane::MyPrs.index()).then_some(Progress {
                        done: 3,
                        total: Some(12),
                        unit: ProgressUnit::PullRequests,
                    }),
                    ..LaneActivity::default()
                };
            }
            inputs.pins = PinsRow::default();
        }
        SyncScene::Throttled => {
            inputs.hold = Some(Hold::Paused {
                until: now + 40 * SECOND,
            });
            inputs.gov = GovSnapshot {
                paused_until: Some(now + 40 * SECOND),
                resource: Some("Core".into()),
                ..GovSnapshot::default()
            };
        }
        SyncScene::Failed => {
            inputs.lanes[Lane::WorkItems.index()].failure = Some((
                FailReason::Status {
                    status: 503,
                    code: Some("TF400733".into()),
                    message: None,
                },
                Some(now + 30 * SECOND),
            ));
        }
        SyncScene::Paused => {
            inputs.prefs.pause = Pause::Until(now + 60 * MINUTE);
        }
        SyncScene::MissingScope => {
            inputs.missing_scopes = vec!["vso.build".into()];
        }
        SyncScene::Offline => inputs.online = false,
    }
    inputs
}

/// A hub over fixtures: `entries` in the log, `passes` in the tracker.
pub fn hub(entries: Vec<Entry>, passes: Vec<PassRecord>, now: Millis) -> ActivityHub {
    ActivityHub {
        net: net_log(entries),
        passes: Arc::new(Tracker::from_fixture(passes, lanes_ok(now), pins_row(now))),
    }
}

/// The subject of a request about a PR (in-app only, Q-2).
pub fn with_subject(mut entry: Entry, subject: Subject) -> Entry {
    entry.subject = Some(subject);
    entry
}
