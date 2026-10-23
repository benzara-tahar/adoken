//! Pure inbox view-model: row text, empty-state copy, and selection moves.
//! No gpui here so it's unit-testable.

use adoken_model::{Millis, RunResult, Subject, SyncEventKind};
use adoken_notify::{
    Clock, Engine, Filter, Kind, Notification, Priority, Source, StateFilter, clock_label,
};

use crate::palette::catalog::{Target, relative_time};

/// Everything one row paints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowText {
    pub title: String,
    pub subtitle: String,
    pub count: Option<String>,
    pub time: String,
    pub unread: bool,
    pub snoozed_badge: bool,
    pub priority: Priority,
    pub glyph: Glyph,
    /// Screen-reader label: the whole row spelled out.
    pub label: String,
}

/// Leading icon choice (mapped to an `IconName` + token by the view).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    PullRequest,
    WorkItem,
    BuildFailed,
    BuildPassed,
    Mention,
    Nudge,
}

pub fn glyph(n: &Notification) -> Glyph {
    match n.source {
        Source::PullRequests => Glyph::PullRequest,
        Source::WorkItems => Glyph::WorkItem,
        Source::Mentions => Glyph::Mention,
        Source::Nudges => Glyph::Nudge,
        Source::Pipelines => match n.ctx.build_result {
            Some(RunResult::Failed) => Glyph::BuildFailed,
            _ => Glyph::BuildPassed,
        },
    }
}

pub fn row_text(n: &Notification, now: Millis) -> RowText {
    let count = n.count_label();
    let count = (!count.is_empty()).then_some(count);
    let time = relative_time(n.updated_at, now);
    let mut label = format!(
        "{}, {}, {}, {} priority",
        n.ctx.title,
        n.summary,
        if n.read { "read" } else { "unread" },
        n.priority.description().to_lowercase()
    );
    if let Some(count) = &count {
        label.push_str(&format!(", {count}"));
    }
    if n.snoozed_badge {
        label.push_str(", back from snooze");
    }
    RowText {
        title: if n.ctx.title.is_empty() {
            n.subject.key()
        } else {
            n.ctx.title.clone()
        },
        subtitle: format!("{} · {}", n.source.label(), n.summary),
        count,
        time,
        unread: !n.read,
        snoozed_badge: n.snoozed_badge,
        priority: n.priority,
        glyph: glyph(n),
        label,
    }
}

/// The account row's title (notify's enrichment names it the same).
pub const ACCOUNT_TITLE: &str = "Azure DevOps token";

/// What `↩` does on the account row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountOpen {
    /// "adoken couldn't replace its token automatically": the one item
    /// that offers **Replace token…** (owner decision 2026-10-01).
    ReplaceToken,
    /// Any other token notice: Settings › Account.
    Settings,
}

pub fn account_open(n: &Notification) -> Option<AccountOpen> {
    match (&n.subject, n.kind) {
        (Subject::Account, Kind::Sync(SyncEventKind::TokenNotReplaced)) => {
            Some(AccountOpen::ReplaceToken)
        }
        (Subject::Account, _) => Some(AccountOpen::Settings),
        _ => None,
    }
}

/// Where Enter goes (PR → review, WI/run → detail).
pub fn target(n: &Notification) -> Option<Target> {
    match n.subject {
        Subject::Pr(id) => Some(Target::PullRequest(id)),
        Subject::WorkItem(id) => Some(Target::WorkItem(id)),
        Subject::Build(id) => Some(Target::Run(id)),
        Subject::Account => None,
    }
}

pub fn pin_subject(notification: &Notification) -> Option<adoken_model::PinSubject> {
    use adoken_model::PinSubject;
    match notification.subject {
        Subject::Pr(id) => Some(PinSubject::Pr(id)),
        Subject::WorkItem(id) => Some(PinSubject::WorkItem(id)),
        Subject::Build(_) => Some(PinSubject::Definition {
            project_id: notification.ctx.project.clone()?,
            id: notification.ctx.pipeline_id?,
        }),
        Subject::Account => None,
    }
}

/// The auto-complete guard alert (spec 0004 FR-12, AC-15): new commits on
/// a PR where I set auto-complete in adoken. Its row offers one-key
/// "Cancel auto-complete" next to "Review changes" (Enter).
pub fn auto_complete_guard(n: &Notification) -> Option<i64> {
    match n.subject {
        Subject::Pr(id)
            if n.kind == Kind::Sync(SyncEventKind::PrPushed) && n.ctx.auto_complete_mine =>
        {
            Some(id)
        }
        _ => None,
    }
}

/// Empty state copy: `(headline, body, offer "Clear filters")`.
pub fn empty_copy(engine: &Engine, filter: &Filter) -> (String, String, bool) {
    if filter.is_narrowed() {
        return (
            format!("No notifications match {}", filter.describe()),
            "Clear the filters to see everything.".into(),
            true,
        );
    }
    let now = engine.now();
    match filter.state {
        StateFilter::All | StateFilter::Unread => {
            let headline = if filter.state == StateFilter::All {
                "You're all caught up"
            } else {
                "No unread notifications"
            };
            let body = match engine.next_snooze_return() {
                Some(at) => {
                    let when = clock_label(at, now, engine.clock());
                    let at = if when.starts_with(|c: char| c.is_ascii_digit()) {
                        "at "
                    } else {
                        ""
                    };
                    format!("Next snoozed item returns {at}{when}.")
                }
                None => "Nothing needs you right now.".into(),
            };
            (headline.into(), body, false)
        }
        StateFilter::Snoozed => (
            "Nothing snoozed".into(),
            "Press s on a notification to snooze it.".into(),
            false,
        ),
        StateFilter::Done => (
            "Nothing done yet".into(),
            "Done notifications are kept for 30 days.".into(),
            false,
        ),
    }
}

/// "Quiet hours until 08:00".
pub fn quiet_label(engine: &Engine) -> Option<String> {
    engine.quiet_until().map(|until| {
        format!(
            "Quiet hours until {}",
            clock_label(until, engine.now(), engine.clock())
        )
    })
}

/// Selection after the list changed: stay on the same index (the next row
/// slides into place), clamped.
pub fn clamp_selection(selected: usize, len: usize) -> usize {
    selected.min(len.saturating_sub(1))
}

/// `1`–`5` → source.
pub fn source_for_digit(digit: usize) -> Option<Source> {
    Source::ALL.get(digit.checked_sub(1)?).copied()
}

/// Short snooze confirmation for the footer.
pub fn snoozed_status(until: Option<Millis>, now: Millis, clock: &dyn Clock) -> String {
    match until {
        Some(at) => format!("Snoozed until {} · z to undo", clock_label(at, now, clock)),
        None => "Snoozed until it changes · z to undo".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use adoken_model::SyncEvent;
    use adoken_notify::{
        DEFAULT_MORNING, DeliveryEnv, Effects, EventContext, FakeClock, Incoming, NotifySettings,
        SnoozeChoice,
    };

    use super::*;

    const NOW: Millis = 1_790_328_600_000;

    fn engine() -> Engine {
        let clock = FakeClock::new(NOW, 0);
        let mut engine = Engine::new(Arc::new(clock), NotifySettings::default());
        for (i, kind) in [SyncEventKind::PrCommentAdded, SyncEventKind::PrCommentAdded]
            .into_iter()
            .enumerate()
        {
            let event = SyncEvent {
                id: 0,
                kind,
                subject: Subject::Pr(4312),
                dedupe_key: i.to_string(),
                actor: Some("p".into()),
                self_caused: false,
                at: NOW - 5 * 60_000,
                summary: "Priya Shah commented".into(),
            };
            let ctx = EventContext {
                title: "!4312 Retry idempotency keys".into(),
                ..EventContext::default()
            };
            engine.ingest(
                Incoming::from_sync(&event, ctx),
                &DeliveryEnv::default(),
                &mut Effects::default(),
            );
        }
        engine
    }

    #[test]
    fn rows_spell_out_counts_and_state() {
        let engine = engine();
        let row = row_text(engine.items().first().unwrap(), NOW);
        assert_eq!(row.title, "!4312 Retry idempotency keys");
        assert_eq!(row.subtitle, "Pull requests · Priya Shah commented");
        assert_eq!(row.count.as_deref(), Some("2 new comments"));
        assert_eq!(row.time, "5m");
        assert!(row.unread);
        assert_eq!(row.glyph, Glyph::PullRequest);
        assert!(row.label.contains("unread") && row.label.contains("2 new comments"));
    }

    #[test]
    fn empty_copy_distinguishes_zero_from_filtered() {
        let mut engine = engine();
        let id = engine.items()[0].id;
        let filter = Filter::default();
        engine.snooze(id, SnoozeChoice::TomorrowMorning, DEFAULT_MORNING);
        let (headline, body, clear) = empty_copy(&engine, &filter);
        assert_eq!(headline, "You're all caught up");
        assert_eq!(body, "Next snoozed item returns tomorrow 09:00.");
        assert!(!clear);

        let mut narrowed = filter;
        narrowed.toggle_source(Source::Pipelines);
        let (headline, _, clear) = empty_copy(&engine, &narrowed);
        assert_eq!(headline, "No notifications match Pipelines");
        assert!(clear);
    }

    #[test]
    fn selection_and_digits() {
        assert_eq!(clamp_selection(5, 3), 2);
        assert_eq!(clamp_selection(0, 0), 0);
        assert_eq!(source_for_digit(1), Some(Source::PullRequests));
        assert_eq!(source_for_digit(5), Some(Source::Nudges));
        assert_eq!(source_for_digit(0), None);
        assert_eq!(source_for_digit(6), None);
    }

    #[test]
    fn only_pushes_on_my_auto_complete_offer_cancel() {
        let clock = FakeClock::new(NOW, 0);
        let mut engine = Engine::new(Arc::new(clock), NotifySettings::default());
        for (id, mine) in [(912, true), (913, false)] {
            let event = SyncEvent {
                id: 0,
                kind: SyncEventKind::PrPushed,
                subject: Subject::Pr(id),
                dedupe_key: "b0b0".into(),
                actor: Some("p".into()),
                self_caused: false,
                at: NOW,
                summary: "pushed".into(),
            };
            let ctx = EventContext {
                auto_complete_mine: mine,
                ..EventContext::default()
            };
            engine.ingest(
                Incoming::from_sync(&event, ctx),
                &DeliveryEnv::default(),
                &mut Effects::default(),
            );
        }
        let guards: Vec<Option<i64>> = engine.items().iter().map(auto_complete_guard).collect();
        assert!(guards.contains(&Some(912)));
        assert!(guards.contains(&None));
        assert!(auto_complete_guard(&self::engine().items()[0]).is_none());
    }

    #[test]
    fn pinning_notifications_uses_the_subject_and_maps_runs_to_project_scoped_definitions() {
        use adoken_model::PinSubject;
        let mut notification = engine().items()[0].clone();
        notification.subject = Subject::Pr(7);
        assert_eq!(pin_subject(&notification), Some(PinSubject::Pr(7)));
        notification.subject = Subject::WorkItem(8);
        assert_eq!(pin_subject(&notification), Some(PinSubject::WorkItem(8)));
        notification.subject = Subject::Build(999);
        assert_eq!(pin_subject(&notification), None);
        notification.ctx.project = Some("project-a".into());
        notification.ctx.pipeline_id = Some(42);
        assert_eq!(
            pin_subject(&notification),
            Some(PinSubject::Definition {
                project_id: "project-a".into(),
                id: 42
            })
        );
        notification.subject = Subject::Account;
        assert_eq!(pin_subject(&notification), None);
    }
}
