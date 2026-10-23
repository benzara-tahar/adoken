//! The Sync tab's copy (spec 0016 FR-7, FR-8, FR-13; activity.md §4): kind
//! rows' state lines, the recent passes list with no-change passes folded
//! into one line (display only), change summaries and change words. Pure:
//! no gpui.

use adoken_model::Millis;
use adoken_sync::activity::{
    ChangeWord, Changes, FailReason, KindChanges, KindRow, Outcome, PassRecord, Progress,
    ProgressUnit, RowKind, RowState, SyncInputs, Trigger,
};
use adoken_sync::cadence::{INDEX_FG, Lane, Lanes};

use crate::settings::sync_prefs as codec;

const SECOND: Millis = 1_000;
const MINUTE: Millis = 60 * SECOND;
const HOUR: Millis = 60 * MINUTE;
const DAY: Millis = 24 * HOUR;

/// "12s ago", "3 min ago", "2 h ago", "4 d ago".
pub fn ago(ms: Millis) -> String {
    match ms.max(0) {
        d if d < MINUTE => format!("{}s ago", d / SECOND),
        d if d < HOUR => format!("{} min ago", d / MINUTE),
        d if d < DAY => format!("{} h ago", d / HOUR),
        d => format!("{} d ago", d / DAY),
    }
}

/// "next in": 10 s steps under a minute ("40s"), whole minutes above
/// ("8 min"); it never ticks every second (activity.md §4.2).
pub fn next_in(ms: Millis) -> String {
    let ms = ms.max(0);
    if ms < MINUTE {
        let tens = ceil_div(ms, 10 * SECOND).max(1) * 10;
        if tens >= 60 {
            "1 min".into()
        } else {
            format!("{tens}s")
        }
    } else {
        format!("{} min", ceil_div(ms, MINUTE))
    }
}

/// A countdown in whole seconds under a minute ("40s"), minutes above.
pub fn countdown(ms: Millis) -> String {
    let ms = ms.max(0);
    if ms < MINUTE {
        format!("{}s", ceil_div(ms, SECOND))
    } else {
        format!("{} min", ceil_div(ms, MINUTE))
    }
}

/// `n / d` rounded up, for `n ≥ 0`.
fn ceil_div(n: Millis, d: Millis) -> Millis {
    (n + d - 1) / d
}

/// Local `HH:MM` at `offset`.
pub fn clock(at: Millis, offset: time::UtcOffset) -> String {
    super::net::time_text(at, offset, false)[..5].to_owned()
}

pub fn kind_title(kind: RowKind) -> &'static str {
    match kind {
        RowKind::MyPrs => "My pull requests",
        RowKind::AllPrs => "All active pull requests",
        RowKind::WorkItems => "Work items",
        RowKind::Pipelines => "Pipeline runs",
        RowKind::Index => "Search index and projects",
        RowKind::Pinned => "Pinned items",
    }
}

/// How a row's state looks: which glyph and tone (activity.md §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// `circle-check`, success.
    Synced,
    /// `loader-circle`, info, spinning.
    Syncing,
    /// `circle-dot-dashed`, tertiary.
    NotSynced,
    /// `circle-dashed`, tertiary.
    Manual,
    /// `pause`, secondary.
    Paused,
    /// `timer`, warning.
    Slowed,
    /// `clock`, warning.
    Throttled,
    /// `circle-x`, danger.
    Failed,
    /// `wifi-off`, tertiary.
    Offline,
    /// `lock`, tertiary (danger when the token is rejected).
    Locked,
    LockedDanger,
    /// `pin`, tertiary.
    Pinned,
}

/// One kind row's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowText {
    pub glyph: Glyph,
    pub title: &'static str,
    /// Trailing on line 1: "1 min", "Manual only", "Paused".
    pub cadence: Option<String>,
    pub line: String,
    /// FR-4: "My pull requests, synced 40 seconds ago, next in 20 seconds".
    pub label: String,
}

fn progress_text(progress: &Progress) -> String {
    let unit = match progress.unit {
        ProgressUnit::PullRequests => "pull requests",
        ProgressUnit::Projects => "projects",
        ProgressUnit::WorkItems => "work items",
        ProgressUnit::Timelines => "timelines",
        ProgressUnit::IndexItems => "items",
    };
    match progress.total {
        Some(total) => format!(
            "{} of {} {unit}",
            super::net::count_text(progress.done as usize),
            super::net::count_text(total as usize)
        ),
        None => format!(
            "{} {unit} so far",
            super::net::count_text(progress.done as usize)
        ),
    }
}

/// "Azure DevOps returned 503", "couldn't reach dev.azure.com", …
pub fn fail_text(reason: &FailReason) -> String {
    match reason {
        FailReason::Status { status, code, .. } => match code {
            Some(code) => format!("Azure DevOps returned {status} ({code})"),
            None => format!("Azure DevOps returned {status}"),
        },
        FailReason::Unreachable { host } => format!("couldn't reach {host}"),
        FailReason::Decode => "unexpected response, adoken may need an update".into(),
        FailReason::Unauthorized => "Azure DevOps refused the token".into(),
        FailReason::Other(text) => text.clone(),
    }
}

/// "Build (Read)" for `vso.build` (pins.md §6.2 naming).
pub fn scope_name(scope: &str) -> String {
    match scope {
        "vso.build" => "Build (Read)".into(),
        "vso.work" => "Work Items (Read)".into(),
        "vso.code" => "Code (Read)".into(),
        "vso.identity" => "Identity (Read)".into(),
        other => other.into(),
    }
}

/// Screen-reader wording: "40s" → "40 seconds", "1 min" → "1 minute",
/// "⌘R" → "Command R", " · " → ", ".
pub(crate) fn spoken(text: &str) -> String {
    let text = text.replace(" · ", ", ").replace("⌘R", "Command R");
    let mut out: Vec<String> = Vec::new();
    for token in text.split(' ') {
        let (word, comma) = match token.strip_suffix(',') {
            Some(word) => (word, ","),
            None => (token, ""),
        };
        let previous_one = out.last().is_some_and(|w| w == "1");
        let spoken = if let Some(n) = word
            .strip_suffix('s')
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            format!("{n} {}", if n == "1" { "second" } else { "seconds" })
        } else if word == "min" {
            (if previous_one { "minute" } else { "minutes" }).to_owned()
        } else if word == "s" {
            (if previous_one { "second" } else { "seconds" }).to_owned()
        } else if word == "h" {
            (if previous_one { "hour" } else { "hours" }).to_owned()
        } else {
            word.to_owned()
        };
        out.push(format!("{spoken}{comma}"));
    }
    out.join(" ")
}

/// The kind row's text at `now` (FR-8 states, activity.md §4.2 copy).
pub fn row_text(
    row: &KindRow,
    inputs: &SyncInputs,
    offset: time::UtcOffset,
    now: Millis,
) -> RowText {
    let title = kind_title(row.kind);
    let synced = row.last_ok.map(|at| format!("synced {}", ago(now - at)));
    let next = row
        .next_due
        .map(|at| format!("next in {}", next_in(at - now)));
    let join =
        |parts: Vec<Option<String>>| parts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let mut cadence = match row.kind {
        RowKind::Index => Some(codec::interval_label(INDEX_FG)),
        RowKind::Pinned => None,
        _ => row.cadence.map(codec::cadence_label),
    };
    let (glyph, line) = if row.kind == RowKind::Pinned && matches!(row.state, RowState::Synced) {
        let pins = &inputs.pins;
        let line = if pins.pins == 0 {
            "Nothing pinned".to_owned()
        } else {
            let mut parts = vec![format!(
                "{} {}",
                pins.pins,
                if pins.pins == 1 { "pin" } else { "pins" }
            )];
            if pins.unavailable > 0 {
                parts.push(format!("{} not available", pins.unavailable));
            }
            parts.push(match pins.last_full_refresh {
                Some(at) => format!(
                    "refreshed with All active pull requests, Work items and Pipeline runs {}",
                    ago(now - at)
                ),
                None => "refreshed with All active pull requests, Work items and Pipeline runs"
                    .to_owned(),
            });
            parts.join(" · ")
        };
        (Glyph::Pinned, line)
    } else {
        match &row.state {
            RowState::TokenRejected => (
                Glyph::LockedDanger,
                "Token rejected · sync stopped".to_owned(),
            ),
            RowState::NeedsScope(scope) => (
                Glyph::Locked,
                format!("Your token doesn't include {}", scope_name(scope)),
            ),
            RowState::Offline => (Glyph::Offline, join(vec![Some("Offline".into()), synced])),
            RowState::WaitingForAdo { until, .. } => (
                Glyph::Throttled,
                format!(
                    "Waiting for Azure DevOps · resuming in {}",
                    countdown(until - now)
                ),
            ),
            RowState::WaitingForBudget { until } => (
                Glyph::Slowed,
                format!(
                    "Waiting for adoken's request budget · resuming in {}",
                    countdown(until - now)
                ),
            ),
            RowState::Syncing(progress) => (
                Glyph::Syncing,
                match progress {
                    Some(p) => format!("Syncing · {}", progress_text(p)),
                    None => "Syncing…".into(),
                },
            ),
            RowState::Failed { reason, retry_at } => (
                Glyph::Failed,
                match retry_at {
                    Some(at) => format!(
                        "Couldn't sync: {} · retrying in {}",
                        fail_text(reason),
                        countdown(at - now)
                    ),
                    None => format!("Couldn't sync: {}", fail_text(reason)),
                },
            ),
            RowState::Paused { until } => {
                cadence = Some("Paused".into());
                let head = match until {
                    Some(at) => format!("Paused until {}", clock(*at, offset)),
                    None => "Paused".into(),
                };
                (Glyph::Paused, join(vec![Some(head), synced]))
            }
            RowState::PausedInBackground => (
                Glyph::Paused,
                "Paused in the background · resumes when adoken is in front".into(),
            ),
            RowState::Slowed {
                remaining_pct,
                resets_at,
            } => (
                Glyph::Slowed,
                join(vec![
                    Some("Slowed to protect your rate limit".into()),
                    remaining_pct.map(|p| format!("Azure DevOps rate limit {}% left", p.round())),
                    resets_at.map(|at| format!("resets in {}", next_in(at - now))),
                    next,
                ]),
            ),
            RowState::ManualOnly => {
                cadence = Some("Manual only".into());
                (
                    Glyph::Manual,
                    join(vec![
                        Some("Manual only".into()),
                        synced,
                        Some("⌘R syncs it".into()),
                    ]),
                )
            }
            RowState::Synced => {
                let mut line = join(vec![
                    row.last_ok.map(|at| format!("Synced {}", ago(now - at))),
                    next,
                ]);
                if !inputs.foreground {
                    line.push_str(" (in the background)");
                }
                (Glyph::Synced, line)
            }
            RowState::NotSyncedYet => (
                Glyph::NotSynced,
                match row.next_due {
                    Some(at) => format!("Not synced yet · first pass in {}", next_in(at - now)),
                    None => "Not synced yet".into(),
                },
            ),
        }
    };
    let label = match &cadence {
        Some(c) if !matches!(c.as_str(), "Paused" | "Manual only") => {
            format!("{title}, {}, every {}", spoken(&line), spoken(c))
        }
        _ => format!("{title}, {}", spoken(&line)),
    };
    RowText {
        glyph,
        title,
        cadence,
        line,
        label,
    }
}

/// The row the keyboard lands on when the tab opens: the first one that
/// isn't Synced, else the first (activity.md §6.3).
pub fn first_attention(rows: &[KindRow]) -> usize {
    rows.iter()
        .position(|r| !matches!(r.state, RowState::Synced) && r.kind != RowKind::Pinned)
        .unwrap_or(0)
}

// ── passes ──────────────────────────────────────────────────────────────

/// A line of the Recent passes list.
#[derive(Clone, Debug, PartialEq)]
pub enum PassLine {
    One(Box<PassRecord>),
    /// Consecutive successful no-change passes of the same lanes (FR-13,
    /// design Q-5): newest first in `ids`.
    Folded {
        ids: Vec<u64>,
        oldest: Millis,
        newest: Millis,
        lanes: Lanes,
    },
}

impl PassLine {
    /// A stable key for expansion state and list identity.
    pub fn key(&self) -> u64 {
        match self {
            PassLine::One(pass) => pass.id,
            PassLine::Folded { ids, .. } => ids[0],
        }
    }
}

pub fn changes_empty(changes: &Changes) -> bool {
    let kind = |k: &KindChanges| k.new == 0 && k.updated == 0 && k.removed == 0;
    kind(&changes.prs)
        && kind(&changes.work_items)
        && kind(&changes.runs)
        && changes.pr_comments == 0
        && changes.wi_comments == 0
        && changes.index_entries == 0
        && changes.projects == 0
        && changes.repos == 0
}

fn foldable(pass: &PassRecord) -> bool {
    matches!(pass.outcome, Outcome::Ok)
        && matches!(pass.trigger, Trigger::Scheduled)
        && pass.events == 0
        && changes_empty(&pass.changes)
}

/// Fold `passes` (newest first) for display. Passes that sent nothing and
/// changed nothing (deferred) aren't listed (FR-11).
pub fn fold(passes: &[PassRecord]) -> Vec<PassLine> {
    let mut lines: Vec<PassLine> = Vec::new();
    for pass in passes {
        if matches!(pass.outcome, Outcome::Deferred) && pass.requests == 0 {
            continue;
        }
        if foldable(pass) {
            match lines.last_mut() {
                Some(PassLine::Folded {
                    ids, oldest, lanes, ..
                }) if *lanes == pass.lanes => {
                    ids.push(pass.id);
                    *oldest = pass.started;
                    continue;
                }
                Some(PassLine::One(prev)) if foldable(prev) && prev.lanes == pass.lanes => {
                    let prev = prev.clone();
                    *lines.last_mut().expect("just matched") = PassLine::Folded {
                        ids: vec![prev.id, pass.id],
                        oldest: pass.started,
                        newest: prev.started,
                        lanes: pass.lanes,
                    };
                    continue;
                }
                _ => {}
            }
        }
        lines.push(PassLine::One(Box::new(pass.clone())));
    }
    lines
}

/// Kind names of `lanes`, joined (both run lanes read "Pipeline runs").
pub fn lanes_text(lanes: Lanes) -> String {
    let mut names: Vec<&str> = Vec::new();
    for lane in lanes.iter() {
        let name = match lane {
            Lane::MyPrs => "My pull requests",
            Lane::AllPrs => "All active pull requests",
            Lane::WorkItems => "Work items",
            Lane::LiveRuns | Lane::DoneRuns => "Pipeline runs",
            Lane::Index => "Search index and projects",
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.join(", ")
}

/// Who started the pass: lanes, or "You: Sync now", "You: Sync #20101",
/// "Demo feed" (activity.md §4.3). `true` when the user started it.
pub fn origin_text(pass: &PassRecord) -> (String, bool) {
    let lanes = lanes_text(pass.lanes);
    match &pass.trigger {
        Trigger::Scheduled => (lanes, false),
        Trigger::CatchUp => (format!("{lanes} · catch-up"), false),
        Trigger::FirstSync => (format!("First sync · {lanes}"), false),
        Trigger::SyncNow => ("You: Sync now".into(), true),
        Trigger::ItemSync(id) => (format!("You: Sync #{id}"), true),
        Trigger::OpenPr(id) => (format!("You: Open !{id}"), true),
        Trigger::Reread(action) => (format!("You: {} · re-read", action.label()), true),
        Trigger::Completing(id) => (format!("You: Complete !{id} · re-read"), true),
        Trigger::LoadOlderRuns => ("You: Load older runs".into(), true),
        Trigger::DemoFeed => ("Demo feed".into(), false),
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// "3 PRs updated · 2 work items changed · 1 run finished · 2
/// notifications"; "No changes".
pub fn summary(changes: &Changes, events: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let prs = &changes.prs;
    if prs.new > 0 {
        parts.push(plural(prs.new, "new PR", "new PRs"));
    }
    if prs.updated > 0 {
        parts.push(format!("{} updated", plural(prs.updated, "PR", "PRs")));
    }
    if prs.removed > 0 {
        parts.push(format!("{} closed", plural(prs.removed, "PR", "PRs")));
    }
    if changes.pr_comments > 0 {
        parts.push(plural(changes.pr_comments, "PR comment", "PR comments"));
    }
    let wi = &changes.work_items;
    if wi.new > 0 {
        parts.push(plural(wi.new, "new work item", "new work items"));
    }
    if wi.updated > 0 {
        parts.push(format!(
            "{} changed",
            plural(wi.updated, "work item", "work items")
        ));
    }
    if wi.removed > 0 {
        parts.push(format!(
            "{} removed",
            plural(wi.removed, "work item", "work items")
        ));
    }
    if changes.wi_comments > 0 {
        parts.push(plural(
            changes.wi_comments,
            "work item comment",
            "work item comments",
        ));
    }
    let runs = &changes.runs;
    if runs.new > 0 {
        parts.push(format!("{} started", plural(runs.new, "run", "runs")));
    }
    if runs.removed > 0 {
        parts.push(format!("{} finished", plural(runs.removed, "run", "runs")));
    }
    if runs.updated > 0 {
        parts.push(format!("{} updated", plural(runs.updated, "run", "runs")));
    }
    if changes.index_entries > 0 {
        parts.push(format!(
            "{} updated",
            plural(changes.index_entries, "search entry", "search entries")
        ));
    }
    if changes.projects > 0 {
        parts.push(format!(
            "{} changed",
            plural(changes.projects, "project", "projects")
        ));
    }
    if changes.repos > 0 {
        parts.push(format!(
            "{} changed",
            plural(changes.repos, "repository", "repositories")
        ));
    }
    if events > 0 {
        parts.push(plural(events as u32, "notification", "notifications"));
    }
    parts
}

/// How a pass row reads besides its time and origin.
#[derive(Clone, Debug, PartialEq)]
pub enum PassOutcome {
    Running { requests: u32 },
    Changes(Vec<String>),
    NoChanges,
    Failed(String),
    Throttled,
    Stopped,
}

pub fn outcome(pass: &PassRecord) -> PassOutcome {
    match &pass.outcome {
        Outcome::Running => PassOutcome::Running {
            requests: pass.requests,
        },
        Outcome::Failed(reason) => PassOutcome::Failed(format!(
            "Failed: {} on {}",
            match reason {
                FailReason::Status { status, .. } => status.to_string(),
                other => fail_text(other),
            },
            lanes_text(pass.lanes)
        )),
        Outcome::Throttled { .. } => PassOutcome::Throttled,
        Outcome::Canceled => PassOutcome::Stopped,
        Outcome::Ok | Outcome::Deferred => {
            let parts = summary(&pass.changes, pass.events);
            if parts.is_empty() {
                PassOutcome::NoChanges
            } else {
                PassOutcome::Changes(parts)
            }
        }
    }
}

/// Which Sync tab actions work now (FR-14, FR-15) and why not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncActions {
    pub sync_now: bool,
    /// Pause and Resume.
    pub pause: bool,
    pub reason: Option<&'static str>,
}

pub fn sync_actions(demo: bool, offline: bool) -> SyncActions {
    if demo {
        SyncActions {
            sync_now: false,
            pause: false,
            reason: Some("Demo org: nothing syncs"),
        }
    } else if offline {
        SyncActions {
            sync_now: false,
            pause: true,
            reason: Some("Offline — reconnect to sync"),
        }
    } else {
        SyncActions {
            sync_now: true,
            pause: true,
            reason: None,
        }
    }
}

/// "9 req · 1.1 s · 0.004".
pub fn meta_text(pass: &PassRecord) -> String {
    let wall = pass
        .ended
        .map(|end| super::net::latency_text((end - pass.started).max(0) as u64));
    let mut parts = vec![format!("{} req", pass.requests)];
    parts.extend(wall);
    parts.push(format!("{:.3}", pass.tstu));
    parts.join(" · ")
}

/// The change word (activity.md §4.4).
pub fn change_word(word: &ChangeWord) -> String {
    match word {
        ChangeWord::New => "New".into(),
        ChangeWord::Updated => "Updated".into(),
        ChangeWord::NewComment => "New comment".into(),
        ChangeWord::VoteChanged => "Vote changed".into(),
        ChangeWord::Pushed => "Pushed".into(),
        ChangeWord::Completed => "Completed".into(),
        ChangeWord::Abandoned => "Abandoned".into(),
        ChangeWord::State(state) => format!("State: {state}"),
        ChangeWord::AssignedToYou => "Assigned to you".into(),
        ChangeWord::Removed => "No longer synced".into(),
        ChangeWord::Started => "Started".into(),
        ChangeWord::Finished(result) => match result {
            adoken_model::RunResult::Succeeded => "Succeeded".into(),
            adoken_model::RunResult::PartiallySucceeded => "Partially succeeded".into(),
            adoken_model::RunResult::Failed => "Failed".into(),
            adoken_model::RunResult::Canceled => "Canceled".into(),
        },
    }
}

/// An expanded pass's changed items, grouped by kind in fixed order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Pr,
    WorkItem,
    Run,
}

pub fn items(changes: &Changes) -> Vec<(ItemKind, &KindChanges)> {
    [
        (ItemKind::Pr, &changes.prs),
        (ItemKind::WorkItem, &changes.work_items),
        (ItemKind::Run, &changes.runs),
    ]
    .into_iter()
    .filter(|(_, k)| !k.ids.is_empty())
    .collect()
}

pub fn group_title(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Pr => "Pull requests",
        ItemKind::WorkItem => "Work items",
        ItemKind::Run => "Pipeline runs",
    }
}

/// `!4821`, `#20101`, run `#…` (by id until its number is read).
pub fn item_ref(kind: ItemKind, id: i64) -> String {
    match kind {
        ItemKind::Pr => format!("!{id}"),
        ItemKind::WorkItem | ItemKind::Run => format!("#{id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::fixtures;

    #[test]
    fn consecutive_no_change_passes_fold_into_one_line() {
        let lines = fold(&fixtures::passes(fixtures::NOW));
        // changes · 4 folded · You: Sync #id · failed
        assert_eq!(lines.len(), 4, "{lines:#?}");
        match &lines[1] {
            PassLine::Folded {
                ids,
                oldest,
                newest,
                ..
            } => {
                assert_eq!(ids.len(), 4);
                assert!(oldest < newest);
            }
            other => panic!("expected a folded line, got {other:?}"),
        }
        assert!(
            matches!(&lines[2], PassLine::One(p) if origin_text(p).0.starts_with("You: Sync #"))
        );
        assert!(matches!(
            outcome(match &lines[3] {
                PassLine::One(p) => p,
                _ => unreachable!(),
            }),
            PassOutcome::Failed(_)
        ));
    }

    #[test]
    fn a_pass_with_changes_or_another_trigger_breaks_the_fold() {
        let passes = fixtures::passes(fixtures::NOW);
        let mut quiet = passes[1].clone();
        quiet.trigger = Trigger::SyncNow;
        let list = vec![passes[1].clone(), quiet, passes[2].clone()];
        let lines = fold(&list);
        assert_eq!(lines.len(), 3, "a ⌘R pass is its own row");
    }

    #[test]
    fn demo_disables_sync_now_and_pause_with_the_reason() {
        // AC-A11 / FR-15.
        let demo = sync_actions(true, false);
        assert!(!demo.sync_now && !demo.pause);
        assert_eq!(demo.reason, Some("Demo org: nothing syncs"));
        let offline = sync_actions(false, true);
        assert!(!offline.sync_now && offline.pause);
        assert_eq!(sync_actions(false, false).reason, None);
        // A demo-feed tick reads "Demo feed" with what it added.
        let mut tick = fixtures::passes(fixtures::NOW)[1].clone();
        tick.trigger = Trigger::DemoFeed;
        tick.events = 2;
        assert_eq!(origin_text(&tick), ("Demo feed".to_owned(), false));
        assert_eq!(
            outcome(&tick),
            PassOutcome::Changes(vec!["2 notifications".into()])
        );
    }

    #[test]
    fn summaries_count_and_pluralize() {
        let pass = &fixtures::passes(fixtures::NOW)[0];
        assert_eq!(
            summary(&pass.changes, 2),
            [
                "3 PRs updated",
                "2 work items changed",
                "1 run finished",
                "2 notifications"
            ]
        );
        assert!(summary(&Changes::default(), 0).is_empty());
    }

    #[test]
    fn next_in_counts_in_ten_second_steps_then_minutes() {
        assert_eq!(next_in(3_000), "10s");
        assert_eq!(next_in(38_000), "40s");
        assert_eq!(next_in(59_000), "1 min");
        assert_eq!(next_in(8 * MINUTE - 1), "8 min");
        assert_eq!(countdown(39_100), "40s");
    }

    #[test]
    fn row_lines_follow_the_state() {
        let now = fixtures::NOW;
        let offset = time::UtcOffset::UTC;
        let inputs = fixtures::inputs(fixtures::SyncScene::Failed, now);
        let rows = adoken_sync::activity::rows(&inputs, now);
        let work = rows.iter().find(|r| r.kind == RowKind::WorkItems).unwrap();
        let text = row_text(work, &inputs, offset, now);
        assert_eq!(text.glyph, Glyph::Failed);
        assert_eq!(
            text.line,
            "Couldn't sync: Azure DevOps returned 503 (TF400733) · retrying in 30s"
        );
        let mine = rows.iter().find(|r| r.kind == RowKind::MyPrs).unwrap();
        let text = row_text(mine, &inputs, offset, now);
        assert_eq!(text.glyph, Glyph::Synced);
        assert_eq!(text.line, "Synced 12s ago · next in 40s");
        assert_eq!(
            text.label,
            "My pull requests, Synced 12 seconds ago, next in 40 seconds, every 1 minute"
        );
        assert_eq!(
            first_attention(&rows),
            2,
            "keyboard lands on the failing kind"
        );
        // Pins refresh inside the failing warm pass: the row shares its
        // failure; once every pinned step ran, it counts pins.
        let pinned = rows.iter().find(|r| r.kind == RowKind::Pinned).unwrap();
        assert_eq!(row_text(pinned, &inputs, offset, now).glyph, Glyph::Failed);
        let idle = fixtures::inputs(fixtures::SyncScene::Idle, now);
        let rows = adoken_sync::activity::rows(&idle, now);
        let pinned = rows.iter().find(|r| r.kind == RowKind::Pinned).unwrap();
        let text = row_text(pinned, &idle, offset, now);
        assert_eq!(text.glyph, Glyph::Pinned);
        assert!(
            text.line.starts_with("4 pins · 1 not available"),
            "{}",
            text.line
        );
    }
}
