//! Spec 0019 on the L+T fixture (300 files, 500 file threads, 25 PR-level
//! threads) through real windows, real keystrokes and gpui's frame trace.
//! Screenshots go to `target/review-polish-smoke/`.
//!
//! **Budgets (AC-A6, AC-B9, §7).** The L+T review alone, 1280 × 800, with
//! only the selected file's diff computed (as a review opens): each
//! keystroke of `src/auth -test` handled and the tree repainted < 16 ms;
//! `⇧T` likewise; `t` paints the Threads view in its first frame; the list
//! wheeled top to bottom a viewport per frame keeps frame p99 < 16 ms;
//! `↩` on a card whose diff is computed shows Files with the thread in view
//! in one frame; and the heap the Threads view adds, read slowly enough
//! that every card's excerpt is computed, stays under 10 MB.
//!
//! **Shell run (AC-S2).** The real shell on the demo store with L+T seeded
//! as a demo PR: `/` `auth` `↓` `↩` `j`, `⇧T`, `t`, `n` ×5, `↩`, `r`
//! `Esc`, back to the list, `⌘K` › Mark as draft, `⌘P` › "Copy source
//! branch"; frame p99 < 16 ms throughout.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("review_polish_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adoken_app::palette::catalog::Target;
    use adoken_app::review::data::Layout;
    use adoken_app::review::fixtures::{self, Fixture};
    use adoken_app::review::pr::{PrReview, ReviewMode};
    use adoken_app::review::threads::{Group, Item};
    use adoken_app::review::{Part, ReviewView, story};
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_perf::{Budget, FrameRecorder, Samples};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AnyWindowHandle, AppContext as _, Entity, Focusable as _, HeadlessAppContext, Keystroke,
        ScrollDelta, ScrollWheelEvent, TouchPhase, WindowHandle, point, px, size,
    };

    /// Live heap bytes across every thread (excerpt diffs are computed on
    /// the background executor and kept by the review).
    mod heap {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::sync::atomic::{AtomicIsize, Ordering};

        static LIVE: AtomicIsize = AtomicIsize::new(0);

        pub fn live() -> isize {
            LIVE.load(Ordering::Relaxed)
        }

        pub struct Counting;

        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
                unsafe { System.alloc(layout) }
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
                unsafe { System.alloc_zeroed(layout) }
            }

            unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
                LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
                unsafe { System.dealloc(ptr, layout) }
            }

            unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
                LIVE.fetch_add(
                    new_size as isize - layout.size() as isize,
                    Ordering::Relaxed,
                );
                unsafe { System.realloc(ptr, layout, new_size) }
            }
        }
    }

    #[global_allocator]
    static ALLOCATOR: heap::Counting = heap::Counting;

    const FRAME: Duration = Duration::from_millis(16);
    const MEMORY: isize = 10 * 1024 * 1024;
    const WIDTH: f32 = 1280.0;
    const HEIGHT: f32 = 800.0;
    /// Over the Threads list (right of the file tree).
    const THREADS_AT: (f32, f32) = (820.0, 480.0);
    const TYPED: &str = "src/auth -test";
    /// The L+T PR's id in the demo store.
    const PR: i64 = 9001;

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
            // As `adoken_app::run`: the shell tells it which view is in
            // front, which the `⌘P` copy and status commands ask (FR-19).
            adoken_app::background::init(cx);
            cx.bind_keys(adoken_app::key_bindings());
        });
        budgets(&mut cx)?;
        shell_run(&mut cx)?;
        println!("review_polish_smoke: L+T budgets and the shell run passed");
        Ok(())
    }

    // ── budgets ──────────────────────────────────────────────────────────

    fn budgets(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let fixture = fixtures::large(300, 40_000);
        let expected = fixture
            .detail
            .files
            .iter()
            .filter(|f| f.path.contains("src/auth") && !f.path.contains("test"))
            .count();
        let window = cx.open_window(size(px(WIDTH), px(HEIGHT)), move |window, cx| {
            cx.new(|cx| {
                story::fixture_review(fixture, None, Layout::Unified, Part::All, window, cx)
            })
        })?;
        cx.run_until_parked();
        window.update(cx, |review, window, cx| {
            window.activate_window();
            window.focus(&review.focus_handle(cx), cx);
            // As a review opens: only the selected file's diff is
            // computed; the Threads view computes the rest as cards dwell.
            review.story_uncomputed();
        })?;
        let any: AnyWindowHandle = window.into();
        render(cx, any)?;
        render(cx, any)?;
        save(cx, any, "budget-files")?;

        // AC-A6: every keystroke of `src/auth -test`, handled and painted.
        press(cx, any, "/")?;
        ensure!(
            window.update(cx, |r, w, cx| r.filter_focused(w, cx))?,
            "/ focuses the filter"
        );
        let mut keystrokes = Samples::new("filter keystroke");
        for ch in TYPED.chars() {
            keystrokes.record(timed(cx, any, typed(ch)?)?);
        }
        let (text, listed) = window.update(cx, |r, _, _| {
            (r.filter_text().to_owned(), r.listed_files().len())
        })?;
        ensure!(
            text == TYPED && listed == expected,
            "`{text}` lists {listed} files, not {expected}"
        );
        save(cx, any, "budget-filter")?;
        report("AC-A6 filter keystroke (handled + painted)", &keystrokes)?;
        press(cx, any, "escape")?;
        press(cx, any, "escape")?;

        // §7: ⇧T in one frame.
        let shift_t = timed(cx, any, Keystroke::parse("shift-t")?)?;
        ensure!(
            window.update(cx, |r, _, _| r.threads_only_shown())?,
            "⇧T turns on Only files with threads"
        );
        println!("review_polish_smoke: ⇧T handled + painted {}", ms(shift_t));
        adoken_perf::ensure_budget!(shift_t < FRAME, "⇧T took {}", ms(shift_t));
        press(cx, any, "shift-t")?;

        // AC-B9: `t` paints the Threads view in its first frame.
        cx.run_until_parked();
        let before = heap::live();
        let t = timed(cx, any, Keystroke::parse("t")?)?;
        let (mode, card) = window.update(cx, |r, _, _| (r.review_mode(), r.focused_card_top()))?;
        ensure!(
            mode == ReviewMode::Threads && card.is_some(),
            "the first frame after t shows Threads with its focused card ({mode:?}, {card:?})"
        );
        println!("review_polish_smoke: t handled + painted {}", ms(t));
        adoken_perf::ensure_budget!(t < FRAME, "t took {}", ms(t));
        save(cx, any, "budget-threads")?;

        // AC-B9: top to bottom at maximum velocity (a viewport per frame,
        // no card dwells), then back up slowly enough that every card
        // dwells and its excerpt computes.
        press(cx, any, "home")?;
        let mut frames = FrameRecorder::start();
        let mut steps = 0;
        while !window.update(cx, |r, _, _| r.threads_scroll().1)? {
            ensure!(steps < 400, "the Threads list reaches its end");
            wheel(cx, any, HEIGHT)?;
            steps += 1;
        }
        frames.collect();
        let scroll = frames.draw().clone();
        drop(frames);
        let rows = window.update(cx, |r, _, _| r.thread_items().len())?;
        println!("review_polish_smoke: {rows} Threads rows, end reached in {steps} frames");
        save(cx, any, "budget-threads-end")?;
        report("AC-B9 Threads scroll frame", &scroll)?;
        let mut up = 0;
        while window.update(cx, |r, _, _| r.threads_scroll().0)? > 0 {
            ensure!(up < 800, "the Threads list reaches its top");
            wheel(cx, any, -HEIGHT / 2.0)?;
            dwell(cx, any)?;
            up += 1;
        }
        dwell(cx, any)?;
        let computed = window.update(cx, |r, _, _| {
            let files: std::collections::HashSet<usize> = r
                .thread_items()
                .iter()
                .filter_map(|item| match item {
                    Item::Card {
                        group: Group::File(f),
                        ..
                    } => Some(*f),
                    _ => None,
                })
                .collect();
            let done = files
                .iter()
                .filter(|f| r.loaded().files[**f].diff.is_some())
                .count();
            (done, files.len())
        })?;
        let added = heap::live() - before;
        println!(
            "review_polish_smoke: Threads view heap +{:.2} MB with {}/{} files' excerpts computed (budget 10 MB)",
            added as f64 / 1_048_576.0,
            computed.0,
            computed.1
        );
        ensure!(
            computed.0 == computed.1,
            "reading every card computes every excerpt: {computed:?}"
        );
        ensure!(added < MEMORY, "the Threads view adds {added} bytes");

        // AC-B9 / AC-B6: ↩ on a file card whose diff is computed.
        press(cx, any, "home")?;
        press(cx, any, "j")?;
        let (thread, file) = window
            .update(cx, |r, _, _| {
                let thread = r.focused_card()?;
                let item = r
                    .thread_items()
                    .iter()
                    .find(|i| i.thread() == Some(thread))?;
                match item {
                    Item::Card {
                        group: Group::File(f),
                        ..
                    } => Some((thread, *f)),
                    _ => None,
                }
            })?
            .context("j lands on the first file's card")?;
        ensure!(
            window.update(cx, |r, _, _| r.loaded().files[file].diff.is_some())?,
            "its diff is computed"
        );
        let enter = timed(cx, any, Keystroke::parse("enter")?)?;
        let landed = window.update(cx, |r, _, _| {
            (
                r.review_mode(),
                r.selected_file(),
                r.thread_cursor_thread(),
                r.thread_cursor_on_screen(),
            )
        })?;
        ensure!(
            landed == (ReviewMode::Files, file, Some(thread), true),
            "the first frame after ↩ shows the thread in Files: {landed:?}"
        );
        println!(
            "review_polish_smoke: ↩ to a computed diff handled + painted {}",
            ms(enter)
        );
        adoken_perf::ensure_budget!(enter < FRAME, "↩ took {}", ms(enter));
        save(cx, any, "budget-open-in-diff")?;
        close(cx, any)
    }

    /// Lets the cards on screen dwell (FR-11) and their excerpts land. The
    /// dwell compares two layouts, so it can take two timer rounds.
    fn dwell(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        for _ in 0..2 {
            cx.advance_clock(Duration::from_millis(200));
            cx.run_until_parked();
            render(cx, window)?;
            cx.run_until_parked();
        }
        Ok(())
    }

    fn report(name: &'static str, samples: &Samples) -> anyhow::Result<()> {
        let summary = samples.summary().context("samples")?;
        let check = Budget::FRAME.check(&summary);
        println!(
            "review_polish_smoke: {name}: n={} p50={} p99={} max={}",
            summary.count,
            ms(summary.p50),
            ms(summary.p99),
            ms(summary.max)
        );
        adoken_perf::ensure_budget!(check.passed(), "{name} over budget: {}", check.row());
        Ok(())
    }

    // ── shell run (AC-S2) ────────────────────────────────────────────────

    /// L+T as demo PR !9001: the demo org's first repository, its user as
    /// the reviewer, blobs in the store's cache under object ids.
    fn seed(store: &adoken_store::Store, mut fixture: Fixture) -> anyhow::Result<String> {
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        ensure!(ds.pr(PR).is_none(), "!{PR} is free in the demo org");
        let repo = ds.repos.first().context("a demo repository")?;
        let pr = &mut fixture.detail.pr;
        pr.repo_id = repo.id.clone();
        pr.project_id = repo.project_id.clone();
        for reviewer in &mut pr.reviewers {
            if reviewer.person.as_str() == fixtures::ME {
                reviewer.person = ds.me.clone();
            }
        }
        let branch = pr
            .source_branch
            .trim_start_matches("refs/heads/")
            .to_owned();
        let mut ids = std::collections::HashMap::new();
        for change in &mut fixture.detail.files {
            for blob in [&mut change.old_content, &mut change.new_content]
                .into_iter()
                .flatten()
            {
                let next = ids.len();
                let id = ids
                    .entry(blob.clone())
                    .or_insert_with(|| format!("{:040x}", 0xb10b_0000 + next));
                let text = fixture.blobs.get(blob.as_str()).context("fixture blob")?;
                store.blobs().put(id, text.as_bytes())?;
                *blob = id.clone();
            }
        }
        store.write(|tx| {
            for person in fixture
                .people
                .iter()
                .filter(|p| p.id.as_str() != fixtures::ME)
            {
                adoken_store::write::upsert_person(tx, person, false)?;
            }
            let detail = &fixture.detail;
            adoken_store::write::upsert_pr(tx, &detail.pr)?;
            adoken_store::write::put_pr_iterations(tx, PR, &detail.iterations)?;
            adoken_store::write::put_pr_files(tx, PR, &detail.files)?;
            for thread in &detail.threads {
                adoken_store::write::upsert_thread(tx, thread)?;
            }
            adoken_store::write::mark_pr_hydrated(tx, PR, adoken_demo::DEMO_NOW)?;
            Ok(())
        })?;
        Ok(branch)
    }

    fn shell_run(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW)?;
        let fixture = fixtures::large(300, 40_000);
        ensure!(fixture.detail.pr.id == PR, "L+T is !{PR}");
        let branch = seed(&store, fixture)?;
        let window = cx.open_window(size(px(WIDTH), px(HEIGHT)), |window, cx| {
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
        settle(cx, |cx| {
            window.update(cx, |shell, _, _| shell.review_view().is_some())
        })
        .context("the store opens")?;
        let any: AnyWindowHandle = window.into();
        let mut frames = FrameRecorder::start();

        window.update(cx, |shell, window, cx| {
            shell.open_target(Target::PullRequest(PR), window, cx)
        })?;
        settle(cx, |cx| Ok(review(cx, window)?.is_some()))
            .with_context(|| format!("!{PR}'s review opens"))?;
        let review = review(cx, window)?.context("the review")?;
        settle(cx, |cx| {
            render(cx, any)?;
            Ok(read(cx, &review, |r| {
                r.loaded().files[r.selected_file()].diff.is_some()
            }))
        })
        .context("the first file's diff computes")?;
        paint(cx, any, "shell-review")?;

        // `/` `auth` `↓` `↩` `j`.
        press(cx, any, "/")?;
        for ch in "auth".chars() {
            press_keystroke(cx, any, typed(ch)?)?;
        }
        let (listed, selected) = read(cx, &review, |r| {
            (r.listed_files().to_vec(), r.selected_file())
        });
        ensure!(listed.len() > 2, "auth lists files");
        // ↓: the next listed file (the first when the selection isn't).
        let next = listed
            .iter()
            .position(|f| *f == selected)
            .map_or(0, |at| at + 1);
        press(cx, any, "down")?;
        ensure!(
            read(cx, &review, |r| r.selected_file()) == listed[next],
            "↓ selects the next listed file"
        );
        settle(cx, |cx| {
            render(cx, any)?;
            Ok(read(cx, &review, |r| {
                r.loaded().files[listed[next]].diff.is_some()
            }))
        })
        .context("its diff loads")?;
        press(cx, any, "enter")?;
        ensure!(
            !window.update(cx, |_, w, cx| review.read(cx).filter_focused(w, cx))?,
            "↩ returns the keyboard"
        );
        press(cx, any, "j")?;
        ensure!(
            read(cx, &review, |r| r.selected_file()) == listed[next + 1],
            "j moves among the listed files"
        );
        paint(cx, any, "shell-filter")?;

        // `⇧T`, `t`, `n` ×5, `↩`.
        press(cx, any, "shift-t")?;
        ensure!(
            read(cx, &review, |r| r.threads_only_shown()),
            "⇧T: only files with threads"
        );
        paint(cx, any, "shell-threads-only")?;
        press(cx, any, "t")?;
        ensure!(
            read(cx, &review, |r| r.review_mode()) == ReviewMode::Threads,
            "t shows Threads"
        );
        paint(cx, any, "shell-threads")?;
        let mut cards = vec![read(cx, &review, |r| r.focused_card())];
        for _ in 0..5 {
            press(cx, any, "n")?;
            cards.push(read(cx, &review, |r| r.focused_card()));
        }
        ensure!(
            cards.windows(2).all(|w| w[0] != w[1] && w[1].is_some()),
            "n ×5 moves card by card: {cards:?}"
        );
        dwell(cx, any)?;
        paint(cx, any, "shell-threads-n")?;
        let thread = cards[5].context("a focused card")?;
        press(cx, any, "enter")?;
        settle(cx, |cx| {
            render(cx, any)?;
            Ok(read(cx, &review, |r| {
                r.review_mode() == ReviewMode::Files
                    && r.thread_cursor_thread() == Some(thread)
                    && r.thread_cursor_on_screen()
            }))
        })
        .context("↩ lands on the thread in Files")?;
        paint(cx, any, "shell-open-in-diff")?;

        // `r` `Esc`.
        press(cx, any, "r")?;
        ensure!(
            read(cx, &review, |r| r.composer_state().is_some()),
            "r opens the reply composer"
        );
        paint(cx, any, "shell-reply")?;
        press(cx, any, "escape")?;
        ensure!(
            read(cx, &review, |r| r.composer_state().is_none()),
            "Esc closes it"
        );

        // Back to the list; `⌘K` › Mark as draft.
        press(cx, any, "escape")?;
        let view = window
            .update(cx, |shell, _, _| shell.review_view().cloned())?
            .context("the PR view")?;
        settle(cx, |cx| {
            Ok(!view.read_with(cx, |v, _| v.is_showing_review()))
        })
        .context("Esc goes back to the list")?;
        ensure!(
            window.update(cx, |shell, _, cx| shell.selected_pr_id(cx))? == Some(PR),
            "the list keeps !{PR} selected"
        );
        paint(cx, any, "shell-list")?;
        // A draft leaves To review; All keeps the row in view.
        palette_command(cx, window, "Pull requests: all")?;
        ensure!(
            view.read_with(cx, |v, _| v.row_ids().contains(&PR)),
            "All lists !{PR}"
        );
        if window.update(cx, |shell, _, cx| shell.selected_pr_id(cx))? != Some(PR) {
            ensure!(
                view.update(cx, |v, cx| story::select_pr(v, PR, cx)),
                "!{PR} selectable in All"
            );
        }
        press(cx, any, "cmd-k")?;
        let labels = view
            .read_with(cx, |v, cx| v.row_menu_labels(cx))
            .context("⌘K opens the row menu")?;
        let at = labels
            .iter()
            .position(|l| l.as_ref() == "Mark as draft")
            .with_context(|| format!("Mark as draft in {labels:?}"))?;
        paint(cx, any, "shell-row-menu")?;
        for _ in 0..at {
            press(cx, any, "down")?;
        }
        press(cx, any, "enter")?;
        settle(cx, |cx| {
            Ok(view.read_with(cx, |v, _| v.listed_pr(PR).is_some_and(|p| p.is_draft)))
        })
        .context("⌘K › Mark as draft makes !9001 a draft")?;
        paint(cx, any, "shell-draft")?;
        ensure!(
            window.update(cx, |shell, _, cx| shell.selected_pr_id(cx))? == Some(PR),
            "the draft stays selected in All"
        );

        // `⌘P` › "Copy source branch".
        cx.update(|cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(String::new())));
        palette_command(cx, window, "Copy source branch")?;
        let clip = cx.update(|cx| cx.read_from_clipboard().and_then(|c| c.text()));
        ensure!(
            clip.as_deref() == Some(branch.as_str()),
            "the source branch is copied: {clip:?}"
        );
        paint(cx, any, "shell-copied-branch")?;
        ensure!(
            window.update(cx, |shell, _, _| shell.nav())? == Nav::PullRequests,
            "still on Pull requests"
        );

        frames.collect();
        let summary = frames.draw().summary().context("frames recorded")?;
        println!(
            "review_polish_smoke shell frames: n={} p50={} p99={} max={}",
            summary.count,
            ms(summary.p50),
            ms(summary.p99),
            ms(summary.max)
        );
        adoken_perf::ensure_budget!(summary.p99 < FRAME, "frame p99 {} ≥ 16 ms", ms(summary.p99));
        drop(frames);
        close(cx, any)
    }

    fn review(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<Option<Entity<PrReview>>> {
        window.update(cx, |shell, _, cx| {
            shell
                .review_view()
                .and_then(|view: &Entity<ReviewView>| view.read(cx).open_review())
                .filter(|review| review.read(cx).loaded().pr().id == PR)
        })
    }

    fn read<R>(
        cx: &mut HeadlessAppContext,
        review: &Entity<PrReview>,
        f: impl FnOnce(&PrReview) -> R,
    ) -> R {
        review.read_with(cx, |r, _| f(r))
    }

    // ── driving ──────────────────────────────────────────────────────────

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

    /// `⌘P`, `title` typed, `↩` once it's the selected result.
    fn palette_command(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        title: &str,
    ) -> anyhow::Result<()> {
        press(cx, window.into(), "cmd-p")?;
        window.update(cx, |shell, _, cx| {
            shell
                .palette()
                .update(cx, |palette, cx| palette.set_query(title, cx))
        })?;
        settle(cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell.palette().read(cx).selected_title().as_ref() == title
            })
        })
        .with_context(|| format!("⌘P finds {title:?}"))?;
        press(cx, window.into(), "enter")
    }

    /// The keystroke typing `ch` produces.
    fn typed(ch: char) -> anyhow::Result<Keystroke> {
        let key = match ch {
            ' ' => "space".to_owned(),
            c => c.to_string(),
        };
        let mut keystroke = Keystroke::parse(&key).context("keystroke")?;
        keystroke.key_char = Some(ch.to_string());
        Ok(keystroke)
    }

    fn press(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        keys: &str,
    ) -> anyhow::Result<()> {
        press_keystroke(cx, window, Keystroke::parse(keys).context("keystroke")?)
    }

    /// Paints (so the focus path is in the dispatch tree), then the
    /// keystroke, then lets background work land.
    fn press_keystroke(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        keystroke: Keystroke,
    ) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// The keystroke handled and the next frame drawn: what the user waits
    /// for between the key and the result on screen.
    fn timed(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        keystroke: Keystroke,
    ) -> anyhow::Result<Duration> {
        let took = cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            let started = Instant::now();
            window.dispatch_keystroke(keystroke, cx);
            window.draw(cx).clear();
            started.elapsed()
        })?;
        cx.run_until_parked();
        Ok(took)
    }

    /// A wheel of `down` px over the Threads list, then the frame that
    /// applies it.
    fn wheel(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        down: f32,
    ) -> anyhow::Result<()> {
        let position = point(px(THREADS_AT.0), px(THREADS_AT.1));
        cx.update_window(window, |_, window, cx| {
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

    fn render(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, cx| window.draw(cx).clear())?;
        Ok(())
    }

    fn close(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, _| window.remove_window())?;
        cx.run_until_parked();
        Ok(())
    }

    fn paint(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        name: &str,
    ) -> anyhow::Result<()> {
        render(cx, window)?;
        save(cx, window, name)
    }

    /// The last drawn frame, as `target/review-polish-smoke/{name}.png`.
    fn save(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        name: &str,
    ) -> anyhow::Result<()> {
        let image = cx
            .update_window(window, |_, window, _| window.render_to_image())?
            .context("rendered image")?;
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/review-polish-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }

    fn ms(d: Duration) -> String {
        format!("{:.2} ms", d.as_secs_f64() * 1e3)
    }
}
