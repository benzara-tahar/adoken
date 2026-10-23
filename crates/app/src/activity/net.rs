//! The Network tab's view state over the session's [`NetLog`] (spec 0016
//! FR-22, FR-23, FR-24; activity.md §5): the entries this view holds
//! (pulled incrementally by sequence number, FR-6), filters, the live tail
//! (follow / "N new"), the cursor and the open detail, plus the copy the
//! table shows. Pure: no gpui.

use std::collections::VecDeque;

use adoken_model::Millis;
use adoken_sync::attribution::{Origin, OriginGroup};
use adoken_sync::ledger::{Entry, NetLog, Status, StatusClass, TotalsKey, TotalsRow};

/// Entries the view keeps (the ring's size, FR-26).
pub const CAPACITY: usize = 10_000;

/// One held entry plus its lowercase search text (purpose, route, method,
/// status, origin), built once so a filter keystroke over 10,000 rows
/// stays inside one frame.
#[derive(Clone, Debug)]
pub struct Held {
    pub entry: Entry,
    hay: String,
}

impl Held {
    fn new(entry: Entry) -> Self {
        let hay = format!(
            "{} {} {} {} {}",
            entry.purpose,
            entry.route,
            entry.method,
            status_text(&entry.status),
            entry.origin.label()
        )
        .to_lowercase();
        Self { entry, hay }
    }
}

/// An origin filter choice: a whole group ("You") or one origin by its
/// label ("Work items", "You: vote").
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginChoice {
    Group(OriginGroup),
    Label(String),
}

impl OriginChoice {
    pub fn label(&self) -> String {
        match self {
            OriginChoice::Group(group) => group_label(*group).to_owned(),
            OriginChoice::Label(label) => label.clone(),
        }
    }

    fn matches(&self, origin: &Origin) -> bool {
        match self {
            OriginChoice::Group(group) => origin.group() == *group,
            OriginChoice::Label(label) => origin.label() == *label,
        }
    }
}

pub fn group_label(group: OriginGroup) -> &'static str {
    group.label()
}

/// FR-23 filters; they combine with AND, choices within one filter with OR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetFilter {
    /// Danger + warning (FR-23).
    pub failed_only: bool,
    pub origins: Vec<OriginChoice>,
    /// `(method, route)` endpoints (Totals › Show requests).
    pub endpoints: Vec<(String, String)>,
    pub class: Option<StatusClass>,
    pub text: String,
}

impl NetFilter {
    pub fn is_active(&self) -> bool {
        *self != NetFilter::default()
    }

    pub fn matches(&self, held: &Held) -> bool {
        let entry = &held.entry;
        let class = entry.class();
        if self.failed_only && !matches!(class, StatusClass::Warning | StatusClass::Danger) {
            return false;
        }
        if self.class.is_some_and(|c| c != class) {
            return false;
        }
        if !self.origins.is_empty() && !self.origins.iter().any(|o| o.matches(&entry.origin)) {
            return false;
        }
        if !self.endpoints.is_empty()
            && !self
                .endpoints
                .iter()
                .any(|(method, route)| method == entry.method && *route == entry.route)
        {
            return false;
        }
        self.text
            .split_whitespace()
            .all(|token| held.hay.contains(&token.to_lowercase()))
    }
}

/// What an update did to the visible rows (the view scrolls with it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ingest {
    /// Matching entries appended at the tail.
    pub appended: usize,
    /// Visible rows dropped from the top by ring eviction.
    pub evicted: usize,
    /// The cursor's entry was evicted ("Older requests were dropped").
    pub cursor_evicted: bool,
}

/// Changes to the visible rows since the table last synced its list
/// state: evicted from the top, appended at the bottom, rows re-read in
/// place, or a reset (filter change, a row joining the middle).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableDelta {
    pub reset: bool,
    pub evicted: usize,
    pub appended: usize,
    pub changed: bool,
}

/// The Network tab's rows and cursor.
#[derive(Clone, Debug)]
pub struct NetView {
    held: VecDeque<Held>,
    /// Held entries still in flight, re-read on each update.
    in_flight: Vec<u64>,
    filter: NetFilter,
    /// Sequence numbers of the rows passing the filter, ascending.
    visible: Vec<u64>,
    /// Following the tail: new rows keep the viewport at the bottom.
    pub follow: bool,
    pub cursor: Option<u64>,
    /// Matching rows that arrived while not following.
    pub new_count: usize,
    /// The row whose detail is open.
    pub detail: Option<u64>,
    /// What changed in `visible` since the table last looked.
    delta: TableDelta,
}

impl NetView {
    /// Hand the accumulated [`TableDelta`] to the table.
    pub fn take_delta(&mut self) -> TableDelta {
        std::mem::take(&mut self.delta)
    }
}

impl Default for NetView {
    fn default() -> Self {
        Self {
            held: VecDeque::new(),
            in_flight: Vec::new(),
            filter: NetFilter::default(),
            visible: Vec::new(),
            follow: true,
            cursor: None,
            new_count: 0,
            detail: None,
            delta: TableDelta::default(),
        }
    }
}

impl NetView {
    /// Pull what changed in `log` since the last update: new entries by
    /// sequence number and the completions of held in-flight ones. Never a
    /// full copy (FR-6).
    pub fn update(&mut self, log: &NetLog) -> Ingest {
        let after = self.held.back().map_or(0, |h| h.entry.seq);
        let appended = log.entries_since(after);
        let completed = if self.in_flight.is_empty() {
            Vec::new()
        } else {
            log.get(&self.in_flight)
        };
        self.apply(appended, completed, log.first_seq())
    }

    /// [`Self::update`] with the pulled data (tests feed it directly).
    pub fn apply(&mut self, appended: Vec<Entry>, completed: Vec<Entry>, first_seq: u64) -> Ingest {
        let mut ingest = Ingest::default();
        // Completions first: they replace rows already held.
        for entry in completed {
            let Some(ix) = self.index_of(entry.seq) else {
                continue;
            };
            let seq = entry.seq;
            if !matches!(entry.status, Status::InFlight) {
                self.in_flight.retain(|s| *s != seq);
            }
            let before = self.filter.matches(&self.held[ix]);
            self.held[ix] = Held::new(entry);
            let after = self.filter.matches(&self.held[ix]);
            if before != after {
                // A row joined or left the middle: the table re-lays out.
                self.delta.reset = true;
                match self.visible.binary_search(&seq) {
                    Ok(pos) if !after => {
                        self.visible.remove(pos);
                    }
                    Err(pos) if after => self.visible.insert(pos, seq),
                    _ => {}
                }
            } else if after {
                self.delta.changed = true;
            }
        }
        for entry in appended {
            if self.held.back().is_some_and(|h| h.entry.seq >= entry.seq) {
                continue;
            }
            if matches!(entry.status, Status::InFlight) {
                self.in_flight.push(entry.seq);
            }
            let held = Held::new(entry);
            if self.filter.matches(&held) {
                self.visible.push(held.entry.seq);
                ingest.appended += 1;
                self.delta.appended += 1;
            }
            self.held.push_back(held);
        }
        // Eviction: the ring dropped everything below `first_seq`; keep at
        // most the ring's size either way.
        let floor = self.held.back().map_or(first_seq, |h| {
            first_seq.max((h.entry.seq + 1).saturating_sub(CAPACITY as u64))
        });
        while self.held.front().is_some_and(|h| h.entry.seq < floor) {
            self.held.pop_front();
        }
        self.in_flight.retain(|s| *s >= floor);
        let evicted = self.visible.partition_point(|s| *s < floor);
        if evicted > 0 {
            self.visible.drain(..evicted);
            ingest.evicted = evicted;
            self.delta.evicted += evicted;
        }
        if self.cursor.is_some_and(|c| c < floor) {
            self.cursor = self.visible.first().copied();
            ingest.cursor_evicted = true;
        }
        if self.detail.is_some_and(|d| d < floor) {
            self.detail = None;
        }
        if self.follow {
            self.new_count = 0;
        } else {
            self.new_count += ingest.appended;
        }
        ingest
    }

    fn index_of(&self, seq: u64) -> Option<usize> {
        let first = self.held.front()?.entry.seq;
        let ix = usize::try_from(seq.checked_sub(first)?).ok()?;
        (self.held.get(ix)?.entry.seq == seq).then_some(ix)
    }

    pub fn entry(&self, seq: u64) -> Option<&Entry> {
        self.index_of(seq).map(|ix| &self.held[ix].entry)
    }

    /// Visible rows, oldest first (the table's order: newest at the bottom).
    pub fn visible(&self) -> &[u64] {
        &self.visible
    }

    pub fn row(&self, ix: usize) -> Option<&Entry> {
        self.visible.get(ix).and_then(|seq| self.entry(*seq))
    }

    /// Every held entry, oldest first.
    pub fn held(&self) -> impl DoubleEndedIterator<Item = &Entry> {
        self.held.iter().map(|h| &h.entry)
    }

    pub fn held_len(&self) -> usize {
        self.held.len()
    }

    /// Entries the export writes: the filtered rows when a filter is on,
    /// else everything kept (FR-25).
    pub fn export_rows(&self) -> Vec<Entry> {
        if self.filter.is_active() {
            self.visible
                .iter()
                .filter_map(|seq| self.entry(*seq).cloned())
                .collect()
        } else {
            self.held().cloned().collect()
        }
    }

    pub fn oldest_at(&self) -> Option<Millis> {
        self.held.front().map(|h| h.entry.at)
    }

    pub fn filter(&self) -> &NetFilter {
        &self.filter
    }

    /// Replace the filter and recompute the visible rows. The cursor stays
    /// on its entry when it still matches, else the nearest older row.
    pub fn set_filter(&mut self, filter: NetFilter) {
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        self.delta.reset = true;
        self.visible = self
            .held
            .iter()
            .filter(|h| self.filter.matches(h))
            .map(|h| h.entry.seq)
            .collect();
        if let Some(cursor) = self.cursor {
            let pos = self.visible.partition_point(|s| *s <= cursor);
            self.cursor = pos
                .checked_sub(1)
                .map(|p| self.visible[p])
                .or_else(|| self.visible.first().copied());
        }
        if self
            .detail
            .is_some_and(|d| self.visible.binary_search(&d).is_err())
        {
            self.detail = None;
        }
        if self.follow {
            self.new_count = 0;
        }
    }

    pub fn update_filter(&mut self, change: impl FnOnce(&mut NetFilter)) {
        let mut filter = self.filter.clone();
        change(&mut filter);
        self.set_filter(filter);
    }

    pub fn cursor_index(&self) -> Option<usize> {
        self.cursor
            .and_then(|seq| self.visible.binary_search(&seq).ok())
    }

    /// Move the cursor by `delta` rows; leaving the last row stops
    /// following, landing on it resumes (FR-22).
    pub fn move_cursor(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        let current = self.cursor_index().unwrap_or(last);
        let next = current.saturating_add_signed(delta).min(last);
        self.cursor = Some(self.visible[next]);
        self.follow = next == last;
        if self.follow {
            self.new_count = 0;
        }
    }

    pub fn home(&mut self) {
        if let Some(first) = self.visible.first() {
            self.cursor = Some(*first);
            self.follow = self.visible.len() == 1;
        }
    }

    /// `End` / "Jump to latest": the newest row, following again.
    pub fn jump_to_latest(&mut self) {
        self.cursor = self.visible.last().copied();
        self.follow = true;
        self.new_count = 0;
    }

    /// The user scrolled the table: following iff they're at the bottom
    /// with the cursor on the last row (or none).
    pub fn scrolled(&mut self, at_end: bool) {
        let on_last = self.cursor.is_none() || self.cursor == self.visible.last().copied();
        self.follow = at_end && on_last;
        if self.follow {
            self.new_count = 0;
        }
    }

    pub fn toggle_detail(&mut self) {
        self.detail = match (self.detail, self.cursor) {
            (Some(open), Some(cursor)) if open == cursor => None,
            (_, cursor) => cursor,
        };
    }
}

/// Sort keys of the Totals table (activity.md §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TotalsSort {
    Endpoint,
    Requests,
    Failed,
    P95,
    Bytes,
    #[default]
    Tstu,
}

/// Sort `rows` by `sort`, largest first (Endpoint: A→Z); `ascending` flips.
pub fn sort_totals(rows: &mut [TotalsRow], sort: TotalsSort, ascending: bool) {
    rows.sort_by(|a, b| {
        let order = match sort {
            TotalsSort::Endpoint => totals_key_text(&b.key).cmp(&totals_key_text(&a.key)),
            TotalsSort::Requests => a.requests.cmp(&b.requests),
            TotalsSort::Failed => a.failures.cmp(&b.failures),
            TotalsSort::P95 => a.p95_ms.cmp(&b.p95_ms),
            TotalsSort::Bytes => a.bytes.cmp(&b.bytes),
            TotalsSort::Tstu => a.tstu.total_cmp(&b.tstu),
        };
        if ascending { order } else { order.reverse() }
    });
}

/// The Totals row's endpoint/origin/tier text.
pub fn totals_key_text(key: &TotalsKey) -> String {
    match key {
        TotalsKey::Endpoint { route, .. } => route.clone(),
        TotalsKey::Origin(origin) => origin.label(),
        TotalsKey::Tier(tier) => tier_label(*tier).to_owned(),
    }
}

pub fn tier_label(tier: adoken_sync::Tier) -> &'static str {
    match tier {
        adoken_sync::Tier::User => "You (user priority)",
        adoken_sync::Tier::Hot => "Hot (my PRs, live runs)",
        adoken_sync::Tier::Warm => "Warm (lists, work items)",
        adoken_sync::Tier::Cold => "Cold (search index)",
    }
}

// ── copy ────────────────────────────────────────────────────────────────

/// The status column's text: "200", "Failed", "Offline", "Canceled", "…".
pub fn status_text(status: &Status) -> String {
    match status {
        Status::InFlight => "…".into(),
        Status::Http(code) => code.to_string(),
        Status::Failed { offline: true } => "Offline".into(),
        Status::Failed { offline: false } => "Failed".into(),
        Status::Canceled => "Canceled".into(),
    }
}

/// How a status pill looks (activity.md §5.3): success is plain, anything
/// else carries a glyph (and a wash, by tone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pill {
    /// 2xx, 304, other 3xx: the code in `text.secondary`, no wash.
    Plain,
    Canceled,
    InFlight,
    /// 429, or `Retry-After` / `X-RateLimit-Delay` on any code.
    Delayed,
    /// Other 4xx.
    Warning,
    /// 401, 403.
    Auth,
    /// 5xx, 203 sign-in page, no response.
    Danger,
    /// No response because the Mac was offline.
    Offline,
}

pub fn pill(entry: &Entry) -> Pill {
    match entry.status {
        Status::InFlight => Pill::InFlight,
        Status::Canceled => Pill::Canceled,
        Status::Failed { offline: true } => Pill::Offline,
        Status::Failed { offline: false } => Pill::Danger,
        Status::Http(401 | 403) => Pill::Auth,
        Status::Http(code) if code >= 500 || code == 203 => Pill::Danger,
        Status::Http(429) => Pill::Delayed,
        _ if entry.delayed() => Pill::Delayed,
        Status::Http(code) if code >= 400 => Pill::Warning,
        Status::Http(_) => Pill::Plain,
    }
}

/// "84 ms", "1.2 s", "12 s".
pub fn latency_text(ms: u64) -> String {
    match ms {
        ms if ms < 1000 => format!("{ms} ms"),
        ms if ms < 10_000 => format!("{:.1} s", ms as f64 / 1000.0),
        ms => format!("{} s", ms / 1000),
    }
}

/// "0 B", "512 B", "12.3 KB", "1.2 MB"; "—" without a response.
pub fn size_text(bytes: Option<u64>) -> String {
    match bytes {
        None => "—".into(),
        Some(b) if b < 1024 => format!("{b} B"),
        Some(b) if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        Some(b) => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

/// "0.004"; "—" when Azure DevOps sent no `X-RateLimit-Cost`.
pub fn tstu_text(cost: Option<f64>) -> String {
    match cost {
        Some(cost) => format!("{cost:.3}"),
        None => "—".into(),
    }
}

/// "1,284".
pub fn count_text(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Local wall-clock `HH:MM:SS.mmm` at `offset`.
pub fn time_text(at: Millis, offset: time::UtcOffset, millis: bool) -> String {
    let Ok(t) = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1_000_000) else {
        return String::new();
    };
    let t = t.to_offset(offset);
    if millis {
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            t.hour(),
            t.minute(),
            t.second(),
            t.millisecond()
        )
    } else {
        format!("{:02}:{:02}:{:02}", t.hour(), t.minute(), t.second())
    }
}

/// The row's spoken label (FR-4): "14:02:11, GET, 200, PR threads, My pull
/// requests, 182 milliseconds, 4 kilobytes, 0.004 TSTU".
pub fn row_label(entry: &Entry, offset: time::UtcOffset) -> String {
    let status = match entry.status {
        Status::InFlight => "in flight".to_owned(),
        Status::Canceled => "canceled".to_owned(),
        Status::Failed { offline: true } => "failed: offline".to_owned(),
        Status::Failed { offline: false } => "failed with no response".to_owned(),
        Status::Http(code) if matches!(entry.class(), StatusClass::Danger) => {
            format!("failed with {code}")
        }
        Status::Http(429) => "throttled, 429".to_owned(),
        Status::Http(code) => code.to_string(),
    };
    let mut parts = vec![
        time_text(entry.at, offset, false),
        entry.method.to_owned(),
        status,
        entry.purpose.to_owned(),
        entry.origin.label(),
    ];
    if let Some(ms) = entry.elapsed_ms {
        parts.push(format!("{ms} milliseconds"));
    }
    if let Some(bytes) = entry.bytes {
        parts.push(format!("{} kilobytes", bytes.div_ceil(1024)));
    }
    if let Some(cost) = entry.rate.cost {
        parts.push(format!("{cost:.3} TSTU"));
    }
    parts.join(", ")
}

/// Copy details (activity.md §5.4): `key: value` lines, error code only,
/// never the subject or Azure DevOps' message.
pub fn details_text(entry: &Entry, offset: time::UtcOffset) -> String {
    let mut lines = vec![
        format!("Started: {}", time_text(entry.at, offset, true)),
        format!("Method: {}", entry.method),
        format!("Host: {}", entry.host),
        format!("Path: {}", entry.route),
        format!("Purpose: {}", entry.purpose),
        format!("Origin: {}", entry.origin.label()),
        format!("Attempt: {}", entry.attempt),
        format!("Status: {}", status_text(&entry.status)),
        format!("Queued: {}", latency_text(entry.queued_ms)),
    ];
    if let Some(ms) = entry.elapsed_ms {
        lines.push(format!("Elapsed: {}", latency_text(ms)));
    }
    lines.push(format!("Size: {}", size_text(entry.bytes)));
    lines.push(format!("Cost: {}", tstu_text(entry.rate.cost)));
    if let Some(error) = &entry.error {
        let code = error
            .code
            .as_deref()
            .map(|c| format!(" {c}"))
            .unwrap_or_default();
        lines.push(format!("Error: {:?}{code}", error.kind));
    }
    if let Some(id) = &entry.rate.activity_id {
        lines.push(format!("Activity id: {id}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::fixtures;

    fn view_of(entries: Vec<Entry>) -> NetView {
        let mut view = NetView::default();
        let first = entries.first().map_or(1, |e| e.seq);
        view.apply(entries, Vec::new(), first);
        view
    }

    #[test]
    fn failed_only_and_origin_intersect_on_ten_thousand_entries() {
        // AC-B9: 37 failures among 10,000.
        let entries = fixtures::ledger(10_000, 37, fixtures::NOW);
        let failures = entries
            .iter()
            .filter(|e| matches!(e.class(), StatusClass::Warning | StatusClass::Danger))
            .count();
        assert_eq!(failures, 37);
        let mut view = view_of(entries);
        assert_eq!(view.visible().len(), 10_000);
        view.update_filter(|f| f.failed_only = true);
        assert_eq!(view.visible().len(), 37);
        let work_items_failed = view
            .visible()
            .iter()
            .filter(|s| view.entry(**s).unwrap().origin.label() == "Work items")
            .count();
        assert!(work_items_failed > 0 && work_items_failed < 37);
        view.update_filter(|f| f.origins = vec![OriginChoice::Label("Work items".into())]);
        assert_eq!(view.visible().len(), work_items_failed, "intersection");
        view.update_filter(|f| *f = NetFilter::default());
        assert_eq!(view.visible().len(), 10_000, "⌘⌫ clears");
    }

    #[test]
    fn matching_entries_join_the_tail_and_others_do_not() {
        let entries = fixtures::ledger(100, 5, fixtures::NOW);
        let next = entries.last().unwrap().seq + 1;
        let mut view = view_of(entries);
        view.update_filter(|f| f.failed_only = true);
        let before = view.visible().len();
        let ok = fixtures::entry(
            next,
            fixtures::NOW,
            Status::Http(200),
            fixtures::lane_origin(),
        );
        let failed = fixtures::entry(
            next + 1,
            fixtures::NOW,
            Status::Http(503),
            fixtures::lane_origin(),
        );
        let ingest = view.apply(vec![ok, failed], Vec::new(), 1);
        assert_eq!(ingest.appended, 1);
        assert_eq!(view.visible().len(), before + 1);
        assert_eq!(view.visible().last(), Some(&(next + 1)));
        assert_eq!(view.new_count, 0, "following: no pill");
    }

    #[test]
    fn scrolled_up_new_rows_count_and_leave_the_cursor() {
        let entries = fixtures::ledger(50, 0, fixtures::NOW);
        let mut view = view_of(entries);
        view.jump_to_latest();
        view.move_cursor(-10);
        assert!(!view.follow, "leaving the last row stops following");
        let cursor = view.cursor;
        let more: Vec<Entry> = (51..=62)
            .map(|seq| {
                fixtures::entry(
                    seq,
                    fixtures::NOW,
                    Status::Http(200),
                    fixtures::lane_origin(),
                )
            })
            .collect();
        view.apply(more, Vec::new(), 1);
        assert_eq!(view.new_count, 12, "\"12 new\"");
        assert_eq!(view.cursor, cursor, "the cursor stays on its entry");
        view.jump_to_latest();
        assert!(view.follow && view.new_count == 0);
        assert_eq!(view.cursor, Some(62));
    }

    #[test]
    fn completion_moves_an_in_flight_row_into_failed_only() {
        let mut view = NetView::default();
        let flying = fixtures::entry(1, fixtures::NOW, Status::InFlight, fixtures::lane_origin());
        view.apply(vec![flying.clone()], Vec::new(), 1);
        view.update_filter(|f| f.failed_only = true);
        assert!(view.visible().is_empty());
        let done = Entry {
            status: Status::Http(503),
            elapsed_ms: Some(1200),
            ..flying
        };
        view.apply(Vec::new(), vec![done], 1);
        assert_eq!(view.visible(), &[1]);
    }

    #[test]
    fn eviction_drops_from_the_top_and_moves_an_evicted_cursor() {
        let entries = fixtures::ledger(20, 0, fixtures::NOW);
        let mut view = view_of(entries);
        view.home();
        let ingest = view.apply(Vec::new(), Vec::new(), 6);
        assert_eq!(ingest.evicted, 5);
        assert!(ingest.cursor_evicted);
        assert_eq!(view.cursor, Some(6), "oldest remaining row");
        assert_eq!(view.visible().len(), 15);
    }

    #[test]
    fn text_filter_matches_every_token_case_insensitively() {
        let mut view = view_of(fixtures::ledger(200, 10, fixtures::NOW));
        view.update_filter(|f| f.text = "THREADS 503".into());
        assert!(view.visible().iter().all(|s| {
            let e = view.entry(*s).unwrap();
            e.purpose.to_lowercase().contains("threads") && e.status == Status::Http(503)
        }));
    }

    #[test]
    fn pills_follow_the_status_classes() {
        let e = |status| fixtures::entry(1, 0, status, fixtures::lane_origin());
        assert_eq!(pill(&e(Status::Http(200))), Pill::Plain);
        assert_eq!(pill(&e(Status::Http(304))), Pill::Plain);
        assert_eq!(pill(&e(Status::Http(404))), Pill::Warning);
        assert_eq!(pill(&e(Status::Http(429))), Pill::Delayed);
        assert_eq!(pill(&e(Status::Http(401))), Pill::Auth);
        assert_eq!(pill(&e(Status::Http(503))), Pill::Danger);
        assert_eq!(pill(&e(Status::Failed { offline: true })), Pill::Offline);
        assert_eq!(pill(&e(Status::Canceled)), Pill::Canceled);
        assert_eq!(pill(&e(Status::InFlight)), Pill::InFlight);
    }

    #[test]
    fn numbers_read_as_the_design_shows() {
        assert_eq!(latency_text(84), "84 ms");
        assert_eq!(latency_text(1234), "1.2 s");
        assert_eq!(latency_text(12_400), "12 s");
        assert_eq!(size_text(Some(12_595)), "12.3 KB");
        assert_eq!(size_text(None), "—");
        assert_eq!(count_text(1284), "1,284");
        assert_eq!(count_text(10_000), "10,000");
        assert_eq!(tstu_text(Some(0.0041)), "0.004");
    }
}
