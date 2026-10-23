//! Today (spec 0017) through real windows, real key dispatch and gpui's
//! frame trace. Screenshots go to `target/today-smoke/`.
//!
//! **Shell run (AC-33, AC-25, keys of AC-24).** The real shell on the demo
//! org anchored at the wall clock (Today runs on the system clock, as with
//! `ADOKEN_DEMO=1`). `settings.toml` makes every hour of every day a working
//! hour, so Up next shows whenever this runs. `⌘2` then `⌘1`, `j`/`k`/`h`/`l`,
//! `s` and `d` on waiting rows, `e` on Up next, `↩` on a PR row then `⌘[`
//! back to Today with the same row and scroll offset, a resize from 1280 to
//! 900 px wide across the breakpoint, and a store commit (one of my PRs
//! merges, or today's merges reopen) that changes the mood; frame p99 < 16 ms.
//!
//! **Mood art (AC-28, AC-30).** A Today view switched through the eight
//! mood days, with Reduce Motion off then on, compared pixel by pixel: the
//! first frame of a switch already shows everything but the art box final
//! and the old art still in that box; the new art then swaps in one frame
//! under Reduce Motion, or crossfades with every frame under 16 ms. The
//! asset source counts loads of the illustrations: none happens inside a
//! frame (the UI-thread decode counter; gpui decodes on its background
//! executor), one per switch; at most two illustrations stay decoded.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("today_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::borrow::Cow;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::today::model::{self, Layout, Loaded, Row, RowKey};
    use adoken_app::today::{TodayView, art_path};
    use adoken_demo::now_ms;
    use adoken_model::PrStatus;
    use adoken_nudge::{DayMood, FixedClock, Mood, Quiet, Settings};
    use adoken_theme::{ThemeRegistry, motion};
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AnyWindowHandle, App, AppContext as _, AssetSource, Entity, HeadlessAppContext,
        ImgResourceLoader, Keystroke, Resource, SharedString, Window, WindowHandle, px, size,
    };
    use image::RgbaImage;

    const FRAME: Duration = Duration::from_millis(16);

    /// Set around every `Window::draw`: an asset load while it is set
    /// happened inside a frame on the UI thread.
    static IN_FRAME: AtomicBool = AtomicBool::new(false);
    static ART_LOADS: AtomicUsize = AtomicUsize::new(0);
    static ART_LOADS_IN_FRAME: AtomicUsize = AtomicUsize::new(0);

    /// The app's embedded assets, counting loads of the Today illustrations.
    struct CountingAssets;

    impl AssetSource for CountingAssets {
        fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
            if path.starts_with("images/today/") {
                ART_LOADS.fetch_add(1, Ordering::SeqCst);
                if IN_FRAME.load(Ordering::SeqCst) {
                    ART_LOADS_IN_FRAME.fetch_add(1, Ordering::SeqCst);
                }
            }
            adoken_ui::Assets.load(path)
        }

        fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
            adoken_ui::Assets.list(path)
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(CountingAssets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            adoken_gallery::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
        });
        shell_run(&mut cx)?;
        art_run(&mut cx)?;
        ensure!(
            ART_LOADS_IN_FRAME.load(Ordering::SeqCst) == 0,
            "AC-30: no illustration loads or decodes inside a frame"
        );
        println!("today_smoke: shell run and mood art passed");
        Ok(())
    }

    // ── shell run ────────────────────────────────────────────────────────

    fn shell_run(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let settings = dir.path().join("settings.toml");
        std::fs::write(
            &settings,
            "[nudges]\ndays = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\n\
             start = \"00:00\"\nend = \"23:59\"\n",
        )?;
        cx.update(|cx| {
            adoken_app::settings::init(settings, false, cx);
            cx.set_reduce_motion(false);
        });
        let store = adoken_demo::open_store_at(dir.path(), now_ms())?;
        let window = cx.open_window(size(px(1280.0), px(800.0)), |window, cx| {
            cx.new(|cx| {
                Shell::new(
                    StoreSource::Open {
                        store: store.clone(),
                        demo: false,
                    },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                )
            })
        })?;
        settle(cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell
                    .today_view()
                    .is_some_and(|today| today.read(cx).loaded().is_some_and(|l| l.up_next_visible))
            })
        })
        .context("the store opens and Today computes its day with Up next")?;
        let today = window
            .update(cx, |shell, _, _| shell.today_view().cloned())?
            .context("Today view")?;
        let mut frames = adoken_perf::FrameRecorder::start();
        paint(cx, window.into(), "shell-today")?;
        let (layout, mood) = read(cx, &today, |view, loaded| {
            (view.current_layout(), loaded.day.summary.mood)
        })?;
        println!("today_smoke: demo day {layout:?}, mood {mood:?}");
        ensure!(
            layout.wide && layout.prs >= 2 && layout.up_next && layout.waiting >= 2,
            "the demo day fills both columns: {layout:?}"
        );

        // ⌘1 (from another section) shows Today with the keyboard on it.
        press(cx, window.into(), "cmd-2")?;
        ensure!(nav(cx, window)? == Nav::Inbox, "⌘2 shows the Inbox");
        press(cx, window.into(), "cmd-1")?;
        ensure!(nav(cx, window)? == Nav::Today, "⌘1 shows Today");
        ensure!(
            window.update(cx, |shell, window, cx| shell.content_has_focus(window, cx))?,
            "Today has the keyboard"
        );

        // j / k within Your PRs; l to the right column, h back (AC-24).
        ensure!(row(cx, &today)? == Some(Row::Pr(0)), "the first PR row");
        press(cx, window.into(), "j")?;
        ensure!(row(cx, &today)? == Some(Row::Pr(1)), "j: the next PR row");
        press(cx, window.into(), "k")?;
        ensure!(row(cx, &today)? == Some(Row::Pr(0)), "k: back up");
        press(cx, window.into(), "j")?;
        press(cx, window.into(), "l")?;
        let right = row(cx, &today)?.context("a row after l")?;
        ensure!(
            column(cx, &today, right)? == 1,
            "l: the right column ({right:?})"
        );
        press(cx, window.into(), "h")?;
        let left = row(cx, &today)?.context("a row after h")?;
        ensure!(matches!(left, Row::Pr(_)), "h: back to Your PRs ({left:?})");

        // s on a waiting row snoozes its N1; d on another marks it done.
        press(cx, window.into(), "l")?;
        for _ in 0..layout.waiting + layout.loose + 1 {
            press(cx, window.into(), "k")?;
        }
        ensure!(
            row(cx, &today)? == Some(Row::UpNext),
            "the right column starts with Up next"
        );
        press(cx, window.into(), "j")?;
        let first = waiting_key(cx, &today)?.context("j: a waiting row")?;
        press(cx, window.into(), "s")?;
        ensure!(
            quiet(cx, &today, first)? == Some(Quiet::Snoozed),
            "s snoozes review !{first}"
        );
        let second = read(cx, &today, |_, loaded| {
            loaded
                .day
                .waiting
                .iter()
                .find(|card| card.quiet.is_none())
                .map(|card| card.pr_id)
        })?
        .context("a second waiting row")?;
        select(cx, window, &today, RowKey::Review(second))?;
        press(cx, window.into(), "d")?;
        ensure!(
            quiet(cx, &today, second)? == Some(Quiet::Done),
            "d marks review !{second} done"
        );
        paint(cx, window.into(), "shell-triaged")?;

        // e hides Up next for the day.
        select(cx, window, &today, RowKey::UpNext)?;
        press(cx, window.into(), "e")?;
        let (up_next, visible) = read(cx, &today, |view, loaded| {
            (view.current_layout().up_next, loaded.up_next_visible)
        })?;
        ensure!(!up_next && !visible, "e hides Up next");

        // AC-25: ↩ on a PR row low enough to scroll, then ⌘[.
        press(cx, window.into(), "h")?;
        let prs = read(cx, &today, |view, _| view.current_layout().prs)?;
        for _ in 0..prs {
            press(cx, window.into(), "j")?;
        }
        render(cx, window.into())?;
        let (before, offset) =
            today.read_with(cx, |view, _| (view.nav_state(), view.scroll_offset().y));
        let Some(RowKey::Pr(pr)) = before.selected else {
            bail!("a PR row is selected: {:?}", before.selected);
        };
        println!("today_smoke: opening !{pr} at scroll {offset:?}");
        ensure!(
            offset < px(0.0) && before.scroll.is_some(),
            "the last PR row scrolled Today"
        );
        press(cx, window.into(), "enter")?;
        settle(cx, |cx| {
            Ok(window.update(cx, |shell, _, cx| shell.open_review_id(cx))? == Some(pr))
        })
        .context("↩ opens the PR")?;
        ensure!(nav(cx, window)? == Nav::PullRequests, "the review shows");
        paint(cx, window.into(), "shell-review")?;
        press(cx, window.into(), "cmd-[")?;
        settle(cx, |cx| Ok(nav(cx, window)? == Nav::Today)).context("⌘[ returns")?;
        // The anchor applies in the prepaint after the rows paint, then
        // asks for the next frame.
        render(cx, window.into())?;
        cx.run_until_parked();
        render(cx, window.into())?;
        let (after, back) =
            today.read_with(cx, |view, _| (view.nav_state(), view.scroll_offset().y));
        ensure!(
            after == before,
            "⌘[: the same row and scroll anchor ({before:?} → {after:?})"
        );
        ensure!(
            (back - offset).abs() < px(1.0),
            "⌘[: the same scroll offset ({offset:?} → {back:?})"
        );
        ensure!(
            window.update(cx, |shell, window, cx| shell.content_has_focus(window, cx))?,
            "⌘[: Today has the keyboard again"
        );
        paint(cx, window.into(), "shell-back")?;

        // AC-23 in the shell: 1280 → 900 px crosses the breakpoint; the row
        // stays selected.
        let selected = before.selected;
        cx.update_window(window.into(), |_, window, cx| {
            window.resize(size(px(900.0), px(800.0)));
            window.bounds_changed(cx);
        })?;
        let mut draws = 0;
        while read(cx, &today, |view, _| view.current_layout().wide)? {
            ensure!(draws < 2, "one column within a frame of the resize");
            render(cx, window.into())?;
            cx.run_until_parked();
            draws += 1;
        }
        println!("today_smoke: one column {draws} frame(s) after the resize");
        let narrow = today.read_with(cx, |view, _| view.nav_state().selected);
        ensure!(
            narrow == selected,
            "the breakpoint keeps the row ({selected:?} → {narrow:?})"
        );
        paint(cx, window.into(), "shell-narrow")?;
        // Wheel back to the top, where the card shows the mood.
        cx.update_window(window.into(), |_, window, cx| {
            draw(window, cx);
            window.dispatch_event(
                gpui::PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position: gpui::point(px(600.0), px(400.0)),
                    delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(10_000.0))),
                    ..Default::default()
                }),
                cx,
            );
        })?;
        render(cx, window.into())?;
        ensure!(
            today.read_with(cx, |view, _| view.scroll_offset().y) == px(0.0),
            "the wheel scrolls Today to the top"
        );

        // A store commit that changes the mood: one of my PRs merges now
        // (or, on a day that already shipped, today's merges reopen).
        let (me, mine, merged) = read(cx, &today, |_, loaded| {
            let me = loaded.snapshot.me.clone();
            let mine = loaded
                .snapshot
                .prs
                .iter()
                .find(|pr| pr.author == me && pr.status == PrStatus::Active)
                .cloned();
            (me, mine, loaded.snapshot.merged_today.clone())
        })?;
        let now = now_ms();
        store.write(|tx| {
            if mood == DayMood::Mood(Mood::Shipped) {
                for mut pr in merged.into_iter().filter(|pr| pr.author == me) {
                    pr.status = PrStatus::Active;
                    pr.closed = None;
                    pr.updated = now;
                    adoken_store::write::upsert_pr(tx, &pr)?;
                }
            } else if let Some(mut pr) = mine.clone() {
                pr.status = PrStatus::Completed;
                pr.closed = Some(now);
                pr.updated = now;
                adoken_store::write::upsert_pr(tx, &pr)?;
            }
            Ok(())
        })?;
        settle(cx, |cx| {
            Ok(read(cx, &today, |_, loaded| loaded.day.summary.mood)? != mood)
        })
        .context("the commit changes the mood")?;
        let changed = read(cx, &today, |_, loaded| loaded.day.summary.mood)?;
        println!("today_smoke: store commit, mood {mood:?} → {changed:?}");
        // Paint through the crossfade at a real frame cadence.
        let fade = Instant::now();
        while fade.elapsed() < Duration::from_millis(u64::from(motion::BASE) + 50) {
            render(cx, window.into())?;
            std::thread::sleep(Duration::from_millis(8));
        }
        cx.advance_clock(Duration::from_millis(u64::from(motion::BASE)));
        cx.run_until_parked();
        paint(cx, window.into(), "shell-mood-change")?;

        frames.collect();
        let summary = frames.draw().summary().context("frames recorded")?;
        println!(
            "today_smoke shell frames: n={}, p50={:?}, p99={:?}, max={:?}",
            summary.count, summary.p50, summary.p99, summary.max
        );
        adoken_perf::ensure_budget!(summary.p99 < FRAME, "frame p99 {:?} ≥ 16 ms", summary.p99);
        drop(frames);
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        Ok(())
    }

    fn nav(cx: &mut HeadlessAppContext, window: WindowHandle<Shell>) -> anyhow::Result<Nav> {
        window.update(cx, |shell, _, _| shell.nav())
    }

    fn read<R>(
        cx: &mut HeadlessAppContext,
        today: &Entity<TodayView>,
        f: impl FnOnce(&TodayView, &Loaded) -> R,
    ) -> anyhow::Result<R> {
        today
            .read_with(cx, |view, _| view.loaded().map(|loaded| f(view, loaded)))
            .context("Today has a day")
    }

    fn row(cx: &mut HeadlessAppContext, today: &Entity<TodayView>) -> anyhow::Result<Option<Row>> {
        Ok(today.read_with(cx, |view, _| view.current()))
    }

    fn column(
        cx: &mut HeadlessAppContext,
        today: &Entity<TodayView>,
        row: Row,
    ) -> anyhow::Result<usize> {
        read(cx, today, |view, _| view.current_layout().locate(row))?
            .map(|(column, _)| column)
            .context("the row is on screen")
    }

    /// The selected waiting row's PR.
    fn waiting_key(
        cx: &mut HeadlessAppContext,
        today: &Entity<TodayView>,
    ) -> anyhow::Result<Option<i64>> {
        read(cx, today, |view, loaded| {
            match view.current().and_then(|row| model::key(loaded, row)) {
                Some(RowKey::Review(id)) => Some(id),
                _ => None,
            }
        })
    }

    fn quiet(
        cx: &mut HeadlessAppContext,
        today: &Entity<TodayView>,
        pr: i64,
    ) -> anyhow::Result<Option<Quiet>> {
        read(cx, today, |_, loaded| {
            loaded
                .day
                .waiting
                .iter()
                .find(|card| card.pr_id == pr)
                .and_then(|card| card.quiet)
        })
    }

    /// Moves the selection with `j` / `k` until `key` is selected.
    fn select(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        today: &Entity<TodayView>,
        key: RowKey,
    ) -> anyhow::Result<()> {
        let at = |cx: &mut HeadlessAppContext| {
            read(cx, today, |view, loaded| {
                let layout: Layout = view.current_layout();
                let target = model::find(loaded, &layout, key).and_then(|r| layout.locate(r));
                let current = view.current().and_then(|r| layout.locate(r));
                (target, current)
            })
        };
        for _ in 0..64 {
            let (Some(target), Some(current)) = at(cx)? else {
                bail!("{key:?} is on screen");
            };
            if target == current {
                return Ok(());
            }
            let keys = if target.0 != current.0 {
                if target.0 > current.0 { "l" } else { "h" }
            } else if target.1 > current.1 {
                "j"
            } else {
                "k"
            };
            press(cx, window.into(), keys)?;
        }
        bail!("couldn't reach {key:?} with the keyboard")
    }

    // ── mood art ─────────────────────────────────────────────────────────

    /// The art box at 2× (design §3.1: 264 × 176).
    const ART_BOX: (u32, u32) = (528, 352);

    fn art_run(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let days = adoken_gallery::today::mood_days();
        let moods: Vec<Mood> = days.iter().map(|d| d.day.summary.mood.art()).collect();
        ensure!(
            moods.iter().collect::<std::collections::HashSet<_>>().len() == 8,
            "the eight mood days show eight moods: {moods:?}"
        );
        cx.update(|cx| cx.set_reduce_motion(false));
        let first = days[0].clone();
        let window = cx.open_window(size(px(1180.0), px(900.0)), |_, cx| {
            cx.new(|cx| {
                let clock = Arc::new(FixedClock::new(first.now));
                TodayView::with_loaded(first, clock, Settings::default(), cx)
            })
        })?;
        let handle: AnyWindowHandle = window.into();
        // gpui decodes the first art on its background executor, then the
        // next frame paints it.
        render(cx, handle)?;
        cx.run_until_parked();
        render(cx, handle)?;
        let mut settled = image(cx, handle)?;
        let mut frames = adoken_perf::FrameRecorder::start();
        let mut fade_frames = adoken_perf::Samples::new("crossfade frame draw");

        let mut order: Vec<(usize, bool)> = (1..8).map(|i| (i, false)).collect();
        order.extend((0..8).map(|i| (i, true)));
        for (ix, reduce) in order {
            cx.update(|cx| cx.set_reduce_motion(reduce));
            let loads = ART_LOADS.load(Ordering::SeqCst);
            let day = days[ix].clone();
            window.update(cx, |view, _, cx| view.set_loaded(day, cx))?;
            // The switch's first frame, before the background executor ran.
            render(cx, handle)?;
            let first = image(cx, handle)?;
            cx.run_until_parked();
            ensure!(
                ART_LOADS.load(Ordering::SeqCst) == loads + 1,
                "{:?}: one load of its art per switch",
                moods[ix]
            );
            // The decoded art replaces the old one.
            let mut shown = Vec::new();
            if reduce {
                render(cx, handle)?;
                shown.push(image(cx, handle)?);
            } else {
                frames.collect();
                let start = Instant::now();
                while start.elapsed() < Duration::from_millis(u64::from(motion::BASE) + 60) {
                    render(cx, handle)?;
                    shown.push(image(cx, handle)?);
                    std::thread::sleep(Duration::from_millis(12));
                }
                frames.collect();
                for d in frames.draw().as_slice().iter().rev().take(shown.len()) {
                    fade_frames.record(*d);
                }
                let mid = decoded(cx)?;
                ensure!(
                    mid.len() == 2,
                    "{:?}: two illustrations decoded mid-fade, got {mid:?}",
                    moods[ix]
                );
                cx.advance_clock(Duration::from_millis(u64::from(motion::BASE)));
                cx.run_until_parked();
            }
            render(cx, handle)?;
            let now = image(cx, handle)?;
            let kept = decoded(cx)?;
            ensure!(
                kept == [moods[ix]],
                "{:?}: only the shown illustration stays decoded, got {kept:?}",
                moods[ix]
            );
            // Nothing keeps animating once the art is in.
            std::thread::sleep(Duration::from_millis(40));
            render(cx, handle)?;
            ensure!(
                same(&now, &image(cx, handle)?, None),
                "{:?}: Today is still once the art is in",
                moods[ix]
            );

            // AC-30: everything but one art-sized box is final in the first
            // frame, and that box still shows the old art (no blank).
            let art = diff_box(&first, &now).context("the art changed")?;
            ensure!(
                art.2 - art.0 <= ART_BOX.0 && art.3 - art.1 <= ART_BOX.1,
                "{:?}: the first frame differs from the final one only in the art box, got {art:?}",
                moods[ix]
            );
            ensure!(
                same(&first, &settled, Some(art)),
                "{:?}: the old art stays until the new one is decoded",
                moods[ix]
            );
            ensure!(
                !same(&first, &settled, None),
                "{:?}: the card's text changed in the first frame",
                moods[ix]
            );
            // AC-28: one-frame swap, or a crossfade through blended frames.
            let blended = shown
                .iter()
                .filter(|f| !same(f, &settled, Some(art)) && !same(f, &now, Some(art)))
                .count();
            if reduce {
                ensure!(
                    blended == 0 && same(&shown[0], &now, None),
                    "{:?}: Reduce Motion swaps the art in one frame",
                    moods[ix]
                );
            } else {
                ensure!(
                    blended >= 2,
                    "{:?}: the art crossfades ({blended} blended frames of {})",
                    moods[ix],
                    shown.len()
                );
            }
            save(
                &now,
                &format!(
                    "mood-{}{}",
                    moods[ix].id(),
                    if reduce { "-reduced" } else { "" }
                ),
            )?;
            settled = now;
        }
        frames.collect();
        let fade = fade_frames.summary().context("crossfade frames")?;
        let all = frames.draw().summary().context("frames")?;
        println!(
            "today_smoke art: {} switches, crossfade frames n={} p50={:?} max={:?}; all frames p99={:?}; illustration loads {} (in a frame: {})",
            15,
            fade.count,
            fade.p50,
            fade.max,
            all.p99,
            ART_LOADS.load(Ordering::SeqCst),
            ART_LOADS_IN_FRAME.load(Ordering::SeqCst)
        );
        adoken_perf::ensure_budget!(fade.max < FRAME, "a crossfade frame took {:?}", fade.max);
        drop(frames);
        cx.update_window(handle, |_, window, _| window.remove_window())?;
        Ok(())
    }

    /// The illustrations decoded and cached now.
    fn decoded(cx: &mut HeadlessAppContext) -> anyhow::Result<Vec<Mood>> {
        Ok(cx.update(|cx: &mut App| {
            Mood::ALL
                .into_iter()
                .filter(|mood| {
                    cx.peek_asset::<ImgResourceLoader>(&Resource::Embedded(art_path(*mood).into()))
                        .is_some_and(|r| r.is_ok())
                })
                .collect()
        }))
    }

    /// The bounding box `(x0, y0, x1, y1)` of the pixels that differ.
    fn diff_box(a: &RgbaImage, b: &RgbaImage) -> Option<(u32, u32, u32, u32)> {
        let mut found: Option<(u32, u32, u32, u32)> = None;
        for (x, y, pa) in a.enumerate_pixels() {
            if pa != b.get_pixel(x, y) {
                found = Some(match found {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
        found
    }

    /// `a` and `b` are pixel-identical inside `area` (all of them if `None`).
    fn same(a: &RgbaImage, b: &RgbaImage, area: Option<(u32, u32, u32, u32)>) -> bool {
        let (x0, y0, x1, y1) = area.unwrap_or((0, 0, a.width(), a.height()));
        a.dimensions() == b.dimensions()
            && (y0..y1).all(|y| (x0..x1).all(|x| a.get_pixel(x, y) == b.get_pixel(x, y)))
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

    /// One frame, flagged so asset loads inside it are counted.
    fn draw(window: &mut Window, cx: &mut App) {
        IN_FRAME.store(true, Ordering::SeqCst);
        window.draw(cx).clear();
        IN_FRAME.store(false, Ordering::SeqCst);
    }

    /// Paints (so the focus path is in the dispatch tree), types `keys`
    /// like the user would, then lets background work land.
    fn press(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        keys: &str,
    ) -> anyhow::Result<()> {
        let keystroke = Keystroke::parse(keys).context("keystroke")?;
        cx.update_window(window, |_, window, cx| {
            draw(window, cx);
            window.dispatch_keystroke(keystroke, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    fn render(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<()> {
        cx.update_window(window, |_, window, cx| draw(window, cx))?;
        Ok(())
    }

    /// The last drawn frame's pixels.
    fn image(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> anyhow::Result<RgbaImage> {
        cx.update_window(window, |_, window, _| window.render_to_image())?
            .context("rendered image")
    }

    fn paint(
        cx: &mut HeadlessAppContext,
        window: AnyWindowHandle,
        name: &str,
    ) -> anyhow::Result<()> {
        render(cx, window)?;
        save(&image(cx, window)?, name)
    }

    fn save(image: &RgbaImage, name: &str) -> anyhow::Result<()> {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/today-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
