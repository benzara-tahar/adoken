//! Statusbar activity and the Activity drawer (spec 0016 AC-S5,
//! docs/design/activity.md §10) over the demo store with synthetic
//! activity fixtures. The clock is pinned to the dataset's "now" and the
//! time zone to UTC, so snapshots don't drift.

use adoken_app::activity::fixtures::{self, NOW, SyncScene};
use adoken_app::activity::indicator::{IndicatorView, ProblemTone, Problems};
use adoken_app::activity::statusbar::{self, LoadState};
use adoken_app::activity::{Activity, Tab};
use adoken_app::shell::{Nav, Shell, StoreSource};
use adoken_app::sync::SyncState;
use adoken_sync::activity::PassRecord;
use adoken_sync::cadence::Pause;
use adoken_sync::ledger::{Entry, Status};
use adoken_sync::{Phase, SyncStatus};
use adoken_theme::{radius, space, typography};
use adoken_ui::{ActiveTheme, Tooltip, TypeStyleExt};
use gpui::{AnyElement, App, Context, Entity, Window, div, prelude::*, px};

/// One drawer story: what the shell shows and the fixtures behind it.
struct Scene {
    key: &'static str,
    demo: bool,
    rail: bool,
    tab: Tab,
    sync: Option<SyncScene>,
    entries: fn() -> Vec<Entry>,
    passes: fn() -> Vec<PassRecord>,
    state: Option<SyncState>,
    /// Extra setup once the drawer is open.
    after: fn(&mut Shell, &mut Window, &mut Context<Shell>),
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            key: "",
            demo: false,
            rail: false,
            tab: Tab::Sync,
            sync: Some(SyncScene::Idle),
            entries: || fixtures::live_mix(NOW),
            passes: || fixtures::passes(NOW),
            state: None,
            after: |_, _, _| {},
        }
    }
}

fn running(phase: Phase) -> SyncState {
    SyncState::Running(SyncStatus {
        phase,
        last_ok_hot: Some(NOW - 12_000),
        last_ok_warm: Some(NOW - 60_000),
        last_ok_cold: None,
    })
}

/// The real shell on the demo store with the drawer open on `scene`.
fn drawer(scene: Scene, window: &mut Window, cx: &mut App) -> AnyElement {
    let shell = window.use_keyed_state(scene.key, cx, |window, cx| {
        let dir = crate::story_dir(scene.key);
        let store = adoken_demo::open_store_at(&dir, NOW).expect("seed the demo store");
        let mut shell = Shell::new(
            StoreSource::Open {
                store,
                demo: scene.demo,
            },
            adoken_app::shell::commands(),
            window,
            cx,
        );
        shell.show(Nav::PullRequests, window, cx);
        shell.set_sidebar_collapsed(scene.rail, cx);
        shell
            .palette()
            .update(cx, |palette, cx| palette.set_clock(NOW, cx));
        let hub = fixtures::hub((scene.entries)(), (scene.passes)(), NOW);
        let inputs = scene.sync.map(|s| fixtures::inputs(s, NOW));
        shell.activity().update(cx, |activity, cx| {
            activity.set_hub(hub, cx);
            activity.set_fixture(NOW, inputs, cx);
        });
        if scene.state.is_some() {
            shell.set_sync_state(scene.state.clone(), cx);
        }
        shell.open_activity(scene.tab, false, window, cx);
        (scene.after)(&mut shell, window, cx);
        // Lay the table out over the fixture rows before the first frame.
        if let Some(panel) = shell.activity_panel().cloned() {
            panel.update(cx, |panel, cx| panel.sync_table_now(cx));
        }
        shell
    });
    div().size_full().child(shell).into_any_element()
}

fn activity_of(shell: &Shell) -> Entity<Activity> {
    shell.activity().clone()
}

// ── Sync ────────────────────────────────────────────────────────────────

pub fn sync_idle(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync",
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_syncing(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-syncing",
            sync: Some(SyncScene::Syncing),
            passes: || {
                let mut passes = vec![fixtures::running_pass(NOW)];
                passes.extend(fixtures::passes(NOW));
                passes
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_first(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-first",
            sync: Some(SyncScene::First),
            passes: || vec![fixtures::running_pass(NOW)],
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_throttled(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-throttled",
            sync: Some(SyncScene::Throttled),
            state: Some(running(Phase::Throttled {
                until: NOW + 40_000,
                resource: Some("Core".into()),
            })),
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_failed(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-failed",
            sync: Some(SyncScene::Failed),
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_paused(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-paused",
            sync: Some(SyncScene::Paused),
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_missing_scope(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-scope",
            sync: Some(SyncScene::MissingScope),
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_demo(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-demo",
            demo: true,
            sync: None,
            entries: Vec::new,
            passes: Vec::new,
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_empty(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-empty",
            sync: Some(SyncScene::First),
            passes: Vec::new,
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn sync_no_token(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-no-token",
            state: Some(SyncState::NoToken),
            sync: None,
            entries: Vec::new,
            passes: Vec::new,
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// 900 × 600 with the sidebar: the drawer is < 760 px wide, one column.
pub fn sync_narrow(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-sync-narrow",
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// 900 × 600 with the rail.
pub fn drawer_rail(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-drawer-rail",
            rail: true,
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// The drawer at its 160 px minimum on a 600 px window, on Network.
pub fn drawer_min(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-drawer-min",
            tab: Tab::Network,
            after: |shell, _, cx| {
                activity_of(shell).update(cx, |a, cx| a.set_height(160.0, cx));
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

// ── Network ─────────────────────────────────────────────────────────────

pub fn network(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network",
            tab: Tab::Network,
            after: |shell, _, cx| {
                activity_of(shell).update(cx, |a, cx| {
                    a.net.move_cursor(-3);
                    a.net.jump_to_latest();
                    cx.notify();
                });
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// Failed only, scrolled up, with new requests arriving ("N new").
pub fn network_failed(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-failed",
            tab: Tab::Network,
            entries: || fixtures::ledger(400, 12, NOW - 60_000),
            after: |shell, _, cx| {
                let activity = activity_of(shell);
                activity.update(cx, |a, cx| {
                    a.net.update_filter(|f| f.failed_only = true);
                    a.net.jump_to_latest();
                    a.net.move_cursor(-6);
                    cx.notify();
                });
                if let Some(panel) = shell.activity_panel().cloned() {
                    panel.update(cx, |panel, cx| panel.sync_table_now(cx));
                }
                let hub = activity.read(cx).hub().cloned().expect("fixture hub");
                for k in 0..3u64 {
                    hub.net.record_fixture(fixtures::entry(
                        401 + k,
                        NOW - 1_000 + k as i64,
                        Status::Http(503),
                        fixtures::lane_origin(),
                    ));
                }
                activity.update(cx, |a, cx| a.refresh(cx));
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// The 503's detail: rate headers, Retry-After, activity id.
pub fn network_detail(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-detail",
            tab: Tab::Network,
            after: |shell, _, cx| {
                activity_of(shell).update(cx, |a, cx| {
                    let failed =
                        a.net.visible().iter().copied().find(|seq| {
                            a.net.entry(*seq).map(|e| e.status) == Some(Status::Http(503))
                        });
                    a.net.cursor = failed;
                    a.net.follow = false;
                    a.net.toggle_detail();
                    cx.notify();
                });
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn network_totals(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-totals",
            tab: Tab::Network,
            entries: || fixtures::ledger(600, 9, NOW),
            after: |shell, _, cx| {
                if let Some(panel) = shell.activity_panel().cloned() {
                    panel.update(cx, |panel, cx| panel.show_totals(true, cx));
                }
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn network_empty(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-empty",
            tab: Tab::Network,
            entries: Vec::new,
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn network_filtered(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-filtered",
            tab: Tab::Network,
            after: |shell, _, cx| {
                if let Some(panel) = shell.activity_panel().cloned() {
                    panel.update(cx, |panel, cx| panel.set_filter_text("threads 418", cx));
                }
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}

pub fn network_demo(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-demo",
            demo: true,
            tab: Tab::Network,
            sync: None,
            entries: Vec::new,
            passes: Vec::new,
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// 10,000 rows scrolled to the middle (900 px: column drops).
pub fn network_10k(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-ten-thousand",
            tab: Tab::Network,
            entries: || fixtures::ledger(10_000, 37, NOW),
            after: middle,
            ..Scene::default()
        },
        window,
        cx,
    )
}

/// 10,000 rows at 1440 px: every column.
pub fn network_10k_wide(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "activity-network-ten-thousand-wide",
            tab: Tab::Network,
            entries: || fixtures::ledger(10_000, 37, NOW),
            after: middle,
            ..Scene::default()
        },
        window,
        cx,
    )
}

fn middle(shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) {
    activity_of(shell).update(cx, |a, cx| {
        a.net.home();
        a.net.move_cursor(5_000);
        cx.notify();
    });
    if let Some(panel) = shell.activity_panel().cloned() {
        panel.update(cx, |panel, cx| panel.sync_table_now(cx));
    }
}

// ── statusbar ───────────────────────────────────────────────────────────

/// Every statusbar activity state (activity.md §10 `statusbar-activity`).
pub fn statusbar_states(_window: &mut Window, cx: &mut App) -> AnyElement {
    let offset = time::UtcOffset::UTC;
    let item = |load, demo, state: Option<SyncState>, pause| {
        statusbar::sync_item(
            load,
            demo,
            state.as_ref(),
            Some(NOW - 12_000),
            pause,
            false,
            offset,
            NOW,
        )
    };
    let shown = |busy, value: Option<&str>, problems| IndicatorView::Shown {
        busy,
        value: value.map(str::to_owned),
        problems,
    };
    let danger = Some(Problems {
        tone: ProblemTone::Danger,
        danger: 2,
        warning: 0,
    });
    let warning = Some(Problems {
        tone: ProblemTone::Warning,
        danger: 0,
        warning: 1,
    });
    let idle = Some(running(Phase::Idle));
    let rows = vec![
        (
            "idle",
            item(LoadState::Ready, false, idle.clone(), Pause::Off),
            shown(false, Some("200"), None),
            false,
        ),
        (
            "in flight",
            item(
                LoadState::Ready,
                false,
                Some(running(Phase::Syncing(adoken_sync::Tier::Hot))),
                Pause::Off,
            ),
            shown(true, Some("3"), None),
            false,
        ),
        (
            "failures",
            item(LoadState::Ready, false, idle.clone(), Pause::Off),
            shown(false, Some("503"), danger),
            false,
        ),
        (
            "warning",
            item(LoadState::Ready, false, idle.clone(), Pause::Off),
            shown(false, Some("404"), warning),
            false,
        ),
        (
            "paused",
            item(
                LoadState::Ready,
                false,
                idle.clone(),
                Pause::Until(NOW + 3_600_000),
            ),
            shown(false, Some("200"), None),
            false,
        ),
        (
            "paused until resumed",
            item(LoadState::Ready, false, idle.clone(), Pause::UntilResumed),
            shown(false, Some("200"), None),
            false,
        ),
        (
            "throttled",
            item(
                LoadState::Ready,
                false,
                Some(running(Phase::Throttled {
                    until: NOW + 40_000,
                    resource: Some("Core".into()),
                })),
                Pause::Off,
            ),
            shown(false, Some("429"), warning),
            false,
        ),
        (
            "offline",
            item(
                LoadState::Ready,
                false,
                Some(running(Phase::Offline)),
                Pause::Off,
            ),
            IndicatorView::Offline,
            false,
        ),
        (
            "no token",
            item(
                LoadState::Ready,
                false,
                Some(SyncState::NoToken),
                Pause::Off,
            ),
            IndicatorView::Hidden,
            false,
        ),
        (
            "token rejected",
            item(
                LoadState::Ready,
                false,
                Some(running(Phase::Unauthorized)),
                Pause::Off,
            ),
            shown(false, Some("401"), danger),
            false,
        ),
        (
            "demo",
            item(LoadState::Ready, true, None, Pause::Off),
            IndicatorView::Hidden,
            true,
        ),
        (
            "indicator off",
            item(LoadState::Ready, false, idle, Pause::Off),
            IndicatorView::Hidden,
            false,
        ),
    ];
    let theme = cx.theme().clone();
    let colors = &theme.colors;
    div()
        .size_full()
        .p(px(space::S12))
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .bg(colors.bg_app)
        .children(rows.into_iter().map(|(name, item, view, demo)| {
            div()
                .flex()
                .flex_col()
                .gap(px(space::S2))
                .child(
                    div()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, &theme)
                        .child(name),
                )
                .child(
                    div()
                        .h(px(24.0))
                        .px(px(space::S12))
                        .flex()
                        .items_center()
                        .gap(px(space::S6))
                        .rounded(px(radius::XS))
                        .border_t_1()
                        .border_color(colors.border_subtle)
                        .bg(colors.bg_sidebar)
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, &theme)
                        .child(statusbar::render_sync_item(
                            &item,
                            false,
                            false,
                            false,
                            &theme,
                            |_, _, _| {},
                        ))
                        .child("· Contoso")
                        .child(div().flex_1())
                        .children(statusbar::render_network(
                            &view,
                            false,
                            false,
                            0.0,
                            &theme,
                            |window, cx| Tooltip::text("")(window, cx),
                            |_, _, _| {},
                        ))
                        .when(demo, |d| {
                            d.child(
                                div()
                                    .px(px(space::S6))
                                    .rounded(px(radius::XS))
                                    .bg(colors.bg_raised)
                                    .child("Demo mode · fake data, stays on this Mac"),
                            )
                        }),
                )
        }))
        .into_any_element()
}

/// 900 px window with the drawer closed and a throttled sync: the §2.5
/// collapse order in the real statusbar.
pub fn statusbar_900(window: &mut Window, cx: &mut App) -> AnyElement {
    drawer(
        Scene {
            key: "statusbar-activity-narrow",
            state: Some(running(Phase::Throttled {
                until: NOW + 40_000,
                resource: Some("Core".into()),
            })),
            after: |shell, _, cx| {
                activity_of(shell).update(cx, |a, cx| a.close(cx));
            },
            ..Scene::default()
        },
        window,
        cx,
    )
}
