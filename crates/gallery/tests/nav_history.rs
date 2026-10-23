//! Back / forward across views (spec 0018) through the real shell on the
//! demo org, headless, with the app's key bindings and real keystrokes and
//! mouse events: the owner's two scenarios (AC-1, AC-2), push rules
//! (AC-3), `Esc` in a review (AC-8), key contexts (AC-9) and the titlebar
//! buttons (AC-10). Screens land in `target/nav-history/`. macOS only,
//! like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("nav_history: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use adoken_app::activity::{Tab, fixtures};
    use adoken_app::palette::catalog::Target;
    use adoken_app::review::pr::history::ReviewPosition;
    use adoken_app::review::pr::{ReviewMode, THREADS_VIEW_KEYS, TOGGLE_LAYOUT_KEYS};
    use adoken_app::shell::history::{CAPACITY, Subject, truncate_title};
    use adoken_app::shell::navigate::{Entry, Location};
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::workitems::detail::model::Stop;
    use adoken_sync::activity::{ChangeWord, Changes, KindChanges, Outcome, Trigger};
    use adoken_sync::pr_files::PrFiles;
    use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
    use adoken_sync::writes::Writer;
    use adoken_sync::{ApiRequest, Governor, SystemClock, Transport};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AppContext as _, Focusable as _, HeadlessAppContext, Keystroke, MouseButton,
        MouseDownEvent, MouseUpEvent, NavigationDirection, Pixels, Point, ScrollDelta,
        ScrollWheelEvent, TouchPhase, WindowHandle, point, px, size,
    };

    /// Heap bytes per thread, for the history's size (AC-12).
    mod alloc_count {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static LIVE: Cell<isize> = const { Cell::new(0) };
        }

        fn add(bytes: isize) {
            let _ = LIVE.try_with(|live| live.set(live.get() + bytes));
        }

        /// Bytes this thread allocated and hasn't freed.
        pub fn live() -> isize {
            LIVE.try_with(Cell::get).unwrap_or(0)
        }

        pub struct Counting;

        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                add(layout.size() as isize);
                unsafe { System.alloc(layout) }
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                add(layout.size() as isize);
                unsafe { System.alloc_zeroed(layout) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                add(-(layout.size() as isize));
                unsafe { System.dealloc(ptr, layout) }
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                add(new_size as isize - layout.size() as isize);
                unsafe { System.realloc(ptr, layout, new_size) }
            }
        }
    }

    #[global_allocator]
    static ALLOCATOR: alloc_count::Counting = alloc_count::Counting;

    /// The titlebar's `‹` and `›`, right after the traffic lights (84 px),
    /// 24 px square.
    const BACK_AT: Point<Pixels> = point(px(96.0), px(19.0));
    const FORWARD_AT: Point<Pixels> = point(px(124.0), px(19.0));
    /// Empty titlebar between the org name and the search field.
    const TITLEBAR_AT: Point<Pixels> = point(px(400.0), px(19.0));
    const WINDOW_WIDTH: f32 = 1200.0;
    /// Short enough that the Inbox scrolls.
    const WINDOW_HEIGHT: f32 = 520.0;
    /// Owner scenario A's height: the demo's diff and file tree scroll.
    const SCENARIO_A_HEIGHT: f32 = 440.0;
    /// In a review at 1200 × 440: the diff pane, the file tree, and the
    /// header's first linked work item (its Work items row).
    const DIFF_AT: Point<Pixels> = point(px(780.0), px(340.0));
    const TREE_AT: Point<Pixels> = point(px(344.0), px(385.0));
    const WORK_ITEM_AT: Point<Pixels> = point(px(372.0), px(158.0));
    /// The content pane, and the sidebar's Pipelines row.
    const CONTENT_AT: Point<Pixels> = point(px(700.0), px(300.0));
    const SIDEBAR_PIPELINES_AT: Point<Pixels> = point(px(80.0), px(180.0));

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
            gpui_tokio::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
            // The Activity drawer lists fixture passes (AC-3's drawer item).
            adoken_app::sync::install_activity_hub(
                fixtures::hub(
                    Vec::new(),
                    fixtures::passes(adoken_demo::DEMO_NOW),
                    adoken_demo::DEMO_NOW,
                ),
                cx,
            );
        });
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW)?;
        let window = cx.open_window(size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), |window, cx| {
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

        // ── launch (FR-1, AC-10): one entry, nothing either way ─────────
        let (len, back, forward) = history(&mut cx, window)?;
        ensure!(
            len == 1 && back.is_none() && forward.is_none(),
            "launch: {len}"
        );
        paint(&mut cx, window, "launch")?;

        // ── palette (AC-11): "Go back" with nothing behind changes nothing
        palette_command(&mut cx, window, "Go back")?;
        ensure!(
            history(&mut cx, window)? == (1, None, None) && nav(&mut cx, window)? == Nav::Today,
            "\"Go back\" at launch changes nothing"
        );

        // ── push rules (AC-3): ⌘2 pushes, ⌘2 again doesn't ──────────────
        press(&mut cx, window, "cmd-2")?;
        ensure!(nav(&mut cx, window)? == Nav::Inbox, "⌘2 shows the Inbox");
        ensure!(history(&mut cx, window)?.0 == 2, "a section switch pushes");
        press(&mut cx, window, "cmd-2")?;
        ensure!(
            history(&mut cx, window)?.0 == 2,
            "the same section pushes nothing"
        );

        // ── owner scenario B (AC-2), for each kind of target ────────────
        for (key, label, shows) in [
            ("1", "Inbox · Unread · Pull requests", Nav::PullRequests),
            ("2", "Inbox · Unread · Work items", Nav::WorkItems),
            ("3", "Inbox · Unread · Pipelines", Nav::Pipelines),
        ] {
            scenario_b(&mut cx, window, key, label, shows)?;
        }

        // ── owner scenario A (AC-1): PR → linked work item → back, then a
        // replaced review (AC-7) and a dropped file (AC-6) ──────────────
        resize(&mut cx, window, SCENARIO_A_HEIGHT)?;
        let (a_pr, a_linked, a_at) = scenario_a(&mut cx, window)?;
        deferred_restore(&mut cx, window, a_pr, &a_at)?;
        dropped_file(&mut cx, window, a_pr, a_linked)?;
        resize(&mut cx, window, WINDOW_HEIGHT)?;

        // ── every other open source pushes one entry (AC-3), FR-4 actions
        // none; the palette's commands do what the keys do (AC-11) ────────
        push_sources(&mut cx, window, a_pr, a_linked)?;

        // ── mouse back / forward buttons (FR-11) and the titlebar ‹ › ───
        let at = point(px(700.0), px(300.0));
        let before = history(&mut cx, window)?;
        ensure!(before.1.is_some(), "there is somewhere to go back to");
        let shown = nav(&mut cx, window)?;
        navigate_button(&mut cx, window, NavigationDirection::Back, at)?;
        ensure!(
            nav(&mut cx, window)? != shown,
            "the mouse back button goes back"
        );
        navigate_button(&mut cx, window, NavigationDirection::Forward, at)?;
        ensure!(
            nav(&mut cx, window)? == shown,
            "the mouse forward button returns"
        );
        click(&mut cx, window, BACK_AT)?;
        ensure!(nav(&mut cx, window)? != shown, "the titlebar ‹ goes back");
        paint(&mut cx, window, "titlebar-back")?;
        click(&mut cx, window, FORWARD_AT)?;
        ensure!(
            nav(&mut cx, window)? == shown,
            "the titlebar › goes forward"
        );

        // ── forward truncation (AC-4): back, then a new open ───────────
        press(&mut cx, window, "cmd-[")?;
        ensure!(
            history(&mut cx, window)?.2.is_some(),
            "forward enabled after back"
        );
        press(&mut cx, window, "cmd-5")?;
        ensure!(
            nav(&mut cx, window)? == Nav::Pipelines,
            "⌘5 shows Pipelines"
        );
        ensure!(
            history(&mut cx, window)?.2.is_none(),
            "a new open drops the forward entries"
        );

        // ── `Esc` in a review (AC-8): list → review → Esc is a back ─────
        press(&mut cx, window, "cmd-3")?;
        ensure!(nav(&mut cx, window)? == Nav::PullRequests, "⌘3 shows PRs");
        if review(&mut cx, window)?.is_some() {
            // Still on scenario B's review: Esc (an open of the list).
            press(&mut cx, window, "escape")?;
            ensure!(review(&mut cx, window)?.is_none(), "Esc shows the list");
        }
        let entries = history(&mut cx, window)?.0;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |cx| Ok(review(cx, window)?.is_some())).context("↩ opens a review")?;
        let pr = review(&mut cx, window)?.context("open review")?;
        let (after, back, _) = history(&mut cx, window)?;
        ensure!(after == entries + 1, "the row's open pushed one entry");
        ensure!(
            back.as_deref() == Some("Pull requests"),
            "back reads {back:?}"
        );
        ensure!(
            location(&mut cx, window)? == Location::Review(pr),
            "the entry is the review"
        );
        press(&mut cx, window, "escape")?;
        ensure!(review(&mut cx, window)?.is_none(), "Esc shows the list");
        let (after, _, forward) = history(&mut cx, window)?;
        ensure!(after == entries && forward.is_some(), "Esc was a back");
        press(&mut cx, window, "cmd-]")?;
        settle(&mut cx, |cx| Ok(review(cx, window)? == Some(pr))).context("⌘] reopens")?;
        press(&mut cx, window, "cmd-[")?;
        ensure!(
            review(&mut cx, window)?.is_none(),
            "⌘[ shows the list again"
        );
        paint(&mut cx, window, "review-back")?;

        let entries = history(&mut cx, window)?.0;
        ensure!(entries <= 50, "bounded: {entries}");

        // ── no requests from back / forward (AC-12, FR-13) ──────────────
        no_requests_on_back_forward(&mut cx, window, &store, a_pr, a_linked)?;
        println!(
            "nav_history: launch, push rules, owner scenarios A and B, deferred restore, dropped file, push sources, mouse and titlebar back / forward, Esc, no requests"
        );
        Ok(())
    }

    /// Inbox filtered to Unread + one source, the 4th row (or the last but
    /// one) selected, `↩` opens it; `⌘[` brings back the same chips, the
    /// same notification selected though now read, the same scroll, and
    /// `↓` moves to the row that followed it.
    fn scenario_b(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        source_key: &str,
        label: &str,
        shows: Nav,
    ) -> anyhow::Result<()> {
        press(cx, window, "cmd-2")?;
        ensure!(nav(cx, window)? == Nav::Inbox, "⌘2 shows the Inbox");
        press(cx, window, "cmd-backspace")?;
        action(cx, window, Box::new(adoken_app::inbox::ShowUnread))?;
        press(cx, window, source_key)?;
        let rows = inbox_rows(cx, window)?;
        ensure!(rows.len() >= 2, "{label}: the demo has 2+ rows ({rows:?})");
        let ix = 3.min(rows.len() - 2);
        press(cx, window, "home")?;
        for _ in 0..ix {
            press(cx, window, "down")?;
        }
        let opened = selected(cx, window)?.context("a row is selected")?;
        ensure!(opened == rows[ix], "{label}: row {ix} selected");
        let next = rows[ix + 1];
        let scroll = inbox_scroll(cx, window)?;
        let entries = history(cx, window)?.0;

        press(cx, window, "enter")?;
        settle(cx, |cx| Ok(nav(cx, window)? == shows))
            .with_context(|| format!("{label}: ↩ opens the target in {shows:?}"))?;
        let (after, back, _) = history(cx, window)?;
        ensure!(
            after == entries + 1,
            "{label}: the open pushed one entry ({entries} → {after})"
        );
        ensure!(
            back.as_deref() == Some(label),
            "{label}: the back destination reads {back:?}"
        );
        paint(cx, window, &format!("opened-{source_key}"))?;

        press(cx, window, "cmd-[")?;
        ensure!(
            nav(cx, window)? == Nav::Inbox,
            "{label}: ⌘[ shows the Inbox"
        );
        let (_, _, forward) = history(cx, window)?;
        ensure!(forward.is_some(), "{label}: forward is enabled");
        ensure!(
            selected(cx, window)? == Some(opened),
            "{label}: the opened notification is selected again"
        );
        ensure!(
            inbox_rows(cx, window)?.contains(&opened),
            "{label}: listed although now read"
        );
        let back_scroll = inbox_scroll(cx, window)?;
        ensure!(
            (back_scroll - scroll).abs() <= 1.0,
            "{label}: same viewport offset ({back_scroll} vs {scroll})"
        );
        ensure!(
            window.update(cx, |shell, window, cx| shell.content_has_focus(window, cx))?,
            "{label}: the list has the keyboard"
        );
        paint(cx, window, &format!("back-{source_key}"))?;
        press(cx, window, "down")?;
        ensure!(
            selected(cx, window)? == Some(next),
            "{label}: ↓ selects the row that followed"
        );
        Ok(())
    }

    /// The demo PR with the most changed files among those a work item
    /// links to.
    fn linked_pr_with_most_files() -> anyhow::Result<i64> {
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        let linked: std::collections::HashSet<i64> = ds
            .work_items
            .iter()
            .flat_map(|w| w.linked_pr_ids.iter().copied())
            .collect();
        ds.pr_extras
            .iter()
            .filter(|(id, _)| linked.contains(id))
            .max_by_key(|(id, extras)| (extras.files.len(), std::cmp::Reverse(**id)))
            .map(|(id, _)| *id)
            .context("a linked demo PR")
    }

    /// Owner scenario A: the review of a PR on its third file, the diff
    /// and the tree scrolled, the line cursor 4 lines below the first
    /// visible one; a click on the header's linked work item shows it in
    /// Work items; `⌘[` brings the review back exactly, `↓` moves the
    /// cursor on; `⌘]` shows the work item. Then the same with the mouse
    /// back button and the titlebar `‹`.
    fn scenario_a(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<(i64, i64, ReviewPosition)> {
        let pr = linked_pr_with_most_files()?;
        let entries = history(cx, window)?.0;
        window.update(cx, |shell, window, cx| {
            shell.open_target(
                adoken_app::palette::catalog::Target::PullRequest(pr),
                window,
                cx,
            )
        })?;
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                review.pr_id() == pr && !review.linked_work_items().is_empty()
            })?
            .unwrap_or(false))
        })
        .with_context(|| format!("!{pr} opens with its linked work items"))?;
        ensure!(
            history(cx, window)?.0 == entries + 1,
            "opening !{pr} pushed one entry"
        );
        let (title, linked, third, ix) = with_review(cx, window, |review, _| {
            let listed = review.listed_files();
            let third = *listed.get(2)?;
            Some((
                review.pr_title().to_owned(),
                review.linked_work_items()[0].id,
                review.loaded().files[third].path.clone(),
                third,
            ))
        })?
        .flatten()
        .with_context(|| format!("!{pr} lists 3+ files"))?;

        // The third listed file, by `j` like the user would.
        for _ in 0..8 {
            if review_position(cx, window)?.file.as_deref() == Some(third.as_str()) {
                break;
            }
            press(cx, window, "j")?;
        }
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                review.position().file.as_deref() == Some(third.as_str())
                    && review.loaded().files[ix].diff.is_some()
            })?
            .unwrap_or(false))
        })
        .with_context(|| format!("!{pr}: `j` selects the third file {third} and its diff"))?;

        // The line cursor a few lines down, then the diff scrolled part-way
        // into a row above it, then the tree scrolled.
        let tree_before = review_position(cx, window)?.tree_top;
        for _ in 0..5 {
            press(cx, window, "down")?;
        }
        wheel(cx, window, DIFF_AT, 30.0)?;
        wheel(cx, window, TREE_AT, 37.0)?;
        let at = review_position(cx, window)?;
        paint(cx, window, "a-review-placed")?;
        ensure!(
            at.diff_top.0 > 0 && at.diff_top.1 > 0.0,
            "the diff scrolled part-way into a row: {at:?}"
        );
        ensure!(
            at.line_cursor.is_some_and(|line| line > at.diff_top.0),
            "the line cursor sits below the first visible row: {at:?}"
        );
        ensure!(at.tree_top != tree_before, "the tree scrolled: {at:?}");

        // The header's linked work item, clicked.
        click(cx, window, WORK_ITEM_AT)?;
        settle(cx, |cx| {
            Ok(nav(cx, window)? == Nav::WorkItems
                && work_item_subject(cx, window)? == Some(Subject::WorkItem(linked)))
        })
        .with_context(|| format!("!{pr}: a click on #{linked} shows it in Work items"))?;
        let (_, back, _) = history(cx, window)?;
        let label = format!("!{pr} {}", truncate_title(&title));
        ensure!(
            back.as_deref() == Some(label.as_str()),
            "the back destination reads {back:?}, not {label:?}"
        );
        paint(cx, window, "a-work-item")?;

        // ⌘[: the same review, exactly; the keyboard in the diff.
        press(cx, window, "cmd-[")?;
        expect_review(cx, window, pr, &at, "⌘[")?;
        let line = at.line_cursor;
        press(cx, window, "down")?;
        let moved = review_position(cx, window)?.line_cursor;
        ensure!(
            moved > line,
            "↓ moves the line cursor on ({line:?} → {moved:?})"
        );
        press(cx, window, "up")?;
        let at = review_position(cx, window)?;
        ensure!(at.line_cursor == line, "↑ moves it back: {at:?}");
        paint(cx, window, "a-back")?;

        // ⌘]: the work item, selected, its view with the keyboard.
        press(cx, window, "cmd-]")?;
        expect_work_item(cx, window, linked, "⌘]")?;

        // The mouse back button (over the titlebar: a press over a pane
        // would give that pane the keyboard first), then the titlebar `›`
        // and `‹`.
        navigate_button(cx, window, NavigationDirection::Back, TITLEBAR_AT)?;
        expect_review(cx, window, pr, &at, "the mouse back button")?;
        click(cx, window, FORWARD_AT)?;
        expect_work_item(cx, window, linked, "the titlebar ›")?;
        click(cx, window, BACK_AT)?;
        expect_review(cx, window, pr, &at, "the titlebar ‹")?;
        history_memory(cx, window)?;
        Ok((pr, linked, at))
    }

    /// AC-12, §7: 50 review entries like this one take ≤ 100 KB, ≤ 2 KB
    /// each (heap bytes this thread allocates to hold them, measured).
    fn history_memory(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<()> {
        let (review, bytes) = window.update(cx, |shell, _, _| {
            let entry = shell.history().current().clone();
            let review = matches!(entry.location, Location::Review(_));
            let before = alloc_count::live();
            let kept: Vec<Entry> = (0..CAPACITY).map(|_| entry.clone()).collect();
            // Labels are shared strings: count each as its own copy.
            let labels = CAPACITY * entry.label.len();
            let bytes = (alloc_count::live() - before) as usize + labels;
            drop(kept);
            (review, bytes)
        })?;
        ensure!(review, "the current entry is the review");
        ensure!(
            bytes <= 100 * 1024 && bytes / CAPACITY <= 2 * 1024,
            "{CAPACITY} review entries take {bytes} bytes"
        );
        println!(
            "nav_history: {CAPACITY} review entries take {bytes} bytes ({} per entry)",
            bytes / CAPACITY
        );
        Ok(())
    }

    /// AC-7: back to a review whose entity another PR's open replaced
    /// reopens it from the store and applies the file, diff anchor and line
    /// cursor once its diff lands; `j` before then cancels that and the
    /// selection follows the key.
    fn deferred_restore(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        pr: i64,
        at: &ReviewPosition,
    ) -> anyhow::Result<()> {
        let other = adoken_demo::SCENARIO_PRS[0];
        open_review(cx, window, other)?;
        press(cx, window, "cmd-[")?;
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                review.pr_id() == pr && !review.restore_pending()
            })?
            .unwrap_or(false))
        })
        .context("⌘[ reopens the replaced review")?;
        expect_review(cx, window, pr, at, "back to a replaced review")?;
        paint(cx, window, "deferred-applied")?;

        // Again, with `j` pressed while the diff is still computing.
        press(cx, window, "cmd-]")?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(other))).context("⌘] shows the other")?;
        let third = at.file.clone();
        let keystroke = Keystroke::parse("cmd-[")?;
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        let executor = cx.update(|cx| cx.background_executor().clone());
        let mut waiting = false;
        for _ in 0..10_000 {
            waiting = with_review(cx, window, |review, _| {
                review.pr_id() == pr && review.restore_pending()
            })?
            .unwrap_or(false);
            if waiting || !executor.tick() {
                break;
            }
        }
        ensure!(
            waiting,
            "the reopened review waits for its diff before applying the restore"
        );
        let listed = with_review(cx, window, |review, _| {
            let paths: Vec<String> = review
                .listed_files()
                .iter()
                .map(|f| review.loaded().files[*f].path.clone())
                .collect();
            paths
        })?
        .context("the reopened review")?;
        let from = listed
            .iter()
            .position(|p| Some(p) == third.as_ref())
            .context("the restored file is listed")?;
        press(cx, window, "j")?;
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                !review.restore_pending() && review.position().file == listed.get(from + 1).cloned()
            })?
            .unwrap_or(false))
        })
        .context("`j` moves to the next file")?;
        let now = review_position(cx, window)?;
        ensure!(
            now.diff_top == (0, 0.0) && now.line_cursor.is_none(),
            "the cancelled restore left the next file's diff at its top: {now:?}"
        );
        paint(cx, window, "deferred-cancelled")?;
        Ok(())
    }

    /// AC-6: while away, a new iteration dropped the selected file: back
    /// selects the file now at its tree position, its diff from the top.
    fn dropped_file(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        pr: i64,
        linked: i64,
    ) -> anyhow::Result<()> {
        ensure!(review(cx, window)? == Some(pr), "on !{pr}'s review");
        press(cx, window, "k")?;
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                let file = review.position().file;
                review
                    .loaded()
                    .files
                    .iter()
                    .any(|f| Some(&f.path) == file.as_ref() && f.diff.is_some())
            })?
            .unwrap_or(false))
        })?;
        for _ in 0..3 {
            press(cx, window, "down")?;
        }
        let (position, change) = with_review(cx, window, |review, _| {
            let file = review.position().file?;
            let ix = review.loaded().files.iter().position(|f| f.path == file)?;
            let position = review.listed_files().iter().position(|f| *f == ix)?;
            Some((position, review.loaded().files[ix].change.path.clone()))
        })?
        .flatten()
        .context("a selected, listed file")?;
        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::WorkItem(linked), window, cx)
        })?;
        ensure!(nav(cx, window)? == Nav::WorkItems, "the work item shows");
        // The sync's new file list, without that file (0019 FR-6).
        window.update(cx, |shell, _, cx| {
            if let Some(review) = shell.review_view().and_then(|v| v.read(cx).open_review()) {
                review.update(cx, |review, cx| {
                    let mut detail = review.loaded().detail.clone();
                    detail.files.retain(|f| f.path != change);
                    review.set_files(detail, cx);
                });
            }
        })?;
        press(cx, window, "cmd-[")?;
        let expected = with_review(cx, window, |review, _| {
            let ix = *review.listed_files().get(position)?;
            Some(review.loaded().files[ix].path.clone())
        })?
        .flatten()
        .context("a file at the old position")?;
        settle(cx, |cx| {
            Ok(with_review(cx, window, |review, _| {
                review.position().file.as_ref() == Some(&expected) && !review.restore_pending()
            })?
            .unwrap_or(false))
        })
        .with_context(|| format!("back selects {expected}, now at the dropped file's place"))?;
        let now = review_position(cx, window)?;
        ensure!(
            now.diff_top == (0, 0.0) && now.line_cursor.is_none(),
            "its diff shows from the top: {now:?}"
        );
        paint(cx, window, "dropped-file")?;
        Ok(())
    }

    /// The review of `pr` through the shell's target routing, once built.
    fn open_review(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        pr: i64,
    ) -> anyhow::Result<()> {
        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::PullRequest(pr), window, cx)
        })?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(pr)))
            .with_context(|| format!("!{pr}'s review opens"))
    }

    /// `⌘P`, `title` typed, `↩`.
    fn palette_command(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        title: &str,
    ) -> anyhow::Result<()> {
        palette_pick(cx, window, title, title)
    }

    /// `⌘P`, `query` typed, `↩` once the selected result's title contains
    /// `pick`.
    fn palette_pick(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        query: &str,
        pick: &str,
    ) -> anyhow::Result<()> {
        press(cx, window, "cmd-p")?;
        window.update(cx, |shell, _, cx| {
            shell
                .palette()
                .update(cx, |palette, cx| palette.set_query(query, cx))
        })?;
        settle(cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell.palette().read(cx).selected_title().contains(pick)
            })
        })
        .with_context(|| format!("the palette selects {pick:?} for {query:?}"))?;
        press(cx, window, "enter")?;
        ensure!(
            !window.update(cx, |shell, _, cx| shell.palette().read(cx).is_visible())?,
            "↩ closes the palette"
        );
        Ok(())
    }

    /// One open from `source`: exactly one entry more, its back destination
    /// the place left.
    fn expect_one_push(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        source: &str,
        before: (usize, String),
    ) -> anyhow::Result<()> {
        let (entries, back, _) = history(cx, window)?;
        ensure!(
            entries == before.0 + 1,
            "{source}: one entry pushed ({} → {entries})",
            before.0
        );
        ensure!(
            back.as_deref() == Some(before.1.as_str()),
            "{source}: back goes to {:?}, not {back:?}",
            before.1
        );
        Ok(())
    }

    /// Entries and the label of the place shown, before an open.
    fn here(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<(usize, String)> {
        let entries = history(cx, window)?.0;
        let label = window.update(cx, |shell, window, cx| {
            shell.current_label(window, cx).to_string()
        })?;
        Ok((entries, label))
    }

    /// AC-3: every open source pushes exactly one entry whose back
    /// destination is the place left; FR-4 actions in a review push none
    /// and come back with a later back. AC-11: the palette's "Go back" /
    /// "Go forward" do what `⌘[` / `⌘]` do. Today comes back by row (FR-5).
    fn push_sources(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        pr: i64,
        linked: i64,
    ) -> anyhow::Result<()> {
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);

        // FR-4 in the review: `j`, `t` and the layout toggle push nothing.
        ensure!(review(cx, window)? == Some(pr), "on !{pr}'s review");
        let entries = history(cx, window)?.0;
        press(cx, window, "j")?;
        press(cx, window, THREADS_VIEW_KEYS)?;
        press(cx, window, TOGGLE_LAYOUT_KEYS)?;
        press(cx, window, TOGGLE_LAYOUT_KEYS)?;
        ensure!(
            history(cx, window)?.0 == entries,
            "j, t and ⇧⌘D push nothing"
        );
        let mode =
            |cx: &mut HeadlessAppContext| with_review(cx, window, |review, _| review.review_mode());
        ensure!(mode(cx)? == Some(ReviewMode::Threads), "t shows Threads");

        // The review's work item link (FR-3.2); back shows Threads again.
        let before = here(cx, window)?;
        click(cx, window, WORK_ITEM_AT)?;
        settle(cx, |cx| {
            Ok(work_item_subject(cx, window)? == Some(Subject::WorkItem(linked)))
        })
        .context("the review's work item link opens it")?;
        expect_one_push(cx, window, "the review's work item link", before)?;
        press(cx, window, "cmd-[")?;
        ensure!(
            mode(cx)? == Some(ReviewMode::Threads),
            "back restores the Threads view"
        );
        press(cx, window, THREADS_VIEW_KEYS)?;

        // A palette result.
        let other = ds
            .prs
            .iter()
            .find(|p| p.id != pr && ds.pr_extras.contains_key(&p.id))
            .context("another demo PR")?;
        let before = here(cx, window)?;
        palette_pick(cx, window, &format!("!{}", other.id), &other.title)?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(other.id)))
            .context("the palette result opens its review")?;
        expect_one_push(cx, window, "a palette result", before)?;

        // AC-11: the palette's commands move like the keys.
        palette_command(cx, window, "Go back")?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(pr))).context("\"Go back\"")?;
        palette_command(cx, window, "Go forward")?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(other.id))).context("\"Go forward\"")?;

        // A Today card; back shows the same row selected, scrolled the same.
        press(cx, window, "cmd-1")?;
        settle(cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell
                    .today_view()
                    .is_some_and(|today| today.read(cx).loaded().is_some())
            })
        })
        .context("Today loads")?;
        press(cx, window, "home")?;
        press(cx, window, "down")?;
        press(cx, window, "down")?;
        wheel(cx, window, CONTENT_AT, 90.0)?;
        let today = |cx: &mut HeadlessAppContext| {
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
            window
                .update(cx, |shell, _, cx| {
                    shell.today_view().map(|today| {
                        let today = today.read(cx);
                        (
                            today.nav_state().selected,
                            today.scroll_offset().y / px(1.0),
                        )
                    })
                })?
                .context("Today")
        };
        let (selected, offset) = today(cx)?;
        ensure!(
            selected.is_some() && offset < 0.0,
            "a Today row selected, scrolled: {selected:?} {offset}"
        );
        let before = here(cx, window)?;
        press(cx, window, "enter")?;
        settle(cx, |cx| Ok(nav(cx, window)? != Nav::Today)).context("↩ opens a Today card")?;
        expect_one_push(cx, window, "a Today card", before)?;
        press(cx, window, "cmd-[")?;
        ensure!(nav(cx, window)? == Nav::Today, "⌘[ shows Today");
        today(cx)?;
        let (back_selected, back_offset) = today(cx)?;
        ensure!(
            back_selected == selected && (back_offset - offset).abs() <= 1.0,
            "Today comes back on the same row and scroll: {back_selected:?} {back_offset} vs {selected:?} {offset}"
        );
        paint(cx, window, "today-back")?;

        // The first pin, from `⌘J`.
        let before = here(cx, window)?;
        press(cx, window, "cmd-j")?;
        press(cx, window, "enter")?;
        settle(cx, |cx| Ok(nav(cx, window)? != Nav::Today)).context("⌘J ↩ opens the first pin")?;
        expect_one_push(cx, window, "a pin", before)?;

        // An Activity drawer item: a pass's changed PR.
        press(cx, window, "cmd-1")?;
        let (prs, _, _) = fixtures::demo_ids();
        let mut pass = fixtures::running_pass(adoken_demo::DEMO_NOW);
        pass.id = 99;
        pass.trigger = Trigger::SyncNow;
        pass.outcome = Outcome::Ok;
        pass.ended = Some(adoken_demo::DEMO_NOW);
        pass.changes = Changes {
            prs: KindChanges {
                updated: prs.len() as u32,
                ids: prs.iter().map(|id| (*id, ChangeWord::Updated)).collect(),
                ..KindChanges::default()
            },
            ..Changes::default()
        };
        cx.update(|cx| adoken_app::sync::activity_hub(cx))
            .context("the activity hub")?
            .passes
            .push_fixture(pass);
        window.update(cx, |shell, window, cx| {
            shell.open_activity(Tab::Sync, false, window, cx);
            shell.activity().update(cx, |a, cx| a.refresh(cx));
        })?;
        cx.run_until_parked();
        window.update(cx, |shell, window, cx| {
            if let Some(panel) = shell.activity_panel().cloned() {
                panel.update(cx, |panel, cx| panel.focus_passes(window, cx));
            }
        })?;
        press(cx, window, "home")?;
        if !window.update(cx, |shell, _, cx| shell.activity().read(cx).is_expanded(99))? {
            press(cx, window, "right")?;
        }
        let before = here(cx, window)?;
        press(cx, window, "down")?;
        press(cx, window, "enter")?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(prs[0])))
            .context("↩ on a pass's changed PR opens its review")?;
        expect_one_push(cx, window, "an Activity drawer item", before)?;
        window.update(cx, |shell, window, cx| shell.toggle_activity(window, cx))?;

        // A work item detail's parent link.
        let (child, parent) = ds
            .work_items
            .iter()
            .find_map(|w| {
                let parent = w.parent_id?;
                ds.work_items
                    .iter()
                    .any(|p| p.id == parent)
                    .then_some((w.id, parent))
            })
            .context("a demo work item with a parent")?;
        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::WorkItem(child), window, cx)
        })?;
        let detail = |cx: &mut HeadlessAppContext| {
            window.update(cx, |shell, _, cx| {
                shell
                    .work_items()
                    .and_then(|view| view.read(cx).detail_item())
                    .map(|item| item.id)
            })
        };
        settle(cx, |cx| Ok(detail(cx)? == Some(child))).context("the child's detail")?;
        let row = window
            .update(cx, |shell, _, cx| {
                shell
                    .work_items()
                    .and_then(|view| view.read(cx).link_row_of(parent))
            })?
            .context("the parent's link row")?;
        window.update(cx, |shell, window, cx| {
            if let Some(view) = shell.work_items().cloned() {
                view.update(cx, |view, cx| {
                    view.story_cursor(Stop::Link(row), window, cx)
                });
            }
        })?;
        let before = here(cx, window)?;
        press(cx, window, "enter")?;
        settle(cx, |cx| Ok(detail(cx)? == Some(parent))).context("↩ on the parent link")?;
        expect_one_push(cx, window, "a work item's parent link", before)?;

        // A sidebar click.
        let before = here(cx, window)?;
        click(cx, window, SIDEBAR_PIPELINES_AT)?;
        ensure!(
            nav(cx, window)? == Nav::Pipelines,
            "the sidebar click shows Pipelines"
        );
        expect_one_push(cx, window, "a sidebar click", before)?;
        println!(
            "nav_history: one push each from a palette result, the review's work item link, Today, a ⌘J pin, the Activity drawer, a parent link and the sidebar"
        );
        Ok(())
    }

    /// Azure DevOps that answers 404 to everything and counts requests.
    #[derive(Default)]
    struct Counter(AtomicUsize);

    impl Transport for Counter {
        fn send(
            &self,
            _: ApiRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ApiResponse, TransportError>> + Send + '_>>
        {
            self.0.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(ApiResponse {
                    status: 404,
                    rate: RateHeaders::default(),
                    body: Vec::new(),
                    content_type: None,
                    validators: Default::default(),
                })
            })
        }
    }

    /// AC-12, FR-13: with the connected review, work item and file paths
    /// going through a request counter, opens do request and back /
    /// forward (keys, mouse buttons) send nothing.
    fn no_requests_on_back_forward(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        store: &adoken_store::Store,
        pr: i64,
        linked: i64,
    ) -> anyhow::Result<()> {
        let ado = Arc::new(Counter::default());
        let clock = Arc::new(SystemClock);
        let gov = Arc::new(Governor::new(clock.clone()));
        let org = url::Url::parse("https://dev.azure.com/contoso/")?;
        let writer = Writer::new(
            store.clone(),
            ado.clone(),
            gov.clone(),
            clock.clone(),
            org.clone(),
        );
        let files = Arc::new(PrFiles::new(store.clone(), ado.clone(), gov, clock, org));
        let (control, _control) = tokio::sync::mpsc::unbounded_channel();
        cx.update(|cx| {
            adoken_app::review::writes::set_access(
                adoken_app::review::writes::Access::Remote(adoken_app::review::writes::Remote {
                    writer: tokio::sync::watch::channel(Some(writer.clone())).1,
                    control,
                    link_work_items: true,
                }),
                cx,
            );
            adoken_app::workitems::writes::set_access(
                adoken_app::workitems::writes::Access::Remote {
                    writer: tokio::sync::watch::channel(Some(writer.work_items())).1,
                    can_write: true,
                },
                cx,
            );
            adoken_app::review::blobs::set_files(tokio::sync::watch::channel(Some(files)).1, cx);
        });
        let sent = |cx: &mut HeadlessAppContext| {
            // Requests run on Tokio: wait until none arrives for a while.
            let mut last = ado.0.load(Ordering::SeqCst);
            let mut quiet = 0;
            while quiet < 5 {
                cx.run_until_parked();
                std::thread::sleep(Duration::from_millis(30));
                let now = ado.0.load(Ordering::SeqCst);
                quiet = if now == last { quiet + 1 } else { 0 };
                last = now;
            }
            last
        };
        let other = adoken_demo::SCENARIO_PRS[1];
        let start = sent(cx);
        open_review(cx, window, other)?;
        let opened = sent(cx);
        ensure!(
            opened > start,
            "opening a review requests through the counter ({start} → {opened})"
        );
        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::PullRequest(pr), window, cx)
        })?;
        settle(cx, |cx| Ok(review(cx, window)? == Some(pr)))?;
        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::WorkItem(linked), window, cx)
        })?;
        press(cx, window, "cmd-2")?;
        press(cx, window, "enter")?;
        let baseline = sent(cx);
        let steps: [(&str, Option<NavigationDirection>); 8] = [
            ("cmd-[", None),
            ("cmd-[", None),
            ("cmd-[", None),
            ("cmd-]", None),
            ("cmd-]", None),
            ("", Some(NavigationDirection::Back)),
            ("", Some(NavigationDirection::Back)),
            ("", Some(NavigationDirection::Forward)),
        ];
        let mut shown = Vec::new();
        for (keys, button) in steps {
            match button {
                Some(direction) => navigate_button(cx, window, direction, TITLEBAR_AT)?,
                None => press(cx, window, keys)?,
            }
            let after = sent(cx);
            shown.push((nav(cx, window)?, review(cx, window)?));
            ensure!(
                after == baseline,
                "back / forward sent {} request(s) on reaching {:?}",
                after - baseline,
                shown.last()
            );
        }
        println!(
            "nav_history: opens sent {} requests; 8 back / forward steps through {shown:?} sent 0",
            baseline - start
        );
        Ok(())
    }

    fn resize(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        height: f32,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.resize(size(px(WINDOW_WIDTH), px(height)));
            window.bounds_changed(cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// The review of `pr` shows `at` (± 1 px) with the keyboard.
    fn expect_review(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        pr: i64,
        at: &ReviewPosition,
        how: &str,
    ) -> anyhow::Result<()> {
        ensure!(
            nav(cx, window)? == Nav::PullRequests && review(cx, window)? == Some(pr),
            "{how}: the review of !{pr} shows"
        );
        let back = review_position(cx, window)?;
        let near = |a: f32, b: f32| (a - b).abs() <= 1.0;
        ensure!(
            back.file == at.file,
            "{how}: same file ({back:?} vs {at:?})"
        );
        ensure!(
            back.diff_top.0 == at.diff_top.0 && near(back.diff_top.1, at.diff_top.1),
            "{how}: same first visible diff row and offset ({back:?} vs {at:?})"
        );
        ensure!(
            back.line_cursor == at.line_cursor,
            "{how}: same line cursor ({back:?} vs {at:?})"
        );
        let tree = |p: &ReviewPosition| p.tree_top.as_ref().map(|t| (t.row, t.offset));
        ensure!(
            match (tree(&back), tree(at)) {
                (Some((a, x)), Some((b, y))) => a == b && near(x, y),
                (a, b) => a == b,
            },
            "{how}: same tree scroll ({back:?} vs {at:?})"
        );
        let focused = window.update(cx, |shell, window, cx| {
            shell
                .review_view()
                .and_then(|view| view.read(cx).open_review())
                .is_some_and(|review| review.focus_handle(cx).is_focused(window))
        })?;
        ensure!(focused, "{how}: the review has the keyboard");
        Ok(())
    }

    fn expect_work_item(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        id: i64,
        how: &str,
    ) -> anyhow::Result<()> {
        ensure!(
            nav(cx, window)? == Nav::WorkItems
                && work_item_subject(cx, window)? == Some(Subject::WorkItem(id)),
            "{how}: #{id} shows in Work items"
        );
        ensure!(
            window.update(cx, |shell, window, cx| shell.content_has_focus(window, cx))?,
            "{how}: Work items has the keyboard"
        );
        Ok(())
    }

    /// The review's position once painted.
    fn review_position(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<ReviewPosition> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })?;
        with_review(cx, window, |review, _| review.position())?.context("an open review")
    }

    fn work_item_subject(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Option<Subject>> {
        window.update(cx, |shell, window, cx| {
            shell
                .work_items()
                .and_then(|view| view.read(cx).nav_state(window, cx).subject())
        })
    }

    /// `f` on the open review, if one is built.
    fn with_review<R>(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        f: impl FnOnce(&adoken_app::review::pr::PrReview, &gpui::App) -> R,
    ) -> anyhow::Result<Option<R>> {
        window.update(cx, |shell, _, cx| {
            let review = shell.review_view()?.read(cx).open_review()?;
            Some(f(review.read(cx), cx))
        })
    }

    type Snapshot = (usize, Option<String>, Option<String>);

    /// Entries, the back destination and the forward destination.
    fn history(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Snapshot> {
        window.update(cx, |shell, _, _| {
            let history = shell.history();
            (
                // Entries up to and including the current one.
                history.cursor() + 1,
                history.peek_back().map(|e| e.label.to_string()),
                history.peek_forward().map(|e| e.label.to_string()),
            )
        })
    }

    fn nav(cx: &mut HeadlessAppContext, window: WindowHandle<Shell>) -> anyhow::Result<Nav> {
        window.update(cx, |shell, _, _| shell.nav())
    }

    /// The PR whose review is open, once built.
    fn review(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Option<i64>> {
        window.update(cx, |shell, _, cx| shell.open_review_id(cx))
    }

    fn location(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Location> {
        window.update(cx, |shell, _, _| shell.history().current().location)
    }

    fn inbox_rows(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Vec<i64>> {
        window.update(cx, |shell, _, cx| {
            shell
                .inbox_view()
                .map(|inbox| inbox.read(cx).row_ids().to_vec())
                .unwrap_or_default()
        })
    }

    fn selected(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Option<i64>> {
        window.update(cx, |shell, _, cx| {
            shell
                .inbox_view()
                .and_then(|inbox| inbox.read(cx).selected_notification())
        })
    }

    /// The Inbox list's scroll offset once painted.
    fn inbox_scroll(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<f32> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })?;
        window.update(cx, |shell, _, cx| {
            shell
                .inbox_view()
                .map_or(0.0, |inbox| inbox.read(cx).scroll_top())
        })
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

    /// Paints (so the focus path is in the dispatch tree), then types
    /// `keys` like the user would, then lets background work land.
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
        Ok(())
    }

    fn navigate_button(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        direction: NavigationDirection,
        position: Point<gpui::Pixels>,
    ) -> anyhow::Result<()> {
        mouse(cx, window, MouseButton::Navigate(direction), position)
    }

    fn click(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        position: Point<gpui::Pixels>,
    ) -> anyhow::Result<()> {
        mouse(cx, window, MouseButton::Left, position)
    }

    /// A wheel scroll of `down` px over `position`, then a paint so the
    /// list applies it.
    fn wheel(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        position: Point<Pixels>,
        down: f32,
    ) -> anyhow::Result<()> {
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
                gpui::PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position,
                    delta: ScrollDelta::Pixels(point(px(0.0), px(-down))),
                    modifiers: Default::default(),
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
            window.draw(cx).clear();
        })?;
        cx.run_until_parked();
        Ok(())
    }

    fn mouse(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        button: MouseButton,
        position: Point<gpui::Pixels>,
    ) -> anyhow::Result<()> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position,
                    ..Default::default()
                }),
                cx,
            );
            window.draw(cx).clear();
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(MouseDownEvent {
                    button,
                    position,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(MouseUpEvent {
                    button,
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
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/nav-history");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
