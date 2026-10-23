//! Spec 0005 §6 acceptance criteria: event fixtures, an injected clock, and
//! a fake OS-notification + sound sink.

use std::sync::Arc;

use adoken_model::time::{DAY, HOUR, MINUTE, SECOND};
use adoken_model::{Millis, RunResult, Subject, SyncEvent, SyncEventKind, Vote};
use adoken_notify::*;

/// 2026-09-21, a Monday, 00:00 UTC.
const MONDAY: Millis = 20_717 * DAY;

fn at(day: i64, hour: i64, minute: i64) -> Millis {
    MONDAY + day * DAY + hour * HOUR + minute * MINUTE
}

struct Harness {
    clock: FakeClock,
    engine: Engine,
    env: DeliveryEnv,
    sink: Recorder,
    next_key: u32,
}

impl Harness {
    fn new(now: Millis) -> Self {
        Self::with(now, NotifySettings::default())
    }

    fn with(now: Millis, settings: NotifySettings) -> Self {
        let clock = FakeClock::new(now, 0);
        Self {
            engine: Engine::new(Arc::new(clock.clone()), settings),
            clock,
            env: DeliveryEnv::default(),
            sink: Recorder::default(),
            next_key: 0,
        }
    }

    fn event(&mut self, kind: SyncEventKind, subject: Subject, ctx: EventContext) -> Incoming {
        self.next_key += 1;
        let event = SyncEvent {
            id: 0,
            kind,
            subject,
            dedupe_key: self.next_key.to_string(),
            actor: Some("priya".into()),
            self_caused: false,
            at: self.clock.now(),
            summary: format!("{} #{}", kind.as_str(), self.next_key),
        };
        Incoming::from_sync(&event, ctx)
    }

    fn ingest(&mut self, incoming: Incoming) -> Outcome {
        let mut effects = Effects::default();
        let outcome = self.engine.ingest(incoming, &self.env, &mut effects);
        effects.dispatch(&mut self.sink, &mut Recorder::default());
        self.sink.sounds.extend(effects.sounds);
        outcome
    }

    fn tick(&mut self) {
        let effects = self.engine.tick(&self.env);
        effects.dispatch(&mut self.sink, &mut Recorder::default());
        self.sink.sounds.extend(effects.sounds);
    }

    fn banners(&self) -> usize {
        self.sink.banners.len()
    }

    fn inbox(&self) -> Vec<&Notification> {
        self.engine.list(&Filter::default())
    }
}

fn pr_ctx(repo: &str) -> EventContext {
    EventContext {
        title: "!912 Fix flaky checkout".into(),
        repo_id: Some(format!("id-{repo}")),
        repo_name: Some(repo.into()),
        ..EventContext::default()
    }
}

fn build_ctx(pipeline: i64, branch: &str, result: RunResult) -> EventContext {
    EventContext {
        title: format!("payments-ci {branch}"),
        pipeline_id: Some(pipeline),
        pipeline_name: Some("payments-ci".into()),
        branch: Some(branch.into()),
        build_result: Some(result),
        mine: true,
        ..EventContext::default()
    }
}

fn quiet_18_to_08() -> NotifySettings {
    let mut settings = NotifySettings::default();
    settings.quiet_hours.enabled = true;
    settings.quiet_hours.schedule = Schedule::new(vec![Span::new(
        Weekdays::ALL,
        TimeOfDay::hm(18, 0),
        TimeOfDay::hm(8, 0),
    )]);
    settings
}

#[test]
fn ac1_bursts_coalesce_into_one_row_and_one_banner() {
    let mut settings = NotifySettings::default();
    // Comments default to inbox-only; make them banner so coalescing shows.
    settings.priorities.set(RuleKey::PrComment, Priority::P1);
    let mut h = Harness::with(at(0, 10, 0), settings);
    for _ in 0..5 {
        let e = h.event(
            SyncEventKind::PrCommentAdded,
            Subject::Pr(912),
            pr_ctx("web"),
        );
        h.ingest(e);
        h.clock.advance(18 * SECOND);
    }
    let inbox = h.inbox();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].count(), 5);
    assert_eq!(inbox[0].count_label(), "5 new comments");
    assert_eq!(h.banners(), 1);
    assert_eq!(h.sink.banners[0].thread_id, "pr:912");

    // A new burst after the window may interrupt again.
    h.clock.advance(10 * MINUTE);
    let e = h.event(
        SyncEventKind::PrCommentAdded,
        Subject::Pr(912),
        pr_ctx("web"),
    );
    h.ingest(e);
    assert_eq!(h.banners(), 2);
    assert_eq!(h.inbox().len(), 1);
}

#[test]
fn coalesced_burst_still_interrupts_for_a_more_urgent_event() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.banners(), 0);
    h.clock.advance(30 * SECOND);
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.banners(), 1);
    assert_eq!(h.inbox()[0].priority, Priority::P1);
    assert_eq!(h.inbox()[0].count_label(), "2 updates");
}

#[test]
fn ac2_self_caused_is_hidden_unless_asked_for() {
    let ctx = EventContext {
        vote: Some(Vote::Approved),
        ..pr_ctx("web")
    };
    let mut h = Harness::new(at(0, 10, 0));
    let mut e = h.event(SyncEventKind::PrVoteChanged, Subject::Pr(5), ctx.clone());
    e.self_caused = true;
    assert_eq!(h.ingest(e), Outcome::SelfCaused);
    assert!(h.inbox().is_empty());

    let mut settings = NotifySettings {
        show_self_caused: true,
        ..NotifySettings::default()
    };
    settings.priorities.set(RuleKey::VoteApproval, Priority::P1);
    let mut h = Harness::with(at(0, 10, 0), settings);
    let mut e = h.event(SyncEventKind::PrVoteChanged, Subject::Pr(5), ctx);
    e.self_caused = true;
    assert!(matches!(h.ingest(e), Outcome::Added(_)));
    assert_eq!(h.inbox().len(), 1);
    assert_eq!(h.banners(), 0, "own activity never interrupts");
}

#[test]
fn ac3_default_priorities_decide_banners() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    let e = h.event(SyncEventKind::PrPushed, Subject::Pr(2), pr_ctx("web"));
    h.ingest(e);
    let rows = h.inbox();
    let mention = rows.iter().find(|n| n.subject == Subject::Pr(1)).unwrap();
    let push = rows.iter().find(|n| n.subject == Subject::Pr(2)).unwrap();
    assert_eq!(mention.priority, Priority::P1);
    assert_eq!(push.priority, Priority::P3);
    assert_eq!(h.banners(), 1);
    assert_eq!(h.sink.banners[0].notification, Some(mention.id));
    assert!(h.sink.sounds.is_empty(), "sounds are off by default");
}

#[test]
fn ac4_quiet_hours_hold_then_release_one_summary() {
    let mut h = Harness::with(at(0, 22, 0), quiet_18_to_08());
    h.env.sounds_enabled = true;
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.inbox().len(), 1, "inbox content is never gated");
    assert_eq!(h.inbox()[0].held_until, Some(at(1, 8, 0)));
    assert_eq!(h.banners(), 0);
    assert_eq!(h.engine.quiet_until(), Some(at(1, 8, 0)));

    h.clock.set(at(1, 7, 59));
    h.tick();
    assert_eq!(h.banners(), 0);
    assert!(h.sink.sounds.is_empty());

    h.clock.set(at(1, 8, 0));
    h.tick();
    assert_eq!(h.banners(), 1);
    h.tick();
    assert_eq!(h.banners(), 1, "released exactly once");
    assert_eq!(h.inbox()[0].held_until, None);
    assert_eq!(h.engine.next_wake(), None);
}

#[test]
fn ac5_p0_goes_through_quiet_hours() {
    let mut settings = quiet_18_to_08();
    settings.rules.push(UserRule {
        name: "Lead mentions".into(),
        enabled: true,
        when: RuleMatch {
            kind: Some(Kind::Sync(SyncEventKind::MentionedInPr)),
            actor: Some("priya".into()),
            ..RuleMatch::default()
        },
        then: RuleAction::SetPriority(Priority::P0),
    });
    let mut h = Harness::with(at(0, 22, 0), settings);
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.inbox()[0].priority, Priority::P0);
    assert_eq!(h.banners(), 1);
}

#[test]
fn ac6_delivery_window_holds_weekend_pipelines_until_monday() {
    let mut settings = NotifySettings::default();
    settings.windows.insert(
        Source::Pipelines,
        Schedule::new(vec![Span::new(
            Weekdays::WORKDAYS,
            TimeOfDay::hm(9, 0),
            TimeOfDay::hm(18, 0),
        )]),
    );
    let saturday = at(5, 10, 0);
    let mut h = Harness::with(saturday, settings);
    for build in 0..4 {
        let e = h.event(
            SyncEventKind::BuildCompleted,
            Subject::Build(100 + build),
            build_ctx(build, "main", RunResult::Failed),
        );
        h.ingest(e);
        h.clock.advance(3 * HOUR);
    }
    // A mention on Saturday isn't held by the pipelines window.
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.banners(), 1);

    h.clock.set(at(6, 23, 0));
    h.tick();
    assert_eq!(h.banners(), 1, "zero pipeline banners over the weekend");

    h.clock.set(at(7, 9, 0));
    h.tick();
    assert_eq!(h.banners(), 2);
    let summary = &h.sink.banners[1];
    assert_eq!(summary.title, "While you were away");
    assert_eq!(summary.body, "4 pipeline updates");
    assert_eq!(summary.notification, None);
}

#[test]
fn held_items_triaged_before_release_stay_quiet() {
    let mut h = Harness::with(at(0, 22, 0), quiet_18_to_08());
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    let e = h.event(
        SyncEventKind::ReviewRequested,
        Subject::Pr(2),
        pr_ctx("web"),
    );
    h.ingest(e);
    let first = h
        .inbox()
        .iter()
        .find(|n| n.subject == Subject::Pr(1))
        .unwrap()
        .id;
    h.engine.mark_done(first);
    h.clock.set(at(1, 8, 0));
    h.tick();
    assert_eq!(h.banners(), 1);
    assert_eq!(
        h.sink.banners[0].thread_id, "pr:2",
        "only the untriaged item"
    );
}

#[test]
fn mixed_kinds_summary_lists_counts_per_kind() {
    let mut h = Harness::with(at(0, 22, 0), quiet_18_to_08());
    for pr in 1..=2 {
        let e = h.event(
            SyncEventKind::ReviewRequested,
            Subject::Pr(pr),
            pr_ctx("web"),
        );
        h.ingest(e);
    }
    let e = h.event(
        SyncEventKind::PrMergeConflict,
        Subject::Pr(3),
        pr_ctx("web"),
    );
    h.ingest(e);
    // Another source gets its own summary.
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(4), pr_ctx("web"));
    h.ingest(e);
    h.clock.set(at(1, 9, 0));
    h.tick();
    assert_eq!(h.banners(), 2);
    assert_eq!(
        h.sink.banners[0].body,
        "2 review requests, 1 merge conflict"
    );
    assert_eq!(h.sink.banners[1].thread_id, "pr:4");
}

#[test]
fn ac7_snooze_tomorrow_resurfaces_once() {
    let wed_17 = at(2, 17, 0);
    let mut h = Harness::new(wed_17);
    let e = h.event(
        SyncEventKind::ReviewRequested,
        Subject::Pr(1),
        pr_ctx("web"),
    );
    h.ingest(e);
    let id = h.inbox()[0].id;
    h.engine.open(id);
    h.engine
        .snooze(id, SnoozeChoice::TomorrowMorning, DEFAULT_MORNING);
    assert!(h.inbox().is_empty());
    assert_eq!(h.engine.next_snooze_return(), Some(at(3, 9, 0)));
    let before = h.banners();

    h.clock.set(at(3, 8, 59));
    h.tick();
    assert!(h.inbox().is_empty());

    h.clock.set(at(3, 9, 0));
    h.tick();
    let n = h.engine.get(id).unwrap();
    assert!(n.in_inbox() && !n.read && n.snoozed_badge);
    assert_eq!(h.banners(), before + 1, "P1 resurfaces with its banner");

    // Asleep past the snooze: resurfaces once on wake.
    h.engine
        .snooze(id, SnoozeChoice::TomorrowMorning, DEFAULT_MORNING);
    h.clock.set(at(6, 12, 0));
    h.tick();
    h.tick();
    assert_eq!(h.banners(), before + 2);
    assert!(h.engine.get(id).unwrap().snoozed_badge);
}

#[test]
fn ac8_until_it_changes_ignores_self_caused_events() {
    let settings = NotifySettings {
        show_self_caused: true,
        ..NotifySettings::default()
    };
    let mut h = Harness::with(at(0, 10, 0), settings);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(7), pr_ctx("web"));
    h.ingest(e);
    let id = h.inbox()[0].id;
    h.engine
        .snooze(id, SnoozeChoice::UntilChanged, DEFAULT_MORNING);
    h.clock.advance(HOUR);

    let mut mine = h.event(SyncEventKind::PrPushed, Subject::Pr(7), pr_ctx("web"));
    mine.self_caused = true;
    h.ingest(mine);
    assert!(h.inbox().is_empty(), "my own push doesn't wake it");

    let theirs = h.event(SyncEventKind::PrPushed, Subject::Pr(7), pr_ctx("web"));
    h.ingest(theirs);
    let n = h.engine.get(id).unwrap();
    assert!(n.in_inbox() && n.snoozed_badge && !n.read);
}

#[test]
fn time_snoozed_rows_stay_snoozed_and_silent_on_new_activity() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(7), pr_ctx("web"));
    h.ingest(e);
    let id = h.inbox()[0].id;
    h.engine
        .snooze(id, SnoozeChoice::ThreeHours, DEFAULT_MORNING);
    h.clock.advance(HOUR);
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(7), pr_ctx("web"));
    h.ingest(e);
    assert!(h.inbox().is_empty());
    assert_eq!(h.banners(), 1);
}

#[test]
fn ac9_muting_a_repo_hides_existing_and_future() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(2), pr_ctx("api"));
    h.ingest(e);
    let web = Mute::Repo {
        id: "id-web".into(),
        name: "web".into(),
    };
    h.engine.mute(web.clone());
    assert_eq!(h.inbox().len(), 1);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(3), pr_ctx("web"));
    assert_eq!(h.ingest(e), Outcome::Muted);
    assert_eq!(h.inbox().len(), 1);

    h.engine.unmute(&web);
    assert_eq!(h.inbox().len(), 1, "existing rows stay hidden");
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(3), pr_ctx("web"));
    assert!(matches!(h.ingest(e), Outcome::Added(_)));
    assert_eq!(h.inbox().len(), 2);
}

#[test]
fn ac10_still_green_is_quiet_but_fixed_keeps_priority() {
    let mut settings = NotifySettings::default();
    settings
        .priorities
        .set(RuleKey::BuildFinished, Priority::P1);
    let mut h = Harness::with(at(0, 10, 0), settings);
    let green = |h: &mut Harness, build| {
        let e = h.event(
            SyncEventKind::BuildCompleted,
            Subject::Build(build),
            build_ctx(1, "main", RunResult::Succeeded),
        );
        h.ingest(e);
    };
    green(&mut h, 1);
    assert_eq!(
        h.banners(),
        1,
        "first green of the definition keeps priority"
    );
    green(&mut h, 2);
    let still = h
        .engine
        .list(&Filter::default())
        .into_iter()
        .find(|n| n.subject == Subject::Build(2))
        .unwrap();
    assert_eq!(still.priority, Priority::P3);
    assert_eq!(h.banners(), 1, "still green is inbox only");

    let e = h.event(
        SyncEventKind::BuildCompleted,
        Subject::Build(3),
        build_ctx(1, "main", RunResult::Failed),
    );
    h.ingest(e);
    assert_eq!(h.banners(), 2);
    green(&mut h, 4);
    let fixed = h
        .engine
        .list(&Filter::default())
        .into_iter()
        .find(|n| n.subject == Subject::Build(4))
        .unwrap();
    assert_eq!(fixed.priority, Priority::P1);
    assert_eq!(h.banners(), 3, "fixed keeps its configured priority");

    // Another branch has its own history.
    let e = h.event(
        SyncEventKind::BuildCompleted,
        Subject::Build(5),
        build_ctx(1, "release", RunResult::Succeeded),
    );
    h.ingest(e);
    assert_eq!(h.banners(), 4);
}

#[test]
fn ac11_triage_keys_and_exact_undo() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(
        SyncEventKind::ReviewRequested,
        Subject::Pr(1),
        pr_ctx("web"),
    );
    h.ingest(e);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(2), pr_ctx("web"));
    h.ingest(e);
    let id = h.engine.list(&Filter::default())[1].id;
    let pristine: Vec<Notification> = h.engine.items().to_vec();

    assert!(h.engine.mark_done(id));
    assert_eq!(h.inbox().len(), 1);
    assert!(h.engine.undo());
    assert_eq!(h.engine.items(), pristine.as_slice());

    h.engine.toggle_read(id);
    assert!(h.engine.get(id).unwrap().read);
    h.engine.undo();
    assert_eq!(h.engine.items(), pristine.as_slice());

    h.engine.snooze(id, SnoozeChoice::OneHour, DEFAULT_MORNING);
    assert_eq!(h.inbox().len(), 1);
    h.engine.undo();
    assert_eq!(h.engine.items(), pristine.as_slice());

    h.engine.mute_subject(id);
    assert_eq!(h.inbox().len(), 1);
    assert_eq!(h.engine.mutes().len(), 1);
    h.engine.undo();
    assert_eq!(h.engine.items(), pristine.as_slice());
    assert!(h.engine.mutes().is_empty());

    assert!(!h.engine.undo(), "nothing left to undo");
}

#[test]
fn ac12_badge_counts_unread_p0_and_p1() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    let e = h.event(
        SyncEventKind::ReviewRequested,
        Subject::Pr(2),
        pr_ctx("web"),
    );
    h.ingest(e);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(3), pr_ctx("web"));
    h.ingest(e);
    assert_eq!(h.engine.badge(), 2);
    assert_eq!(h.engine.unread_count(), 3);
    let first = h.engine.list(&Filter::default()).last().unwrap().id;
    h.engine.open(first);
    assert_eq!(h.engine.badge(), 1);
    let mut settings = h.engine.settings().clone();
    settings.badge = BadgeMode::AllUnread;
    h.engine.set_settings(settings);
    assert_eq!(h.engine.badge(), 2);
}

#[test]
fn ac13_denied_permission_changes_nothing_but_banners() {
    let run = |permission| {
        let mut h = Harness::new(at(0, 10, 0));
        h.env.permission = permission;
        let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
        let outcome = h.ingest(e);
        (outcome, h.engine.items().to_vec(), h.banners(), h)
    };
    let (granted, granted_items, granted_banners, _) = run(OsPermission::Granted);
    let (denied, denied_items, denied_banners, mut h) = run(OsPermission::Denied);
    assert_eq!(granted, denied);
    // Bursts record what interrupted; compare the inbox-visible state.
    let visible = |items: &[Notification]| {
        items
            .iter()
            .map(|n| (n.subject.clone(), n.priority, n.read, n.count()))
            .collect::<Vec<_>>()
    };
    assert_eq!(visible(&granted_items), visible(&denied_items));
    assert_eq!((granted_banners, denied_banners), (1, 0));

    assert!(h.engine.permission_notice(&h.env));
    h.engine.dismiss_permission_notice();
    assert!(!h.engine.permission_notice(&h.env), "shown at most once");
}

#[test]
fn ac14_replayed_events_are_idempotent() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    assert!(matches!(h.ingest(e.clone()), Outcome::Added(_)));
    assert_eq!(h.ingest(e), Outcome::Duplicate);
    assert_eq!(h.inbox().len(), 1);
    assert_eq!(h.inbox()[0].count(), 1);
    assert_eq!(h.banners(), 1);
}

#[test]
fn done_rows_reopen_on_new_activity() {
    let mut h = Harness::new(at(0, 10, 0));
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(1), pr_ctx("web"));
    h.ingest(e);
    let id = h.inbox()[0].id;
    h.engine.mark_done(id);
    h.clock.advance(HOUR);
    let e = h.event(SyncEventKind::PrCommentAdded, Subject::Pr(1), pr_ctx("web"));
    assert_eq!(h.ingest(e), Outcome::Updated(id));
    let n = h.engine.get(id).unwrap();
    assert!(n.in_inbox() && !n.read);
    assert_eq!(n.count(), 1, "fresh activity, not the old count");
}

#[test]
fn source_minimum_priority_filters() {
    let mut settings = NotifySettings::default();
    settings
        .min_priority
        .insert(Source::Pipelines, Priority::P2);
    let mut h = Harness::with(at(0, 10, 0), settings);
    let e = h.event(
        SyncEventKind::BuildCompleted,
        Subject::Build(1),
        build_ctx(1, "main", RunResult::Succeeded),
    );
    assert_eq!(h.ingest(e), Outcome::Filtered);
    let e = h.event(
        SyncEventKind::BuildCompleted,
        Subject::Build(2),
        build_ctx(1, "main", RunResult::Failed),
    );
    assert!(matches!(h.ingest(e), Outcome::Added(_)));
}

#[test]
fn state_round_trips_and_retention_prunes_old_done_rows() {
    let mut h = Harness::with(at(0, 22, 0), quiet_18_to_08());
    let e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    let replay = e.clone();
    h.ingest(e);
    let e = h.event(
        SyncEventKind::ReviewRequested,
        Subject::Pr(2),
        pr_ctx("web"),
    );
    h.ingest(e);
    let done = h.engine.list(&Filter::default())[0].id;
    h.engine.mark_done(done);

    // Restart: same rows, same held interruption, replay still deduped.
    let state = h.engine.state();
    let json = serde_json::to_string(&state).unwrap();
    let restored: EngineState = serde_json::from_str(&json).unwrap();
    let mut again = Engine::from_state(Arc::new(h.clock.clone()), restored);
    let mut effects = Effects::default();
    assert_eq!(
        again.ingest(replay, &h.env, &mut effects),
        Outcome::Duplicate
    );
    h.clock.set(at(1, 8, 0));
    let released = again.tick(&h.env);
    assert_eq!(released.banners.len(), 1);

    h.clock.set(at(40, 0, 0));
    again.prune();
    assert!(again.get(done).is_none());
    assert_eq!(again.items().len(), 1);
    let changes = again.take_changes();
    assert_eq!(changes.deleted, vec![done]);
}

#[test]
fn stale_events_land_silently() {
    let mut h = Harness::new(at(0, 10, 0));
    let mut e = h.event(SyncEventKind::MentionedInPr, Subject::Pr(1), pr_ctx("web"));
    e.at = at(0, 9, 0);
    h.ingest(e);
    assert_eq!(h.inbox().len(), 1);
    assert_eq!(
        h.banners(),
        0,
        "an hour-old mention from backfill doesn't ping"
    );
}
