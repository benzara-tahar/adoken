//! Settings (spec 0009) through the real app code, headless: queued writes
//! coalesce and keep other tables and comments (AC-1), a broken file is
//! never overwritten (AC-3), outside edits are picked up by the poll (FR-4),
//! `[general] motion` applies live (FR-3), and every tab renders over the
//! written file, also while searching. macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("settings: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use adoken_app::settings::{self, SettingsView, Tab};
    use anyhow::{Context as _, ensure};
    use gpui::{AppContext as _, HeadlessAppContext, Render, px, size};

    const BEFORE: &str = "# my notes stay\n[palette.rows.pr]\ntitle = \"{title}\" # custom\n";

    /// Counts `settings::observe` callbacks.
    struct Watcher {
        seen: Rc<Cell<usize>>,
        _subscription: gpui::Subscription,
    }

    impl Render for Watcher {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).expect("valid test TOML")
    }

    pub fn run() -> anyhow::Result<()> {
        // The env override would mask the setting under test.
        // SAFETY: single-threaded here, before any app thread starts.
        unsafe { std::env::remove_var("ADOKEN_REDUCE_MOTION") };
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, BEFORE)?;

        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            adoken_app::background::init(cx);
            adoken_ui::Theme::install(
                adoken_theme::ThemeRegistry::builtin()
                    .default_variant(adoken_theme::Appearance::Dark),
                cx,
            );
            settings::init(path.clone(), true, cx);
        });
        cx.run_until_parked();
        let seen = Rc::new(Cell::new(0));
        let _watcher = cx.open_window(size(px(10.0), px(10.0)), {
            let seen = seen.clone();
            move |_, cx| {
                cx.new(|cx| Watcher {
                    seen: seen.clone(),
                    _subscription: settings::observe(cx, move |this: &mut Watcher, _| {
                        this.seen.set(this.seen.get() + 1)
                    }),
                })
            }
        })?;

        // A burst (a dragging slider) coalesces into one write per table.
        cx.update(|cx| {
            settings::write_table(cx, "sound", table("enabled = true\nmaster = 0.2\n"));
            settings::write_table(cx, "sound", table("enabled = true\nmaster = 0.4\n"));
            settings::write_table(cx, "sound", table("enabled = true\nmaster = 0.45\n"));
            ensure!(settings::is_writing(cx), "queued");
            Ok(())
        })?;
        cx.run_until_parked();
        let text = std::fs::read_to_string(&path)?;
        ensure!(
            text.starts_with(BEFORE),
            "comments and [palette] kept:\n{text}"
        );
        ensure!(
            table(&text)["sound"] == toml::Value::Table(table("enabled = true\nmaster = 0.45\n")),
            "latest value wins:\n{text}"
        );
        ensure!(
            seen.get() == 1,
            "one change notification, got {}",
            seen.get()
        );
        ensure!(cx.update(|cx| settings::generation(cx)) == 1);
        ensure!(!cx.update(|cx| settings::is_writing(cx)));

        // Motion applies live after the write (FR-3).
        cx.update(|cx| cx.set_reduce_motion(false));
        cx.update(|cx| settings::write_table(cx, "general", table("motion = \"reduced\"\n")));
        cx.run_until_parked();
        ensure!(
            cx.update(|cx| cx.reduce_motion()),
            "motion override applied"
        );
        ensure!(cx.update(|cx| settings::reduce_motion_override(cx)) == Some(true));

        // An outside edit is noticed by the poll.
        let before = seen.get();
        std::thread::sleep(std::time::Duration::from_millis(1100)); // mtime granularity
        std::fs::write(
            &path,
            format!(
                "{}\n[ken]\nenabled = true\n",
                std::fs::read_to_string(&path)?
            ),
        )?;
        cx.advance_clock(settings::POLL * 2);
        cx.run_until_parked();
        ensure!(seen.get() > before, "outside edit observed");

        // Every tab renders over the file, also while filtering.
        let window = cx.open_window(size(px(940.0), px(680.0)), |window, cx| {
            cx.new(|cx| SettingsView::new(Tab::General, window, cx))
        })?;
        cx.run_until_parked();
        for tab in Tab::ALL {
            cx.update_window(window.into(), |_, window, cx| {
                window
                    .root::<SettingsView>()
                    .flatten()
                    .context("settings root")?
                    .update(cx, |view, cx| view.show_tab(tab, window, cx));
                window.draw(cx).clear();
                anyhow::Ok(())
            })??;
        }

        // A syntax error: nothing is written, the error is reported.
        std::fs::write(&path, "[sound\nenabled = true\n")?;
        cx.update(|cx| settings::write_table(cx, "sound", table("enabled = false\n")));
        cx.run_until_parked();
        ensure!(
            std::fs::read_to_string(&path)? == "[sound\nenabled = true\n",
            "untouched"
        );
        let error = cx
            .update(|cx| settings::write_error(cx))
            .context("error kept")?;
        ensure!(error.contains("syntax error"), "{error}");

        println!("settings: ok");
        Ok(())
    }
}
