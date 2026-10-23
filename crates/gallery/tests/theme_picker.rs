//! Theme library + picker behavior through the real app code, headless
//! (spec 0007 §3.1, AC-3): hovering and arrowing change nothing, click /
//! Enter applies and persists one theme id to `settings.toml` without
//! dropping other `[appearance]` keys, an old light/dark pair migrates, and
//! a user theme file hot-reloads (a broken edit keeps the last good
//! version). macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("theme_picker: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::Duration;

    use adoken_app::theme_picker::{self, ThemeLibrary, ThemePicker, library};
    use adoken_theme::Appearance;
    use adoken_ui::ActiveTheme;
    use anyhow::{Context as _, ensure};
    use gpui::{
        AppContext as _, Focusable as _, HeadlessAppContext, InputEvent as _, Modifiers,
        MouseMoveEvent, WindowHandle, point, px, size,
    };

    fn active(cx: &mut HeadlessAppContext) -> String {
        cx.update(|cx| cx.theme().id.to_string())
    }

    fn chosen(cx: &mut HeadlessAppContext) -> String {
        cx.update(|cx| cx.global::<ThemeLibrary>().settings().theme.clone())
    }

    /// Paints (so this frame's hover listeners exist), then moves the pointer.
    fn hover(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ThemePicker>,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_event(
                MouseMoveEvent {
                    position: point(px(x), px(y)),
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
        })?;
        cx.run_until_parked();
        Ok(())
    }

    const MINE: &str = "name = \"Mine\"\n[[variant]]\nappearance = \"light\"\nid = \"mine-light\"\n[variant.colors]\nbg.app = \"#fafafa\"\n";

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| cx.set_reduce_motion(true));

        // Library: the macOS appearance at launch picks the default.
        cx.update(|cx| theme_picker::init(Appearance::Light, cx));
        ensure!(active(&mut cx) == "adoken-light");
        cx.update(|cx| theme_picker::init(Appearance::Dark, cx));
        ensure!(active(&mut cx) == "adoken-dark");
        // Any theme applies now, whatever its appearance (no pairing).
        cx.update(|cx| theme_picker::commit("catppuccin-latte", cx));
        ensure!(
            active(&mut cx) == "catppuccin-latte",
            "light theme on dark macOS"
        );
        cx.update(|cx| theme_picker::commit("tokyo-night", cx));
        ensure!(active(&mut cx) == "tokyo-night");
        cx.update(|cx| theme_picker::commit("no-such-theme", cx));
        ensure!(active(&mut cx) == "tokyo-night", "unknown ids are ignored");

        // The picker: hover and arrows only move the highlight; a click
        // (apply) and Enter paint and stay after the pointer leaves.
        let window = cx.open_window(size(px(900.0), px(700.0)), |window, cx| {
            cx.new(|cx| ThemePicker::new(window, cx))
        })?;
        cx.run_until_parked();
        for x in [120.0, 420.0, 720.0] {
            for y in [120.0, 260.0, 400.0, 560.0] {
                hover(&mut cx, window, x, y)?;
                ensure!(active(&mut cx) == "tokyo-night", "hover at {x},{y} painted");
            }
        }
        window.update(&mut cx, |picker, _, cx| picker.highlight(4, cx))?;
        ensure!(
            active(&mut cx) == "tokyo-night",
            "the highlight doesn't paint"
        );
        window.update(&mut cx, |picker, _, cx| picker.apply(4, cx))?;
        let applied = active(&mut cx);
        ensure!(applied != "tokyo-night", "apply paints");
        ensure!(chosen(&mut cx) == applied, "apply is the saved choice");
        hover(&mut cx, window, -50.0, -50.0)?;
        ensure!(
            active(&mut cx) == applied,
            "leaving the grid keeps the click"
        );
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        ensure!(active(&mut cx) == applied, "closing the picker keeps it");

        // Files: an old "Match system" pair migrates to the theme macOS was
        // showing, a user theme hot-reloads, and a pick rewrites the file.
        let dir = tempfile::tempdir()?;
        let settings_path = dir.path().join("settings.toml");
        std::fs::write(
            &settings_path,
            "[appearance]\naccent = \"shu\"\nmode = \"system\"\nlight_theme = \"mine-light\"\ndark_theme = \"dracula\"\n\n[sound]\nenabled = true\n",
        )?;
        let themes = dir.path().join("themes");
        std::fs::create_dir_all(&themes)?;
        let file = themes.join("mine.adoken-theme.toml");
        std::fs::write(&file, MINE)?;
        cx.allow_parking();
        cx.update(|cx| {
            // macOS is light at launch: the light slot was showing.
            theme_picker::init(Appearance::Light, cx);
            adoken_app::settings::init(settings_path.clone(), true, cx);
            theme_picker::load(cx);
        });
        cx.run_until_parked();
        ensure!(
            active(&mut cx) == "mine-light",
            "old pair loads its light slot"
        );
        ensure!(
            cx.update(|cx| cx.global::<ThemeLibrary>().settings_issues().is_empty()),
            "old keys are not reported"
        );

        std::fs::write(&file, MINE.replace("#fafafa", "#f0e0d0"))?;
        cx.advance_clock(library::POLL * 2);
        cx.run_until_parked();
        let bg = cx.update(|cx| cx.theme().raw.bg_app.to_string());
        ensure!(bg == "#f0e0d0", "edit hot-reloads (bg.app = {bg})");

        std::fs::write(&file, "name = \"Mine\"\n[[variant]\n")?;
        cx.advance_clock(library::POLL * 2);
        cx.run_until_parked();
        ensure!(
            active(&mut cx) == "mine-light",
            "broken edit keeps last good"
        );
        let error = cx.update(|cx| {
            cx.global::<ThemeLibrary>()
                .files()
                .first()
                .and_then(|file| file.error.clone())
        });
        ensure!(error.is_some_and(|e| e.contains("TOML")), "error surfaced");

        // Enter on a highlighted card applies and persists.
        let window = cx.open_window(size(px(900.0), px(700.0)), |window, cx| {
            cx.new(|cx| ThemePicker::new(window, cx))
        })?;
        cx.run_until_parked();
        window.update(&mut cx, |picker, window, cx| {
            picker.set_query("dracula", cx);
            picker.highlight(0, cx);
            window.focus(&picker.focus_handle(cx), cx);
        })?;
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(Box::new(theme_picker::Apply), cx);
        })?;
        cx.run_until_parked();
        ensure!(active(&mut cx) == "dracula", "Enter applies");
        cx.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        let saved = std::fs::read_to_string(&settings_path).context("settings.toml")?;
        ensure!(saved.contains("theme = \"dracula\""), "persisted:\n{saved}");
        for old in ["mode", "light_theme", "dark_theme"] {
            ensure!(!saved.contains(old), "{old} dropped:\n{saved}");
        }
        ensure!(
            saved.contains("accent = \"shu\"") && saved.contains("[sound]\nenabled = true"),
            "other keys kept:\n{saved}"
        );

        // Restart: the saved choice comes back, even on the other appearance.
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        cx.update(|cx| {
            theme_picker::init(Appearance::Light, cx);
            theme_picker::load(cx);
        });
        cx.run_until_parked();
        ensure!(active(&mut cx) == "dracula", "choice survives a restart");
        println!("theme_picker: click/Enter apply, no hover preview, migration and hot reload ok");
        Ok(())
    }
}
