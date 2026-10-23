//! Celebrations (spec 0022) through the real shell on the demo org, with
//! real key dispatch and the demo write paths (`apply_local`):
//!
//! - Idle: no frame requested, no timer, nothing allocated (AC-16).
//! - Work items on the owner's process (custom "Code Review", "Done"): the
//!   effect starts on the frame that clears the pending clock and not
//!   before (AC-6); keys work during a burst (AC-15); frames stay under
//!   16 ms (frame recorder); after it ends, no frame or timer (AC-16); a
//!   backward move celebrates nothing (AC-2).
//! - Reduce Motion: the glyph (and a Milestone's toast) for exactly 3 s, no
//!   particle, no animation frame (AC-10).
//! - Pull requests: my Complete answered `completed` bursts from the
//!   status chip on the frame that applies it; auto-complete celebrates
//!   nothing (Q-4); a queued Complete celebrates when a re-read stores
//!   `completed`, from its toast when the review isn't open; rejected by
//!   policy, first seen after 10 min, or never completed here: nothing
//!   (AC-7, FR-1).
//! - Ken arcade: someone else's PR gets the seal; my PR gets the K.O. on
//!   the confirming frame and no celebration (AC-9).
//!
//! Screenshots in `target/celebrate-smoke/`. macOS only, like the others.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("celebrate_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adoken_app::celebrate::{Anchor, Hooks};
    use adoken_app::ken::KenLayer;
    use adoken_app::palette::catalog::Target;
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::workitems::WorkItemsView;
    use adoken_app::workitems::writes::{Field, WiEdits};
    use adoken_ken::settings::Intensity;
    use adoken_model::{
        MergeStatus, Millis, PrStatus, PullRequest, StateCategory, TypeMeta, WiState, WorkItemType,
    };
    use adoken_store::Store;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{AppContext as _, Entity, HeadlessAppContext, Keystroke, WindowHandle, px, size};

    const MINUTE: Millis = 60_000;
    const HOUR: Millis = 60 * MINUTE;
    /// Every effect ends within 1.6 s (FR-12); real frames, so some slack.
    const EFFECT_WALL: Duration = Duration::from_millis(2_500);

    /// Fields drop in order: entity handles before the app (`cx` last).
    struct Run {
        window: WindowHandle<Shell>,
        store: Store,
        layer: Entity<KenLayer>,
        edits: Entity<WiEdits>,
        view: Entity<WorkItemsView>,
        me: String,
        /// The celebration clock, moved forward by each section.
        clock: Millis,
        /// Draw times of every frame an effect played (frame recorder).
        effect_frames: Vec<Duration>,
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
            cx.set_reduce_motion(false);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
            adoken_gallery::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir.path(), now)?;
        let me = store.me()?.context("demo identity")?.id;
        let window = cx.open_window(size(px(1440.0), px(900.0)), |window, cx| {
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
            window.update(cx, |shell, _, _| shell.work_items().is_some())
        })?;
        let view = window
            .update(&mut cx, |shell, _, _| shell.work_items().cloned())?
            .context("work items view")?;
        let edits = cx
            .update(|cx| WiEdits::global(cx))
            .context("work item writes")?;
        let layer = cx
            .update(|cx| adoken_app::ken::layer_entity(cx))
            .context("the Ken layer is mounted with Ken off")?;
        cx.update(|cx| edits.update(cx, |edits, cx| edits.set_clock(now, cx)));
        window.update(&mut cx, |_, window, _| window.activate_window())?;
        cx.run_until_parked();
        let active = window.update(&mut cx, |_, window, _| window.is_window_active())?;
        ensure!(active, "the window is key");

        let mut run = Run {
            cx,
            window,
            store,
            layer,
            edits,
            view,
            me,
            clock: now,
            effect_frames: Vec::new(),
        };
        idle(&mut run)?;
        work_items(&mut run)?;
        reduced(&mut run)?;
        pull_requests(&mut run)?;
        arcade(&mut run)?;
        run.cx
            .update_window(run.window.into(), |_, window, _| window.remove_window())?;
        let frames = adoken_perf::Summary::of("draw", &run.effect_frames)
            .context("effect frames recorded")?;
        let max = run.effect_frames.iter().max().copied().unwrap_or_default();
        println!(
            "celebrate_smoke effect frames: n={}, p50={:?}, p99={:?}, max={max:?}",
            frames.count, frames.p50, frames.p99
        );
        ensure!(
            frames.p99 < Duration::from_millis(16),
            "frames during effects under 16 ms: p99 {:?}, max {max:?}",
            frames.p99
        );
        println!(
            "celebrate_smoke: idle, work item step + milestone on their confirming frames, reduce motion glyph/toast for 3 s, PR complete / auto-complete / queued re-read / rejected / 11 min / sync-only, arcade seal and K.O. handoff passed"
        );
        Ok(())
    }

    /// AC-16: celebrations on, nothing happening: no frame, timer or buffer.
    fn idle(run: &mut Run) -> anyhow::Result<()> {
        for _ in 0..30 {
            draw(run)?;
        }
        let hooks = hooks(run);
        ensure!(
            hooks.frames >= 30,
            "the layer renders every frame: {hooks:?}"
        );
        ensure!(
            hooks.frames_requested == 0 && hooks.timers == 0 && hooks.effects_started == 0,
            "idle: {hooks:?}"
        );
        let (bytes, seal) = read(run, |layer| (layer.celebration_bytes(), layer.seal_built()));
        ensure!(
            bytes == 0 && !seal,
            "nothing allocated idle: {bytes} B, seal {seal}"
        );
        Ok(())
    }

    /// The owner's process: Active → Code Review (Step), → Done
    /// (Milestone), then Done → Active (nothing).
    fn work_items(run: &mut Run) -> anyhow::Result<()> {
        let id = active_item(run, 0)?;
        open(run, id)?;

        advance_celebration_clock(run, HOUR);
        let step = change_state(run, id, "code review", "Code Review")?;
        ensure!(
            read(run, KenLayer::live_particles) == 14,
            "a calm puff is 14 pieces"
        );
        ensure!(
            read(run, KenLayer::celebration_toasts).is_empty(),
            "a Step adds no toast"
        );
        std::thread::sleep(Duration::from_millis(200));
        paint(run, "step-200ms")?;
        let ended = play_out(run)?;
        println!(
            "celebrate_smoke: step started on frame {:?}, ended after {ended:?}",
            step.first_effect_frame
        );

        advance_celebration_clock(run, 2 * MINUTE);
        let milestone = change_state(run, id, "done", "Done")?;
        ensure!(
            read(run, KenLayer::live_particles) == 24,
            "a calm burst's first wave is 24 pieces"
        );
        // AC-15: keys work during the burst.
        press(run, "s")?;
        let picker = run.cx.update(|cx| run.view.read(cx).picker_open());
        ensure!(picker, "s opens the state menu during the burst");
        press(run, "escape")?;
        std::thread::sleep(Duration::from_millis(150));
        paint(run, "milestone-mid")?;
        let ended = play_out(run)?;
        println!(
            "celebrate_smoke: milestone started on frame {:?}, ended after {ended:?}",
            milestone.last_effect_frame
        );

        // AC-2: backward celebrates nothing.
        advance_celebration_clock(run, 2 * MINUTE);
        let before = hooks(run).effects_started;
        set_state(run, id, "active", "Active")?;
        for _ in 0..5 {
            draw(run)?;
        }
        ensure!(
            hooks(run).effects_started == before && read(run, KenLayer::live_particles) == 0,
            "Done → Active celebrates nothing"
        );
        Ok(())
    }

    /// AC-10: Reduce Motion: the glyph from the confirming frame until
    /// exactly 3 s later, the Milestone's toast, no particle and no frame.
    fn reduced(run: &mut Run) -> anyhow::Result<()> {
        run.cx.update(|cx| cx.set_reduce_motion(true));
        let id = active_item(run, 1)?;
        open(run, id)?;
        advance_celebration_clock(run, HOUR);
        let at = run.clock;
        let before = hooks(run);
        set_state(run, id, "done", "Done")?;
        draw(run)?;
        let after = hooks(run);
        ensure!(
            after.effects_started == before.effects_started
                && after.frames_requested == before.frames_requested,
            "no effect, no animation frame: {before:?} → {after:?}"
        );
        ensure!(read(run, KenLayer::live_particles) == 0, "no particle");
        ensure!(
            read(run, |l| l.glyph_showing(Anchor::WorkItem(id))),
            "the glyph shows from the confirming frame"
        );
        ensure!(after.timers == 1, "one glyph timer: {after:?}");
        let toasts = read(run, KenLayer::celebration_toasts);
        ensure!(
            toasts.iter().any(|t| t.as_ref() == "Moved to Done"),
            "the Milestone toast shows: {toasts:?}"
        );
        paint(run, "reduced-glyph-toast")?;

        set_celebration_clock(run, at + 2_999);
        ensure!(
            read(run, |l| l.glyph_showing(Anchor::WorkItem(id))),
            "still there at 2.999 s"
        );
        set_celebration_clock(run, at + 3_000);
        run.cx.advance_clock(Duration::from_millis(3_000));
        run.cx.run_until_parked();
        draw(run)?;
        ensure!(
            !read(run, |l| l.glyph_showing(Anchor::WorkItem(id))),
            "gone at 3 s"
        );
        let end = hooks(run);
        ensure!(
            end.timers == 0 && end.frames_requested == before.frames_requested,
            "no timer and no frame after it: {end:?}"
        );
        paint(run, "reduced-after-3s")?;
        run.cx.update(|cx| cx.set_reduce_motion(false));
        Ok(())
    }

    fn pull_requests(run: &mut Run) -> anyhow::Result<()> {
        let mut candidates = completable(run, false)?.into_iter();

        // My Complete answered `completed` (the demo org): the burst comes
        // from the review's status chip on the frame that applies it.
        let pr = candidates.next().context("a PR to complete")?;
        advance_celebration_clock(run, HOUR);
        open_review(run, pr)?;
        let before = hooks(run);
        press(run, "shift-m")?;
        press(run, "cmd-enter")?;
        ensure!(
            pr_status(run, pr)? == PrStatus::Completed,
            "!{pr} completed in the store"
        );
        let started = started_next_frame(run, before.effects_started)?;
        println!(
            "celebrate_smoke: !{pr} confirmed after frame {:?}, burst on frame {:?}",
            started.last_queued_frame, started.last_effect_frame
        );
        let toasts = read(run, KenLayer::celebration_toasts);
        ensure!(
            !toasts.iter().any(|t| t.starts_with("Merged into ")),
            "anchored on the status chip, no merge toast: {toasts:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
        paint(run, "pr-complete")?;
        play_out(run)?;

        // Auto-complete (Q-4): never celebrated.
        let auto = candidates.next().context("a PR for auto-complete")?;
        advance_celebration_clock(run, 2 * MINUTE);
        open_review(run, auto)?;
        let before = hooks(run);
        press(run, "shift-m")?;
        press(run, "cmd-shift-enter")?;
        ensure!(
            pr_status(run, auto)? == PrStatus::Active,
            "auto-complete leaves !{auto} active"
        );
        for _ in 0..5 {
            draw(run)?;
        }
        ensure!(
            hooks(run).effects_started == before.effects_started,
            "auto-complete celebrates nothing"
        );

        // A queued Complete: confirmed by the re-read storing `completed`,
        // with the review closed: the toast, and the burst from it.
        let queued = candidates.next().context("a PR for the queued path")?;
        show_work_items(run)?;
        advance_celebration_clock(run, HOUR);
        let before = hooks(run);
        run.cx
            .update(|cx| adoken_app::celebrate::complete_accepted(queued, cx));
        for _ in 0..5 {
            draw(run)?;
        }
        ensure!(
            hooks(run).effects_started == before.effects_started,
            "nothing while Completing…"
        );
        store_pr(run, queued, |pr| {
            pr.status = PrStatus::Completed;
            pr.merge_status = MergeStatus::Succeeded;
        })?;
        let started = until_effect(run, before.effects_started)?;
        let toasts = read(run, KenLayer::celebration_toasts);
        ensure!(
            toasts.iter().any(|t| t.starts_with("Merged into ")),
            "no review open: the Milestone toast: {toasts:?}"
        );
        println!(
            "celebrate_smoke: queued !{queued} burst from its toast on frame {:?}",
            started.last_effect_frame
        );
        std::thread::sleep(Duration::from_millis(150));
        paint(run, "pr-queued-toast")?;
        play_out(run)?;

        // Rejected by policy, first seen after 10 min, never completed here.
        let rejected = candidates.next().context("a PR to reject")?;
        let late = candidates.next().context("a late PR")?;
        let synced = candidates.next().context("a PR merged elsewhere")?;
        advance_celebration_clock(run, HOUR);
        let before = hooks(run);
        let toasts_before = read(run, KenLayer::celebration_toasts).len();
        run.cx.update(|cx| {
            adoken_app::celebrate::complete_accepted(rejected, cx);
            adoken_app::celebrate::complete_accepted(late, cx);
        });
        store_pr(run, rejected, |pr| {
            pr.merge_status = MergeStatus::RejectedByPolicy;
        })?;
        wait_commits(run)?;
        advance_celebration_clock(run, 11 * MINUTE);
        store_pr(run, late, |pr| {
            pr.status = PrStatus::Completed;
            pr.merge_status = MergeStatus::Succeeded;
        })?;
        store_pr(run, synced, |pr| {
            pr.status = PrStatus::Completed;
            pr.merge_status = MergeStatus::Succeeded;
        })?;
        wait_commits(run)?;
        for _ in 0..5 {
            draw(run)?;
        }
        ensure!(
            hooks(run).effects_started == before.effects_started
                && read(run, KenLayer::celebration_toasts).len() <= toasts_before,
            "rejected, 11 min late, and sync-observed merges celebrate nothing"
        );
        Ok(())
    }

    /// Ken arcade: someone else's PR → the seal; my PR → the K.O. on the
    /// confirming frame, no confetti, seal or toast.
    fn arcade(run: &mut Run) -> anyhow::Result<()> {
        run.layer.update(&mut run.cx, |layer, cx| {
            layer.update_settings(
                |ken| {
                    ken.enabled = true;
                    ken.intensity = Intensity::Arcade;
                    ken.particles = true;
                    ken.ko = true;
                },
                cx,
            )
        });
        run.cx.run_until_parked();

        let theirs = completable(run, false)?
            .into_iter()
            .next()
            .context("someone else's PR")?;
        advance_celebration_clock(run, HOUR);
        open_review(run, theirs)?;
        ensure!(!read(run, KenLayer::seal_built), "no seal before");
        let before = hooks(run);
        press(run, "shift-m")?;
        press(run, "cmd-enter")?;
        started_next_frame(run, before.effects_started)?;
        ensure!(read(run, KenLayer::seal_built), "the seal stamps");
        ensure!(
            !run.cx.update(|cx| adoken_app::ken::ko_showing(cx)),
            "no K.O. for someone else's PR"
        );
        std::thread::sleep(Duration::from_millis(200));
        paint(run, "arcade-seal")?;
        play_out(run)?;

        let mine = completable(run, true)?
            .into_iter()
            .next()
            .context("a PR of mine")?;
        advance_celebration_clock(run, HOUR);
        open_review(run, mine)?;
        let before = hooks(run);
        let toasts = read(run, KenLayer::celebration_toasts).len();
        press(run, "shift-m")?;
        press(run, "cmd-enter")?;
        ensure!(
            pr_status(run, mine)? == PrStatus::Completed,
            "!{mine} completed"
        );
        draw(run)?;
        let playing = read(run, KenLayer::ko_playing);
        ensure!(playing, "the K.O. plays from the confirming frame");
        let after = hooks(run);
        ensure!(
            after.effects_started == before.effects_started,
            "no celebration with the K.O.: {after:?}"
        );
        ensure!(
            read(run, KenLayer::celebration_toasts).len() == toasts,
            "no celebration toast with the K.O."
        );
        std::thread::sleep(Duration::from_millis(300));
        paint(run, "ko")?;
        // Any key dismisses the K.O.; it exits, then nothing replays.
        press(run, "escape")?;
        let start = Instant::now();
        while run.cx.update(|cx| adoken_app::ken::ko_showing(cx)) {
            ensure!(start.elapsed() < Duration::from_secs(6), "the K.O. ends");
            std::thread::sleep(Duration::from_millis(16));
            draw(run)?;
        }
        for _ in 0..10 {
            run.cx.advance_clock(Duration::from_secs(1));
            run.cx.run_until_parked();
            draw(run)?;
        }
        ensure!(
            !run.cx.update(|cx| adoken_app::ken::ko_showing(cx))
                && hooks(run).effects_started == before.effects_started,
            "no second K.O. or celebration"
        );

        // AC-9 (b): Ken subtle: one K.O. toast, no celebration.
        run.layer.update(&mut run.cx, |layer, cx| {
            layer.update_settings(|ken| ken.intensity = Intensity::Subtle, cx)
        });
        let mine = completable(run, true)?
            .into_iter()
            .next()
            .context("another PR of mine")?;
        advance_celebration_clock(run, HOUR);
        open_review(run, mine)?;
        let before = hooks(run);
        let (ken_toasts, toasts) =
            read(run, |l| (l.ken_toast_count(), l.celebration_toasts().len()));
        press(run, "shift-m")?;
        press(run, "cmd-enter")?;
        draw(run)?;
        let after = read(run, |l| {
            (
                l.ken_toast_count(),
                l.celebration_toasts().len(),
                l.ko_showing(),
            )
        });
        ensure!(
            after == (ken_toasts + 1, toasts, false)
                && hooks(run).effects_started == before.effects_started,
            "subtle: one K.O. toast, no overlay, no celebration: {after:?}"
        );
        paint(run, "ko-subtle-toast")?;
        Ok(())
    }

    // ── helpers ─────────────────────────────────────────────────────────

    fn hooks(run: &mut Run) -> Hooks {
        read(run, KenLayer::celebration_hooks)
    }

    fn read<R>(run: &mut Run, f: impl FnOnce(&KenLayer) -> R) -> R {
        let layer = run.layer.clone();
        run.cx.update(|cx| f(layer.read(cx)))
    }

    fn set_celebration_clock(run: &mut Run, at: Millis) {
        run.clock = at;
        run.layer.update(&mut run.cx, |layer, _| {
            layer.set_celebration_clock(Some(at))
        });
    }

    /// Past the 1.5 s spacing and the 10 min window of the rate limits.
    fn advance_celebration_clock(run: &mut Run, by: Millis) {
        let at = run.clock + by;
        set_celebration_clock(run, at);
    }

    /// The `nth` Active, non-Task demo work item, given the owner's process
    /// (custom "Code Review" and "Committed" in progress, "Done" completed).
    fn active_item(run: &mut Run, nth: usize) -> anyhow::Result<i64> {
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        let item = ds
            .work_items
            .iter()
            .filter(|w| w.state == "Active" && w.kind != WorkItemType::Task)
            .nth(nth)
            .context("an Active work item")?;
        let state = |name: &str, category| WiState {
            name: name.into(),
            category,
            color: None,
        };
        let meta = TypeMeta {
            states: vec![
                state("New", StateCategory::Proposed),
                state("Active", StateCategory::InProgress),
                state("Code Review", StateCategory::InProgress),
                state("Committed", StateCategory::InProgress),
                state("Done", StateCategory::Completed),
                state("Removed", StateCategory::Removed),
            ],
            transitions: HashMap::new(),
            fetched_ms: adoken_demo::DEMO_NOW,
        };
        run.store.write(|tx| {
            adoken_store::wi::put_type_meta(tx, &item.project_id, item.kind.as_str(), &meta)
        })?;
        Ok(item.id)
    }

    fn open(run: &mut Run, id: i64) -> anyhow::Result<()> {
        let view = run.view.clone();
        run.window.update(&mut run.cx, |shell, window, cx| {
            shell.show(Nav::WorkItems, window, cx);
            shell.open_target(Target::WorkItem(id), window, cx);
        })?;
        settle(&mut run.cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).detail_item().is_some_and(|i| i.id == id)))
        })
        .context("detail loads")?;
        draw(run)
    }

    fn show_work_items(run: &mut Run) -> anyhow::Result<()> {
        run.window.update(&mut run.cx, |shell, window, cx| {
            shell.show(Nav::WorkItems, window, cx)
        })?;
        run.cx.run_until_parked();
        draw(run)
    }

    /// `s`, `query`, `↩`, then wait until the write is confirmed and the
    /// pending clock cleared.
    fn set_state(run: &mut Run, id: i64, query: &str, expect: &str) -> anyhow::Result<()> {
        press(run, "s")?;
        let view = run.view.clone();
        settle(&mut run.cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).picker_open()))
        })
        .context("s opens the state menu")?;
        type_text(run, query)?;
        press(run, "enter")?;
        let store = run.store.clone();
        settle(&mut run.cx, |_| Ok(item_state(&store, id)? == expect))
            .with_context(|| format!("#{id} moves to {expect}"))?;
        let edits = run.edits.clone();
        settle(&mut run.cx, |cx| {
            Ok(cx.update(|cx| edits.read(cx).status(id, Field::State).is_none()))
        })
        .context("the pending clock clears")
    }

    /// Like [`set_state`], frame by frame: before the answer no particle
    /// exists; the frame that clears the pending clock starts the effect.
    fn change_state(run: &mut Run, id: i64, query: &str, expect: &str) -> anyhow::Result<Hooks> {
        press(run, "s")?;
        let view = run.view.clone();
        settle(&mut run.cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).picker_open()))
        })
        .context("s opens the state menu")?;
        type_text(run, query)?;
        let before = hooks(run).effects_started;
        press(run, "enter")?;
        let edits = run.edits.clone();
        let pending = |run: &mut Run| {
            run.cx
                .update(|cx| edits.read(cx).status(id, Field::State).is_some())
        };
        ensure!(pending(run), "↩ shows the pending clock");
        // The layer's frame index in the update that clears the pending
        // clock (observers run before that update's frame is drawn).
        let cleared: Rc<Cell<Option<u64>>> = Rc::default();
        let _watch = {
            let (cleared, layer) = (cleared.clone(), run.layer.clone());
            let edits = run.edits.clone();
            run.cx.update(|cx| {
                cx.observe(&edits, move |edits, cx| {
                    if cleared.get().is_none() && edits.read(cx).status(id, Field::State).is_none()
                    {
                        cleared.set(Some(layer.read(cx).celebration_hooks().frames));
                    }
                })
            })
        };
        let mut waited = 0;
        while pending(run) {
            draw(run)?;
            ensure!(
                hooks(run).effects_started == before && read(run, KenLayer::live_particles) == 0,
                "no particle before the confirmation"
            );
            ensure!(waited < 300, "the demo write answers within 300 ms");
            run.cx.advance_clock(Duration::from_millis(10));
            run.cx.run_until_parked();
            waited += 10;
        }
        ensure!(
            item_state(&run.store, id)? == expect,
            "#{id} confirmed {expect}"
        );
        let after = started_next_frame(run, before)?;
        ensure!(
            cleared.get().is_some() && cleared.get() == after.last_queued_frame,
            "#{id} → {expect}: queued in the update that clears the pending clock ({:?}): {after:?}",
            cleared.get()
        );
        // The chip the effect rises from shows the confirmed state on that
        // frame (not the old one until the store reload lands).
        let shown = run
            .cx
            .update(|cx| run.view.read(cx).detail_item().map(|item| item.state));
        ensure!(
            shown.as_deref() == Some(expect),
            "the chip reads {expect} on the confirming frame: {shown:?}"
        );
        println!(
            "celebrate_smoke: #{id} → {expect} answered after {waited} ms; pending cleared after frame {:?}, effect on frame {:?}",
            cleared.get(),
            after.last_effect_frame
        );
        Ok(after)
    }

    /// The celebration queued by the last update starts on the next frame
    /// (drawn by that update, or now): one more effect, on the frame after
    /// the one it was queued behind.
    fn started_next_frame(run: &mut Run, before: u64) -> anyhow::Result<Hooks> {
        draw(run)?;
        let hooks = hooks(run);
        ensure!(
            hooks.effects_started == before + 1
                && hooks.last_queued_frame.is_some()
                && hooks.last_effect_frame == hooks.last_queued_frame.map(|f| f + 1),
            "the effect starts on the frame after its trigger: {hooks:?}"
        );
        Ok(hooks)
    }

    /// Draw real frames until the effect ends; then idle frames request
    /// nothing. Returns how long it played.
    fn play_out(run: &mut Run) -> anyhow::Result<Duration> {
        let mut frames = adoken_perf::FrameRecorder::start();
        let start = Instant::now();
        loop {
            let requested = hooks(run).frames_requested;
            std::thread::sleep(Duration::from_millis(8));
            draw(run)?;
            if hooks(run).frames_requested == requested {
                break;
            }
            ensure!(start.elapsed() < EFFECT_WALL, "the effect ends");
        }
        let played = start.elapsed();
        frames.collect();
        run.effect_frames
            .extend_from_slice(frames.draw().as_slice());
        drop(frames);
        let end = hooks(run);
        ensure!(read(run, KenLayer::live_particles) == 0, "no particle left");
        ensure!(
            end.last_live_frame.is_some_and(|f| end.frames - f <= 1),
            "the last requested frame is within one frame of the end: {end:?}"
        );
        for _ in 0..10 {
            draw(run)?;
        }
        let idle = hooks(run);
        ensure!(
            idle.frames_requested == end.frames_requested && idle.timers == 0,
            "idle after the effect: {idle:?}"
        );
        Ok(played)
    }

    /// Frames until a celebration starts (it may wait for a store commit).
    /// Without a visible anchor it starts from its toast, re-queued on the
    /// frame the toast first shows.
    fn until_effect(run: &mut Run, before: u64) -> anyhow::Result<Hooks> {
        for _ in 0..500 {
            run.cx.run_until_parked();
            draw(run)?;
            let hooks = hooks(run);
            if hooks.effects_started > before {
                ensure!(
                    hooks.effects_started == before + 1
                        && hooks.last_effect_frame == hooks.last_queued_frame,
                    "started on the frame its toast shows: {hooks:?}"
                );
                return Ok(hooks);
            }
            run.cx.advance_clock(Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("no celebration started")
    }

    /// Let store commits reach their watchers (a thread, then a debounce).
    fn wait_commits(run: &mut Run) -> anyhow::Result<()> {
        for _ in 0..60 {
            run.cx.run_until_parked();
            run.cx.advance_clock(Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        run.cx.run_until_parked();
        Ok(())
    }

    /// Active, completable PRs (mine or someone else's), not yet used.
    fn completable(run: &mut Run, mine: bool) -> anyhow::Result<Vec<i64>> {
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        let mut ids = Vec::new();
        for pr in &ds.prs {
            let Some(stored) = run.store.pr(pr.id)? else {
                continue;
            };
            let pr = stored.pr;
            if (pr.author == run.me) == mine
                && adoken_app::review::writes::complete_blocker(&pr, false).is_none()
            {
                ids.push(pr.id);
            }
        }
        ensure!(!ids.is_empty(), "completable PRs (mine: {mine})");
        Ok(ids)
    }

    fn open_review(run: &mut Run, id: i64) -> anyhow::Result<()> {
        run.window.update(&mut run.cx, |shell, window, cx| {
            shell.open_target(Target::PullRequest(id), window, cx)
        })?;
        let window = run.window;
        settle(&mut run.cx, |cx| {
            window.update(cx, |shell, _, cx| shell.open_review_id(cx) == Some(id))
        })
        .with_context(|| format!("review !{id} opens"))?;
        draw(run)?;
        Ok(())
    }

    fn pr_status(run: &mut Run, id: i64) -> anyhow::Result<PrStatus> {
        Ok(run.store.pr(id)?.context("PR in store")?.pr.status)
    }

    /// What a sync re-read stores for `id`.
    fn store_pr(
        run: &mut Run,
        id: i64,
        change: impl FnOnce(&mut PullRequest),
    ) -> anyhow::Result<()> {
        let mut pr = run.store.pr(id)?.context("PR in store")?.pr;
        change(&mut pr);
        if pr.status == PrStatus::Completed {
            pr.closed = Some(run.clock);
        }
        run.store
            .write(|tx| adoken_store::write::upsert_pr(tx, &pr).map(|_| ()))?;
        Ok(())
    }

    fn item_state(store: &Store, id: i64) -> anyhow::Result<String> {
        Ok(store
            .with_conn(|c| adoken_store::read::work_item(c, id))?
            .context("stored work item")?
            .state)
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

    fn draw(run: &mut Run) -> anyhow::Result<()> {
        run.cx
            .update_window(run.window.into(), |_, window, cx| window.draw(cx).clear())?;
        Ok(())
    }

    fn press(run: &mut Run, keys: &str) -> anyhow::Result<()> {
        let keystroke = Keystroke::parse(keys).context("keystroke")?;
        run.cx.update_window(run.window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        run.cx.run_until_parked();
        Ok(())
    }

    fn type_text(run: &mut Run, text: &str) -> anyhow::Result<()> {
        for ch in text.chars() {
            let key = if ch == ' ' {
                "space".to_owned()
            } else {
                ch.to_string()
            };
            let mut keystroke = Keystroke::parse(&key).context("keystroke")?;
            keystroke.key_char = Some(ch.to_string());
            run.cx.update_window(run.window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_keystroke(keystroke, cx);
            })?;
            run.cx.run_until_parked();
        }
        Ok(())
    }

    fn paint(run: &mut Run, name: &str) -> anyhow::Result<()> {
        let image = run
            .cx
            .update_window(run.window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.render_to_image()
            })?
            .context("rendered shell image")?;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/celebrate-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
