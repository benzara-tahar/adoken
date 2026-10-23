//! Resource meter stories (spec 0023 AC-11, activity.md §2.7): the real
//! shell on the demo store with fixture activity, the drawer closed, and
//! the meter pinned to a fixture reading (no sampling), so snapshots don't
//! drift.

use adoken_app::activity::fixtures::{self, NOW, SyncScene};
use adoken_app::resource::meter::{Details, Memory, MeterText, Reading};
use adoken_app::shell::{Nav, Shell, StoreSource};
use adoken_app::sync::SyncState;
use adoken_sync::{Phase, SyncStatus};
use gpui::{AnyElement, App, Window, div, prelude::*};

const MIB: u64 = 1024 * 1024;

/// One meter story.
struct Scene {
    key: &'static str,
    text: MeterText,
    footprint_mb: u64,
    state: Option<SyncState>,
    menu: bool,
}

fn idle() -> SyncState {
    SyncState::Running(SyncStatus {
        phase: Phase::Idle,
        last_ok_hot: Some(NOW - 12_000),
        last_ok_warm: Some(NOW - 60_000),
        last_ok_cold: None,
    })
}

fn bar(scene: Scene, window: &mut Window, cx: &mut App) -> AnyElement {
    let shell = window.use_keyed_state(scene.key, cx, |window, cx| {
        let dir = crate::story_dir(scene.key);
        let store = adoken_demo::open_store_at(&dir, NOW).expect("seed the demo store");
        let mut shell = Shell::new(
            StoreSource::Open { store, demo: false },
            adoken_app::shell::commands(),
            window,
            cx,
        );
        shell.show(Nav::PullRequests, window, cx);
        shell
            .palette()
            .update(cx, |palette, cx| palette.set_clock(NOW, cx));
        let hub = fixtures::hub(fixtures::live_mix(NOW), fixtures::passes(NOW), NOW);
        shell.activity().update(cx, |activity, cx| {
            activity.set_hub(hub, cx);
            activity.set_fixture(NOW, Some(fixtures::inputs(SyncScene::Idle, NOW)), cx);
        });
        shell.set_sync_state(scene.state.clone(), cx);
        let details = Details {
            reading: Reading {
                footprint: scene.footprint_mb * MIB,
                resident: 325 * MIB,
                peak: Some(373 * MIB),
                cpu_ns: 134_000_000_000,
            },
            cpu_recent: scene.text.cpu.map(f64::from),
            uptime_ms: (3 * 3600 + 12 * 60) * 1000,
        };
        let meter = shell.resource_meter().clone();
        meter.update(cx, |meter, cx| {
            meter.set_fixture(scene.text.clone(), details, cx)
        });
        shell
    });
    if scene.menu {
        // The menu anchors to where the meter painted: open it once a
        // frame has laid the statusbar out (the snapshot's settling paint).
        let meter = shell.read(cx).resource_meter().clone();
        let (laid_out, open) = {
            let meter = meter.read(cx);
            (meter.laid_out(), meter.menu_open())
        };
        if laid_out && !open {
            meter.update(cx, |meter, cx| meter.open_menu(|_, _| {}, window, cx));
        }
    }
    div().size_full().child(shell).into_any_element()
}

/// "260 MB · 1% CPU" beside the network indicator (AC-11).
pub fn on(window: &mut Window, cx: &mut App) -> AnyElement {
    bar(
        Scene {
            key: "statusbar-resource-meter",
            text: MeterText {
                memory: Memory::Mb(260),
                cpu: Some(1),
            },
            footprint_mb: 258,
            state: Some(idle()),
            menu: false,
        },
        window,
        cx,
    )
}

/// A narrow window with a long sync label: the CPU part gives way first
/// (§2.5 step 2).
pub fn narrow(window: &mut Window, cx: &mut App) -> AnyElement {
    bar(
        Scene {
            key: "statusbar-resource-meter-narrow",
            text: MeterText {
                memory: Memory::Mb(260),
                cpu: Some(1),
            },
            footprint_mb: 258,
            state: Some(SyncState::Running(SyncStatus {
                phase: Phase::Throttled {
                    until: NOW + 40_000,
                    resource: Some("Core".into()),
                },
                last_ok_hot: Some(NOW - 12_000),
                last_ok_warm: Some(NOW - 60_000),
                last_ok_cold: None,
            })),
            menu: false,
        },
        window,
        cx,
    )
}

/// "1.2 GB · 3% CPU": the GB form (FR-6).
pub fn gigabytes(window: &mut Window, cx: &mut App) -> AnyElement {
    bar(
        Scene {
            key: "statusbar-resource-meter-gigabytes",
            text: MeterText {
                memory: Memory::Gb(12),
                cpu: Some(3),
            },
            footprint_mb: 1_229,
            state: Some(idle()),
            menu: false,
        },
        window,
        cx,
    )
}

/// The meter's menu: Copy resource usage, Hide from statusbar (FR-10).
pub fn menu(window: &mut Window, cx: &mut App) -> AnyElement {
    bar(
        Scene {
            key: "statusbar-resource-meter-menu",
            text: MeterText {
                memory: Memory::Mb(260),
                cpu: Some(1),
            },
            footprint_mb: 258,
            state: Some(idle()),
            menu: true,
        },
        window,
        cx,
    )
}
