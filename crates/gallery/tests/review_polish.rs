//! Spec 0019 view tests through a real window, headless, with the app's key
//! bindings and real keystrokes (fixture S, spec §6):
//!
//! - A: the tree filter's keys (AC-A2), a selection the filter hides
//!   (AC-A3), "Only files with threads" in both modes (AC-A4), the filter
//!   surviving a push (AC-A5).
//! - B: the Threads view's order and counts (AC-B1), scopes and a resolve
//!   that keeps its card (AC-B2), keys (AC-B5), Open in diff (AC-B6),
//!   `n`/`p` across files (AC-B7), a live update that keeps the focused card
//!   where it is (AC-B8).
//! - D/E on the demo list: the row menu's status items (AC-D1), `⇧P` /
//!   `⇧A` / `⇧S` with "Sending…", one write in flight, a refusal's revert
//!   and toast (AC-D2), the list and the review showing one truth (AC-D5),
//!   Publish from a draft's header (AC-D4), Copy title (AC-E1).
//!
//! macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("review_polish: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;
    use std::sync::Arc;

    use adoken_app::review::data::{Layout, ListTab};
    use adoken_app::review::fixtures::{self, Fixture};
    use adoken_app::review::pr::{PrReview, ReviewMode};
    use adoken_app::review::threads::{Group, Item};
    use adoken_app::review::{CopyPrTitle, Part, ReviewView, story};
    use adoken_model::{ChangeType, PrFileChange, PrStatus, PullRequest, ThreadStatus};
    use adoken_store::{PrFilter, Store, write};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, ensure};
    use gpui::{
        Action, AnyWindowHandle, AppContext as _, Focusable as _, HeadlessAppContext, Keystroke,
        WindowHandle, px, size,
    };

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
        filter_keys(&mut cx)?;
        selection_outside_filter(&mut cx)?;
        threads_only(&mut cx)?;
        filter_survives_push(&mut cx)?;
        threads_view(&mut cx)?;
        let dir = tempfile::tempdir()?;
        resolve_keeps_card(&mut cx, &dir.path().join("resolve"))?;
        open_in_diff(&mut cx)?;
        n_p_across_files(&mut cx)?;
        live_update_keeps_focus(&mut cx)?;
        list_status(&mut cx, &dir.path().join("list"))?;
        println!(
            "review_polish: filter, Threads view, list status actions and copy behave as spec 0019 says"
        );
        Ok(())
    }

    // ── harness ──────────────────────────────────────────────────────────

    /// Fixture S's review in a 1280 × 800 window, the review focused;
    /// writes land in `store` when given.
    fn open(
        cx: &mut HeadlessAppContext,
        fixture: Fixture,
        store: Option<Store>,
    ) -> anyhow::Result<WindowHandle<PrReview>> {
        let window = cx.open_window(size(px(1280.0), px(800.0)), move |window, cx| {
            cx.new(|cx| {
                story::fixture_review(fixture, store, Layout::Unified, Part::All, window, cx)
            })
        })?;
        cx.run_until_parked();
        window.update(cx, |review, window, cx| {
            window.activate_window();
            window.focus(&review.focus_handle(cx), cx);
        })?;
        draw(cx, window.into())?;
        Ok(window)
    }

    fn draw(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
        cx.run_until_parked();
        Ok(())
    }

    /// Paints, then presses `keys` like the user would.
    fn press(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        keys: &str,
    ) -> anyhow::Result<()> {
        let keystroke = Keystroke::parse(keys).context("keystroke")?;
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// Types `text` into whatever has the keyboard.
    fn type_text(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        text: &str,
    ) -> anyhow::Result<()> {
        for ch in text.chars() {
            let key = match ch {
                ' ' => "space".to_owned(),
                c => c.to_string(),
            };
            let mut keystroke = Keystroke::parse(&key).context("keystroke")?;
            keystroke.key_char = Some(ch.to_string());
            cx.update_window(window, |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_keystroke(keystroke, cx);
            })?;
            cx.run_until_parked();
        }
        Ok(())
    }

    fn dispatch(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        action: impl Action,
    ) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_action(action.boxed_clone(), cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    fn close(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        Ok(())
    }

    fn file(review: &PrReview, path: &str) -> usize {
        review
            .loaded()
            .files
            .iter()
            .position(|f| f.path.ends_with(path))
            .unwrap_or_else(|| panic!("{path} in fixture S"))
    }

    /// The `LoadedPr::threads` index of thread `id`.
    fn thread(review: &PrReview, id: i64) -> usize {
        review
            .loaded()
            .threads
            .iter()
            .position(|t| t.id == id)
            .unwrap_or_else(|| panic!("thread {id} in fixture S"))
    }

    fn focused_id(review: &PrReview) -> Option<i64> {
        review.focused_card().map(|t| review.loaded().threads[t].id)
    }

    fn cursor_id(review: &PrReview) -> Option<i64> {
        review
            .thread_cursor_thread()
            .map(|t| review.loaded().threads[t].id)
    }

    // ── A ────────────────────────────────────────────────────────────────

    /// AC-A2: `/`, typing, `↓`, `↩`, `j`, `Esc` twice, `⌘⌫`.
    fn filter_keys(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        let (auth, token, token_test) = window.update(cx, |r, _, cx| {
            let ids = (
                file(r, "docs/AUTH.md"),
                file(r, "auth/token.rs"),
                file(r, "token_test.rs"),
            );
            r.select_file(ids.0, cx);
            ids
        })?;

        press(cx, any, "/")?;
        ensure!(
            window.update(cx, |r, w, cx| r.filter_focused(w, cx))?,
            "/ focuses the filter"
        );
        type_text(cx, any, "jkv")?;
        let (text, selected) = window.update(cx, |r, _, _| {
            (r.filter_text().to_owned(), r.selected_file())
        })?;
        ensure!(text == "jkv", "the letters went into the field: {text:?}");
        ensure!(selected == auth, "typing moved nothing");
        for _ in 0..3 {
            press(cx, any, "backspace")?;
        }
        type_text(cx, any, "auth")?;
        let counts = window.update(cx, |r, _, _| r.tree_counts())?;
        ensure!(
            counts == ("Files 3 of 5".into(), "1/3 viewed".into()),
            "header counts: {counts:?}"
        );
        press(cx, any, "down")?;
        let (selected, diff) = window.update(cx, |r, _, _| {
            let s = r.selected_file();
            (s, r.loaded().files[s].diff.is_some())
        })?;
        ensure!(
            selected == token && diff,
            "↓ selects the next listed file, its diff shown"
        );
        press(cx, any, "enter")?;
        ensure!(
            window.update(cx, |r, w, cx| !r.filter_focused(w, cx)
                && r.focus_handle(cx).is_focused(w))?,
            "↩ returns the keyboard to the review"
        );
        press(cx, any, "j")?;
        ensure!(
            window.update(cx, |r, _, _| r.selected_file())? == token_test,
            "j moves to the next listed file"
        );
        press(cx, any, "j")?;
        ensure!(
            window.update(cx, |r, _, _| r.selected_file())? == token_test,
            "j stops at the last listed file (ledger.rs and retry.rs aren't listed)"
        );

        press(cx, any, "/")?;
        press(cx, any, "escape")?;
        let (text, focused) = window.update(cx, |r, w, cx| {
            (r.filter_text().to_owned(), r.filter_focused(w, cx))
        })?;
        ensure!(text.is_empty() && focused, "Esc clears the text first");
        press(cx, any, "escape")?;
        ensure!(
            window.update(cx, |r, w, cx| !r.filter_focused(w, cx))?,
            "Esc on empty text returns the keyboard"
        );

        press(cx, any, "/")?;
        type_text(cx, any, "auth")?;
        press(cx, any, "enter")?;
        press(cx, any, "shift-t")?;
        ensure!(
            window.update(cx, |r, _, _| r.polish_state().threads_only)?,
            "⇧T turns the toggle on"
        );
        press(cx, any, "cmd-backspace")?;
        let state = window.update(cx, |r, _, _| r.polish_state())?;
        ensure!(
            state.filter.is_empty() && !state.threads_only,
            "⌘⌫ clears text and toggle: {state:?}"
        );
        close(cx, any)
    }

    /// AC-A3: the diff stays on a file the filter hides; `k` goes to the
    /// last listed file before it; `v` there keeps the selection.
    fn selection_outside_filter(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        let (retry, token_test) = window.update(cx, |r, _, cx| {
            let ids = (file(r, "pay/retry.rs"), file(r, "token_test.rs"));
            r.select_file(ids.0, cx);
            ids
        })?;
        press(cx, any, "/")?;
        type_text(cx, any, "auth")?;
        let (selected, tree, loading) = window.update(cx, |r, _, _| {
            let loading = (0..r.loaded().files.len()).any(|f| r.is_loading(f));
            (r.selected_file(), r.tree_selected_file(), loading)
        })?;
        ensure!(selected == retry, "the diff still shows retry.rs");
        ensure!(tree.is_none(), "no tree row is selected: {tree:?}");
        ensure!(!loading, "typing loads nothing");
        // A click on the diff: the review has the keyboard, text kept.
        window.update(cx, |r, w, cx| w.focus(&r.focus_handle(cx), cx))?;
        press(cx, any, "k")?;
        ensure!(
            window.update(cx, |r, _, _| r.selected_file())? == token_test,
            "k selects the last listed file before retry.rs"
        );
        press(cx, any, "v")?;
        let (selected, viewed) = window.update(cx, |r, _, _| {
            (r.selected_file(), r.loaded().is_viewed(token_test))
        })?;
        ensure!(viewed, "v marks token_test.rs viewed");
        ensure!(selected == token_test, "v on the last listed file keeps it");
        close(cx, any)
    }

    /// AC-A4 (view part): `⇧T` in Files; forced on and inert in Threads;
    /// back in Files the toggle is as it was.
    fn threads_only(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "shift-t")?;
        let (listed, expected) = window.update(cx, |r, _, _| {
            (
                r.listed_files().to_vec(),
                vec![
                    file(r, "AUTH.md"),
                    file(r, "auth/token.rs"),
                    file(r, "ledger.rs"),
                ],
            )
        })?;
        ensure!(
            listed == expected,
            "⇧T lists files with active threads: {listed:?}"
        );
        press(cx, any, "shift-t")?;
        press(cx, any, "t")?;
        ensure!(
            window.update(cx, |r, _, _| r.review_mode() == ReviewMode::Threads
                && r.threads_only_shown())?,
            "Threads mode forces the toggle on"
        );
        press(cx, any, "shift-t")?;
        ensure!(
            window.update(cx, |r, _, _| r.threads_only_shown()
                && !r.polish_state().threads_only)?,
            "⇧T does nothing in Threads"
        );
        press(cx, any, "t")?;
        ensure!(
            window.update(cx, |r, _, _| !r.threads_only_shown())?,
            "back in Files the toggle is off again"
        );
        close(cx, any)
    }

    /// AC-A5: a push re-applies the filter to the new file list.
    fn filter_survives_push(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "/")?;
        type_text(cx, any, "auth")?;
        press(cx, any, "enter")?;
        let mut pushed = fixtures::small().detail;
        pushed.files.push(PrFileChange {
            path: "/src/auth/session.rs".into(),
            original_path: None,
            change_type: ChangeType::Add,
            old_content: None,
            new_content: Some("token-test-new".into()),
            change_tracking_id: None,
        });
        window.update(cx, |r, _, cx| r.set_files(pushed, cx))?;
        draw(cx, any)?;
        let (text, listed, session) = window.update(cx, |r, _, _| {
            (
                r.filter_text().to_owned(),
                r.listed_files().to_vec(),
                file(r, "auth/session.rs"),
            )
        })?;
        ensure!(text == "auth", "the filter text survives the push");
        ensure!(
            listed.len() == 4 && listed.contains(&session),
            "session.rs is listed after the push: {listed:?}"
        );
        close(cx, any)
    }

    // ── B ────────────────────────────────────────────────────────────────

    /// AC-B1 and AC-B5.
    fn threads_view(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "t")?;
        let (order, counts) = window.update(cx, |r, _, _| {
            let order: Vec<String> = r
                .thread_items()
                .iter()
                .map(|item| match *item {
                    Item::Header { group, count } => format!(
                        "{}·{count}",
                        match group {
                            Group::Pr => "Pull request".to_owned(),
                            Group::File(f) => r.loaded().files[f].path.clone(),
                            Group::Gone => "Gone".to_owned(),
                        }
                    ),
                    Item::Card { thread, .. } => format!("#{}", r.loaded().threads[thread].id),
                })
                .collect();
            (order, r.scope_counts())
        })?;
        let expected = [
            "Pull request·1",
            "docs/AUTH.md·1",
            "#2",
            "src/auth/token.rs·2",
            "#4",
            "#3",
            "src/pay/ledger.rs·1",
            "#6",
            "Gone·1",
            "#7",
        ];
        let got: Vec<&str> = order.iter().map(|s| s.trim_start_matches('/')).collect();
        ensure!(
            got[0] == "Pull request·1" && got[1] == "#1" && got[2..] == expected[1..],
            "groups and cards in FR-8 order: {got:?}"
        );
        ensure!(
            (counts.active, counts.resolved) == (6, 1),
            "Active 6 · Resolved 1 · All 7: {counts:?}"
        );

        // AC-B5: keys. `t` opened on the selected file's (token.rs) first
        // card; Home goes to the first card.
        let start = window.update(cx, |r, _, _| focused_id(r))?;
        ensure!(
            start == Some(4),
            "t opens on the selected file's card: {start:?}"
        );
        press(cx, any, "home")?;
        ensure!(
            window.update(cx, |r, _, _| focused_id(r))? == Some(1),
            "Home: first card"
        );
        for _ in 0..3 {
            press(cx, any, "n")?;
        }
        ensure!(
            window.update(cx, |r, _, _| focused_id(r))? == Some(3),
            "n ×3 lands on token.rs 40–42"
        );
        let tree = window.update(cx, |r, _, _| {
            (r.tree_selected_file(), file(r, "auth/token.rs"))
        })?;
        ensure!(tree.0 == Some(tree.1), "the tree follows the focused card");
        press(cx, any, "p")?;
        ensure!(
            window.update(cx, |r, _, _| focused_id(r))? == Some(4),
            "p goes back"
        );
        press(cx, any, "home")?;
        press(cx, any, "j")?;
        ensure!(
            window.update(cx, |r, _, _| focused_id(r))? == Some(2),
            "j from the Pull request card lands on AUTH.md's card"
        );
        press(cx, any, "end")?;
        draw(cx, any)?;
        let (last, top) = window.update(cx, |r, _, _| (focused_id(r), r.focused_card_top()))?;
        ensure!(last == Some(7), "End lands on old.rs");
        ensure!(top.is_some(), "End scrolled old.rs's card into view");
        let viewed_before = window.update(cx, |r, _, _| r.loaded().viewed.len())?;
        press(cx, any, "v")?;
        press(cx, any, "c")?;
        let (viewed, composer) = window.update(cx, |r, _, _| {
            (r.loaded().viewed.len(), r.composer_state().is_some())
        })?;
        ensure!(
            viewed == viewed_before && !composer,
            "v and c do nothing in Threads"
        );
        press(cx, any, "r")?;
        ensure!(
            window.update(cx, |r, _, _| r.composer_state().is_some())?,
            "r opens the reply composer in the focused card"
        );
        press(cx, any, "escape")?;
        // Read-only: no reply.
        window.update(cx, |r, _, cx| r.story_read_only(cx))?;
        press(cx, any, "r")?;
        ensure!(
            window.update(cx, |r, _, _| r.composer_state().is_none())?,
            "read-only: r does nothing"
        );
        press(cx, any, "t")?;
        ensure!(
            window.update(cx, |r, _, _| r.review_mode() == ReviewMode::Files)?,
            "t again shows Files"
        );
        close(cx, any)
    }

    /// Fixture S written to a demo store (a demo repo, so its rows fit),
    /// so resolves and status changes land somewhere.
    fn stored_small(dir: &Path) -> anyhow::Result<(Store, Fixture)> {
        let store = adoken_demo::open_store_at(dir, adoken_demo::DEMO_NOW)?;
        let repo = store.repos()?.into_iter().next().context("a demo repo")?;
        let mut fixture = fixtures::small();
        fixture.detail.pr.repo_id = repo.id.clone();
        fixture.detail.pr.project_id = repo.project_id.clone();
        store.write(|tx| {
            for person in &fixture.people {
                write::upsert_person(tx, person, false)?;
            }
            write::upsert_pr(tx, &fixture.detail.pr)?;
            for thread in &fixture.detail.threads {
                write::upsert_thread(tx, thread)?;
            }
            Ok(())
        })?;
        Ok((store, fixture))
    }

    /// AC-B2: scopes; `⇧R` keeps the resolved card until the scope changes.
    fn resolve_keeps_card(cx: &mut HeadlessAppContext, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let (store, fixture) = stored_small(dir)?;
        let window = open(cx, fixture, Some(store.clone()))?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "t")?;
        dispatch(cx, any, adoken_app::review::pr::ShowResolvedThreads)?;
        let cards = |cx: &mut HeadlessAppContext| {
            window.update(cx, |r, _, _| {
                r.thread_items()
                    .iter()
                    .filter_map(|i| i.thread())
                    .map(|t| r.loaded().threads[t].id)
                    .collect::<Vec<_>>()
            })
        };
        ensure!(
            cards(cx)? == [5],
            "Resolved lists token_test.rs's card only"
        );
        dispatch(cx, any, adoken_app::review::pr::ShowAllThreads)?;
        ensure!(cards(cx)?.len() == 7, "All lists 7");
        dispatch(cx, any, adoken_app::review::pr::ShowActiveThreads)?;
        press(cx, any, "home")?;
        press(cx, any, "n")?;
        press(cx, any, "n")?;
        ensure!(
            window.update(cx, |r, _, _| focused_id(r))? == Some(4),
            "on token.rs line 10"
        );
        press(cx, any, "shift-r")?;
        let (status, counts, still) = window.update(cx, |r, _, _| {
            let t = thread(r, 4);
            (
                r.loaded().threads[t].status,
                r.scope_counts(),
                r.thread_items().iter().any(|i| i.thread() == Some(t)),
            )
        })?;
        ensure!(status.is_resolved(), "the resolve landed: {status:?}");
        ensure!(still, "the resolved card stays in Active");
        ensure!(
            (counts.active, counts.resolved) == (5, 2),
            "Active 5 · Resolved 2: {counts:?}"
        );
        let stored = store
            .pr(4821)?
            .context("S in the store")?
            .threads
            .into_iter()
            .find(|t| t.id == 4)
            .context("thread 4 stored")?
            .status;
        ensure!(stored == ThreadStatus::Fixed, "stored as Fixed: {stored:?}");
        dispatch(cx, any, adoken_app::review::pr::ShowAllThreads)?;
        dispatch(cx, any, adoken_app::review::pr::ShowActiveThreads)?;
        ensure!(
            !cards(cx)?.contains(&4),
            "switching scope and back drops it from Active"
        );
        close(cx, any)
    }

    /// AC-B6: `↩` on a card opens its thread in the diff; not for the Pull
    /// request card or a file no longer in the PR.
    fn open_in_diff(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "t")?;
        press(cx, any, "home")?;
        press(cx, any, "enter")?;
        ensure!(
            window.update(cx, |r, _, _| r.review_mode() == ReviewMode::Threads)?,
            "↩ on the Pull request card does nothing"
        );
        press(cx, any, "end")?;
        press(cx, any, "enter")?;
        ensure!(
            window.update(cx, |r, _, _| r.review_mode() == ReviewMode::Threads)?,
            "↩ on old.rs's card does nothing"
        );
        press(cx, any, "home")?;
        for _ in 0..3 {
            press(cx, any, "n")?;
        }
        press(cx, any, "enter")?;
        draw(cx, any)?;
        let (mode, selected, tree, cursor, on_screen, token) = window.update(cx, |r, _, _| {
            (
                r.review_mode(),
                r.selected_file(),
                r.tree_selected_file(),
                cursor_id(r),
                r.thread_cursor_on_screen(),
                file(r, "auth/token.rs"),
            )
        })?;
        ensure!(mode == ReviewMode::Files, "↩ shows Files");
        ensure!(
            selected == token && tree == Some(token),
            "token.rs selected"
        );
        ensure!(
            cursor == Some(3),
            "the thread cursor is on 40–42: {cursor:?}"
        );
        ensure!(on_screen, "the thread row is in the diff's visible range");
        press(cx, any, "r")?;
        ensure!(
            window.update(cx, |r, _, _| r.composer_state().is_some())?,
            "r replies to that thread"
        );
        press(cx, any, "escape")?;
        close(cx, any)
    }

    /// AC-B7: in Files, `n`/`p` continue into the next / previous file
    /// with a thread in scope.
    fn n_p_across_files(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        window.update(cx, |r, _, cx| {
            let auth = file(r, "AUTH.md");
            r.select_file(auth, cx);
        })?;
        press(cx, any, "n")?;
        ensure!(
            window.update(cx, |r, _, _| cursor_id(r))? == Some(2),
            "AUTH.md's thread"
        );
        let mut landed = Vec::new();
        for _ in 0..4 {
            press(cx, any, "n")?;
            landed.push(window.update(cx, |r, _, _| cursor_id(r))?);
        }
        ensure!(
            landed == [Some(4), Some(3), Some(6), Some(6)],
            "n: token.rs 10, 40–42, ledger.rs's older code, then nothing: {landed:?}"
        );
        let ledger = window.update(cx, |r, _, _| (r.selected_file(), file(r, "ledger.rs")))?;
        ensure!(ledger.0 == ledger.1, "ledger.rs selected");
        press(cx, any, "p")?;
        press(cx, any, "p")?;
        ensure!(
            window.update(cx, |r, _, _| cursor_id(r))? == Some(4),
            "back on token.rs 10"
        );
        press(cx, any, "p")?;
        let (cursor, selected, auth) = window.update(cx, |r, _, _| {
            (cursor_id(r), r.selected_file(), file(r, "AUTH.md"))
        })?;
        ensure!(
            cursor == Some(2) && selected == auth,
            "p from token.rs line 10 lands on AUTH.md's thread"
        );
        close(cx, any)
    }

    /// AC-B8: a synced thread above the focused card doesn't move it.
    fn live_update_keeps_focus(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = open(cx, fixtures::small(), None)?;
        let any: AnyWindowHandle = window.into();
        press(cx, any, "t")?;
        press(cx, any, "home")?;
        for _ in 0..3 {
            press(cx, any, "n")?;
        }
        draw(cx, any)?;
        let before = window
            .update(cx, |r, _, _| r.focused_card_top())?
            .context("token.rs 40–42 on screen")?;
        let mut fresh = fixtures::small().detail.threads;
        let mut added = fresh[1].clone();
        added.id = 9;
        if let Some(anchor) = added.anchor.as_mut() {
            anchor.right = Some(adoken_model::LineRange { start: 5, end: 5 });
        }
        added.comments[0].id = 90;
        added.comments[0].body = "One more on the auth doc.".into();
        fresh.push(added);
        window.update(cx, |r, _, cx| r.merge_threads(fresh, cx))?;
        draw(cx, any)?;
        draw(cx, any)?;
        let (focused, after, counts) = window.update(cx, |r, _, _| {
            (focused_id(r), r.focused_card_top(), r.scope_counts())
        })?;
        ensure!(focused == Some(3), "the focus stays on 40–42");
        let after = after.context("still on screen")?;
        ensure!(
            (after - before).abs() <= px(1.0),
            "the focused card kept its place: {before:?} → {after:?}"
        );
        ensure!(counts.active == 7, "counts updated: {counts:?}");
        let cards = window.update(cx, |r, _, _| {
            r.thread_items()
                .iter()
                .filter_map(|i| i.thread())
                .map(|t| r.loaded().threads[t].id)
                .collect::<Vec<_>>()
        })?;
        ensure!(
            cards.iter().position(|&id| id == 9) < cards.iter().position(|&id| id == 3),
            "the new card is above: {cards:?}"
        );
        close(cx, any)
    }

    // ── D, E on the list ─────────────────────────────────────────────────

    fn stored_pr(store: &Store, id: i64) -> anyhow::Result<PullRequest> {
        Ok(store.pr(id)?.context("PR in store")?.pr)
    }

    fn select(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
        id: i64,
    ) -> anyhow::Result<()> {
        let listed = window.update(cx, |view, _, cx| story::select_pr(view, id, cx))?;
        ensure!(listed, "!{id} is listed");
        Ok(())
    }

    fn menu(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<ReviewView>,
    ) -> anyhow::Result<Vec<String>> {
        // `⌘K` is bound in the shell's Workspace context; outside a shell
        // the action it dispatches is sent directly.
        dispatch(cx, window.into(), adoken_app::pin_actions::OpenRowMenu)?;
        let labels = window
            .update(cx, |view, _, cx| view.row_menu_labels(cx))?
            .context("⌘K opened the row menu")?;
        press(cx, window.into(), "escape")?;
        window.update(cx, |view, w, cx| w.focus(&view.focus_handle(cx), cx))?;
        Ok(labels.iter().map(|l| l.to_string()).collect())
    }

    /// AC-D1, AC-D2, AC-D4, AC-D5, AC-E1.
    fn list_status(cx: &mut HeadlessAppContext, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir, now)?;
        let prs = store.prs(&PrFilter::default())?;
        let pick = |want: fn(&PullRequest) -> bool| {
            prs.iter()
                .find(|p| want(p))
                .map(|p| p.id)
                .context("such a demo PR")
        };
        let active = pick(|p| p.status == PrStatus::Active && !p.is_draft)?;
        let other = prs
            .iter()
            .find(|p| p.status == PrStatus::Active && !p.is_draft && p.id != active)
            .map(|p| p.id)
            .context("a second active PR")?;
        let draft = pick(|p| p.status == PrStatus::Active && p.is_draft)?;
        let abandoned = pick(|p| p.status == PrStatus::Abandoned)?;
        let completed = pick(|p| p.status == PrStatus::Completed)?;
        // AC-E1's title, padded.
        let mut padded = stored_pr(&store, active)?;
        padded.title = "  Fix auth regression ".into();
        store.write(|tx| write::upsert_pr(tx, &padded).map(|_| ()))?;

        let window = cx.open_window(size(px(1100.0), px(640.0)), |window, cx| {
            cx.new(|cx| {
                let mut view = ReviewView::new(store.clone(), window, cx);
                view.set_clock(now, cx);
                view
            })
        })?;
        cx.run_until_parked();
        let any: AnyWindowHandle = window.into();
        window.update(cx, |view, window, cx| {
            window.activate_window();
            view.set_tab(ListTab::All, cx);
            window.focus(&view.focus_handle(cx), cx);
        })?;
        draw(cx, any)?;

        // AC-D1: the row menu's status items.
        let common = [
            "Open",
            "Open in browser",
            "Copy link",
            "Copy PR id",
            "Copy title",
            "Copy source branch",
        ];
        let status_items = |labels: &[String]| labels[common.len() + 1..].to_vec();
        select(cx, window, active)?;
        let labels = menu(cx, window)?;
        ensure!(labels[..common.len()] == common, "row menu: {labels:?}");
        ensure!(labels[common.len()].contains("Pin"), "then Pin: {labels:?}");
        ensure!(
            status_items(&labels) == ["Mark as draft", "Abandon…"],
            "an active PR: {labels:?}"
        );
        select(cx, window, draft)?;
        let labels = menu(cx, window)?;
        ensure!(
            status_items(&labels) == ["Publish", "Abandon…"],
            "a draft: {labels:?}"
        );
        select(cx, window, abandoned)?;
        let labels = menu(cx, window)?;
        ensure!(
            status_items(&labels) == ["Reactivate"],
            "abandoned: {labels:?}"
        );
        select(cx, window, completed)?;
        let labels = menu(cx, window)?;
        ensure!(status_items(&labels).is_empty(), "completed: {labels:?}");

        // AC-E1 from the list: the selected row's title, trimmed.
        select(cx, window, active)?;
        dispatch(cx, any, CopyPrTitle)?;
        let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        ensure!(
            clip.as_deref() == Some("Fix auth regression"),
            "the title, trimmed: {clip:?}"
        );
        let toast = window.update(cx, |view, _, _| view.toast().map(|t| t.to_string()))?;
        ensure!(toast.as_deref() == Some("Copied title"), "toast: {toast:?}");

        // AC-D2: ⇧P shows Draft with "Sending…" on the next frame; one
        // write in flight; the store gets it.
        let shift_p = Keystroke::parse("shift-p")?;
        cx.update_window(any, |_, w, cx| {
            w.draw(cx).clear();
            w.dispatch_keystroke(shift_p.clone(), cx);
            // A second ⇧P before Azure DevOps answered.
            w.dispatch_keystroke(shift_p.clone(), cx);
        })?;
        // Before the write's task runs: what the next frame paints.
        let sending = window.update(cx, |view, _, _| {
            (
                view.listed_pr(active).map(|p| p.is_draft),
                view.is_sending_status(active),
            )
        })?;
        ensure!(
            sending == (Some(true), true),
            "Draft with Sending… at once: {sending:?}"
        );
        cx.run_until_parked();
        let (shown, still_sending) = window.update(cx, |view, _, _| {
            (
                view.listed_pr(active).map(|p| p.is_draft),
                view.is_sending_status(active),
            )
        })?;
        ensure!(
            shown == Some(true) && !still_sending,
            "confirmed: {shown:?}"
        );
        ensure!(
            stored_pr(&store, active)?.is_draft,
            "one write: the second ⇧P sent nothing"
        );

        // AC-D5: the review opened from the row shows Draft.
        window.update(cx, |view, w, cx| view.open_pr(active, w, cx))?;
        cx.run_until_parked();
        draw(cx, any)?;
        let review = window
            .update(cx, |view, _, _| view.open_review())?
            .context("the review opened")?;
        let draft_shown = cx.update(|cx| review.read(cx).loaded().pr().is_draft);
        ensure!(draft_shown, "the review shows Draft");
        // AC-E1 in the review (Cmd+P path).
        cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(String::new())));
        dispatch(cx, any, CopyPrTitle)?;
        let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        ensure!(
            clip.as_deref() == Some("Fix auth regression"),
            "the review copies its title: {clip:?}"
        );
        // AC-D4: a draft's header publishes with ⇧P.
        press(cx, any, "shift-p")?;
        ensure!(
            !cx.update(|cx| review.read(cx).loaded().pr().is_draft),
            "⇧P in the draft's review publishes it"
        );
        // Abandon from the review, then back: the row shows Abandoned.
        press(cx, any, "shift-a")?;
        press(cx, any, "enter")?;
        ensure!(
            stored_pr(&store, active)?.status == PrStatus::Abandoned,
            "abandoned from the review"
        );
        window.update(cx, |view, w, cx| {
            view.back(w, cx);
            w.focus(&view.focus_handle(cx), cx);
        })?;
        cx.run_until_parked();
        let row = window.update(cx, |view, _, _| view.listed_pr(active).map(|p| p.status))?;
        ensure!(
            row == Some(PrStatus::Abandoned),
            "the row shows Abandoned: {row:?}"
        );

        // ⇧A asks first: Esc sends nothing, ↩ abandons.
        select(cx, window, other)?;
        press(cx, any, "shift-a")?;
        ensure!(
            window.update(cx, |view, _, _| view.list_status_overlay())? == Some("confirm"),
            "⇧A shows the confirmation"
        );
        press(cx, any, "escape")?;
        ensure!(
            stored_pr(&store, other)?.status == PrStatus::Active,
            "Esc sends nothing"
        );
        press(cx, any, "shift-a")?;
        press(cx, any, "enter")?;
        ensure!(
            stored_pr(&store, other)?.status == PrStatus::Abandoned,
            "⇧A ↩ abandons"
        );
        // ⇧S: the status menu at the row (Reactivate for the abandoned row).
        press(cx, any, "shift-s")?;
        ensure!(
            window.update(cx, |view, _, _| view.list_status_overlay())? == Some("menu"),
            "⇧S opens the status menu"
        );
        press(cx, any, "escape")?;

        // A refusal reverts the row and says why.
        select(cx, window, draft)?;
        store.write(|tx| {
            tx.execute("DELETE FROM pull_request WHERE id = ?1", [draft])?;
            Ok(())
        })?;
        press(cx, any, "shift-p")?;
        let (shown, toast) = window.update(cx, |view, _, _| {
            (
                view.listed_pr(draft).map(|p| p.is_draft),
                view.toast().map(|t| t.to_string()),
            )
        })?;
        ensure!(
            shown == Some(true),
            "the refused Publish reverted: {shown:?}"
        );
        let toast = toast.unwrap_or_default();
        ensure!(
            toast.starts_with(&format!("Status of !{draft} not changed. ")),
            "the refusal's toast: {toast:?}"
        );
        close(cx, any)
    }
}
