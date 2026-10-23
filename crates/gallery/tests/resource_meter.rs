//! Resource meter (spec 0023) through the real shell on the demo store,
//! headless, on the deterministic test clock: the ⌘P commands turn it on
//! and off, steady samples never repaint the statusbar (AC-4), a real
//! change repaints once, and "Copy resource usage" copies the §2.7 line
//! (AC-9). The meter samples a scripted source; the copy reads the real
//! process. A screen lands in `target/resource-meter/`. macOS only, like
//! `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("resource_meter: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
    use std::time::Duration;

    use adoken_app::resource::meter::Reading;
    use adoken_app::resource::source::ResourceSource;
    use adoken_app::resource::{CopyResourceUsage, ResourceMeter, ToggleResourceMeter};
    use adoken_app::shell::{Shell, StoreSource};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{AppContext as _, Entity, HeadlessAppContext, WindowHandle, px, size};

    /// Footprint from a shared knob, idle CPU; counts its calls.
    struct Scripted {
        footprint_mb: Arc<AtomicU64>,
        calls: Arc<AtomicUsize>,
    }

    impl ResourceSource for Scripted {
        fn read(&mut self) -> Option<Reading> {
            self.calls.fetch_add(1, Relaxed);
            let footprint = self.footprint_mb.load(Relaxed) << 20;
            Some(Reading {
                footprint,
                resident: footprint + (65 << 20),
                peak: Some(footprint),
                cpu_ns: 0,
            })
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            adoken_gallery::init(cx);
            adoken_app::resource::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW)?;
        let window = cx.open_window(size(px(1200.0), px(520.0)), |window, cx| {
            cx.new(|cx| {
                Shell::new(
                    StoreSource::Open {
                        store: store.clone(),
                        demo: true,
                    },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                )
            })
        })?;
        settle(&mut cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell
                    .inbox_view()
                    .is_some_and(|inbox| !inbox.read(cx).row_ids().is_empty())
            })
        })
        .context("the store opens and the inbox loads")?;

        let footprint_mb = Arc::new(AtomicU64::new(260));
        let calls = Arc::new(AtomicUsize::new(0));
        let (shell, meter) = window.update(&mut cx, |shell, _, cx| {
            let meter = shell.resource_meter().clone();
            meter.update(cx, |meter, cx| {
                meter.set_source(Box::new(Scripted {
                    footprint_mb: footprint_mb.clone(),
                    calls: calls.clone(),
                }));
                // Headless windows are never key; the user's window is.
                meter.set_active(true, cx);
            });
            (cx.entity(), meter)
        })?;
        ensure!(
            text(&mut cx, &meter).is_none() && calls.load(Relaxed) == 0,
            "off by default: no meter, no sampling"
        );

        // ⌘P › "Statusbar: Show or hide memory and CPU": one sample at once
        // (AC-5).
        action(&mut cx, window, Box::new(ToggleResourceMeter))?;
        ensure!(
            text(&mut cx, &meter).as_deref() == Some("260 MB"),
            "shown on the next frame: {:?}",
            text(&mut cx, &meter)
        );
        ensure!(calls.load(Relaxed) == 1, "sampled once at once");
        // The first CPU interval completes and shows.
        tick(&mut cx, window, 5)?;
        ensure!(
            text(&mut cx, &meter).as_deref() == Some("260 MB · 0% CPU"),
            "{:?}",
            text(&mut cx, &meter)
        );
        paint(&mut cx, window, "on")?;

        // AC-4: 60 s of samples wobbling inside the hysteresis band:
        // the meter never notifies and the shell (statusbar) never
        // repaints for it.
        let meter_notifies = count_notifies(&mut cx, &meter);
        let shell_notifies = count_notifies(&mut cx, &shell);
        let before = calls.load(Relaxed);
        for step in 0..30u64 {
            footprint_mb.store(if step % 2 == 0 { 255 } else { 265 }, Relaxed);
            tick(&mut cx, window, 1)?;
        }
        let sampled = calls.load(Relaxed) - before;
        ensure!(
            (29..=31).contains(&sampled),
            "active cadence: one sample per 2 s ({sampled} in 60 s)"
        );
        ensure!(
            meter_notifies.0.get() == 0,
            "steady samples notified {} times",
            meter_notifies.0.get()
        );
        ensure!(
            shell_notifies.0.get() == 0,
            "the statusbar repainted {} times with an unchanged meter",
            shell_notifies.0.get()
        );

        // A real change shows within 5 s and repaints once.
        footprint_mb.store(400, Relaxed);
        tick(&mut cx, window, 3)?;
        ensure!(
            text(&mut cx, &meter).as_deref() == Some("400 MB · 0% CPU"),
            "{:?}",
            text(&mut cx, &meter)
        );
        ensure!(
            meter_notifies.0.get() == 1 && shell_notifies.0.get() == 1,
            "one change, one repaint (meter {}, shell {})",
            meter_notifies.0.get(),
            shell_notifies.0.get()
        );
        drop((meter_notifies, shell_notifies));

        // AC-9: "Copy resource usage" copies one line of the real process.
        action(&mut cx, window, Box::new(CopyResourceUsage))?;
        let copied = cx
            .update(|cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
            .context("clipboard has text")?;
        ensure!(
            copied.starts_with("adoken ") && copied.contains(" MB") && !copied.contains('\n'),
            "copied line: {copied:?}"
        );
        println!("copied: {copied}");

        // The same command again: gone, sampling stops.
        action(&mut cx, window, Box::new(ToggleResourceMeter))?;
        ensure!(text(&mut cx, &meter).is_none(), "hidden");
        let after = calls.load(Relaxed);
        tick(&mut cx, window, 10)?;
        ensure!(
            calls.load(Relaxed) == after,
            "off: no sampling ({} calls)",
            calls.load(Relaxed) - after
        );
        println!("resource_meter: ok ({sampled} samples in 60 s, 0 repaints while steady)");
        Ok(())
    }

    struct Counter(Rc<Cell<usize>>, #[allow(dead_code)] gpui::Subscription);

    fn count_notifies<T: 'static>(cx: &mut HeadlessAppContext, entity: &Entity<T>) -> Counter {
        let count = Rc::new(Cell::new(0));
        let seen = count.clone();
        let subscription = cx.update(|cx| cx.observe(entity, move |_, _| seen.set(seen.get() + 1)));
        Counter(count, subscription)
    }

    fn text(cx: &mut HeadlessAppContext, meter: &Entity<ResourceMeter>) -> Option<String> {
        cx.update(|cx| meter.read(cx).text().map(|t| t.full()))
    }

    /// `n` sampling periods of 2 s, painting each like the window would.
    fn tick(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        n: usize,
    ) -> anyhow::Result<()> {
        for _ in 0..n {
            cx.advance_clock(Duration::from_secs(2));
            cx.run_until_parked();
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
            })?;
        }
        Ok(())
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
        bail!("timed out")
    }

    /// An action dispatched on the focused path, as the palette does.
    fn action(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        action: Box<dyn gpui::Action>,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(action, cx);
        })?;
        cx.run_until_parked();
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
        let image = cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.render_to_image()
        })??;
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/resource-meter");
        std::fs::create_dir_all(&dir)?;
        image.save(dir.join(format!("{name}.png")))?;
        Ok(())
    }
}
