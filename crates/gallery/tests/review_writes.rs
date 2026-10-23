//! Review writes through a real window, headless (spec 0004), with the
//! app's key bindings and real clicks and keystrokes:
//!
//! - Every overlay (vote menu, Complete sheet, status menu, Abandon
//!   confirmation, work item picker) closes on `Esc` and on a click
//!   outside it, and the review has the keyboard again. The vote menu
//!   opened by clicking Vote… once stayed open for good after a click
//!   outside it: focus went to the review, whose key context while a menu
//!   is open binds no `Esc`.
//! - Status changes (⇧P, ⇧A + confirm, Reactivate) and linking / unlinking
//!   a work item through the picker land in the demo org's local store
//!   (`Access::Local`), FR-16 / FR-17.
//! - A line comment posted with ⌘↵ and the Complete sheet's merge: the
//!   async completion closes the composer / sheet and hands focus back to
//!   the review from inside the review's own update; reading the review
//!   entity there double-leased it and aborted the app.
//!
//! macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("review_writes: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashSet;
    use std::sync::Arc;

    use adoken_app::review::blobs::Blobs;
    use adoken_app::review::data::{self, Layout};
    use adoken_app::review::pr::{
        CommentOnLine, CompleteNow, ComposerState, OpenCompleteSheet, Overlay, PostComment,
        PrReview, ReactivatePr, SheetState, UnlinkWorkItem,
    };
    use adoken_demo::{DEFAULT_SEED, RICH_PR_IDS};
    use adoken_model::PrStatus;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, ensure};
    use gpui::{
        Action, AppContext as _, Focusable as _, HeadlessAppContext, InputEvent as _, Keystroke,
        Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, WindowHandle, point,
        px, size,
    };

    const DRAFT: &str = "Can we cap this at 3 attempts?";

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
            // The demo org: writes apply to the local store.
            adoken_gallery::init(cx);
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        overlays_dismiss(&mut cx, dir.path())?;
        status_and_links(&mut cx, dir.path())?;
        post_comment(&mut cx, dir.path())?;
        complete(&mut cx, dir.path())?;
        println!(
            "review_writes: overlays dismiss, status and links change, comment posted and PR completed through the local store"
        );
        Ok(())
    }

    /// Opened by a click or a key, every overlay closes on `Esc` and on a
    /// click outside it, and the review's keys work again.
    fn overlays_dismiss(cx: &mut HeadlessAppContext, dir: &std::path::Path) -> anyhow::Result<()> {
        let window = open(cx, dir, RICH_PR_IDS[2])?;
        let vote = vote_button(cx, window)?;
        // Empty tree pane under the files: a click that lands on the review.
        let outside = point(px(150.0), px(780.0));

        // The reported bug: Vote… clicked, then a click outside the menu.
        click(cx, window, vote)?;
        expect(cx, window, Some(Overlay::VoteMenu), "Vote… opens the menu")?;
        click(cx, window, outside)?;
        expect(cx, window, None, "a click outside closes the vote menu")?;
        press(cx, window, "shift-v")?;
        expect(cx, window, Some(Overlay::VoteMenu), "⇧V works after it")?;
        press(cx, window, "escape")?;
        expect(cx, window, None, "Esc closes the menu ⇧V opened")?;
        click(cx, window, vote)?;
        press(cx, window, "escape")?;
        expect(cx, window, None, "Esc closes the menu Vote… opened")?;

        // The other overlays, by their keys: Esc, then a click outside.
        for (keys, overlay) in [
            ("shift-m", Overlay::Sheet),
            ("shift-s", Overlay::StatusMenu),
            ("shift-a", Overlay::Confirm),
            ("shift-w", Overlay::WorkItemPicker),
        ] {
            press(cx, window, keys)?;
            expect(cx, window, Some(overlay), keys)?;
            press(cx, window, "escape")?;
            expect(cx, window, None, &format!("Esc closes {overlay:?}"))?;
            press(cx, window, keys)?;
            expect(cx, window, Some(overlay), keys)?;
            click(cx, window, outside)?;
            expect(
                cx,
                window,
                None,
                &format!("a click outside closes {overlay:?}"),
            )?;
        }
        let status = window.update(cx, |review, _, _| review.loaded().pr().status)?;
        ensure!(status == PrStatus::Active, "nothing abandoned: {status:?}");
        close(cx, window)
    }

    /// ⇧P, ⇧A + ↩, Reactivate; then ⇧W, an id, ↩ links it and Unlink work
    /// item… ↩ ↩ unlinks it: each shown at once and stored.
    fn status_and_links(cx: &mut HeadlessAppContext, dir: &std::path::Path) -> anyhow::Result<()> {
        let id = RICH_PR_IDS[2];
        let window = open(cx, dir, id)?;
        let store = adoken_demo::open_store_at(dir, adoken_demo::DEMO_NOW)?;
        let stored = || -> anyhow::Result<(PrStatus, bool)> {
            let pr = store.pr(id)?.context("PR in store")?.pr;
            Ok((pr.status, pr.is_draft))
        };
        let shown = |cx: &mut HeadlessAppContext| {
            window.update(cx, |review, _, _| {
                let pr = review.loaded().pr();
                (pr.status, pr.is_draft)
            })
        };

        press(cx, window, "shift-p")?;
        ensure!(
            shown(cx)? == (PrStatus::Active, true),
            "⇧P marks it a draft"
        );
        ensure!(stored()? == (PrStatus::Active, true), "stored as a draft");
        press(cx, window, "shift-p")?;
        ensure!(shown(cx)? == (PrStatus::Active, false), "⇧P publishes it");
        press(cx, window, "shift-a")?;
        expect(cx, window, Some(Overlay::Confirm), "⇧A asks first")?;
        press(cx, window, "enter")?;
        expect(cx, window, None, "↩ confirms")?;
        ensure!(shown(cx)?.0 == PrStatus::Abandoned, "abandoned");
        ensure!(stored()?.0 == PrStatus::Abandoned, "stored abandoned");
        dispatch(cx, window, ReactivatePr)?;
        ensure!(stored()? == (PrStatus::Active, false), "reactivated");

        // A demo work item that isn't linked yet, found by its id.
        let item = adoken_demo::dataset(DEFAULT_SEED)
            .work_items
            .into_iter()
            .find(|w| !w.linked_pr_ids.contains(&id))
            .context("a work item")?;
        let linked = |cx: &mut HeadlessAppContext| {
            window.update(cx, |review, _, _| {
                review
                    .linked_work_items()
                    .iter()
                    .map(|r| r.id)
                    .collect::<Vec<_>>()
            })
        };
        let before = linked(cx)?;
        ensure!(!before.contains(&item.id), "not linked yet: {before:?}");
        press(cx, window, "shift-w")?;
        expect(
            cx,
            window,
            Some(Overlay::WorkItemPicker),
            "⇧W opens the picker",
        )?;
        for digit in item.id.to_string().chars() {
            press(cx, window, &digit.to_string())?;
        }
        press(cx, window, "enter")?;
        expect(cx, window, None, "↩ links and closes the picker")?;
        ensure!(
            linked(cx)?.contains(&item.id),
            "the header lists #{}",
            item.id
        );
        let in_store = || -> anyhow::Result<bool> {
            Ok(store.pr_work_items(id)?.iter().any(|w| w.id == item.id))
        };
        ensure!(in_store()?, "#{} linked in the store", item.id);

        dispatch(cx, window, UnlinkWorkItem)?;
        expect(
            cx,
            window,
            Some(Overlay::WorkItemPicker),
            "Unlink work item…",
        )?;
        let rows = window.update(cx, |review, _, _| review.picker_rows().map(<[_]>::len))?;
        ensure!(
            rows == Some(before.len() + 1),
            "linked items listed: {rows:?}"
        );
        let position = linked(cx)?
            .iter()
            .position(|w| *w == item.id)
            .context("row")?;
        for _ in 0..position {
            press(cx, window, "down")?;
        }
        press(cx, window, "enter")?;
        expect(cx, window, Some(Overlay::Confirm), "unlink asks first")?;
        press(cx, window, "enter")?;
        ensure!(!linked(cx)?.contains(&item.id), "gone from the header");
        ensure!(!in_store()?, "#{} unlinked in the store", item.id);
        close(cx, window)
    }

    /// ⌘↵ in a line composer: the thread lands in the store and the review,
    /// the composer closes, the review keys work again.
    fn post_comment(cx: &mut HeadlessAppContext, dir: &std::path::Path) -> anyhow::Result<()> {
        let id = RICH_PR_IDS[0];
        let window = open(cx, dir, id)?;
        window.update(cx, |review, _, cx| {
            review.story_composer(DRAFT, ComposerState::Editing, cx);
        })?;
        // `c` on the cursor line focuses the composer the story opened.
        dispatch(cx, window, CommentOnLine)?;
        let editing = window.update(cx, |review, window, cx| {
            review
                .composer_state()
                .cloned()
                .zip(Some(!review.focus_handle(cx).is_focused(window)))
        })?;
        ensure!(
            editing == Some((ComposerState::Editing, true)),
            "composer open and focused: {editing:?}"
        );

        dispatch(cx, window, PostComment)?;
        cx.run_until_parked();

        let (composer, posted, focused) = window.update(cx, |review, window, cx| {
            let posted = review
                .loaded()
                .threads
                .iter()
                .any(|t| t.comments.iter().any(|c| c.body == DRAFT));
            let focused = review.focus_handle(cx).is_focused(window);
            (review.composer_state().cloned(), posted, focused)
        })?;
        ensure!(composer.is_none(), "composer closed: {composer:?}");
        ensure!(posted, "the new thread is in the loaded PR");
        ensure!(focused, "focus back on the review (its keys work)");
        let store = adoken_demo::open_store_at(dir, adoken_demo::DEMO_NOW)?;
        let stored = store.pr(id)?.context("PR in store")?;
        ensure!(
            stored
                .threads
                .iter()
                .any(|t| t.comments.iter().any(|c| c.body == DRAFT)),
            "the thread is in the store"
        );
        close(cx, window)
    }

    /// ⇧M then ⌘↵: the PR completes locally, the sheet closes.
    fn complete(cx: &mut HeadlessAppContext, dir: &std::path::Path) -> anyhow::Result<()> {
        let window = open(cx, dir, RICH_PR_IDS[1])?;
        dispatch(cx, window, OpenCompleteSheet)?;
        let (sheet, focused) = window.update(cx, |review, window, cx| {
            (
                review.sheet_state().cloned(),
                review.focus_handle(cx).is_focused(window),
            )
        })?;
        ensure!(
            sheet == Some(SheetState::Idle) && !focused,
            "sheet open and focused: {sheet:?}"
        );

        dispatch(cx, window, CompleteNow)?;
        cx.run_until_parked();

        let (sheet, status, focused) = window.update(cx, |review, window, cx| {
            (
                review.sheet_state().cloned(),
                review.loaded().pr().status,
                review.focus_handle(cx).is_focused(window),
            )
        })?;
        ensure!(sheet.is_none(), "sheet closed: {sheet:?}");
        ensure!(status == PrStatus::Completed, "PR completed: {status:?}");
        ensure!(focused, "focus back on the review");
        close(cx, window)
    }

    /// A review over demo PR `id` read from the seeded store at `dir` (what
    /// `ReviewView::open_pr` builds), busiest file selected, review focused.
    fn open(
        cx: &mut HeadlessAppContext,
        dir: &std::path::Path,
        id: i64,
    ) -> anyhow::Result<WindowHandle<PrReview>> {
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir, now)?;
        let detail = store.pr(id)?.context("demo PR in store")?;
        let people = store.people()?;
        let me = store.me()?.context("demo identity")?.id;
        let blob = |blob_id: &str| store.blob(blob_id).ok().flatten();
        let files = detail.files.len();
        let loaded = data::build(
            data::Source {
                detail,
                people,
                me,
                now,
                viewed: HashSet::new(),
                read_only: false,
                auto_complete: None,
                drafts: Vec::new(),
                blob: &blob,
            },
            files,
        );
        let blobs = Blobs::store(&store, None);
        let window = cx.open_window(size(px(1280.0), px(800.0)), |window, cx| {
            cx.new(|cx| {
                let mut review = PrReview::new(
                    Some(store),
                    blobs,
                    loaded,
                    Some(Layout::Unified),
                    window,
                    cx,
                );
                let busiest = review.busiest_file();
                review.select_file(busiest, cx);
                review
            })
        })?;
        cx.run_until_parked();
        window.update(cx, |review, window, cx| {
            window.activate_window();
            window.focus(&review.focus_handle(cx), cx);
        })?;
        Ok(window)
    }

    /// Paints (so the focus path is in the dispatch tree), then dispatches
    /// `action` from the focused element like a key binding would.
    fn dispatch(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        action: impl Action,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(action.boxed_clone(), cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// Paints, then types `keys` like the user would, then lets the
    /// write and the store land.
    fn press(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
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

    /// Presses the left button at `at`, with a frame in between like a
    /// real click; `release: false` only presses.
    fn mouse(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        at: Point<Pixels>,
        release: bool,
    ) -> anyhow::Result<()> {
        let down = MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        };
        let up = MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: 1,
        };
        let mut events = vec![down.to_platform_input()];
        if release {
            events.push(up.to_platform_input());
        }
        for event in events {
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_event(event, cx);
            })?;
            cx.run_until_parked();
        }
        Ok(())
    }

    fn click(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        at: Point<Pixels>,
    ) -> anyhow::Result<()> {
        mouse(cx, window, at, true)
    }

    /// The open overlay is `overlay`; with none, the review has the
    /// keyboard again.
    fn expect(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
        overlay: Option<Overlay>,
        what: &str,
    ) -> anyhow::Result<()> {
        let (open, focused) = window.update(cx, |review, window, cx| {
            (review.overlay(), review.focus_handle(cx).is_focused(window))
        })?;
        ensure!(open == overlay, "{what}: {open:?}");
        ensure!(
            overlay.is_some() || focused,
            "{what}: the review has the keyboard"
        );
        Ok(())
    }

    /// The header's Vote… button, found the way a pointer finds it: a
    /// press (not released) focuses the button under it. At the header's
    /// left edge the last button is the actions row's Approve; along that
    /// row, the second button is Vote….
    fn vote_button(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<PrReview>,
    ) -> anyhow::Result<Point<Pixels>> {
        let focused_at = |cx: &mut HeadlessAppContext, x: f32, y: f32| {
            mouse(cx, window, point(px(x), px(y)), false)?;
            window.update(cx, |review, window, cx| {
                let root = review.focus_handle(cx);
                window.focused(cx).filter(|f| *f != root)
            })
        };
        let mut row = None;
        for step in 0..100 {
            let y = step as f32 * 4.0;
            if focused_at(cx, 24.0, y)?.is_some() {
                row = Some(y);
            }
        }
        let y = row.context("a button at the header's left edge")?;
        let mut buttons: Vec<(gpui::FocusHandle, f32, f32)> = Vec::new();
        for step in 0..300 {
            let x = step as f32 * 3.0;
            let Some(focus) = focused_at(cx, x, y)? else {
                continue;
            };
            match buttons.last_mut() {
                Some((last, _, end)) if *last == focus => *end = x,
                _ => buttons.push((focus, x, x)),
            }
        }
        let (_, start, end) = buttons.get(1).context("Vote… after Approve")?;
        // Back to the review before the real click.
        mouse(cx, window, point(px(150.0), px(780.0)), true)?;
        Ok(point(px((start + end) / 2.0), px(y)))
    }

    fn close(cx: &mut HeadlessAppContext, window: WindowHandle<PrReview>) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        Ok(())
    }
}
