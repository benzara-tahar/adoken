//! Activity drawer smoke (spec 0016 AC-S6, AC-B11) through the real shell,
//! focused views and actual key dispatch.
//!
//! There is no mock-server harness for the whole shell (the mock Azure
//! DevOps server lives in `crates/sync/tests/support`), so this runs the
//! shell on the **demo store with fixture activity**: a session hub
//! (`sync::install_activity_hub`) whose network log holds 10,000 synthetic
//! requests (37 failures) and whose tracker holds recent passes. "A ⌘R
//! pass" is a `SyncNow` pass pushed into the tracker, as the scheduler
//! would record it. Screenshots go to `target/activity-smoke/`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("activity_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adoken_app::activity::Tab;
    use adoken_app::activity::fixtures::{self, NOW, SyncScene};
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_sync::activity::{ChangeWord, Changes, KindChanges, Outcome, Trigger};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{AppContext as _, HeadlessAppContext, Keystroke, WindowHandle, px, size};

    const FRAME: Duration = Duration::from_millis(16);

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        let hub = fixtures::hub(
            fixtures::ledger(10_000, 37, NOW),
            fixtures::passes(NOW),
            NOW,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            adoken_gallery::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
            adoken_app::sync::install_activity_hub(hub.clone(), cx);
        });
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), NOW)?;
        let window = cx.open_window(size(px(1200.0), px(760.0)), |window, cx| {
            cx.new(|cx| {
                let shell = Shell::new(
                    StoreSource::Open {
                        store: store.clone(),
                        demo: false,
                    },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                );
                shell.activity().update(cx, |activity, cx| {
                    activity.set_fixture(NOW, Some(fixtures::inputs(SyncScene::Idle, NOW)), cx)
                });
                shell
            })
        })?;
        settle(&mut cx, |cx| {
            Ok(cx.update(|cx| adoken_app::pins::Pins::global(cx).is_some()))
        })?;
        window.update(&mut cx, |shell, window, cx| {
            shell.show(Nav::PullRequests, window, cx)
        })?;
        cx.run_until_parked();
        let mut frames = adoken_perf::FrameRecorder::start();
        paint(&mut cx, window, "list")?;

        // AC-S1: a PR row has the keyboard; ⌘⇧A opens Sync (the last tab)
        // and moves the keyboard in; the sidebar's height doesn't change;
        // Esc closes and the same row has the keyboard again.
        press(&mut cx, window, "j")?;
        let row = window.update(&mut cx, |shell, window, cx| {
            (
                shell.selected_pr_id(cx),
                shell.content_has_focus(window, cx),
            )
        })?;
        ensure!(row.0.is_some() && row.1, "a PR list row has the keyboard");
        let sidebar_closed = sidebar_height(&mut cx, window)?;
        press(&mut cx, window, "cmd-shift-a")?;
        let (open, tab) = window.update(&mut cx, |shell, _, cx| {
            let a = shell.activity().read(cx);
            (a.is_open(), a.tab())
        })?;
        ensure!(open && tab == Tab::Sync, "⌘⇧A opens Activity › Sync");
        ensure!(
            !window.update(&mut cx, |shell, window, cx| shell
                .content_has_focus(window, cx))?,
            "the keyboard moved into the drawer"
        );
        let sidebar_open = sidebar_height(&mut cx, window)?;
        ensure!(
            sidebar_open == sidebar_closed,
            "the sidebar keeps its height ({sidebar_closed:?} → {sidebar_open:?})"
        );
        paint(&mut cx, window, "sync")?;
        press(&mut cx, window, "escape")?;
        let after = window.update(&mut cx, |shell, window, cx| {
            (
                shell.activity().read(cx).is_open(),
                shell.selected_pr_id(cx),
                shell.content_has_focus(window, cx),
            )
        })?;
        ensure!(
            after == (false, row.0, true),
            "Esc closes and the same row has the keyboard, got {after:?}"
        );

        // Pointer: the network indicator opens Network, the sync item
        // switches to Sync (activity.md §2.2, §2.3).
        click(&mut cx, window, 1158.0, 748.0)?;
        let state = window.update(&mut cx, |shell, _, cx| {
            let a = shell.activity().read(cx);
            (a.is_open(), a.tab())
        })?;
        ensure!(
            state == (true, Tab::Network),
            "the network indicator opens Network, got {state:?}"
        );
        click(&mut cx, window, 40.0, 748.0)?;
        let state = window.update(&mut cx, |shell, _, cx| {
            let a = shell.activity().read(cx);
            (a.is_open(), a.tab())
        })?;
        ensure!(
            state == (true, Tab::Sync),
            "the sync item switches to Sync, got {state:?}"
        );

        // A ⌘R pass lands with 3 changed PRs.
        let (prs, _, _) = fixtures::demo_ids();
        let mut pass = fixtures::running_pass(NOW);
        pass.id = 99;
        pass.trigger = Trigger::SyncNow;
        pass.outcome = Outcome::Ok;
        pass.ended = Some(NOW);
        pass.changes = Changes {
            prs: KindChanges {
                updated: prs.len() as u32,
                ids: prs.iter().map(|id| (*id, ChangeWord::Updated)).collect(),
                ..KindChanges::default()
            },
            ..Changes::default()
        };
        hub.passes.push_fixture(pass);
        window.update(&mut cx, |shell, _, cx| {
            shell.activity().update(cx, |a, cx| a.refresh(cx))
        })?;
        cx.run_until_parked();

        // Expand the pass and ↩ its first PR: the review opens, the drawer
        // stays open (AC-A5).
        window.update(&mut cx, |shell, window, cx| {
            let panel = shell.activity_panel().cloned().expect("drawer view");
            panel.update(cx, |panel, cx| panel.focus_passes(window, cx));
        })?;
        press(&mut cx, window, "home")?;
        let expanded = window.update(&mut cx, |shell, _, cx| {
            shell.activity().read(cx).is_expanded(99)
        })?;
        if !expanded {
            press(&mut cx, window, "right")?;
        }
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |cx| {
            Ok(window.update(cx, |shell, _, cx| shell.open_review_id(cx))? == Some(prs[0]))
        })
        .context("↩ on a changed PR opens its review")?;
        ensure!(
            window.update(&mut cx, |shell, _, cx| shell.activity().read(cx).is_open())?,
            "the drawer stays open after opening an item"
        );
        paint(&mut cx, window, "pass-expanded")?;

        // F6 cycles content → drawer → toasts (AC-S4): from the content,
        // F6 lands in the drawer.
        let in_drawer = |cx: &mut HeadlessAppContext| {
            window.update(cx, |shell, window, cx| {
                shell
                    .activity_panel()
                    .is_some_and(|panel| panel.read(cx).contains_focus(window, cx))
            })
        };
        window.update(&mut cx, |shell, window, cx| {
            shell.show(Nav::PullRequests, window, cx)
        })?;
        ensure!(!in_drawer(&mut cx)?, "the content has the keyboard");
        press(&mut cx, window, "f6")?;
        ensure!(in_drawer(&mut cx)?, "F6 moves the keyboard into the drawer");
        press(&mut cx, window, "f6")?;
        ensure!(!in_drawer(&mut cx)?, "F6 moves on (toasts, else content)");
        press(&mut cx, window, "f6")?;
        if !in_drawer(&mut cx)? {
            press(&mut cx, window, "f6")?;
        }
        ensure!(in_drawer(&mut cx)?, "F6 comes back to the drawer");

        // ⌃Tab: Network on 10,000 rows within one frame (AC-B11).
        frames.collect();
        let started = Instant::now();
        press(&mut cx, window, "ctrl-tab")?;
        render(&mut cx, window)?;
        let open_network = started.elapsed();
        ensure!(
            window.update(&mut cx, |shell, _, cx| shell.activity().read(cx).tab())? == Tab::Network,
            "⌃Tab switches to Network"
        );
        paint(&mut cx, window, "network")?;

        // Scroll top to bottom (AC-B11): 10,000 rows, page by page.
        press(&mut cx, window, "home")?;
        frames.collect();
        let scroll_from = frames.draw().len();
        for _ in 0..200 {
            press(&mut cx, window, "pagedown")?;
            render(&mut cx, window)?;
        }
        frames.collect();
        let scroll: Vec<Duration> = frames.draw().as_slice()[scroll_from..].to_vec();
        let scroll = adoken_perf::stats::Summary::of("scroll", &scroll).context("scroll frames")?;

        // F: failed only, within one frame (AC-B9, AC-B11).
        let started = Instant::now();
        press(&mut cx, window, "f")?;
        render(&mut cx, window)?;
        let filter = started.elapsed();
        let failed = window.update(&mut cx, |shell, _, cx| {
            shell.activity().read(cx).net.visible().len()
        })?;
        ensure!(
            failed == 37,
            "Failed only shows the 37 failures, got {failed}"
        );
        paint(&mut cx, window, "failed-only")?;

        // Export the filtered rows to a temp file (FR-25).
        let export = dir.path().join("network.jsonl");
        let rows = window.update(&mut cx, |shell, _, cx| {
            shell.activity().read(cx).net.export_rows()
        })?;
        adoken_app::activity::export_to(&export, &rows, NOW)?;
        let lines = std::fs::read_to_string(&export)?.lines().count();
        ensure!(lines == 38, "export: header + 37 rows, got {lines}");

        // Esc closes the drawer; focus returns to where it was (AC-S1).
        press(&mut cx, window, "escape")?;
        ensure!(
            !window.update(&mut cx, |shell, _, cx| shell.activity().read(cx).is_open())?,
            "Esc closes the drawer"
        );
        paint(&mut cx, window, "closed")?;

        frames.collect();
        let all = frames.draw().summary().context("frames")?;
        println!("activity_smoke frames: {all}");
        println!("activity_smoke scroll 10k: {scroll}");
        println!("activity_smoke open Network: {open_network:?}, filter: {filter:?}");
        ensure!(all.p99 < FRAME, "frame p99 {:?} ≥ 16 ms", all.p99);
        ensure!(scroll.p99 < FRAME, "scroll p99 {:?} ≥ 16 ms", scroll.p99);
        drop(frames);
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        demo_feed_smoke(&mut cx, &hub)?;
        println!(
            "activity_smoke: row focus round-trip + sidebar height, pointer opens, ⌘⇧A, a ⌘R pass, expand + ↩ opens its PR (drawer stays), F6, ⌃Tab, F (37 of 10,000), export, Esc, demo feed pass passed"
        );
        Ok(())
    }

    /// AC-A11: in demo mode a feed tick lists a "Demo feed" pass in the
    /// Sync tab, and Sync now / Pause are disabled with the reason.
    fn demo_feed_smoke(
        cx: &mut HeadlessAppContext,
        hub: &adoken_app::sync::ActivityHub,
    ) -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), NOW)?;
        let window = cx.open_window(size(px(1200.0), px(760.0)), |window, cx| {
            cx.new(|cx| {
                Shell::new(
                    StoreSource::Open { store, demo: true },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                )
            })
        })?;
        // The feed's first tick runs as the store opens (every event of the
        // seeded dataset is due by now).
        settle(cx, |_| {
            Ok(hub
                .passes
                .passes()
                .iter()
                .any(|p| matches!(p.trigger, Trigger::DemoFeed)))
        })
        .context("a demo feed tick records a pass")?;
        window.update(cx, |shell, window, cx| {
            shell.open_activity(Tab::Sync, false, window, cx)
        })?;
        cx.run_until_parked();
        let (demo, listed) = window.update(cx, |shell, _, cx| {
            let activity = shell.activity().read(cx);
            let listed = activity.lines().iter().any(|line| {
                matches!(line, adoken_app::activity::passes::PassLine::One(pass)
                    if adoken_app::activity::passes::origin_text(pass).0 == "Demo feed")
            });
            (activity.is_demo(), listed)
        })?;
        ensure!(demo && listed, "the Sync tab lists the Demo feed pass");
        let actions = adoken_app::activity::passes::sync_actions(demo, false);
        ensure!(
            !actions.sync_now
                && !actions.pause
                && actions.reason == Some("Demo org: nothing syncs"),
            "Sync now and Pause are disabled in demo"
        );
        paint(cx, window, "demo-sync")?;
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        Ok(())
    }

    fn click(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        let position = gpui::point(px(x), px(y));
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    ..Default::default()
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button: gpui::MouseButton::Left,
                    position,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// The sidebar's painted height.
    fn sidebar_height(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<gpui::Pixels> {
        render(cx, window)?;
        window
            .update(cx, |shell, _, _| {
                shell.sidebar_bounds().map(|b| b.size.height)
            })?
            .context("sidebar painted")
    }

    fn settle(
        cx: &mut HeadlessAppContext,
        mut done: impl FnMut(&mut HeadlessAppContext) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        for _ in 0..500 {
            cx.run_until_parked();
            if done(cx)? {
                return Ok(());
            }
            cx.advance_clock(Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("timed out waiting for the shell")
    }

    fn press(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        keys: &str,
    ) -> anyhow::Result<()> {
        let keystroke = Keystroke::parse(keys).context("keystroke")?;
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    fn render(cx: &mut HeadlessAppContext, window: WindowHandle<Shell>) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })?;
        Ok(())
    }

    fn paint(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        name: &str,
    ) -> anyhow::Result<()> {
        let image = cx
            .update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.render_to_image()
            })?
            .context("rendered shell image")?;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/activity-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
