//! App shell and Cmd+P stories over the demo org (fake Contoso data).
//!
//! Views are real `adoken_app` entities kept alive with
//! `Window::use_keyed_state`; relative times are pinned to the dataset's
//! fixed "now" so snapshots don't drift with the wall clock.

use std::sync::{Arc, LazyLock};

use adoken_app::palette::PaletteCommand;
use adoken_app::palette::catalog::{Catalog, CatalogData};
use adoken_app::shell::{Nav, Shell, StoreSource};
use adoken_app::sync::{SyncState, indicator, label};
use adoken_demo::Dataset;
use adoken_sync::{Phase, SyncStatus, Tier};
use adoken_theme::{space, typography};
use adoken_ui::{ActiveTheme, Button, ButtonVariant, EmptyState, IconName, Kbd, TypeStyleExt};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

const DEMO_SEED: u64 = adoken_demo::DEFAULT_SEED;

static DATASET: LazyLock<Dataset> = LazyLock::new(|| adoken_demo::dataset(DEMO_SEED));

static CATALOG: LazyLock<Arc<Catalog>> = LazyLock::new(|| {
    let commands = adoken_app::shell::commands()
        .iter()
        .map(|c| adoken_app::palette::catalog::CommandEntry {
            title: c.title,
            keys: c.keys,
        })
        .collect();
    Arc::new(Catalog::new(CatalogData::from_dataset(&DATASET), commands))
});

fn dataset() -> &'static Dataset {
    &DATASET
}

fn catalog() -> Arc<Catalog> {
    CATALOG.clone()
}

fn commands() -> Vec<PaletteCommand> {
    adoken_app::shell::commands()
}

/// The palette, open over an empty window, with `query` typed.
fn palette(
    key: &'static str,
    query: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let shell = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        let store = adoken_demo::open_store_at(&dir, dataset().now).expect("seed the demo store");
        let shell = Shell::new(
            StoreSource::Open { store, demo: true },
            commands(),
            window,
            cx,
        );
        shell.palette().update(cx, |palette, cx| {
            palette.set_clock(dataset().now, cx);
            palette.set_inline(true, cx);
            palette.set_catalog(catalog(), cx);
            palette.open_with(query, window, cx);
        });
        shell
    });
    div()
        .size_full()
        .child(shell.read(cx).palette().clone())
        .into_any_element()
}

pub fn palette_my_work_story(window: &mut Window, cx: &mut App) -> AnyElement {
    palette("palette-my-work", "", window, cx)
}

pub fn palette_prs_story(window: &mut Window, cx: &mut App) -> AnyElement {
    palette("palette-prs", "!retry", window, cx)
}

pub fn palette_filters_story(window: &mut Window, cx: &mut App) -> AnyElement {
    palette("palette-filters", "# type:bug is:open", window, cx)
}

pub fn palette_link_story(window: &mut Window, cx: &mut App) -> AnyElement {
    palette(
        "palette-link",
        "https://dev.azure.com/fabrikam/Web/_git/site/pullrequest/912",
        window,
        cx,
    )
}

pub fn palette_commands_story(window: &mut Window, cx: &mut App) -> AnyElement {
    palette("palette-commands", ">", window, cx)
}

/// The whole shell on a seeded demo store, showing `nav`.
pub(crate) fn shell(key: &'static str, nav: Nav, window: &mut Window, cx: &mut App) -> AnyElement {
    shell_with(key, nav, false, window, cx)
}

/// [`shell`] with the sidebar collapsed to its rail or expanded.
pub(crate) fn shell_with(
    key: &'static str,
    nav: Nav,
    collapsed: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let shell = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        let store = adoken_demo::open_store_at(&dir, dataset().now).expect("seed the demo store");
        let mut shell = Shell::new(
            StoreSource::Open { store, demo: true },
            commands(),
            window,
            cx,
        );
        shell.show(nav, window, cx);
        shell.set_sidebar_collapsed(collapsed, cx);
        shell
            .palette()
            .update(cx, |palette, cx| palette.set_clock(dataset().now, cx));
        shell
    });
    div().size_full().child(shell).into_any_element()
}

pub fn shell_search_story(window: &mut Window, cx: &mut App) -> AnyElement {
    shell("shell-search", Nav::Search, window, cx)
}

pub fn empty_state_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    EmptyState::new(IconName::Search, "Find anything")
        .body("Pull requests, work items, pipeline runs, people.")
        .action(
            Button::new("empty-search", "Search")
                .variant(ButtonVariant::Primary)
                .shortcut(Kbd::parse("secondary-p").expect("static binding")),
        )
        .into_any_element()
}

/// Every statusbar sync state (spec 0002 §3), clock pinned to the dataset.
pub fn sync_status_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let now = dataset().now;
    let running = |phase, last_ok_warm| {
        SyncState::Running(SyncStatus {
            phase,
            last_ok_hot: None,
            last_ok_warm,
            last_ok_cold: None,
        })
    };
    let states = [
        SyncState::Starting,
        running(Phase::Syncing(Tier::Warm), None),
        running(Phase::Idle, Some(now - 12_000)),
        running(Phase::Syncing(Tier::Hot), Some(now - 60_000)),
        running(Phase::Offline, Some(now - 3 * 60_000)),
        running(
            Phase::Throttled {
                until: now + 40_000,
                resource: Some("Core".into()),
            },
            Some(now - 60_000),
        ),
        running(Phase::Unauthorized, Some(now - 60_000)),
        SyncState::NoToken,
        SyncState::NoProjects,
    ];
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .size_full()
        .p(px(space::S16))
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .bg(colors.bg_sidebar)
        .text_color(colors.text_secondary)
        .type_style(typography::SMALL, theme)
        .children(states.iter().map(|state| {
            let (tone, text) = label(state, None, now);
            let (dot, color) = indicator(tone, colors);
            div()
                .flex()
                .items_center()
                .gap(px(space::S6))
                .child(div().text_color(color).child(dot))
                .child(text)
        }))
        .into_any_element()
}

/// The titlebar's back / forward buttons (spec 0018 AC-10): both disabled
/// at launch, back only, both, each with the tooltip it shows on hover.
pub fn history_buttons_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    use adoken_app::shell::navigate::{NavButtons, back_tooltip, forward_tooltip};
    use adoken_theme::size;
    let states: [(Option<&str>, Option<&str>); 3] = [
        (None, None),
        (Some("!4821 Retry idempotency keys"), None),
        (
            Some("Inbox · Unread · Pull requests"),
            Some("#20101 Fix login redirect"),
        ),
    ];
    let theme = cx.theme();
    let colors = &theme.colors;
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(colors.bg_app)
        .children(states.into_iter().map(|(back, forward)| {
            div()
                .h(px(size::TITLEBAR))
                .px(px(space::S12))
                .flex()
                .items_center()
                .gap(px(space::S12))
                .border_b_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_sidebar)
                .child(NavButtons::new(
                    back.map(Into::into),
                    forward.map(Into::into),
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, theme)
                        .child(back_tooltip(back))
                        .child(forward_tooltip(forward)),
                )
        }))
        .into_any_element()
}
