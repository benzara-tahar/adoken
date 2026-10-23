//! The network log (spec 0016 group B, FR-16..FR-26; realizes 0002 FR-10).
//!
//! One [`NetLog`] per app session merges the three production transports
//! (main, work-item writes, avatars), each wrapped in a [`Recording`]. An
//! entry is created when the request goes out (in flight) and completed
//! when it ends; a request whose future is dropped completes as
//! [`Status::Canceled`], so the in-flight count can't leak.
//!
//! Redaction (FR-19): the route comes from the allow-list in
//! [`crate::routes`]; the host loses a legacy org name. Never recorded:
//! query strings, fragments, bodies, headers other than the seven rate
//! headers and `x-vss-activityid`, the credential, URL user info. Azure
//! DevOps' error message is kept for the app only (scrubbed of URLs); the
//! export and `ADOKEN_SYNC_LOG` carry the error kind and code only.
//!
//! Totals (FR-24): last hour from the ring (the governor's 300 requests /
//! 5 min cap keeps an hour well inside 10,000 entries), since launch from
//! counters kept apart from the ring, so they stay exact after eviction.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use adoken_model::time::{HOUR, MINUTE};
use adoken_model::{Millis, Subject};
use futures::future::BoxFuture;
use serde_json::{Value, json};

use crate::Tier;
use crate::activity::Signal;
use crate::attribution::{Origin, PassRef};
use crate::clock::Clock;
use crate::governor::WINDOW_MS;
use crate::routes;
use crate::transport::{ApiRequest, ApiResponse, RateHeaders, Transport, TransportError};

/// Entries kept in memory, oldest dropped first (FR-26).
pub const MAX_ENTRIES: usize = 10_000;
/// Azure DevOps' per-user limit per sliding 5 minutes.
pub const ADO_LIMIT_TSTU: f64 = crate::cadence::RATE_LIMIT_TSTU;
/// Failures older than this stop counting for the indicator (FR-21).
pub const UNSEEN_WINDOW_MS: Millis = 5 * MINUTE;
/// Export format version (first line of the JSON Lines file).
pub const EXPORT_VERSION: u32 = 1;
/// The redaction note of every export (FR-25, Q-9).
pub const REDACTION_NOTE: &str = "Recorded on this Mac only. Routes are templates ({org}, \
{project}, {repo}, {name}, {id}, {sha}, {n}); no query strings, request or response bodies, \
headers other than rate limits and x-vss-activityid, item ids, names, Azure DevOps messages \
or credentials.";

/// How a request ended (FR-17 `status`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    InFlight,
    Http(u16),
    /// No response: offline, DNS, TLS, timeout (`offline`), or the HTTP
    /// layer refused to send inside a `Retry-After` window.
    Failed {
        offline: bool,
    },
    /// Abandoned before a response (pass canceled by sleep, view closed).
    Canceled,
}

/// FR-20 tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusClass {
    Quiet,
    Success,
    Warning,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// An HTTP error status (4xx/5xx, 203 sign-in page).
    Http,
    /// Nothing reached Azure DevOps.
    Offline,
    /// The credential was refused (401 / sign-in page via the HTTP layer).
    Unauthorized,
    /// The HTTP layer is inside a `Retry-After` window.
    Throttled,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Http => "http",
            ErrorKind::Offline => "offline",
            ErrorKind::Unauthorized => "unauthorized",
            ErrorKind::Throttled => "throttled",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorInfo {
    pub kind: ErrorKind,
    /// Azure DevOps' error code (`TF400733`, `VS402335` …).
    pub code: Option<String>,
    /// Azure DevOps' message, without URLs. In-app only, never exported.
    pub message: Option<String>,
}

/// One request (FR-17).
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Monotonic per session, in send order.
    pub seq: u64,
    /// Wall clock when the request went out (the injected clock).
    pub at: Millis,
    pub method: &'static str,
    /// POST/PUT/PATCH that change server state (a WIQL POST is a query).
    pub write: bool,
    pub host: String,
    pub route: String,
    pub purpose: &'static str,
    pub origin: Origin,
    pub pass: Option<PassRef>,
    /// The item it was about. In-app only (Q-2): never exported or copied.
    pub subject: Option<Subject>,
    pub tier: Option<Tier>,
    pub attempt: u32,
    pub queued_ms: u64,
    /// `None` while in flight.
    pub elapsed_ms: Option<u64>,
    pub status: Status,
    /// Response body size; `None` without a response.
    pub bytes: Option<u64>,
    pub rate: RateHeaders,
    pub error: Option<ErrorInfo>,
    /// Sent on an optional transport (avatars, work item links): background
    /// failures don't light the indicator (FR-21, Q-10).
    pub optional: bool,
    /// TSTU the governor booked for it: `X-RateLimit-Cost`, else the
    /// request's estimate; 0 without a response.
    pub booked_tstu: f64,
}

impl Entry {
    /// `Retry-After` or `X-RateLimit-Delay` > 0 on the response.
    pub fn delayed(&self) -> bool {
        self.rate.retry_after_ms.is_some_and(|ms| ms > 0)
            || self.rate.delay_ms.is_some_and(|ms| ms > 0)
    }

    /// FR-20.
    pub fn class(&self) -> StatusClass {
        match self.status {
            Status::InFlight | Status::Canceled => StatusClass::Quiet,
            Status::Failed { .. } => match self.error.as_ref().map(|e| e.kind) {
                Some(ErrorKind::Throttled) => StatusClass::Warning,
                _ => StatusClass::Danger,
            },
            Status::Http(code) => match code {
                401 | 403 | 203 | 500..=599 => StatusClass::Danger,
                _ if self.delayed() => StatusClass::Warning,
                400..=499 => StatusClass::Warning,
                200..=299 | 304 => StatusClass::Success,
                _ => StatusClass::Quiet,
            },
        }
    }

    /// A failure (warning or danger, FR-23 "Failed only").
    pub fn failed(&self) -> bool {
        self.class() >= StatusClass::Warning
    }

    /// Whether a failure of this entry may change the statusbar indicator
    /// (FR-21): background requests on optional transports don't.
    pub fn counts_for_indicator(&self) -> bool {
        !self.optional || self.origin.user_caused()
    }

    /// Requests the governor books in its ledger (answered ones), the base
    /// of per-tier totals (FR-24 "equal `GovSnapshot::ledger`").
    fn answered(&self) -> bool {
        matches!(self.status, Status::Http(_))
    }

    /// The FR-17 export fields only (FR-25): never the subject or Azure
    /// DevOps' message.
    pub fn export_json(&self) -> Value {
        let status = match self.status {
            Status::Http(code) => json!(code),
            Status::Failed { .. } => json!("Failed"),
            Status::Canceled => json!("Canceled"),
            Status::InFlight => json!("In flight"),
        };
        let r = &self.rate;
        json!({
            "seq": self.seq,
            "at": crate::wire::format_time(self.at),
            "method": self.method,
            "host": self.host,
            "route": self.route,
            "purpose": self.purpose,
            "origin": self.origin.label(),
            "pass": self.pass.map(|p| crate::wire::format_time(p.started)),
            "attempt": self.attempt,
            "queued_ms": self.queued_ms,
            "elapsed_ms": self.elapsed_ms,
            "status": status,
            "bytes": self.bytes,
            "rate": {
                "cost": r.cost,
                "limit": r.limit,
                "remaining": r.remaining,
                "reset": r.reset_s,
                "resource": r.resource,
                "delay_ms": r.delay_ms,
                "retry_after_ms": r.retry_after_ms,
            },
            "activity_id": r.activity_id,
            "error": self.error.as_ref().map(|e| json!({
                "kind": e.kind.as_str(),
                "code": e.code,
            })),
        })
    }
}

/// One JSON line, the same as an export line (⌘C, FR-25).
pub fn copy_line(entry: &Entry) -> String {
    entry.export_json().to_string()
}

/// The first line of an export: version, time, redaction note.
pub fn export_header(app_version: &str, now: Millis) -> Value {
    json!({
        "adoken_network_log": EXPORT_VERSION,
        "app_version": app_version,
        "exported_at": crate::wire::format_time(now),
        "redaction": REDACTION_NOTE,
    })
}

/// FR-25 / FR-26: JSON Lines, header first. Run it off the UI thread.
pub fn export_jsonl(
    entries: &[Entry],
    out: &mut impl Write,
    app_version: &str,
    now: Millis,
) -> std::io::Result<()> {
    writeln!(out, "{}", export_header(app_version, now))?;
    for e in entries {
        writeln!(out, "{}", e.export_json())?;
    }
    out.flush()
}

/// What the statusbar indicator samples (FR-21).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetSummary {
    pub in_flight: usize,
    pub oldest_in_flight_at: Option<Millis>,
    pub last_completed: Option<Entry>,
    /// Indicator-relevant failures of the last 5 minutes: `(seq, at, class)`.
    pub recent_problems: Vec<(u64, Millis, StatusClass)>,
    /// Requests since launch (completed).
    pub total: u64,
}

impl NetSummary {
    /// Unseen `(danger, warning)` failures after `seen_through_seq`.
    pub fn unseen(&self, seen_through_seq: u64) -> (usize, usize) {
        self.recent_problems
            .iter()
            .filter(|(seq, ..)| *seq > seen_through_seq)
            .fold((0, 0), |(d, w), (.., class)| match class {
                StatusClass::Danger => (d + 1, w),
                _ => (d, w + 1),
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Endpoint,
    Origin,
    Tier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    LastHour,
    SinceLaunch,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TotalsKey {
    Endpoint {
        method: &'static str,
        route: String,
        purpose: &'static str,
    },
    Origin(Origin),
    Tier(Tier),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TotalsRow {
    pub key: TotalsKey,
    pub requests: usize,
    /// Warning + danger.
    pub failures: usize,
    pub ok: usize,
    pub warnings: usize,
    pub dangers: usize,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub p95_queue_ms: u64,
    pub bytes: u64,
    pub tstu: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    /// Sorted by TSTU, descending (the design's default sort).
    pub rows: Vec<TotalsRow>,
    pub requests: usize,
    pub failures: usize,
    pub tstu: f64,
    /// Last 5 minutes' TSTU, as `Activity::window_cost` measures it.
    pub last_5min_tstu: f64,
    /// Its share of Azure DevOps' 200 TSTU, in percent.
    pub share_of_limit_pct: f64,
}

/// Log-bucketed latencies (≈10% resolution) for since-launch percentiles.
#[derive(Clone, Debug, Default)]
struct Histogram(Vec<u32>);

const BUCKET_BASE: f64 = 1.1;

impl Histogram {
    fn add(&mut self, ms: u64) {
        let ix = ((ms as f64 + 1.0).ln() / BUCKET_BASE.ln()) as usize;
        if self.0.len() <= ix {
            self.0.resize(ix + 1, 0);
        }
        self.0[ix] += 1;
    }

    /// Upper bound of the bucket holding the `p`-quantile.
    fn quantile(&self, p: f64) -> u64 {
        let total: u64 = self.0.iter().map(|n| u64::from(*n)).sum();
        if total == 0 {
            return 0;
        }
        let rank = ((total as f64 * p).ceil() as u64).max(1);
        let mut seen = 0;
        for (ix, n) in self.0.iter().enumerate() {
            seen += u64::from(*n);
            if seen >= rank {
                return (BUCKET_BASE.powi(ix as i32 + 1) - 1.0).round() as u64;
            }
        }
        0
    }
}

#[derive(Clone, Debug, Default)]
struct Acc {
    requests: usize,
    ok: usize,
    warnings: usize,
    dangers: usize,
    bytes: u64,
    tstu: f64,
    latency: Histogram,
    queue: Histogram,
}

impl Acc {
    fn add(&mut self, e: &Entry) {
        self.requests += 1;
        match e.class() {
            StatusClass::Success => self.ok += 1,
            StatusClass::Warning => self.warnings += 1,
            StatusClass::Danger => self.dangers += 1,
            StatusClass::Quiet => {}
        }
        self.bytes += e.bytes.unwrap_or(0);
        self.tstu += e.booked_tstu;
        self.latency.add(e.elapsed_ms.unwrap_or(0));
        self.queue.add(e.queued_ms);
    }

    fn row(&self, key: TotalsKey) -> TotalsRow {
        TotalsRow {
            key,
            requests: self.requests,
            failures: self.warnings + self.dangers,
            ok: self.ok,
            warnings: self.warnings,
            dangers: self.dangers,
            p50_ms: self.latency.quantile(0.5),
            p95_ms: self.latency.quantile(0.95),
            p95_queue_ms: self.queue.quantile(0.95),
            bytes: self.bytes,
            tstu: self.tstu,
        }
    }
}

/// Since-launch counters (FR-24), apart from the ring.
#[derive(Default)]
struct Launch {
    endpoint: HashMap<(&'static str, String), (&'static str, Acc)>,
    origin: HashMap<Origin, Acc>,
    tier: HashMap<Tier, Acc>,
    total: u64,
}

impl Launch {
    fn add(&mut self, e: &Entry) {
        self.total += 1;
        self.endpoint
            .entry((e.method, e.route.clone()))
            .or_insert_with(|| (e.purpose, Acc::default()))
            .1
            .add(e);
        self.origin.entry(e.origin).or_default().add(e);
        if e.answered()
            && let Some(tier) = e.tier
        {
            self.tier.entry(tier).or_default().add(e);
        }
    }
}

struct Ring {
    entries: VecDeque<Entry>,
    /// Seq of the next request (the first is 1).
    next_seq: u64,
    in_flight: usize,
    launch: Launch,
}

impl Ring {
    fn index(&self, seq: u64) -> Option<usize> {
        let first = self.entries.front()?.seq;
        let ix = seq.checked_sub(first)? as usize;
        (ix < self.entries.len() && self.entries[ix].seq == seq).then_some(ix)
    }

    fn push(&mut self, entry: Entry) {
        if self.entries.len() == MAX_ENTRIES
            && let Some(old) = self.entries.pop_front()
            && old.status == Status::InFlight
        {
            self.in_flight -= 1;
        }
        if entry.status == Status::InFlight {
            self.in_flight += 1;
        } else {
            self.launch.add(&entry);
        }
        self.next_seq = self.next_seq.max(entry.seq + 1);
        self.entries.push_back(entry);
    }
}

/// The session's request log, shared by every [`Recording`].
pub struct NetLog {
    clock: Arc<dyn Clock>,
    ring: Mutex<Ring>,
    signal: Arc<Signal>,
    /// `ADOKEN_SYNC_LOG`: one FR-25 JSON line per completed request.
    sink: Mutex<Option<std::fs::File>>,
}

impl NetLog {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self::with_signal(clock, Arc::new(Signal::default()))
    }

    /// Bump `signal` on every change (the activity publisher, FR-6).
    pub fn with_signal(clock: Arc<dyn Clock>, signal: Arc<Signal>) -> Self {
        NetLog {
            clock,
            ring: Mutex::new(Ring {
                entries: VecDeque::with_capacity(1024),
                next_seq: 1,
                in_flight: 0,
                launch: Launch::default(),
            }),
            signal,
            sink: Mutex::new(None),
        }
    }

    /// Fixtures (gallery, tests): a log holding `entries` (send order),
    /// with since-launch counters seeded from the completed ones.
    pub fn from_entries(clock: Arc<dyn Clock>, entries: Vec<Entry>) -> Self {
        let log = Self::new(clock);
        {
            let mut ring = log.lock();
            for e in entries {
                ring.push(e);
            }
        }
        log
    }

    /// Insert or replace an entry by `seq` (fixtures, live-tail stories).
    pub fn record_fixture(&self, entry: Entry) {
        let mut ring = self.lock();
        match ring.index(entry.seq) {
            Some(ix) => {
                let was_in_flight = ring.entries[ix].status == Status::InFlight;
                if was_in_flight && entry.status != Status::InFlight {
                    ring.in_flight -= 1;
                    ring.launch.add(&entry);
                }
                ring.entries[ix] = entry;
            }
            None => ring.push(entry),
        }
        drop(ring);
        self.signal.bump();
    }

    /// Also append every completed request to `file` as an FR-25 JSON
    /// line (`ADOKEN_SYNC_LOG`, FR-26).
    pub fn set_sink(&self, file: std::fs::File) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
    }

    pub fn signal(&self) -> &Arc<Signal> {
        &self.signal
    }

    pub fn generation(&self) -> u64 {
        self.signal.generation()
    }

    fn lock(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Seq of the oldest entry kept (0 when empty).
    pub fn first_seq(&self) -> u64 {
        self.lock().entries.front().map_or(0, |e| e.seq)
    }

    /// Seq of the newest entry (0 when none).
    pub fn last_seq(&self) -> u64 {
        self.lock().next_seq - 1
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn in_flight(&self) -> usize {
        self.lock().in_flight
    }

    /// The whole ring, oldest first (export).
    pub fn entries(&self) -> Vec<Entry> {
        self.lock().entries.iter().cloned().collect()
    }

    /// Entries newer than `after_seq`, oldest first (FR-6 incremental pull).
    pub fn entries_since(&self, after_seq: u64) -> Vec<Entry> {
        let ring = self.lock();
        let Some(first) = ring.entries.front().map(|e| e.seq) else {
            return Vec::new();
        };
        let skip = (after_seq + 1).saturating_sub(first) as usize;
        ring.entries.iter().skip(skip).cloned().collect()
    }

    /// The current state of `seqs` still kept (in-flight rows a view holds).
    pub fn get(&self, seqs: &[u64]) -> Vec<Entry> {
        let ring = self.lock();
        seqs.iter()
            .filter_map(|seq| ring.index(*seq).map(|ix| ring.entries[ix].clone()))
            .collect()
    }

    /// The statusbar indicator's sample (FR-21).
    pub fn summary(&self, now: Millis) -> NetSummary {
        let ring = self.lock();
        let mut out = NetSummary {
            in_flight: ring.in_flight,
            total: ring.launch.total,
            ..NetSummary::default()
        };
        for e in ring.entries.iter().rev() {
            let recent = e.at > now - UNSEEN_WINDOW_MS;
            if e.status == Status::InFlight {
                out.oldest_in_flight_at = Some(e.at);
            } else if out.last_completed.is_none() {
                out.last_completed = Some(e.clone());
            }
            if recent && e.failed() && e.counts_for_indicator() {
                out.recent_problems.push((e.seq, e.at, e.class()));
            }
            if !recent && out.last_completed.is_some() && ring.in_flight == 0 {
                break;
            }
        }
        out.recent_problems.reverse();
        out
    }

    /// FR-24 totals grouped by `group` over `window`.
    pub fn totals(&self, group: Group, window: Window, now: Millis) -> Totals {
        let ring = self.lock();
        let mut rows: Vec<TotalsRow> = match window {
            Window::SinceLaunch => match group {
                Group::Endpoint => ring
                    .launch
                    .endpoint
                    .iter()
                    .map(|((method, route), (purpose, acc))| {
                        acc.row(TotalsKey::Endpoint {
                            method,
                            route: route.clone(),
                            purpose,
                        })
                    })
                    .collect(),
                Group::Origin => ring
                    .launch
                    .origin
                    .iter()
                    .map(|(o, acc)| acc.row(TotalsKey::Origin(*o)))
                    .collect(),
                Group::Tier => ring
                    .launch
                    .tier
                    .iter()
                    .map(|(t, acc)| acc.row(TotalsKey::Tier(*t)))
                    .collect(),
            },
            Window::LastHour => {
                let since = now - HOUR;
                let mut accs: HashMap<TotalsKey, Acc> = HashMap::new();
                for e in ring.entries.iter() {
                    if e.at <= since || e.status == Status::InFlight {
                        continue;
                    }
                    let key = match group {
                        Group::Endpoint => TotalsKey::Endpoint {
                            method: e.method,
                            route: e.route.clone(),
                            purpose: e.purpose,
                        },
                        Group::Origin => TotalsKey::Origin(e.origin),
                        Group::Tier => match e.tier {
                            Some(t) if e.answered() => TotalsKey::Tier(t),
                            _ => continue,
                        },
                    };
                    accs.entry(key).or_default().add(e);
                }
                // Exact percentiles over the window.
                let mut lat: HashMap<TotalsKey, (Vec<u64>, Vec<u64>)> = HashMap::new();
                for e in ring.entries.iter() {
                    if e.at <= since || e.status == Status::InFlight {
                        continue;
                    }
                    let key = match group {
                        Group::Endpoint => TotalsKey::Endpoint {
                            method: e.method,
                            route: e.route.clone(),
                            purpose: e.purpose,
                        },
                        Group::Origin => TotalsKey::Origin(e.origin),
                        Group::Tier => match e.tier {
                            Some(t) if e.answered() => TotalsKey::Tier(t),
                            _ => continue,
                        },
                    };
                    let (l, q) = lat.entry(key).or_default();
                    l.push(e.elapsed_ms.unwrap_or(0));
                    q.push(e.queued_ms);
                }
                accs.into_iter()
                    .map(|(key, acc)| {
                        let mut row = acc.row(key.clone());
                        if let Some((l, q)) = lat.get_mut(&key) {
                            row.p50_ms = percentile(l, 0.5);
                            row.p95_ms = percentile(l, 0.95);
                            row.p95_queue_ms = percentile(q, 0.95);
                        }
                        row
                    })
                    .collect()
            }
        };
        rows.sort_by(|a, b| b.tstu.total_cmp(&a.tstu).then(b.requests.cmp(&a.requests)));
        let since = now - WINDOW_MS;
        let last_5min_tstu: f64 = ring
            .entries
            .iter()
            .rev()
            .take_while(|e| e.at > since)
            .map(|e| e.booked_tstu)
            .sum();
        Totals {
            requests: rows.iter().map(|r| r.requests).sum(),
            failures: rows.iter().map(|r| r.failures).sum(),
            tstu: rows.iter().map(|r| r.tstu).sum(),
            rows,
            last_5min_tstu,
            share_of_limit_pct: last_5min_tstu / ADO_LIMIT_TSTU * 100.0,
        }
    }

    /// A request went out: an in-flight entry with the next seq.
    fn start(&self, req: &ApiRequest) -> u64 {
        let meta = req.meta();
        let method = req.method().as_str();
        let host = req.url().host_str().unwrap_or("");
        let matched = routes::match_route(method, host, req.url().path());
        debug_assert!(
            matched.known,
            "request outside the route table (spec 0016 FR-19): {method} {}",
            matched.route
        );
        // Stamped by the governor; a request sent around it still inherits
        // the task's attribution.
        let scoped = || crate::attribution::current();
        let pass = meta
            .pass
            .or_else(|| scoped().and_then(|a| a.pass).map(|p| p.pass));
        let origin = meta
            .origin
            .or_else(|| scoped().map(|a| a.origin))
            .unwrap_or_else(|| {
                debug_assert!(
                    false,
                    "unattributed request (spec 0016 FR-18): {method} {}",
                    matched.route
                );
                log::warn!(target: "adoken_sync", "Unattributed request: {method} {}", matched.route);
                Origin::Unattributed
            });
        let at = self.clock.now_ms();
        let mut ring = self.lock();
        let seq = ring.next_seq;
        ring.push(Entry {
            seq,
            at,
            method,
            write: req.method().is_write(),
            host: matched.host,
            route: matched.route,
            purpose: matched.purpose,
            origin,
            pass,
            subject: matched.subject,
            tier: meta.tier,
            attempt: meta.attempt.max(1),
            queued_ms: meta.queued_ms,
            elapsed_ms: None,
            status: Status::InFlight,
            bytes: None,
            rate: RateHeaders::default(),
            error: None,
            optional: meta.optional,
            booked_tstu: 0.0,
        });
        drop(ring);
        self.signal.bump();
        seq
    }

    /// The request `seq` ended.
    fn finish(
        &self,
        seq: u64,
        elapsed_ms: u64,
        cost_hint: Option<f64>,
        result: Option<&Result<ApiResponse, TransportError>>,
    ) {
        let mut ring = self.lock();
        let Some(ix) = ring.index(seq) else {
            return; // Evicted while in flight.
        };
        let e = &mut ring.entries[ix];
        e.elapsed_ms = Some(elapsed_ms);
        match result {
            None => e.status = Status::Canceled,
            Some(Ok(resp)) => {
                e.status = Status::Http(resp.status);
                e.bytes = Some(resp.body.len() as u64);
                e.rate = resp.rate.clone();
                e.booked_tstu = resp.rate.cost.or(cost_hint).unwrap_or(0.0);
                if resp.status >= 400 || resp.status == 203 {
                    e.error = Some(http_error(&resp.body));
                }
            }
            Some(Err(err)) => {
                let (status, kind) = match err {
                    TransportError::Offline(_) => {
                        (Status::Failed { offline: true }, ErrorKind::Offline)
                    }
                    TransportError::Unauthorized => (Status::Http(401), ErrorKind::Unauthorized),
                    TransportError::Throttled { retry_after_ms } => {
                        e.rate.retry_after_ms = Some(*retry_after_ms);
                        (Status::Failed { offline: false }, ErrorKind::Throttled)
                    }
                };
                e.status = status;
                e.bytes = None;
                e.error = Some(ErrorInfo {
                    kind,
                    code: None,
                    message: None,
                });
            }
        }
        let done = e.clone();
        ring.in_flight -= 1;
        ring.launch.add(&done);
        drop(ring);
        if let Some(file) = self.sink.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            // Best effort: a full disk must not break sync.
            let _ = writeln!(file, "{}", done.export_json());
        }
        self.signal.bump();
    }
}

fn percentile(values: &mut [u64], p: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let rank = ((values.len() as f64 * p).ceil() as usize).clamp(1, values.len());
    values[rank - 1]
}

/// Azure DevOps error bodies: `{"message": "TF401019: …", "typeKey": …}`.
fn http_error(body: &[u8]) -> ErrorInfo {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message")?.as_str().map(str::to_owned));
    let code = message.as_deref().and_then(error_code);
    ErrorInfo {
        kind: ErrorKind::Http,
        code,
        message: message.map(|m| scrub_urls(&m)),
    }
}

/// `TF400733` / `VS402335` at the start of a message.
pub(crate) fn error_code(message: &str) -> Option<String> {
    let head: String = message
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let digits = head.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let letters = head.len() - digits.len();
    ((2..=4).contains(&letters) && digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(head)
}

/// Error text never contains a URL (FR-19).
fn scrub_urls(message: &str) -> String {
    message
        .split(' ')
        .map(|w| if w.contains("://") { "[url]" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A [`Transport`] that records every request into the session's
/// [`NetLog`].
pub struct Recording<T> {
    inner: T,
    log: Arc<NetLog>,
}

impl<T: Transport> Recording<T> {
    pub fn new(inner: T, log: Arc<NetLog>) -> Self {
        Recording { inner, log }
    }

    pub fn log(&self) -> &Arc<NetLog> {
        &self.log
    }

    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// The log's entries, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        self.log.entries()
    }
}

/// Completes an entry as Canceled if the request future is dropped.
struct InFlight<'a> {
    log: &'a NetLog,
    seq: u64,
    started: Instant,
    done: bool,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.log.finish(
                self.seq,
                self.started.elapsed().as_millis() as u64,
                None,
                None,
            );
        }
    }
}

impl<T: Transport> Transport for Recording<T> {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            let cost_hint = req.cost_hint();
            let mut guard = InFlight {
                log: &self.log,
                seq: self.log.start(&req),
                started: Instant::now(),
                done: false,
            };
            let result = self.inner.send(req).await;
            guard.done = true;
            self.log.finish(
                guard.seq,
                guard.started.elapsed().as_millis() as u64,
                cost_hint,
                Some(&result),
            );
            result
        })
    }
}

#[cfg(test)]
mod tests;
