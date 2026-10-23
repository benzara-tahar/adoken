//! Inbox stories (spec 0005) over the demo org (fake Contoso data): the
//! 48 h backlog + first-run baseline + 25 minutes of the live stream, at a
//! fixed clock so snapshots never drift.

use adoken_app::inbox::InboxView;
use adoken_model::time::MINUTE;
use adoken_model::{PrStatus, Subject, SyncEvent, SyncEventKind, short_branch};
use adoken_notify::{
    DeliveryEnv, Effects, EventContext, Filter, Incoming, Priority, Source, StateFilter,
};
use gpui::{AnyElement, App, Context, Window, div, prelude::*};

fn view(
    key: &'static str,
    setup: impl FnOnce(&mut InboxView, &mut Context<InboxView>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |_, cx| {
        let mut view = InboxView::demo(cx);
        setup(&mut view, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

/// Grouped rows (one per PR / work item / run) with counts, priorities,
/// the quiet-hours pill, and the second row selected.
pub fn inbox_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view("inbox", |view, cx| view.select_row(1, cx), window, cx)
}

/// `s` on the selected row.
pub fn inbox_snooze_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "inbox-snooze",
        |view, cx| {
            view.select_row(2, cx);
            view.open_snooze_menu(cx);
        },
        window,
        cx,
    )
}

/// Filter chips narrowed to nothing: offers "Clear filters".
pub fn inbox_filtered_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "inbox-filtered",
        |view, cx| {
            view.update_engine(
                |engine| {
                    let mut filter = Filter::default();
                    filter.toggle_source(Source::Nudges);
                    filter.toggle_priority(Priority::P0);
                    engine.set_filter(filter);
                },
                cx,
            )
        },
        window,
        cx,
    )
}

/// Inbox zero: everything done; the next snooze's return time shows.
pub fn inbox_zero_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "inbox-zero",
        |view, cx| {
            view.update_engine(
                |engine| {
                    let open: Vec<_> = engine
                        .list(&Filter {
                            state: StateFilter::All,
                            ..Filter::default()
                        })
                        .iter()
                        .map(|n| n.id)
                        .collect();
                    for id in open {
                        engine.mark_done(id);
                    }
                },
                cx,
            )
        },
        window,
        cx,
    )
}

/// Spec 0004 FR-12 / AC-15: new commits landed on a PR where I set
/// auto-complete in adoken. The P1 row offers one-key "Cancel
/// auto-complete" (`c`) next to "Review changes" (Enter), selected so the
/// footer shows both.
pub fn inbox_auto_complete_guard_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "inbox-auto-complete-guard",
        |view, cx| {
            let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
            let pr = ds
                .prs
                .iter()
                .find(|pr| pr.author == ds.me && pr.status == PrStatus::Active)
                .expect("the demo org has an active PR of mine");
            let pusher = ds
                .people
                .iter()
                .find(|p| p.id != ds.me && !p.is_group)
                .expect("the demo org has other people");
            let event = SyncEvent {
                id: 0,
                kind: SyncEventKind::PrPushed,
                subject: Subject::Pr(pr.id),
                dedupe_key: "c0ffee1".into(),
                actor: Some(pusher.id.clone()),
                self_caused: false,
                at: ds.now + 24 * MINUTE,
                summary: format!("New commits on !{} after you set auto-complete", pr.id),
            };
            let ctx = EventContext {
                title: format!("!{} {}", pr.id, pr.title),
                project: Some(pr.project_id.clone()),
                repo_id: Some(pr.repo_id.clone()),
                repo_name: ds
                    .repos
                    .iter()
                    .find(|r| r.id == pr.repo_id)
                    .map(|r| r.name.clone()),
                branch: Some(short_branch(&pr.target_branch).to_owned()),
                actor_name: Some(pusher.display_name.clone()),
                mine: true,
                auto_complete_mine: true,
                ..EventContext::default()
            };
            view.update_engine(
                |engine| {
                    engine.ingest(
                        Incoming::from_sync(&event, ctx),
                        &DeliveryEnv::default(),
                        &mut Effects::default(),
                    );
                },
                cx,
            );
            let engine = view.engine();
            let row = engine
                .list(&engine.filter())
                .iter()
                .position(|n| n.subject == Subject::Pr(pr.id))
                .unwrap_or(0);
            view.select_row(row, cx);
        },
        window,
        cx,
    )
}
