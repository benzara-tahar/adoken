//! Pull requests view navigation (spec 0004 §3.1, one pane at a time)
//! through a real window, headless, with the app's key bindings and real
//! keystrokes over the demo org's store: the list has the keyboard as soon
//! as the view is focused (↑/↓, j/k, Home/End, PageUp/PageDown, ←/→ tabs),
//! ↩ opens the full-width review, and Esc returns to the list with the
//! same row selected and the scroll kept, keys working again. `⌘[` is the
//! shell's history (spec 0018, `nav_history.rs`), never the view's. macOS only,
//! like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("pr_list_nav: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_app::review::ReviewView;
    use adoken_app::review::data::ListTab;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, ensure};
    use gpui::{AppContext as _, Focusable, HeadlessAppContext, Keystroke, WindowHandle, px, size};

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            let variant = ThemeRegistry::house_variants()
                .next()
                .expect("a house theme");
            Theme::install(variant, cx);
            adoken_gallery::init(cx);
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir.path(), now)?;

        // Short enough that the All tab scrolls.
        let window = cx.open_window(size(px(1100.0), px(480.0)), |window, cx| {
            cx.new(|cx| {
                let mut view = ReviewView::new(store.clone(), window, cx);
                view.set_clock(now, cx);
                view
            })
        })?;
        cx.run_until_parked();
        // What the shell does when the view is shown.
        window.update(&mut cx, |view, window, cx| {
            window.activate_window();
            view.set_tab(ListTab::All, cx);
            window.focus(&view.focus_handle(cx), cx);
        })?;
        let rows = window.update(&mut cx, |view, _, _| view.row_ids())?;
        ensure!(rows.len() > 12, "the demo org lists enough PRs to scroll");
        let last = rows.len() - 1;

        // The list has the keyboard at once (the To review selection is
        // kept on All, so start from wherever it is).
        let start = row(&mut cx, window)?;
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "down")?;
        ensure!(row(&mut cx, window)? == start + 2, "↓ moves the selection");
        press(&mut cx, window, "home")?;
        ensure!(row(&mut cx, window)? == 0, "Home selects the first row");
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "j")?;
        press(&mut cx, window, "k")?;
        press(&mut cx, window, "k")?;
        ensure!(row(&mut cx, window)? == 1, "j / k move the selection");
        press(&mut cx, window, "end")?;
        ensure!(row(&mut cx, window)? == last, "End selects the last row");
        press(&mut cx, window, "down")?;
        ensure!(row(&mut cx, window)? == last, "no wrap past the end");
        press(&mut cx, window, "home")?;
        ensure!(row(&mut cx, window)? == 0, "Home selects the first row");
        press(&mut cx, window, "pagedown")?;
        let paged = row(&mut cx, window)?;
        ensure!(paged > 1 && paged < last, "PageDown moves a page: {paged}");
        press(&mut cx, window, "pagedown")?;
        let selected = row(&mut cx, window)?;
        ensure!(selected > paged, "PageDown again: {selected}");
        press(&mut cx, window, "pageup")?;
        ensure!(row(&mut cx, window)? == paged, "PageUp moves back a page");
        press(&mut cx, window, "pagedown")?;
        let scroll = scroll_top(&mut cx, window)?;
        ensure!(scroll > 0.0, "the selection scrolled into view");

        // ↩ opens the review full-width; the review has the keyboard.
        let id = rows[selected];
        press(&mut cx, window, "enter")?;
        let (showing, open) = window.update(&mut cx, |view, _, cx| {
            (view.is_showing_review(), view.open_review_id(cx))
        })?;
        ensure!(showing && open == Some(id), "↩ opened !{id}: {open:?}");
        press(&mut cx, window, "down")?;
        ensure!(
            row(&mut cx, window)? == selected,
            "↓ in the review moves the diff, not the list"
        );

        // Esc: the list again, same row, same scroll, keys work.
        press(&mut cx, window, "escape")?;
        ensure!(!showing_review(&mut cx, window)?, "Esc shows the list");
        ensure!(row(&mut cx, window)? == selected, "same row selected");
        let back = scroll_top(&mut cx, window)?;
        ensure!(back == scroll, "scroll kept: {back} vs {scroll}");
        press(&mut cx, window, "up")?;
        ensure!(row(&mut cx, window)? == selected - 1, "↑ works after Esc");

        // ⌘[ is the shell's history back (spec 0018 FR-12): the bare view
        // doesn't bind it.
        press(&mut cx, window, "enter")?;
        ensure!(showing_review(&mut cx, window)?, "↩ reopened");
        press(&mut cx, window, "cmd-[")?;
        ensure!(
            showing_review(&mut cx, window)?,
            "the view leaves ⌘[ to the shell"
        );
        press(&mut cx, window, "escape")?;
        ensure!(row(&mut cx, window)? == selected - 1, "same row after Esc");

        // Opening from elsewhere (⌘P, inbox, Today) goes straight to the
        // review; back lands on that PR's row.
        let elsewhere = rows[last];
        window.update(&mut cx, |view, window, cx| {
            view.open_pr(elsewhere, window, cx)
        })?;
        cx.run_until_parked();
        ensure!(showing_review(&mut cx, window)?, "open_pr shows the review");
        press(&mut cx, window, "escape")?;
        ensure!(row(&mut cx, window)? == last, "back selects the opened PR");

        // ←/→ switch tabs from the list (All → Mine wraps).
        press(&mut cx, window, "right")?;
        let tab_rows = window.update(&mut cx, |view, _, _| view.row_ids())?;
        ensure!(tab_rows.len() < rows.len(), "→ switched from All to Mine");
        press(&mut cx, window, "left")?;
        let tab_rows = window.update(&mut cx, |view, _, _| view.row_ids())?;
        ensure!(tab_rows == rows, "← back to All");

        println!("pr_list_nav: list keys, ↩ opens, Esc returns with row and scroll kept");
        Ok(())
    }

    /// Paints (so the focus path is in the dispatch tree), then types
    /// `keys` like the user would, then lets background work land.
    fn press(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
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

    /// The selected row's index in the list.
    fn row(cx: &mut HeadlessAppContext, window: WindowHandle<ReviewView>) -> anyhow::Result<usize> {
        window
            .update(cx, |view, _, _| {
                let id = view.selected_pr_id()?;
                view.row_ids().iter().position(|row| *row == id)
            })?
            .context("a row is selected")
    }

    fn showing_review(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
    ) -> anyhow::Result<bool> {
        window.update(cx, |view, _, _| view.is_showing_review())
    }

    /// The list's scroll offset once painted.
    fn scroll_top(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
    ) -> anyhow::Result<f32> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })?;
        window.update(cx, |view, _, _| view.list_scroll_top())
    }
}
