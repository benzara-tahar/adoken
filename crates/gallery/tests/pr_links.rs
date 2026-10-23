//! Copy PR id / Copy link / Open in Azure DevOps (spec 0004 §3 step 9)
//! through a real window, headless, over the demo org's store: the keys
//! act on the open review or the list selection, whichever has focus, the
//! clipboard gets `!id` or the canonical web URL, the toast confirms it,
//! and the demo org opens nothing. macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("pr_links: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_app::review::data::ListTab;
    use adoken_app::review::web::DEMO_OPEN_REASON;
    use adoken_app::review::{
        BackToPrList, CopyPrId, CopyPrLink, OpenPrInAdo, ReviewView, SelectNextPr,
    };
    use adoken_demo::RICH_PR_IDS;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, ensure};
    use gpui::{Action, AppContext as _, HeadlessAppContext, WindowHandle, px, size};

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
        });
        let dir = tempfile::tempdir()?;
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir.path(), now)?;
        let open = RICH_PR_IDS[1];
        let detail = store.pr(open)?.context("demo PR in store")?.pr;
        let project = store
            .projects()?
            .into_iter()
            .find(|p| p.id == detail.project_id)
            .context("project")?
            .name;
        let repo = store
            .repos()?
            .into_iter()
            .find(|r| r.id == detail.repo_id)
            .context("repo")?
            .name;

        let window = cx.open_window(size(px(1440.0), px(860.0)), |window, cx| {
            cx.new(|cx| {
                let mut view = ReviewView::new(store.clone(), window, cx);
                view.set_clock(now, cx);
                view
            })
        })?;
        cx.run_until_parked();
        window.update(&mut cx, |view, window, cx| {
            window.activate_window();
            view.set_tab(ListTab::All, cx);
            view.open_pr(open, window, cx);
        })?;
        cx.run_until_parked();

        // The review has focus: its PR's link.
        dispatch(&mut cx, window, CopyPrLink)?;
        let url = format!("https://dev.azure.com/contoso/{project}/_git/{repo}/pullrequest/{open}");
        expect(&mut cx, window, &url, "Copied link")?;
        dispatch(&mut cx, window, CopyPrId)?;
        expect(
            &mut cx,
            window,
            &format!("!{open}"),
            &format!("Copied !{open}"),
        )?;

        // Demo org: nothing opens, the toast says why, the clipboard stays.
        dispatch(&mut cx, window, OpenPrInAdo)?;
        expect(&mut cx, window, &format!("!{open}"), DEMO_OPEN_REASON)?;

        // Back to the list, one row down: the keys act on that row.
        dispatch(&mut cx, window, BackToPrList)?;
        dispatch(&mut cx, window, SelectNextPr)?;
        dispatch(&mut cx, window, CopyPrId)?;
        let copied = clipboard(&mut cx)?;
        let id: i64 = copied
            .strip_prefix('!')
            .and_then(|id| id.parse().ok())
            .with_context(|| format!("copied an id: {copied:?}"))?;
        ensure!(id != open, "the list's selection, not the open review");
        ensure!(store.pr(id)?.is_some(), "!{id} is a synced PR");
        expect(&mut cx, window, &copied, &format!("Copied {copied}"))?;

        println!("pr_links: review and list copy their own PR; the demo org opens nothing");
        Ok(())
    }

    fn clipboard(cx: &mut HeadlessAppContext) -> anyhow::Result<String> {
        cx.update(|cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
            .context("clipboard has text")
    }

    fn expect(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
        copied: &str,
        toast: &str,
    ) -> anyhow::Result<()> {
        let text = clipboard(cx)?;
        ensure!(text == copied, "clipboard {text:?}, want {copied:?}");
        let shown = window.update(cx, |view, _, _| view.toast().map(|t| t.to_string()))?;
        ensure!(
            shown.as_deref() == Some(toast),
            "toast {shown:?}, want {toast:?}"
        );
        Ok(())
    }

    /// Paints (so the focus path is in the dispatch tree), then dispatches
    /// `action` from the focused element like a key binding would.
    fn dispatch(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
        action: impl Action,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(action.boxed_clone(), cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }
}
