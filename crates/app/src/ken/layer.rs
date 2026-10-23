//! The Ken layer: runtime state, the background ingest loop, effect
//! handling (HUD, K.O., toasts, sparks, sounds), and the full-window
//! overlay that paints them. Overlays never take focus and never intercept
//! the pointer; any key, click or scroll dismisses a K.O. and still reaches
//! the app (ken.md §2, §4.2). The layer is present with Ken off: it also
//! runs and paints the celebrations of spec 0022 ([`celebrations`]).

mod celebrations;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use adoken_ken::celebrate::CelebrationSettings;
use adoken_ken::engine::COMBO_WINDOW;
use adoken_ken::engine::ComboView;
use adoken_ken::settings::Conditions;
use adoken_ken::{
    Action, Badge, Ctx, Effect, HitKind, Ken, KenSettings, Surfaces, Week, persist, source,
};
use adoken_model::Millis;
use adoken_notify::schedule::clock_label;
use adoken_notify::{Clock, NotifySettings, QuietHours, SystemClock};
use adoken_sound::SoundEvent;
use adoken_store::Store;
use adoken_theme::{size, space};
use adoken_ui::ActiveTheme;
use gpui::{
    AnyElement, AppContext as _, Context, DispatchPhase, Entity, IntoElement, MouseDownEvent,
    Render, ScrollWheelEvent, SharedString, Subscription, Task, Window, canvas, div, prelude::*,
    px,
};

use self::celebrations::CelebrationState;
use super::card_dialog::{DialogEvent, FightCardDialog};
use super::hud::{ComboHud, FADE_MS, FINISH_MS, HUD_WIDTH, KenToast, StatusChip, toast_stack};
use super::ko::{self, KoOverlay};
use crate::celebrate::{CelebrationToast, Fx, FxColors, GlobalFx};
use crate::palette::now_ms;
use crate::{settings, sound};

/// Ingest poll (store → hits) while Ken is on.
const POLL: Duration = Duration::from_secs(2);
/// Toast lifetime (principles.md §4: 5 s).
const TOAST_LIFE: Duration = Duration::from_secs(5);
/// Toasts shown at once.
const MAX_TOASTS: usize = 3;
/// A queued K.O. waits at most this long for typing / the palette.
const KO_DEFER_MAX: Duration = Duration::from_secs(10);
const KO_RECHECK: Duration = Duration::from_millis(500);
/// Key contexts that defer a K.O. (typing, palette, dialogs).
const DEFER_CONTEXTS: [&str; 4] = [
    adoken_ui::text_input::KEY_CONTEXT,
    "Palette",
    settings::CONTEXT,
    super::card_dialog::CONTEXT,
];
/// The HUD hides when the detail pane would be narrower than 480 px
/// (sidebar 232 + list 380 + 480).
const HUD_MIN_WINDOW: f32 = 1_092.0;
/// HUD inset from the top-right of the content area.
const HUD_TOP: f32 = size::TITLEBAR + space::S16;
/// Statusbar height the toasts sit above.
const STATUSBAR: f32 = 24.0;
/// Sparks per event (ken.md §4.1).
const STEP_SPARKS: usize = 4;
const RANK_UP_SPARKS: usize = 8;
const FINISH_SPARKS: usize = 12;

/// State the background ingest owns between polls.
#[derive(Default)]
struct Shared {
    ken: Ken,
    cursor: Option<i64>,
    loaded: bool,
    /// Actions from the UI (inbox cleared), scored on the next poll.
    pending: Vec<Action>,
    /// The weekly card offer was checked this session.
    offer_checked: bool,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// What one poll hands back to the UI.
#[derive(Default)]
struct Ingested {
    effects: Vec<Effect>,
    combo: Option<ComboView>,
    streak_days: u32,
    shields: u8,
    paused: bool,
    quiet_hours: bool,
    offer: Option<Week>,
    /// "merged into main · 3 reviewers" per K.O.'d PR.
    ko_subtitles: Vec<(i64, String)>,
}

/// Blocking: one ingest step. Background executor only.
fn ingest(
    store: &Store,
    shared: &Mutex<Shared>,
    settings: &KenSettings,
) -> adoken_store::Result<Ingested> {
    let clock = SystemClock;
    let now = clock.now();
    let mut s = lock(shared);
    if !s.loaded {
        persist::migrate(store)?;
        let loaded = persist::load(store)?;
        s.ken = loaded.ken;
        s.cursor = Some(match loaded.cursor {
            Some(cursor) => cursor,
            // First enable on this store: history never scores (ken.md §1).
            None => {
                source::baseline_viewed(store, now)?;
                source::starting_cursor(store)?
            }
        });
        s.loaded = true;
    }
    let cursor = s.cursor.unwrap_or_default();
    let batch = source::poll(store, cursor, now)?;
    let mut actions = std::mem::take(&mut s.pending);
    actions.extend(batch.actions);
    let ctx = Ctx {
        now,
        clock: &clock,
        working_days: settings.working_days,
        backfilling: batch.backfilling,
    };
    let applied = s.ken.apply(actions, &ctx);
    for rejection in &applied.rejected {
        log::debug!("ken: not a hit: {rejection:?}");
    }
    let journal = s.ken.take_journal();
    let moved = batch.cursor.is_some_and(|c| Some(c) != s.cursor);
    if let Some(next) = batch.cursor {
        s.cursor = Some(next);
    }
    if moved || !journal.is_empty() || !applied.accepted.is_empty() {
        persist::save(store, &s.ken, &journal, s.cursor)?;
    }
    let offer = if s.offer_checked {
        None
    } else {
        s.offer_checked = true;
        adoken_ken::week::offer(store, now, &clock)?
    };
    let today = adoken_notify::schedule::LocalTime::of(now, &clock).day;
    let mut ko_subtitles = Vec::new();
    for effect in &applied.effects {
        if let Effect::Ko { prs, .. } = effect {
            for (id, _) in prs {
                if let Some(detail) = store.pr(*id)? {
                    let pr = detail.pr;
                    let reviewers = pr.reviewers.iter().filter(|r| r.vote.as_i8() != 0).count();
                    let reviewers = match reviewers {
                        0 => String::new(),
                        1 => " · 1 reviewer".to_owned(),
                        n => format!(" · {n} reviewers"),
                    };
                    ko_subtitles.push((
                        *id,
                        format!(
                            "merged into {}{reviewers}",
                            adoken_model::short_branch(&pr.target_branch)
                        ),
                    ));
                }
            }
        }
    }
    Ok(Ingested {
        ko_subtitles,
        effects: applied.effects,
        combo: s.ken.combo_view(now),
        streak_days: s.ken.streak.current(today, settings.working_days),
        shields: s.ken.streak.shields,
        paused: s.ken.streak.is_paused(),
        quiet_hours: quiet_hours(store, now, &clock),
        offer,
    })
}

/// Spec 0005 quiet hours (`[notify]` in the settings file next to the store).
fn quiet_hours(store: &Store, now: Millis, clock: &dyn Clock) -> bool {
    let path = adoken_notify::persist::settings_path(store);
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    NotifySettings::from_settings_toml(&text).is_ok_and(|settings| {
        settings.quiet_hours.enabled && settings.quiet_hours.schedule.contains(now, clock)
    })
}

/// What one settings read applies: `[ken]`, `[celebrations]` and spec
/// 0005's quiet hours (celebrations follow them with Ken off too).
struct LoadedSettings {
    ken: KenSettings,
    celebrations: CelebrationSettings,
    quiet: Option<QuietHours>,
}

/// `settings.toml` (blocking).
fn read_settings(path: Option<PathBuf>) -> LoadedSettings {
    let text = path.and_then(|path| std::fs::read_to_string(path).ok());
    let Some(text) = text else {
        return LoadedSettings {
            ken: KenSettings::default(),
            celebrations: CelebrationSettings::default(),
            quiet: None,
        };
    };
    LoadedSettings {
        ken: KenSettings::from_settings_toml(&text),
        celebrations: CelebrationSettings::from_settings_toml(&text).0,
        quiet: NotifySettings::from_settings_toml(&text)
            .ok()
            .map(|settings| settings.quiet_hours)
            .filter(|quiet| quiet.enabled),
    }
}

struct HudAnim {
    appeared: Instant,
    stepped: Instant,
    rank_up: Option<Instant>,
    last_move: Option<(HitKind, Instant)>,
}

struct Finishing {
    view: ComboView,
    at: Instant,
    flourish: bool,
}

struct KoRun {
    prs: Vec<(i64, String)>,
    double: bool,
    subtitle: String,
    started: Instant,
    dismissed_at: Option<f32>,
}

#[derive(Clone)]
struct QueuedKo {
    prs: Vec<(i64, String)>,
    double: bool,
    subtitle: String,
    queued: Instant,
}

struct Toast {
    id: u64,
    body: ToastBody,
}

enum ToastBody {
    Ken(KenToast),
    /// A celebration Milestone (spec 0022, ken.md §12.5).
    Celebration(CelebrationToast),
}

enum Burst {
    Hud(usize),
    Center(usize),
}

pub struct KenLayer {
    store: Store,
    settings: KenSettings,
    shared: Arc<Mutex<Shared>>,
    combo: Option<ComboView>,
    streak_days: u32,
    shields: u8,
    paused: bool,
    quiet_hours: bool,
    window_active: bool,
    hud: Option<HudAnim>,
    finishing: Option<Finishing>,
    ko: Option<KoRun>,
    ko_queue: Vec<QueuedKo>,
    toasts: Vec<Toast>,
    next_toast: u64,
    /// Particles (Ken's sparks, celebration confetti), seals and glows,
    /// shared with the anchoring views ([`crate::celebrate`]).
    fx: Rc<RefCell<Fx>>,
    celebrations: CelebrationState,
    bursts: Vec<Burst>,
    dialog: Option<Entity<FightCardDialog>>,
    last_unread: Option<usize>,
    ingest: Option<Task<()>>,
    recheck: Option<Task<()>>,
    /// Repaints the chip when "Quiet mode for 1 hour" runs out.
    quiet_end: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl KenLayer {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let keys = cx.observe_keystrokes(|this, _, _, cx| this.dismiss_ko(cx));
        let activation = cx.observe_window_activation(window, |this, window, _| {
            this.window_active = window.is_window_active();
        });
        let settings_changed = settings::observe(cx, |this, cx| this.reload_settings(cx));
        let mut this = Self {
            store,
            settings: KenSettings::default(),
            shared: Arc::default(),
            combo: None,
            streak_days: 0,
            shields: 0,
            paused: false,
            quiet_hours: false,
            window_active: window.is_window_active(),
            hud: None,
            finishing: None,
            ko: None,
            ko_queue: Vec::new(),
            toasts: Vec::new(),
            next_toast: 0,
            fx: Rc::new(RefCell::new(Fx::new(0x004B_454E))),
            celebrations: CelebrationState::default(),
            bursts: Vec::new(),
            dialog: None,
            last_unread: None,
            ingest: None,
            recheck: None,
            quiet_end: None,
            _tasks: Vec::new(),
            _subscriptions: vec![keys, activation, settings_changed],
        };
        cx.set_global(GlobalFx(this.fx.clone()));
        this.reload_settings(cx);
        this
    }

    pub fn settings(&self) -> &KenSettings {
        &self.settings
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// A K.O. overlay plays or waits to start (tours hold back meanwhile;
    /// smoke runs).
    pub fn ko_showing(&self) -> bool {
        self.ko.is_some() || !self.ko_queue.is_empty()
    }

    /// The K.O. overlay is on screen (smoke runs).
    pub fn ko_playing(&self) -> bool {
        self.ko.is_some()
    }

    /// How many Ken toasts (badges, K.O. toasts) are on screen.
    pub fn ken_toast_count(&self) -> usize {
        self.toasts
            .iter()
            .filter(|t| matches!(t.body, ToastBody::Ken(_)))
            .count()
    }

    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let path = settings::path(cx);
        let job = cx
            .background_executor()
            .spawn(async move { read_settings(path) });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let LoadedSettings {
                ken,
                celebrations,
                quiet,
            } = job.await;
            this.update(cx, |this, cx| {
                this.apply_celebrations(celebrations, quiet);
                this.apply_settings(ken, cx)
            })
            .ok();
        }));
    }

    fn apply_settings(&mut self, settings: KenSettings, cx: &mut Context<Self>) {
        let was_enabled = self.settings.enabled;
        self.settings = settings;
        match (was_enabled, self.settings.enabled) {
            (false, true) => self.start_ingest(cx),
            (_, true) if self.ingest.is_none() => self.start_ingest(cx),
            (_, false) => {
                self.ingest = None;
                self.combo = None;
                self.hud = None;
                self.finishing = None;
            }
            _ => {}
        }
        sound::set_quiet_mode(self.settings.quiet_mode(), cx);
        self.watch_quiet_end(cx);
        cx.notify();
    }

    /// Write `[ken]` and apply it at once.
    fn save_settings(&mut self, settings: KenSettings, cx: &mut Context<Self>) {
        settings::write_table(
            cx,
            adoken_ken::settings::SETTINGS_TABLE,
            settings.to_table(),
        );
        self.apply_settings(settings, cx);
    }

    pub fn toggle_enabled(&mut self, cx: &mut Context<Self>) {
        let mut settings = self.settings.clone();
        settings.enabled = !settings.enabled;
        let text = if settings.enabled {
            "Arcade mode on · stats start now"
        } else {
            "Arcade mode off"
        };
        self.save_settings(settings, cx);
        // Visible even when turning off (the toast is plain feedback).
        self.push_toast_always(text.into(), false, None, cx);
    }

    pub fn update_settings(
        &mut self,
        change: impl FnOnce(&mut KenSettings),
        cx: &mut Context<Self>,
    ) {
        let mut settings = self.settings.clone();
        change(&mut settings);
        self.save_settings(settings, cx);
    }

    /// "Quiet mode: turn on or off" (spec 0025 FR-8; works with Ken off).
    pub fn toggle_quiet(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        self.update_settings(|settings| settings.toggle_quiet(now), cx);
        self.hide_for_quiet(now);
    }

    /// "Quiet mode for 1 hour".
    pub fn quiet_for_hour(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        self.update_settings(|settings| settings.quiet_for_hour(now), cx);
        self.hide_for_quiet(now);
    }

    /// Quiet mode hides everything that's already up, celebrations too.
    fn hide_for_quiet(&mut self, now: Millis) {
        if self.settings.quiet_active(now) {
            self.toasts.clear();
            self.ko = None;
            self.ko_queue.clear();
            self.clear_celebrations();
        }
    }

    fn watch_quiet_end(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        self.quiet_end = self
            .settings
            .quiet_until_ms()
            .filter(|until| *until > now)
            .map(|until| {
                let wait = Duration::from_millis(until.abs_diff(now));
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    this.update(cx, |_, cx| cx.notify()).ok();
                })
            });
    }

    fn start_ingest(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let shared = self.shared.clone();
        self.ingest = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(settings) = this.read_with(cx, |this, _| this.settings.clone()) else {
                    return;
                };
                if !settings.enabled {
                    return;
                }
                let (store, shared) = (store.clone(), shared.clone());
                let result = cx
                    .background_executor()
                    .spawn(async move { ingest(&store, &shared, &settings) })
                    .await;
                let alive = this.update(cx, |this, cx| match result {
                    Ok(ingested) => this.receive(ingested, cx),
                    Err(error) => log::warn!("ken: ingest failed: {error}"),
                });
                if alive.is_err() {
                    return;
                }
                cx.background_executor().timer(POLL).await;
            }
        }));
    }

    fn surfaces(&self, cx: &gpui::App) -> Surfaces {
        self.settings.surfaces(Conditions {
            reduce_motion: cx.reduce_motion(),
            quiet_hours: self.quiet_hours,
            now: now_ms(),
        })
    }

    fn receive(&mut self, ingested: Ingested, cx: &mut Context<Self>) {
        self.quiet_hours = ingested.quiet_hours;
        self.streak_days = ingested.streak_days;
        self.shields = ingested.shields;
        self.paused = ingested.paused;
        let surfaces = self.surfaces(cx);
        for effect in ingested.effects {
            self.handle(effect, surfaces, &ingested.ko_subtitles, cx);
        }
        self.combo = ingested.combo;
        if self.combo.is_none() {
            self.hud = None;
        }
        if ingested.offer.is_some() && surfaces.toasts {
            self.push_toast(
                "Your week in review is ready".into(),
                false,
                Some("> Ken: Show fight card".into()),
                cx,
            );
        }
        cx.notify();
    }

    fn handle(
        &mut self,
        effect: Effect,
        surfaces: Surfaces,
        ko_subtitles: &[(i64, String)],
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        match effect {
            Effect::Hit { kind, .. } => {
                if let Some(hud) = &mut self.hud {
                    hud.last_move = Some((kind, now));
                } else {
                    self.hud = Some(HudAnim {
                        appeared: now,
                        stepped: now,
                        rank_up: None,
                        last_move: Some((kind, now)),
                    });
                }
            }
            Effect::Combo { count, rank_up, .. } => {
                if !self.settings.combo {
                    return;
                }
                let hud = self.hud.get_or_insert(HudAnim {
                    appeared: now,
                    stepped: now,
                    rank_up: None,
                    last_move: None,
                });
                if count == 2 {
                    hud.appeared = now;
                }
                hud.stepped = now;
                if rank_up {
                    hud.rank_up = Some(now);
                }
                self.finishing = None;
                if surfaces.sparks {
                    self.bursts.push(Burst::Hud(if rank_up {
                        RANK_UP_SPARKS
                    } else {
                        STEP_SPARKS
                    }));
                }
                if surfaces.sounds {
                    self.play(SoundEvent::KenComboStep, Some(count), cx);
                }
            }
            Effect::ComboFinished {
                count,
                rank,
                prs,
                duration,
                flourish,
            } => {
                if !self.settings.combo {
                    return;
                }
                if let (Some(view), true) = (self.combo.clone(), surfaces.hud) {
                    self.finishing = Some(Finishing {
                        view,
                        at: now,
                        flourish,
                    });
                }
                self.hud = None;
                if flourish {
                    if surfaces.sparks {
                        self.bursts.push(Burst::Hud(FINISH_SPARKS));
                    }
                    if surfaces.sounds {
                        self.play(SoundEvent::KenComboFinish, None, cx);
                    }
                    let rank = rank.map_or(String::new(), |r| {
                        format!(" · {}", r.name().text(self.settings.naming))
                    });
                    let minutes = (duration / 60_000).max(1);
                    let prs = if prs == 1 {
                        "1 PR".to_owned()
                    } else {
                        format!("{prs} PRs")
                    };
                    self.push_toast(
                        format!("Combo ×{count}{rank} — {prs} reviewed in {minutes} min").into(),
                        false,
                        None,
                        cx,
                    );
                }
            }
            Effect::Ko { mut prs, double } => {
                if !self.settings.ko {
                    return;
                }
                // Spec 0022 FR-6: a K.O. that already started on the frame
                // that confirmed my Complete shows nothing again.
                prs.retain(|(id, _)| !self.celebrations.ko_started.contains(id));
                if prs.is_empty() {
                    return;
                }
                if surfaces.ko_overlay {
                    let subtitle = prs
                        .first()
                        .and_then(|(id, _)| ko_subtitles.iter().find(|(pr, _)| pr == id))
                        .map_or_else(|| "merged".to_owned(), |(_, text)| text.clone());
                    self.ko_queue.push(QueuedKo {
                        prs,
                        double,
                        subtitle,
                        queued: now,
                    });
                } else {
                    if surfaces.sounds {
                        self.play(SoundEvent::KenKo, None, cx);
                    }
                    self.push_toast(ko::toast_text(&prs, double).into(), true, None, cx);
                }
            }
            Effect::Badge(badge) => {
                if matches!(badge, Badge::Streak { .. }) {
                    if !self.settings.streak {
                        return;
                    }
                } else if !self.settings.badges {
                    return;
                }
                let sound = match badge {
                    Badge::FirstStrike { .. } => Some(SoundEvent::KenFirstStrike),
                    Badge::CleanSweep => Some(SoundEvent::KenCleanSweep),
                    // The streak sound plays with the Streak effect.
                    Badge::Streak { .. } | Badge::Comeback { .. } | Badge::Assist { .. } => None,
                };
                if let Some(sound) = sound
                    && surfaces.sounds
                {
                    self.play(sound, None, cx);
                }
                self.push_toast(badge.toast().into(), false, None, cx);
            }
            Effect::Streak { .. } => {
                if self.settings.streak && surfaces.sounds {
                    self.play(SoundEvent::KenStreak, None, cx);
                }
            }
        }
    }

    fn play(&self, event: SoundEvent, combo_level: Option<u32>, cx: &mut Context<Self>) {
        sound::play_ken(event, combo_level, self.quiet_hours, self.window_active, cx).ok();
    }

    fn push_toast(
        &mut self,
        text: SharedString,
        ko_badge: bool,
        hint: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        if !self.surfaces(cx).toasts {
            return;
        }
        self.push_toast_always(text, ko_badge, hint, cx);
    }

    fn push_toast_always(
        &mut self,
        text: SharedString,
        ko_badge: bool,
        hint: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.push_toast_body(
            ToastBody::Ken(KenToast {
                text,
                ko_badge,
                hint,
            }),
            cx,
        );
    }

    /// Show a toast for [`TOAST_LIFE`]; returns its id.
    fn push_toast_body(&mut self, body: ToastBody, cx: &mut Context<Self>) -> u64 {
        let id = self.add_toast(body, cx);
        cx.notify();
        id
    }

    /// [`Self::push_toast_body`] without notifying, so render can place a
    /// toast for the frame it's building.
    fn add_toast(&mut self, body: ToastBody, cx: &mut Context<Self>) -> u64 {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast { id, body });
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
        self._tasks.retain(|task| !task.is_ready());
        self._tasks.push(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_LIFE).await;
            this.update(cx, |this, cx| {
                this.toasts.retain(|t| t.id != id);
                cx.notify();
            })
            .ok();
        }));
        id
    }

    fn dismiss_ko(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &mut self.ko
            && run.dismissed_at.is_none()
        {
            run.dismissed_at = Some(run.started.elapsed().as_secs_f32() * 1000.0);
            cx.notify();
        }
    }

    /// "Ken: Pause or resume streak": flips the stored state.
    pub fn toggle_streak_paused(&mut self, cx: &mut Context<Self>) {
        self.write_streak_pause(None, cx);
    }

    /// Settings › Ken's Pause and Resume buttons.
    pub fn set_streak_paused(&mut self, paused: bool, cx: &mut Context<Self>) {
        self.write_streak_pause(Some(paused), cx);
    }

    /// `None` flips whatever the store holds.
    fn write_streak_pause(&mut self, paused: Option<bool>, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let shared = self.shared.clone();
        let working = self.settings.working_days;
        let job = cx.background_executor().spawn(async move {
            let clock = SystemClock;
            let mut s = lock(&shared);
            if !s.loaded {
                persist::migrate(&store)?;
                let loaded = persist::load(&store)?;
                s.ken = loaded.ken;
                s.cursor = loaded.cursor;
                s.loaded = loaded.cursor.is_some();
            }
            let ctx = Ctx {
                now: clock.now(),
                clock: &clock,
                working_days: working,
                backfilling: false,
            };
            let paused = paused.unwrap_or(!s.ken.streak.is_paused());
            s.ken.set_streak_paused(paused, &ctx);
            let journal = s.ken.take_journal();
            persist::save(&store, &s.ken, &journal, s.cursor)?;
            Ok::<_, adoken_store::Error>((paused, s.ken.streak.days))
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| match result {
                Ok((paused, days)) => {
                    this.paused = paused;
                    this.streak_days = days;
                    let text = if paused {
                        "Streak paused · resume with > Ken: Pause or resume streak"
                    } else {
                        "Streak resumed"
                    };
                    this.push_toast_always(text.into(), false, None, cx);
                }
                Err(error) => log::warn!("ken: couldn't save the streak: {error}"),
            })
            .ok();
        }));
    }

    /// Settings › Ken › [Reset stats] (after the confirm dialog).
    pub fn reset_stats(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let shared = self.shared.clone();
        let job = cx.background_executor().spawn(async move {
            persist::migrate(&store)?;
            persist::reset(&store)?;
            let mut s = lock(&shared);
            s.ken = Ken::default();
            Ok::<_, adoken_store::Error>(())
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                if let Err(error) = result {
                    log::warn!("ken: reset failed: {error}");
                    return;
                }
                this.combo = None;
                this.hud = None;
                this.finishing = None;
                this.streak_days = 0;
                this.shields = 0;
                this.paused = false;
                this.push_toast_always("Arcade stats reset".into(), false, None, cx);
            })
            .ok();
        }));
    }

    pub fn inbox_unread(&mut self, unread: usize) {
        let previous = self.last_unread.replace(unread);
        if self.settings.enabled && unread == 0 && previous.is_some_and(|p| p > 0) {
            lock(&self.shared)
                .pending
                .push(Action::InboxCleared { at: now_ms() });
        }
    }

    /// Open the fight card for `week` (default: this week); `export` jumps
    /// straight to the save dialog.
    pub fn show_fight_card(&mut self, week: Option<Week>, export: bool, cx: &mut Context<Self>) {
        let week = week.unwrap_or_else(|| Week::of(now_ms(), &SystemClock));
        let dialog = cx.new(|cx| {
            FightCardDialog::new(
                self.store.clone(),
                week,
                self.settings.naming,
                self.streak_days,
                self.shields,
                export,
                cx,
            )
        });
        self._subscriptions
            .push(cx.subscribe(&dialog, |this, _, event, cx| match event {
                DialogEvent::Close => {
                    this.dialog = None;
                    cx.notify();
                }
                DialogEvent::Toast(text) => {
                    this.push_toast_always(text.clone(), false, None, cx);
                }
            }));
        self.dialog = Some(dialog);
        cx.notify();
    }

    /// The statusbar chip for this moment. Quiet mode shows with Ken off
    /// too: it also silences sounds and hides celebrations (ken.md §7).
    pub fn chip(&self, cx: &gpui::App) -> StatusChip {
        let now = now_ms();
        if let Some(label) = quiet_label(&self.settings, now) {
            return StatusChip {
                quiet: Some(label),
                ..StatusChip::default()
            };
        }
        if !self.settings.enabled {
            return StatusChip::default();
        }
        let surfaces = self.surfaces(cx);
        StatusChip {
            combo: self
                .combo
                .as_ref()
                .filter(|_| surfaces.chip && self.settings.combo)
                .filter(|c| c.until >= now)
                .map(|c| (c.count, c.rank, c.until - now)),
            naming: self.settings.naming,
            streak_days: if self.settings.streak {
                self.streak_days
            } else {
                0
            },
            shields: self.shields,
            paused: self.paused && self.settings.streak,
            quiet: None,
        }
    }

    fn start_queued_ko(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ko.is_some() || self.ko_queue.is_empty() {
            return;
        }
        let blocked = window
            .context_stack()
            .iter()
            .any(|context| DEFER_CONTEXTS.iter().any(|c| context.contains(c)))
            || self.dialog.is_some();
        if blocked && self.ko_queue[0].queued.elapsed() < KO_DEFER_MAX {
            if self.recheck.is_none() {
                self.recheck = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(KO_RECHECK).await;
                    this.update(cx, |this, cx| {
                        this.recheck = None;
                        cx.notify();
                    })
                    .ok();
                }));
            }
            return;
        }
        let QueuedKo {
            prs,
            double,
            subtitle,
            ..
        } = self.ko_queue.remove(0);
        let surfaces = self.surfaces(cx);
        if surfaces.sounds {
            self.play(SoundEvent::KenKo, None, cx);
        }
        if surfaces.sparks {
            self.bursts.push(Burst::Center(ko::IMPACT_SPARKS));
        }
        self.ko = Some(KoRun {
            prs,
            double,
            subtitle,
            started: Instant::now(),
            dismissed_at: None,
        });
    }

    fn render_hud(&mut self, surfaces: Surfaces) -> Option<ComboHud> {
        if !surfaces.hud {
            return None;
        }
        let now = now_ms();
        let naming = self.settings.naming;
        if let Some(finishing) = &self.finishing {
            let t = finishing.at.elapsed().as_secs_f32() * 1000.0;
            let length = if finishing.flourish {
                FINISH_MS
            } else {
                FADE_MS
            };
            if t >= length || !surfaces.hud_motion {
                self.finishing = None;
            } else {
                let view = &finishing.view;
                return Some(ComboHud {
                    count: view.count,
                    rank: view.rank,
                    naming,
                    remaining_ms: 0,
                    window_ms: COMBO_WINDOW,
                    motion: true,
                    appear_ms: f32::MAX,
                    step_ms: f32::MAX,
                    rank_up_ms: None,
                    last_move: None,
                    finish: Some((t, finishing.flourish)),
                });
            }
        }
        let combo = self.combo.as_ref().filter(|c| c.until >= now)?;
        let hud = self.hud.as_ref();
        let ms = |at: Instant| at.elapsed().as_secs_f32() * 1000.0;
        Some(ComboHud {
            count: combo.count,
            rank: combo.rank,
            naming,
            remaining_ms: combo.until - now,
            window_ms: COMBO_WINDOW,
            motion: surfaces.hud_motion,
            appear_ms: hud.map_or(f32::MAX, |h| ms(h.appeared)),
            step_ms: hud.map_or(f32::MAX, |h| ms(h.stepped)),
            rank_up_ms: hud.and_then(|h| h.rank_up.map(ms)),
            last_move: hud.and_then(|h| h.last_move.map(|(kind, at)| (kind, ms(at)))),
            finish: None,
        })
    }
}

/// "Quiet mode until 15:40" or "Quiet mode on" (ken.md §7); `None` while
/// Quiet mode is off.
pub(crate) fn quiet_label(settings: &KenSettings, now: Millis) -> Option<String> {
    if !settings.quiet_active(now) {
        return None;
    }
    Some(
        match settings.quiet_until_ms().filter(|until| now < *until) {
            Some(until) => format!("Quiet mode until {}", clock_label(until, now, &SystemClock)),
            None => "Quiet mode on".to_owned(),
        },
    )
}

impl Render for KenLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let surfaces = self.surfaces(cx);
        let viewport = window.viewport_size();
        let (width, height) = (f32::from(viewport.width), f32::from(viewport.height));

        self.start_queued_ko(window, cx);
        // K.O. timeline.
        let ko = self.ko.as_ref().map(|run| {
            let t = run.started.elapsed().as_secs_f32() * 1000.0;
            (t, run.dismissed_at)
        });
        let ko_element = match (&self.ko, ko) {
            (Some(run), Some((t, dismissed))) if !ko::finished(t, dismissed) => {
                window.request_animation_frame();
                let frame = ko::frame(t, dismissed);
                Some(KoOverlay {
                    prs: run.prs.clone(),
                    double: run.double,
                    frame,
                    shake: surfaces.shake,
                    subtitle: run.subtitle.clone(),
                })
            }
            _ => None,
        };
        if ko_element.is_none() {
            self.ko = None;
        }

        // Sparks and celebrations share one particle budget: Ken's bursts
        // resolve at their fixed anchors, then everything steps.
        let hud_anchor = (width - space::S16 - HUD_WIDTH + 36.0, HUD_TOP + 26.0);
        let live = {
            let mut fx = self.fx.borrow_mut();
            fx.hooks.frames += 1;
            for burst in std::mem::take(&mut self.bursts) {
                match burst {
                    Burst::Hud(n) => fx.ken_burst(hud_anchor.0, hud_anchor.1, n),
                    Burst::Center(n) => fx.ken_burst(width / 2.0, height / 2.0, n),
                }
            }
            let now = Instant::now();
            if let Some(last) = fx.last_step {
                fx.step((now - last).as_secs_f32() * 1000.0, width, height);
            }
            let live = fx.is_live();
            fx.last_step = live.then_some(now);
            if live {
                // Frames only while something is live (FR-12).
                window.request_animation_frame();
                fx.hooks.frames_requested += 1;
                fx.hooks.last_live_frame = Some(fx.hooks.frames);
            }
            live
        };
        let live = self.place_unanchored(cx) || live;

        let hud = if width >= HUD_MIN_WINDOW {
            self.render_hud(surfaces)
        } else {
            None
        };
        // Repeating HUD motion (timer bar, pops) on the shared 30 fps
        // clock; nothing scheduled under reduce motion.
        let animating = self.finishing.is_some() || (hud.is_some() && surfaces.hud_motion);
        if animating {
            adoken_ui::motion::pulse_phase(Duration::from_secs(1), cx.entity_id(), cx);
        }

        let weak = cx.entity().downgrade();
        let dismiss_input = ko_element.is_some().then(|| {
            let (down, scroll) = (weak.clone(), weak);
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            down.update(cx, |this, cx| this.dismiss_ko(cx)).ok();
                        }
                    });
                    window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            scroll.update(cx, |this, cx| this.dismiss_ko(cx)).ok();
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });
        let toasts = self
            .toasts
            .iter()
            .map(|t| match &t.body {
                ToastBody::Ken(toast) => toast.clone().into_any_element(),
                ToastBody::Celebration(toast) => toast.clone().into_any_element(),
            })
            .collect::<Vec<AnyElement>>();
        let effects = live.then(|| {
            let (resolve, paint) = (self.fx.clone(), self.fx.clone());
            let fx_colors = FxColors::of(&theme);
            // Last child, so every anchor (views, toasts) has recorded its
            // bounds by this prepaint: the effect starts on this frame.
            canvas(
                move |_, window, _| {
                    let mut fx = resolve.borrow_mut();
                    fx.resolve(width, height);
                    if fx.is_live() {
                        window.request_animation_frame();
                    }
                },
                move |_, _, window, _| crate::celebrate::paint(&paint.borrow(), &fx_colors, window),
            )
            .absolute()
            .inset_0()
            .size_full()
        });
        let announcement = self.celebrations.announcement.clone().map(|text| {
            div()
                .id("celebration-announcement")
                .absolute()
                .size_0()
                .overflow_hidden()
                .role(gpui::Role::Status)
                .aria_label(text)
        });
        div()
            .absolute()
            .inset_0()
            .size_full()
            .children(ko_element)
            .children(dismiss_input)
            .children(hud.map(|hud| {
                div()
                    .absolute()
                    .top(px(HUD_TOP))
                    .right(px(space::S16))
                    .child(hud)
            }))
            .children(announcement)
            .when(!toasts.is_empty(), |layer| {
                layer.child(
                    div()
                        .absolute()
                        .right(px(space::S16))
                        .bottom(px(STATUSBAR + space::S16))
                        .child(toast_stack(toasts)),
                )
            })
            .children(self.dialog.clone())
            .children(effects)
    }
}

/// The statusbar chip as its own view (re-renders with the layer).
pub struct KenChip {
    layer: Entity<KenLayer>,
    _observe: Subscription,
}

impl KenChip {
    pub fn new(layer: Entity<KenLayer>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&layer, |_, _, cx| cx.notify());
        Self {
            layer,
            _observe: observe,
        }
    }
}

impl Render for KenChip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Re-rendered with the layer (every ingest poll while Ken is on).
        let chip = self.layer.read(cx).chip(cx);
        div().flex().items_center().child(chip)
    }
}
