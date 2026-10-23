//! Guided tours (spec 0020 AC-13, with AC-1–AC-4, AC-8 and AC-10 in a real
//! window): the real shell on the demo store with a fresh `settings.toml`,
//! the app keymap and the test scheduler's clock (every tour timer runs on
//! it). Screenshots go to `target/tours-smoke/`.
//!
//! 1. Onboarding's sheet over the loaded shell owns the keyboard: no tour.
//!    It closes (focus back on Today) while the post-onboarding hint
//!    shows: no tour while it shows, none 0.5 s after it hides.
//! 2. The first `⌘P` starts the palette tour (Q-2), never Today's under
//!    it; typing `fix` ends it and lands in the query; with the palette
//!    open nothing starts.
//! 3. Cooldown: nothing at T + 60 s or T + 179.5 s; a key then holds
//!    Today's first use until 1 s of quiet; then it starts and `seen` is
//!    written.
//! 4. `→` `←` `Tab` `↩` `Esc` act on the tour only (Today's selection
//!    stays); `⌘P` › "Tour: Today" replays it without touching `seen`;
//!    `j` ends it and still moves Today's selection by one; a replay `→`
//!    to Done.
//! 5. Reduce Motion on: the mark paints at rest in its first frame and is
//!    gone the frame after `Esc`. Off: it travels in and fades out.
//! 6. After the cooldown a PR opens: its tour starts after 1 s of quiet,
//!    `→` to Files, `j` ends it and selects the file `j` selects with no
//!    tour. `⌘P` › "Tour: Inbox" switches to the Inbox and starts its
//!    tour.
//!
//! Frame p99 < 16 ms throughout.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tours_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use adoken_app::onboarding::{Onboarding, Story};
    use adoken_app::palette::catalog::Target;
    use adoken_app::review::pr::PrReview;
    use adoken_app::settings::model::Motion;
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::today::model::Row;
    use adoken_app::tours::settings::TourSettings;
    use adoken_app::tours::{self, TourId, Tours};
    use adoken_perf::FrameRecorder;
    use adoken_theme::{ThemeRegistry, motion};
    use adoken_ui::Theme;
    use adoken_ui::tour::CoachButton;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Focusable as _,
        HeadlessAppContext, IntoElement, Keystroke, Pixels, Render, Window, WindowHandle, div,
        prelude::*, px, size,
    };

    const FRAME: Duration = Duration::from_millis(16);
    /// Clock steps while waiting on the trigger: finer than its 250 ms poll.
    const STEP: Duration = Duration::from_millis(50);
    /// The review story's PR (5 files, 2 threads).
    const PR: i64 = 4318;

    /// The window root as the app's: the shell, with onboarding's Replace
    /// token sheet over it when open (crates/app/src/lib.rs `Root`).
    struct Harness {
        shell: Entity<Shell>,
        sheet: Option<Entity<Onboarding>>,
    }

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.shell.clone()).children(
                self.sheet
                    .clone()
                    .map(|sheet| div().absolute().inset_0().child(sheet)),
            )
        }
    }

    /// `cx` last: the entity handles drop before the app checks for leaks.
    struct Run {
        window: WindowHandle<Harness>,
        shell: Entity<Shell>,
        tours: Entity<Tours>,
        settings: PathBuf,
        cx: HeadlessAppContext,
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            // The spec's harness: reduce motion on unless stated.
            cx.set_reduce_motion(true);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            adoken_gallery::init(cx);
            cx.clear_key_bindings();
            adoken_app::background::init(cx);
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        let settings = dir.path().join("settings.toml");
        // Fresh `[tours]`; Settings › General › Motion: Reduced (the spec's
        // harness); every hour a working hour, so Today shows Up next.
        std::fs::write(
            &settings,
            "[general]\nmotion = \"reduced\"\n\n\
             [nudges]\ndays = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\n\
             start = \"00:00\"\nend = \"23:59\"\n",
        )?;
        cx.update(|cx| adoken_app::settings::init(settings.clone(), true, cx));
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::now_ms())?;
        let window = cx.open_window(size(px(1280.0), px(800.0)), |window, cx| {
            let shell = cx.new(|cx| {
                Shell::new(
                    StoreSource::Open {
                        store: store.clone(),
                        demo: true,
                    },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                )
            });
            cx.new(|_| Harness { shell, sheet: None })
        })?;
        window.update(&mut cx, |_, window, _| window.activate_window())?;
        let (shell, tours) = window.update(&mut cx, |root, _, cx| {
            (root.shell.clone(), root.shell.read(cx).tours().clone())
        })?;
        let mut run = Run {
            cx,
            window,
            shell,
            tours,
            settings,
        };
        settle(&mut run, |run| {
            let shell = run.shell.clone();
            Ok(run.cx.read_entity(&shell, |shell, cx| {
                shell
                    .today_view()
                    .is_some_and(|today| today.read(cx).loaded().is_some())
            }))
        })
        .context("the demo store opens and Today computes its day")?;
        // Onboarding's sheet over the loaded shell has the keyboard, as
        // Root's `show_sheet` gives it (Replace token, Change access…).
        run.window.update(&mut run.cx, |root, window, cx| {
            let sheet = cx.new(|cx| Onboarding::story(Story::Welcome, window, cx));
            window.focus(&sheet.focus_handle(cx), cx);
            root.sheet = Some(sheet);
            cx.notify();
        })?;

        let mut frames = FrameRecorder::start();
        gates(&mut run)?;
        first_use(&mut run)?;
        keys_and_replay(&mut run)?;
        reduce_motion(&mut run)?;
        review_and_inbox(&mut run)?;
        frames.collect();
        let summary = frames.draw().summary().context("frames recorded")?;
        println!(
            "tours_smoke frames: n={}, p50={:?}, p99={:?}, max={:?}",
            summary.count, summary.p50, summary.p99, summary.max
        );
        adoken_perf::ensure_budget!(summary.p99 < FRAME, "frame p99 {:?} ≥ 16 ms", summary.p99);
        drop(frames);
        let window: AnyWindowHandle = run.window.into();
        run.cx
            .update_window(window, |_, window, _| window.remove_window())?;
        println!("tours_smoke: ok");
        Ok(())
    }

    // ── 1–2: gates ───────────────────────────────────────────────────────

    fn gates(run: &mut Run) -> anyhow::Result<()> {
        ensure!(nav(run) == Nav::Today, "the shell opens on Today");
        idle(run, Duration::from_secs(3))?;
        paint(run, "onboarding-sheet")?;
        ensure!(
            showing(run).is_none(),
            "no tour while onboarding's sheet has the keyboard: {:?}",
            showing(run)
        );

        // The sheet closes (Root's `close_sheet`: focus back on the shown
        // view) while the post-onboarding hint shows.
        run.cx.update(|cx| tours::set_hint_showing(true, cx));
        run.window.update(&mut run.cx, |root, window, cx| {
            root.sheet = None;
            root.shell
                .update(cx, |shell, cx| shell.focus_current(window, cx));
            cx.notify();
        })?;
        idle(run, Duration::from_secs(3))?;
        ensure!(showing(run).is_none(), "no tour while the hint shows");
        run.cx.update(|cx| tours::set_hint_showing(false, cx));
        idle(run, Duration::from_millis(500))?;
        ensure!(
            showing(run).is_none(),
            "0.5 s after the hint hides: still waiting for 1 s of quiet"
        );
        ensure!(seen(run)?.seen.is_empty(), "nothing is seen yet");

        // The first ⌘P (empty query) starts the palette tour inside the
        // palette, never Today's under it.
        press(run, "cmd-p")?;
        idle(run, Duration::from_millis(300))?;
        ensure!(
            matches!(showing(run), Some((TourId::Palette, 0, _))),
            "the first ⌘P starts the palette tour: {:?}",
            showing(run)
        );
        paint(run, "palette-tour")?;
        type_text(run, "fix")?;
        ensure!(showing(run).is_none(), "typing ends the palette tour");
        ensure!(
            query(run) == "fix",
            "every typed character lands in the query: {:?}",
            query(run)
        );
        idle(run, Duration::from_secs(3))?;
        ensure!(
            showing(run).is_none(),
            "nothing starts over the open palette"
        );
        // Esc clears the query, then closes the palette.
        press(run, "escape")?;
        if palette_visible(run) {
            press(run, "escape")?;
        }
        ensure!(!palette_visible(run), "Esc closes the palette");
        Ok(())
    }

    // ── 3: first use after the cooldown and 1 s of quiet ─────────────────

    fn first_use(run: &mut Run) -> anyhow::Result<()> {
        // The palette tour ended ~3 s ago (T).
        idle(run, Duration::from_secs(57))?;
        ensure!(showing(run).is_none(), "T + 60 s: cooldown, no tour");
        idle(run, Duration::from_millis(119_500))?;
        ensure!(showing(run).is_none(), "T + 179.5 s: still the cooldown");
        // A key just before the cooldown ends: Today's tour then waits 1 s
        // from it.
        let before = row(run);
        press(run, "j")?;
        ensure!(row(run) != before, "j moves Today's selection");
        idle(run, Duration::from_millis(700))?;
        ensure!(
            showing(run).is_none(),
            "past the cooldown, 0.7 s after a key: no tour yet"
        );
        idle(run, Duration::from_millis(600))?;
        let Some((TourId::Today, 0, steps)) = showing(run) else {
            bail!(
                "1 s after the last key, past the cooldown: Today's tour ({:?})",
                showing(run)
            );
        };
        ensure!(steps >= 3, "Today's tour shows its visible steps: {steps}");
        settle(run, |run| Ok(seen(run)?.is_seen("today")))
            .context("settings.toml gets seen = [.., \"today\"]")?;
        ensure!(
            seen(run)?.is_seen("palette"),
            "the palette tour was marked seen when it showed"
        );
        paint(run, "today-step-1")?;
        Ok(())
    }

    // ── 4: tour keys, replay, the no-lost-key rule ───────────────────────

    fn keys_and_replay(run: &mut Run) -> anyhow::Result<()> {
        let selected = row(run);
        press(run, "right")?;
        press(run, "right")?;
        ensure!(step(run) == Some(2), "→ ×2: step 3 ({:?})", showing(run));
        press(run, "left")?;
        ensure!(step(run) == Some(1), "←: step 2");
        ensure!(
            button(run) == Some(CoachButton::Next),
            "Next has the tour's focus"
        );
        press(run, "tab")?;
        ensure!(button(run) == Some(CoachButton::Skip), "Tab: Skip tour");
        press(run, "tab")?;
        ensure!(button(run) == Some(CoachButton::Back), "Tab: Back");
        press(run, "tab")?;
        ensure!(button(run) == Some(CoachButton::Next), "Tab wraps to Next");
        press(run, "shift-tab")?;
        ensure!(button(run) == Some(CoachButton::Back), "⇧Tab: Back");
        press(run, "enter")?;
        ensure!(step(run) == Some(0), "↩ on Back: step 1");
        press(run, "enter")?;
        ensure!(step(run) == Some(1), "↩ on Next: step 2");
        paint(run, "today-step-2")?;
        ensure!(
            row(run) == selected,
            "tour keys reach only the tour: Today's selection stays"
        );
        press(run, "escape")?;
        ensure!(showing(run).is_none(), "Esc ends the tour");
        ensure!(row(run) == selected, "Esc reached only the tour");
        let seen_before = std::fs::read_to_string(&run.settings)?;

        // ⌘P › "Tour: Today" replays it, seen or not.
        replay(run, "Tour: Today", TourId::Today)?;
        ensure!(step(run) == Some(0), "the replay starts on step 1");
        idle(run, Duration::from_millis(200))?;
        ensure!(
            std::fs::read_to_string(&run.settings)? == seen_before,
            "a replay leaves settings.toml alone"
        );
        // Any other key ends the tour and still does its job.
        let before = row(run).context("a selected row")?;
        press(run, "j")?;
        ensure!(showing(run).is_none(), "j ends the tour");
        let after = row(run).context("a selected row after j")?;
        ensure!(
            next_row(before) == Some(after),
            "j still moves Today's selection one row: {before:?} → {after:?}"
        );
        press(run, "k")?;
        ensure!(row(run) == Some(before), "k: back, the tour gone");

        // → through every step; on the last, Next reads Done and → ends it.
        replay(run, "Tour: Today", TourId::Today)?;
        let (_, _, steps) = showing(run).context("the replay shows")?;
        for at in 1..steps {
            press(run, "right")?;
            ensure!(step(run) == Some(at), "→: step {}", at + 1);
        }
        paint(run, "today-done")?;
        press(run, "right")?;
        ensure!(showing(run).is_none(), "→ on the last step: Done");
        ensure!(row(run) == Some(before), "Done reached only the tour");
        Ok(())
    }

    fn next_row(row: Row) -> Option<Row> {
        match row {
            Row::Pr(ix) => Some(Row::Pr(ix + 1)),
            _ => None,
        }
    }

    // ── 5: Reduce Motion ─────────────────────────────────────────────────

    fn reduce_motion(run: &mut Run) -> anyhow::Result<()> {
        // On: the first painted mark is already at rest; Esc removes it in
        // the next frame.
        replay(run, "Tour: Today", TourId::Today)?;
        let first = mark(run)?.context("the replay paints its mark")?;
        render(run)?;
        ensure!(
            mark(run)? == Some(first),
            "reduce motion: the mark paints at rest in its first frame"
        );
        press(run, "escape")?;
        render(run)?;
        ensure!(
            mark(run)?.is_none(),
            "reduce motion: gone the frame after Esc"
        );

        // Off: `menu-in` travels toward the anchor, `menu-out` fades.
        set_motion(run, Motion::Full)?;
        replay(run, "Tour: Today", TourId::Today)?;
        let entering = mark(run)?.context("the mark paints")?;
        std::thread::sleep(Duration::from_millis(u64::from(motion::QUICK) + 60));
        render(run)?;
        render(run)?;
        let settled = mark(run)?.context("the mark stays")?;
        ensure!(
            entering != settled,
            "motion on: the mark travels in ({entering:?} → {settled:?})"
        );
        paint(run, "today-motion")?;
        press(run, "escape")?;
        render(run)?;
        ensure!(mark(run)?.is_some(), "motion on: the mark fades out");
        idle(
            run,
            Duration::from_millis(u64::from(motion::QUICK_EXIT) + 50),
        )?;
        ensure!(mark(run)?.is_none(), "the fade ends");
        set_motion(run, Motion::Reduced)?;
        Ok(())
    }

    // ── 6: a PR after the cooldown, then ⌘P › Tour: Inbox ────────────────

    fn review_and_inbox(run: &mut Run) -> anyhow::Result<()> {
        idle(run, Duration::from_secs(181))?;
        let shell = run.shell.clone();
        run.window.update(&mut run.cx, |_, window, cx| {
            shell.update(cx, |shell, cx| {
                shell.open_target(Target::PullRequest(PR), window, cx)
            })
        })?;
        settle_frames(run, |run| {
            Ok(review(run)?.is_some_and(|review| {
                run.cx
                    .read_entity(&review, |r, _| r.loaded().files.len() >= 3)
            }))
        })
        .with_context(|| format!("!{PR}'s review opens with its files"))?;
        ensure!(
            showing(run).is_none(),
            "the review's tour waits for 1 s of quiet"
        );
        idle(run, Duration::from_millis(900))?;
        ensure!(showing(run).is_none(), "0.9 s in: no tour yet");
        // The trigger polls every 250 ms and counts from the first frame
        // its anchors painted in: due within 1.5 s.
        let mut waited = Duration::from_millis(900);
        while showing(run).is_none() && waited < Duration::from_millis(1600) {
            idle(run, STEP)?;
            waited += STEP;
        }
        ensure!(
            matches!(showing(run), Some((TourId::Review, 0, _))),
            "first PR: its tour after 1 s of quiet ({:?} at {waited:?})",
            showing(run)
        );
        press(run, "right")?;
        let title = run.cx.read_entity(&run.tours, |t, _| t.step_title());
        ensure!(title == Some("Files"), "→: the Files step ({title:?})");
        paint(run, "review-files")?;
        let review = review(run)?.context("the review")?;
        let selected = |run: &mut Run| run.cx.read_entity(&review, |r, _| r.selected_file());
        let file = selected(run);
        press(run, "j")?;
        ensure!(showing(run).is_none(), "j ends the review tour");
        let next = selected(run);
        ensure!(
            next != file,
            "j still selects the next file ({file} → {next})"
        );
        // The same file as `j` with no tour (tree order, not index order).
        press(run, "k")?;
        ensure!(selected(run) == file, "k: back to file {file}");
        press(run, "j")?;
        ensure!(
            selected(run) == next,
            "j with no tour lands where j ending the tour did ({next})"
        );

        replay(run, "Tour: Inbox", TourId::Inbox)?;
        ensure!(nav(run) == Nav::Inbox, "Tour: Inbox switches to the Inbox");
        paint(run, "inbox-replay")?;
        press(run, "escape")?;
        Ok(())
    }

    // ── reading ──────────────────────────────────────────────────────────

    fn showing(run: &mut Run) -> Option<(TourId, usize, usize)> {
        run.cx.read_entity(&run.tours, |t, _| t.showing())
    }

    fn step(run: &mut Run) -> Option<usize> {
        showing(run).map(|(_, at, _)| at)
    }

    fn button(run: &mut Run) -> Option<CoachButton> {
        run.cx.read_entity(&run.tours, |t, _| t.focused_button())
    }

    /// The mark in the last painted frame.
    fn mark(run: &mut Run) -> anyhow::Result<Option<Bounds<Pixels>>> {
        Ok(run.cx.read_entity(&run.tours, |t, _| t.mark_bounds()))
    }

    fn nav(run: &mut Run) -> Nav {
        run.cx.read_entity(&run.shell, |s, _| s.nav())
    }

    fn row(run: &mut Run) -> Option<Row> {
        let shell = run.shell.clone();
        run.cx.read_entity(&shell, |s, cx| {
            s.today_view().and_then(|today| today.read(cx).current())
        })
    }

    fn review(run: &mut Run) -> anyhow::Result<Option<Entity<PrReview>>> {
        let shell = run.shell.clone();
        Ok(run.cx.read_entity(&shell, |s, cx| {
            s.review_view()
                .and_then(|view| view.read(cx).open_review())
                .filter(|review| review.read(cx).loaded().pr().id == PR)
        }))
    }

    fn palette_visible(run: &mut Run) -> bool {
        let shell = run.shell.clone();
        run.cx
            .read_entity(&shell, |s, cx| s.palette().read(cx).is_visible())
    }

    fn query(run: &mut Run) -> String {
        let shell = run.shell.clone();
        run.cx.read_entity(&shell, |s, cx| {
            s.palette().read(cx).query_text(cx).to_owned()
        })
    }

    /// `[tours]` as written to disk.
    fn seen(run: &mut Run) -> anyhow::Result<TourSettings> {
        Ok(TourSettings::from_toml(&read(&run.settings)?))
    }

    fn read(path: &Path) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    /// Settings › General › Motion, as the Settings window sets it: the
    /// file (so later reloads agree) and the live value.
    fn set_motion(run: &mut Run, to: Motion) -> anyhow::Result<()> {
        let text = read(&run.settings)?;
        let line = |m: &str| format!("motion = \"{m}\"");
        let (from, into) = match to {
            Motion::Full => (line("reduced"), line("full")),
            _ => (line("full"), line("reduced")),
        };
        ensure!(text.contains(&from), "settings.toml has {from}");
        std::fs::write(&run.settings, text.replace(&from, &into))?;
        run.cx
            .update(|cx| adoken_app::settings::apply_motion(to, cx));
        let reduce = !matches!(to, Motion::Full);
        ensure!(
            run.cx.update(|cx| cx.reduce_motion()) == reduce,
            "reduce motion follows the setting"
        );
        Ok(())
    }

    // ── driving ──────────────────────────────────────────────────────────

    /// `⌘P`, `title` set as the query, `↩` once it's the selected result;
    /// then `tour` shows.
    fn replay(run: &mut Run, title: &str, tour: TourId) -> anyhow::Result<()> {
        press(run, "cmd-p")?;
        let shell = run.shell.clone();
        run.cx.update(|cx| {
            shell.update(cx, |s, cx| {
                s.palette().update(cx, |p, cx| p.set_query(title, cx))
            })
        });
        settle_frames(run, |run| {
            let shell = run.shell.clone();
            Ok(run.cx.read_entity(&shell, |s, cx| {
                s.palette().read(cx).selected_title().as_ref() == title
            }))
        })
        .with_context(|| format!("⌘P finds {title:?}"))?;
        press(run, "enter")?;
        settle_frames(run, |run| Ok(showing(run).is_some_and(|(t, ..)| t == tour)))
            .with_context(|| format!("{title} starts the {} tour", tour.id()))
    }

    /// Waits for `done` drawing frames, without moving the clock (the
    /// store and searches run on real threads).
    fn settle_frames(
        run: &mut Run,
        mut done: impl FnMut(&mut Run) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        for _ in 0..500 {
            run.cx.run_until_parked();
            render(run)?;
            run.cx.run_until_parked();
            if done(run)? {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("timed out")
    }

    /// Setup only: moves the clock while waiting.
    fn settle(
        run: &mut Run,
        mut done: impl FnMut(&mut Run) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        for _ in 0..500 {
            run.cx.run_until_parked();
            if done(run)? {
                return Ok(());
            }
            run.cx.advance_clock(Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("timed out")
    }

    /// No input for `duration` on the test clock, a frame per step.
    fn idle(run: &mut Run, duration: Duration) -> anyhow::Result<()> {
        let mut left = duration;
        while !left.is_zero() {
            let step = left.min(STEP);
            run.cx.advance_clock(step);
            run.cx.run_until_parked();
            render(run)?;
            run.cx.run_until_parked();
            left -= step;
        }
        Ok(())
    }

    fn render(run: &mut Run) -> anyhow::Result<()> {
        let window: AnyWindowHandle = run.window.into();
        run.cx
            .update_window(window, |_, window, cx| window.draw(cx).clear())?;
        Ok(())
    }

    /// Paints (so the focus path is in the dispatch tree), dispatches
    /// `keystroke` like the user typing it, then lets the work land.
    fn dispatch(run: &mut Run, keystroke: Keystroke) -> anyhow::Result<()> {
        let window: AnyWindowHandle = run.window.into();
        run.cx.update_window(window, |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        run.cx.run_until_parked();
        render(run)?;
        run.cx.run_until_parked();
        Ok(())
    }

    fn press(run: &mut Run, keys: &str) -> anyhow::Result<()> {
        dispatch(run, Keystroke::parse(keys).context("keystroke")?)
    }

    fn type_text(run: &mut Run, text: &str) -> anyhow::Result<()> {
        for ch in text.chars() {
            let key = match ch {
                ' ' => "space".to_owned(),
                c => c.to_string(),
            };
            let mut keystroke = Keystroke::parse(&key).context("keystroke")?;
            keystroke.key_char = Some(ch.to_string());
            dispatch(run, keystroke)?;
        }
        Ok(())
    }

    fn paint(run: &mut Run, name: &str) -> anyhow::Result<()> {
        render(run)?;
        let window: AnyWindowHandle = run.window.into();
        let image = run
            .cx
            .update_window(window, |_, window, _| window.render_to_image())?
            .context("rendered image")?;
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tours-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
