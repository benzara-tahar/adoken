//! Pin presentations share the application's group renderer; fixtures are
//! resolved once from the same deterministic store as demo mode.

use std::sync::LazyLock;

use adoken_app::pins::snapshot::{self, Options, Snapshot};
use adoken_app::pins::status::{DefinitionHeadline, Headline, Kind, PrHeadline};
use adoken_model::{Pin, PinSubject, PinUnavailable, StateCategory};
use adoken_theme::{size, space, typography};
use adoken_ui::{ActiveTheme, Icon, IconName, TypeStyleExt};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

const NOW: i64 = adoken_demo::DEMO_NOW;

static FIXTURE: LazyLock<Snapshot> = LazyLock::new(|| {
    let dir = crate::story_dir("pins-fixture");
    let store = adoken_demo::open_store_at(&dir, NOW).expect("seed pin fixture");
    snapshot::read(
        &store,
        Options {
            now: NOW,
            auto_unpin: false,
            stale_after: None,
        },
    )
    .expect("read pin fixture")
    .0
});

static TWENTY: LazyLock<Snapshot> = LazyLock::new(|| {
    let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
    let mut snapshot = Snapshot::default();
    let mut add = |subject, title: String, kind, headline| {
        let mut view = snapshot::PinView::provisional(Pin {
            subject,
            position: snapshot.pins.len() as u32,
            label: None,
            title,
            pinned_at: NOW,
            opened_at: Some(NOW),
            seen_result: None,
            unavailable: None,
        });
        view.kind = kind;
        view.headline = headline;
        snapshot.pins.push(view);
    };
    let prs = [
        PrHeadline::Completed,
        PrHeadline::Abandoned,
        PrHeadline::MergeConflict,
        PrHeadline::CiFailing,
        PrHeadline::ChangesRequested { rejected: true },
        PrHeadline::CiRunning,
        PrHeadline::Approved,
        PrHeadline::Draft,
        PrHeadline::Active,
    ];
    for (pr, headline) in ds.prs.iter().zip(prs) {
        add(
            PinSubject::Pr(pr.id),
            pr.title.clone(),
            Kind::Pr {
                draft: headline == PrHeadline::Draft,
            },
            Headline::Pr(headline),
        );
    }
    let definitions = [
        DefinitionHeadline::Running {
            since: NOW - 12 * adoken_model::time::MINUTE,
        },
        DefinitionHeadline::Failed,
        DefinitionHeadline::Partial,
        DefinitionHeadline::Canceled,
        DefinitionHeadline::Succeeded,
        DefinitionHeadline::NoRuns,
    ];
    for (pipeline, headline) in ds.pipelines.iter().zip(definitions) {
        add(
            PinSubject::Definition {
                project_id: pipeline.project_id.clone(),
                id: pipeline.id,
            },
            pipeline.name.clone(),
            Kind::Definition,
            Headline::Definition(headline),
        );
    }
    for (wi, (state, category)) in ds.work_items.iter().zip([
        ("New", StateCategory::Proposed),
        ("Active", StateCategory::InProgress),
        ("Resolved", StateCategory::Resolved),
        ("Closed", StateCategory::Completed),
    ]) {
        add(
            PinSubject::WorkItem(wi.id),
            wi.title.clone(),
            Kind::WorkItem(Some(wi.kind.clone())),
            Headline::WorkItem {
                state: state.into(),
                category,
            },
        );
    }
    add(
        PinSubject::Pr(99999),
        "!99999".into(),
        Kind::Pr { draft: false },
        Headline::Fetching,
    );
    snapshot.pins[9].pin.label = Some("Payments CI".into());
    snapshot
});

fn gallery_group(
    snapshot: &Snapshot,
    rail: bool,
    folded: bool,
    linked: bool,
    cx: &App,
) -> AnyElement {
    if folded || snapshot.pins.is_empty() {
        return adoken_app::shell::pinned::group(snapshot, rail, folded, NOW, cx);
    }
    let theme = cx.theme();
    let cursor = if snapshot
        .pins
        .iter()
        .any(|pin| matches!(pin.headline, Headline::NotAvailable(_)))
    {
        2
    } else {
        1
    };
    let scroll = gpui::ScrollHandle::new();
    if snapshot.pins.len() == 20 {
        scroll.set_offset(gpui::point(px(0.0), px(-180.0)));
    }
    div()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(space::S2))
        .child(
            div()
                .flex_none()
                .h(px(size::CONTROL_MD))
                .flex()
                .items_center()
                .gap(px(space::S4))
                .border_t_1()
                .border_color(theme.colors.border_subtle)
                .text_color(theme.colors.text_tertiary)
                .type_style(typography::LABEL, theme)
                .when(rail, |header| header.child(Icon::new(IconName::Pin)))
                .when(!rail, |header| {
                    header
                        .child("Pinned")
                        .child(Icon::new(IconName::ChevronDown))
                        .child(div().flex_1())
                        .child(snapshot.pins.len().to_string())
                }),
        )
        .child(
            div()
                .id(if rail {
                    "gallery-pins-rail-scroll"
                } else {
                    "gallery-pins-scroll"
                })
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .flex()
                .flex_col()
                .gap(px(space::S2))
                .children(snapshot.pins.iter().enumerate().map(|(index, view)| {
                    adoken_app::shell::pinned::row(
                        view,
                        rail,
                        NOW,
                        adoken_ui::PinRowState {
                            on_screen: index == 0,
                            cursor: !rail && index == cursor,
                            linked: linked && index == 0,
                            ..Default::default()
                        },
                        cx,
                    )
                })),
        )
        .into_any_element()
}

fn presentation(snapshot: &Snapshot, rail: bool, folded: bool, peek: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let width = if rail { size::RAIL } else { size::SIDEBAR };
    let sections = [
        (IconName::Sun, "Today"),
        (IconName::Inbox, "Inbox"),
        (IconName::GitPullRequest, "Pull requests"),
        (IconName::SquareCheck, "Work items"),
        (IconName::Workflow, "Pipelines"),
        (IconName::Search, "Search"),
    ];
    let row = |icon, title: &'static str| {
        div()
            .flex_none()
            .h(px(size::CONTROL_MD))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .pl(px(space::S12))
            .child(
                Icon::new(icon)
                    .size(size::ICON_MD)
                    .color(theme.colors.text_secondary),
            )
            .when(!rail, |row| row.child(title))
    };
    div()
        .relative()
        .size_full()
        .flex()
        .bg(theme.colors.bg_app)
        .type_style(typography::BODY, theme)
        .text_color(theme.colors.text_secondary)
        .child(
            div()
                .w(px(width))
                .h_full()
                .flex_none()
                .p(px(space::S8))
                .flex()
                .flex_col()
                .gap(px(space::S2))
                .bg(theme.colors.bg_sidebar)
                .children(sections.into_iter().map(|(icon, title)| row(icon, title)))
                .child(gallery_group(snapshot, rail, folded, false, cx))
                .child(div().flex_1().min_h(px(0.0)))
                .child(row(IconName::PanelLeft, "Collapse"))
                .child(row(IconName::Settings, "Settings"))
                .child(row(IconName::User, "Contoso")),
        )
        .child(
            div()
                .flex_1()
                .h_full()
                .p(px(space::S24))
                .child("Your work stays in place"),
        )
        .when(peek, |root| {
            root.child(
                div()
                    .absolute()
                    .left(px(size::RAIL))
                    .top(px(184.0))
                    .w(px(size::SIDEBAR))
                    .max_h(px(280.0))
                    .p(px(space::S8))
                    .bg(theme.colors.bg_overlay)
                    .child(gallery_group(snapshot, false, false, true, cx)),
            )
        })
        .into_any_element()
}

pub fn expanded(_: &mut Window, cx: &mut App) -> AnyElement {
    presentation(&FIXTURE, false, false, false, cx)
}
pub fn folded(_: &mut Window, cx: &mut App) -> AnyElement {
    let mut five = FIXTURE.clone();
    five.pins.push(TWENTY.pins[9].clone());
    let mut quiet = FIXTURE.clone();
    for pin in &mut quiet.pins {
        pin.changed = false;
        if matches!(pin.headline, Headline::Definition(_)) {
            pin.headline = Headline::Definition(DefinitionHeadline::Succeeded);
        }
    }
    div()
        .size_full()
        .flex()
        .gap(px(space::S16))
        .p(px(space::S16))
        .bg(cx.theme().colors.bg_sidebar)
        .child(
            div()
                .w(px(size::SIDEBAR))
                .child(gallery_group(&five, false, true, false, cx)),
        )
        .child(
            div()
                .w(px(size::RAIL))
                .child(gallery_group(&five, true, true, false, cx)),
        )
        .child(
            div()
                .w(px(size::SIDEBAR))
                .child(gallery_group(&quiet, false, true, false, cx)),
        )
        .into_any_element()
}
pub fn rail(_: &mut Window, cx: &mut App) -> AnyElement {
    presentation(&FIXTURE, true, false, false, cx)
}
pub fn peek(window: &mut Window, cx: &mut App) -> AnyElement {
    let tooltip = adoken_ui::Tooltip::text(
        adoken_app::shell::pinned::describe(&FIXTURE.pins[0], NOW).0,
    )(window, cx);
    div()
        .relative()
        .size_full()
        .child(presentation(&FIXTURE, true, false, true, cx))
        .child(
            div()
                .absolute()
                .left(px(size::RAIL + size::SIDEBAR))
                .top(px(216.0))
                .child(tooltip),
        )
        .into_any_element()
}
pub fn empty(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .child(presentation(&Snapshot::default(), false, false, false, cx))
        .child(
            div()
                .w(px(size::RAIL))
                .h_full()
                .bg(cx.theme().colors.bg_sidebar)
                .child(gallery_group(&Snapshot::default(), true, false, false, cx)),
        )
        .into_any_element()
}
pub fn twenty(_: &mut Window, cx: &mut App) -> AnyElement {
    div()
        .size_full()
        .flex()
        .child(presentation(&TWENTY, false, false, false, cx))
        .child(
            div()
                .w(px(size::RAIL))
                .h_full()
                .p(px(space::S8))
                .bg(cx.theme().colors.bg_sidebar)
                .child(gallery_group(&TWENTY, true, false, false, cx)),
        )
        .into_any_element()
}
pub fn unavailable(_: &mut Window, cx: &mut App) -> AnyElement {
    let mut snapshot = FIXTURE.clone();
    for index in [0, 2] {
        let pin = &mut snapshot.pins[index];
        pin.pin.unavailable = Some(PinUnavailable::Gone);
        pin.headline = Headline::NotAvailable(PinUnavailable::Gone);
        pin.changed = false;
    }
    presentation(&snapshot, false, false, false, cx)
}
pub fn stale(window: &mut Window, cx: &mut App) -> AnyElement {
    let mut snapshot = FIXTURE.clone();
    for pin in &mut snapshot.pins {
        pin.stale_since = Some(NOW - 18 * adoken_model::time::MINUTE);
    }
    let tooltip = adoken_ui::Tooltip::text(
        adoken_app::shell::pinned::describe(&snapshot.pins[0], NOW).0,
    )(window, cx);
    div()
        .relative()
        .size_full()
        .child(presentation(&snapshot, false, false, false, cx))
        .child(
            div()
                .absolute()
                .left(px(size::SIDEBAR))
                .top(px(216.0))
                .child(tooltip),
        )
        .into_any_element()
}

fn undo_toast() -> adoken_ui::Toast {
    adoken_ui::Toast::new("unpin", IconName::PinOff, "Unpinned !4293").action(
        adoken_ui::ToastAction::new("Undo", |_, cx| {
            adoken_ui::ToastStack::dismiss_global("unpin", cx);
        })
        .keys("cmd-z", "Command Z"),
    )
}

fn limit_toast() -> adoken_ui::Toast {
    adoken_ui::Toast::new(
        "limit",
        IconName::Info,
        "20 pins is the limit · unpin one first",
    )
    .tone(adoken_ui::ToastTone::Warning)
}

fn toasts(
    key: &'static str,
    toasts: Vec<adoken_ui::Toast>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let stack = window.use_keyed_state(key, cx, |_, cx| {
        let mut stack = adoken_ui::ToastStack::new(space::S16);
        for toast in toasts {
            stack.push(toast, cx);
        }
        stack.set_hovered(true, cx);
        stack
    });
    adoken_ui::ToastStack::install(stack.clone(), cx);
    div()
        .relative()
        .size_full()
        .bg(cx.theme().colors.bg_app)
        .child(stack)
        .into_any_element()
}

pub fn toast_action(window: &mut Window, cx: &mut App) -> AnyElement {
    toasts("toast-stack-action", vec![undo_toast()], window, cx)
}
pub fn toast_limit(window: &mut Window, cx: &mut App) -> AnyElement {
    toasts("toast-stack-limit", vec![limit_toast()], window, cx)
}
pub fn toast_three(window: &mut Window, cx: &mut App) -> AnyElement {
    toasts(
        "toast-stack-three",
        vec![
            undo_toast(),
            limit_toast(),
            adoken_ui::Toast::new(
                "hint",
                IconName::Pin,
                "Only pull requests, work items and pipelines can be pinned",
            ),
        ],
        window,
        cx,
    )
}

pub fn action_menu(window: &mut Window, cx: &mut App) -> AnyElement {
    use adoken_ui::action_menu::{ActionMenu, MenuItem, MenuPosition};
    let stack = window.use_keyed_state("row-action-menu-toast", cx, |_, _| {
        adoken_ui::ToastStack::new(space::S16)
    });
    adoken_ui::ToastStack::install(stack.clone(), cx);
    let menu = window.use_keyed_state("row-action-menu", cx, |window, cx| {
        let done = |_: &mut Window, cx: &mut App| {
            adoken_ui::ToastStack::show(
                adoken_ui::Toast::new("action", IconName::Check, "Action selected"),
                cx,
            );
        };
        ActionMenu::new(
            "Actions for !4293",
            vec![
                MenuItem::new("Open", done).keys("enter"),
                MenuItem::new("Open in browser", done).keys("cmd-enter"),
                MenuItem::new("Rename pin…", done),
                MenuItem::new("Move up", done)
                    .keys("alt-cmd-up")
                    .disabled(Some("Already first")),
                MenuItem::new("Move down", done).keys("alt-cmd-down"),
                MenuItem::new("Unpin", done).keys("backspace"),
            ],
            MenuPosition::at_pointer(gpui::point(px(space::S24), px(space::S24))),
            window,
            cx,
        )
    });
    div()
        .relative()
        .size_full()
        .bg(cx.theme().colors.bg_app)
        .child(menu)
        .child(stack)
        .into_any_element()
}
