//! Today (spec 0017; layout, copy and motion in `docs/design/today.md`): a
//! day-summary card with a mood, my PRs with full titles and what happened
//! on them today, and a side column for Up next, reviews waiting on me and
//! loose ends.
//!
//! Threading: the store is read, the nudge pass runs, the [`adoken_nudge::Day`]
//! (summary, mood, per-PR activity) is built and the ledger saved on a
//! background executor; the UI only lays out the precomputed result. Passes
//! rerun after every store commit (sync pages, debounced), every few
//! minutes, and when the summary changes with time alone (local midnight,
//! the end of the fresh window, the half-day mark; 0017 FR-2). Triage
//! (snooze, done, opt-outs) re-derives synchronously from the in-memory
//! snapshot, then persists in the background. Time comes from an injected
//! [`Clock`]. The only animation is the mood illustration's crossfade, which
//! Reduce Motion turns into a swap.

pub mod model;
mod nav;
mod render;
pub use nav::TodayNavState;
pub use render::{art_path, state_visual};

/// Guided tour anchors (spec 0020 FR-10, 0017).
pub mod anchors {
    pub const SUMMARY: &str = "today.summary";
    pub const PRS: &str = "today.prs";
    pub const UP_NEXT: &str = "today.up-next";
    pub const WAITING: &str = "today.waiting";
    pub const LOOSE_ENDS: &str = "today.loose-ends";
}

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adoken_model::{Millis, Subject};
use adoken_nudge::{
    Clock, LaneHealth, Mood, Settings, SyncHealth, SystemClock, WorkingHours, local_utc_offset_min,
};
use adoken_store::Store;
use adoken_sync::cadence::Lane;
use adoken_theme::typography;
use adoken_ui::{ActiveTheme, TypeStyleExt};
use futures::StreamExt as _;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    Pixels, ScrollHandle, Subscription, Task, Window, actions, div, prelude::*,
};

use crate::palette::PaletteCommand;
use crate::sync::SyncState;
use model::{Layout, Loaded, Row, RowKey, Target};

actions!(
    today,
    [
        /// Select the row in the column to the right (one column: the next row).
        SelectNext,
        /// Select the row in the column to the left (one column: the previous row).
        SelectPrev,
        /// Select the next row in the column.
        SelectDown,
        /// Select the previous row in the column.
        SelectUp,
        /// Select the first row of the page.
        SelectFirst,
        /// Select the last row of the page.
        SelectLast,
        /// Open the selected row (PR review, work item or run); Up next runs its action.
        Activate,
        /// Hide Up next until tomorrow.
        DismissFocus,
        /// Snooze the selected row's nudge until the next workday.
        SnoozeNudge,
        /// Mark the selected row's nudge done until its condition changes.
        MarkNudgeDone,
        /// Never nudge about the selected item again.
        NotForThisItem,
        /// Turn off the selected row's nudge type.
        TurnOffNudgeType,
        /// Copy the names of reviewers to ping.
        CopyReviewerNames,
        /// Recompute Today from the local store now.
        RefreshToday,
    ]
);

const KEY_CONTEXT: &str = "Today";
const DISMISS_FOCUS_KEYS: &str = "e";
const SNOOZE_KEYS: &str = "s";
const DONE_KEYS: &str = "d";
const NOT_FOR_ITEM_KEYS: &str = "x";
const COPY_NAMES_KEYS: &str = "c";
/// Thresholds cross with time alone; re-run the pass this often.
const PASS_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Coalesce bursts of store commits (one sync pass = many pages).
const COMMIT_DEBOUNCE: Duration = Duration::from_millis(500);
/// principles §3.2: nothing for this long, then skeletons.
const SKELETON_AFTER: Duration = Duration::from_millis(150);
/// 0018 FR-8: a pending restore older than this is dropped.
const RESTORE_TTL: Duration = Duration::from_secs(5);
/// A mood change painted later than this after it was computed happened
/// while Today was off screen: the new art is simply there (design §3.4).
const CROSSFADE_WINDOW: Duration = Duration::from_millis(250);

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("right", SelectNext, cx),
        KeyBinding::new("l", SelectNext, cx),
        KeyBinding::new("left", SelectPrev, cx),
        KeyBinding::new("h", SelectPrev, cx),
        KeyBinding::new("down", SelectDown, cx),
        KeyBinding::new("j", SelectDown, cx),
        KeyBinding::new("up", SelectUp, cx),
        KeyBinding::new("k", SelectUp, cx),
        KeyBinding::new("home", SelectFirst, cx),
        KeyBinding::new("end", SelectLast, cx),
        KeyBinding::new("enter", Activate, cx),
        KeyBinding::new(DISMISS_FOCUS_KEYS, DismissFocus, cx),
        KeyBinding::new(SNOOZE_KEYS, SnoozeNudge, cx),
        KeyBinding::new(DONE_KEYS, MarkNudgeDone, cx),
        KeyBinding::new(NOT_FOR_ITEM_KEYS, NotForThisItem, cx),
        KeyBinding::new(COPY_NAMES_KEYS, CopyReviewerNames, cx),
    ]
}

/// Today's actions for Cmd+P (keyboard-first): every key on Today.
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Today: Open selected row",
            keys: Some("enter"),
            action: Box::new(Activate),
        },
        PaletteCommand {
            title: "Today: Next row",
            keys: Some("j"),
            action: Box::new(SelectDown),
        },
        PaletteCommand {
            title: "Today: Previous row",
            keys: Some("k"),
            action: Box::new(SelectUp),
        },
        PaletteCommand {
            title: "Today: Right column",
            keys: Some("l"),
            action: Box::new(SelectNext),
        },
        PaletteCommand {
            title: "Today: Left column",
            keys: Some("h"),
            action: Box::new(SelectPrev),
        },
        PaletteCommand {
            title: "Today: First row",
            keys: Some("home"),
            action: Box::new(SelectFirst),
        },
        PaletteCommand {
            title: "Today: Last row",
            keys: Some("end"),
            action: Box::new(SelectLast),
        },
        PaletteCommand {
            title: "Today: Hide Up next until tomorrow",
            keys: Some(DISMISS_FOCUS_KEYS),
            action: Box::new(DismissFocus),
        },
        PaletteCommand {
            title: "Nudge: Snooze until next workday",
            keys: Some(SNOOZE_KEYS),
            action: Box::new(SnoozeNudge),
        },
        PaletteCommand {
            title: "Nudge: Mark done",
            keys: Some(DONE_KEYS),
            action: Box::new(MarkNudgeDone),
        },
        PaletteCommand {
            title: "Nudge: Not for this item",
            keys: Some(NOT_FOR_ITEM_KEYS),
            action: Box::new(NotForThisItem),
        },
        PaletteCommand {
            title: "Nudge: Turn off this nudge type",
            keys: None,
            action: Box::new(TurnOffNudgeType),
        },
        PaletteCommand {
            title: "Nudge: Copy reviewer names",
            keys: Some(COPY_NAMES_KEYS),
            action: Box::new(CopyReviewerNames),
        },
        PaletteCommand {
            title: "Today: Refresh",
            keys: None,
            action: Box::new(RefreshToday),
        },
    ]
}

/// What Today asks the shell to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodayEvent {
    OpenPr(i64),
    OpenWorkItem(i64),
    OpenRun(i64),
}

enum State {
    Loading,
    /// "Me" isn't known yet (no sync has run).
    NoAccount,
    Ready(Arc<Loaded>),
}

/// The illustration on screen, the one fading out, and the one decoding
/// (FR-29–FR-31: at most two painted, decoded off the UI thread).
#[derive(Default)]
struct Art {
    current: Option<Mood>,
    previous: Option<Mood>,
    /// A new mood's art decoding on the background executor; `current`
    /// stays on screen until it's ready, then they swap.
    pending: Option<Mood>,
    /// When `previous` was replaced; the crossfade only runs if the change
    /// paints right away (Today on screen).
    changed_at: Option<Instant>,
    generation: usize,
    fading: bool,
    _fade: Option<Task<()>>,
    _decode: Option<Task<()>>,
}

pub struct TodayView {
    store: Option<Store>,
    clock: Arc<dyn Clock>,
    settings: Settings,
    sync: SyncHealth,
    state: State,
    /// "Couldn't load Today. {reason}" over the last good day, if any.
    error: Option<String>,
    /// Loading has lasted [`SKELETON_AFTER`]: skeletons show.
    skeleton: bool,
    /// The selected row, by what it shows.
    selected: Option<RowKey>,
    /// 0018: a restore waiting for the first `Day`, with when it was asked.
    pending_restore: Option<(TodayNavState, Instant)>,
    /// 0018: a scroll anchor applied once its row has painted ([`nav`]).
    scroll_restore: Rc<Cell<Option<crate::shell::history::ScrollAnchor<RowKey>>>>,
    /// Stories: show group reviewers regardless of `[review]` (C1).
    groups_override: Option<bool>,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Today's own width as last painted (FR-24 breakpoint).
    width: Rc<Cell<Option<f32>>>,
    /// Painted rows' bounds (FR-26 nearest row, scrolling into view).
    rows: Rc<RefCell<HashMap<Row, Bounds<Pixels>>>>,
    art: Art,
    /// Store generation after our own last write (not a reason to reload).
    own_generation: u64,
    _load: Option<Task<()>>,
    _save: Option<Task<()>>,
    _watch: Option<Task<()>>,
    _tick: Option<Task<()>>,
    _skeleton: Option<Task<()>>,
    _settings: Option<Subscription>,
    _settings_load: Option<Task<()>>,
    _groups: Subscription,
    row_menu: adoken_ui::action_menu::MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    _pins: Option<gpui::Subscription>,
    _release: Subscription,
}

/// Today over the local store, with the wall clock and the OS time zone.
pub fn new(store: Store, window: &mut Window, cx: &mut App) -> Entity<TodayView> {
    let settings =
        Settings::default().with_hours(WorkingHours::default().with_offset(local_utc_offset_min()));
    cx.new(|cx| TodayView::with_store(store, Arc::new(SystemClock), settings, window, cx))
}

/// FR-10 inputs from the running sync: reachability, the token, the My
/// pull requests and Work items lanes' last successes and intervals (a
/// lane set to Manual only or paused by the user never goes stale by age),
/// and the work items scope.
pub fn sync_health(state: &SyncState, now: Millis, cx: &App) -> SyncHealth {
    let inputs = crate::sync::sync_inputs(cx, state);
    let paused = inputs.prefs.pause.resumes(now) != Some(now);
    let lane = |lane: Lane| LaneHealth {
        last_ok: inputs.lanes[lane.index()].last_ok,
        interval: if paused {
            None
        } else {
            inputs.prefs.interval(lane, true)
        },
    };
    SyncHealth {
        online: inputs.online,
        token_rejected: inputs.gov.unauthorized,
        my_prs: lane(Lane::MyPrs),
        work_items: lane(Lane::WorkItems),
        work_scope: !inputs.missing_scopes.iter().any(|s| s == "vso.work"),
    }
}

impl EventEmitter<TodayEvent> for TodayView {}

impl Focusable for TodayView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TodayView {
    pub fn with_store(
        store: Store,
        clock: Arc<dyn Clock>,
        settings: Settings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(Some(store), clock, settings, cx);
        this._skeleton = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SKELETON_AFTER).await;
            this.update(cx, |this, cx| {
                if matches!(this.state, State::Loading) {
                    this.skeleton = true;
                    cx.notify();
                }
            })
            .ok();
        }));
        this.load_settings(cx);
        this.watch(cx);
        this._settings = Some(crate::settings::observe(cx, |this, cx| {
            this.load_settings(cx)
        }));
        this
    }

    /// A view over precomputed data (gallery stories, tests): no store, no IO.
    pub fn with_loaded(
        loaded: Loaded,
        clock: Arc<dyn Clock>,
        settings: Settings,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(None, clock, settings, cx);
        this.art.current = Some(loaded.day.summary.mood.art());
        this.state = State::Ready(Arc::new(loaded));
        this
    }

    /// A view in one of the §3 states without data (stories, tests):
    /// `Loading` with its skeletons, or no account yet.
    pub fn placeholder(
        no_account: bool,
        clock: Arc<dyn Clock>,
        settings: Settings,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::empty(None, clock, settings, cx);
        if no_account {
            this.state = State::NoAccount;
        } else {
            this.skeleton = true;
        }
        this
    }

    /// Stories: the error banner over `loaded` (or alone).
    pub fn with_error(mut self, reason: impl Into<String>) -> Self {
        self.error = Some(reason.into());
        self
    }

    /// Stories: group reviewers shown (C1) without touching settings.
    pub fn with_group_reviewers(mut self, shown: bool) -> Self {
        self.groups_override = Some(shown);
        self
    }

    fn empty(
        store: Option<Store>,
        clock: Arc<dyn Clock>,
        settings: Settings,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            store,
            clock,
            settings,
            sync: SyncHealth::default(),
            state: State::Loading,
            error: None,
            skeleton: false,
            selected: None,
            pending_restore: None,
            scroll_restore: Rc::default(),
            groups_override: None,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            width: Rc::default(),
            rows: Rc::default(),
            art: Art::default(),
            own_generation: 0,
            _load: None,
            _save: None,
            _watch: None,
            _tick: None,
            _skeleton: None,
            _settings: None,
            _settings_load: None,
            _groups: crate::review::groups::observe(cx),
            row_menu: Default::default(),
            row_bounds: Default::default(),
            _pins: crate::pin_actions::observe(cx),
            // FR-31: a closed Today leaves no illustration decoded.
            _release: cx.on_release(|this, cx| {
                let art = &this.art;
                for mood in [art.current, art.previous, art.pending]
                    .into_iter()
                    .flatten()
                {
                    render::evict_art(mood, cx);
                }
            }),
        }
    }

    /// The running sync's state changed (statusbar watcher): rerun the pass
    /// when Today's data health (FR-10) changes.
    pub fn set_sync_state(&mut self, state: &SyncState, cx: &mut Context<Self>) {
        let sync = sync_health(state, self.clock.now(), cx);
        if sync == self.sync {
            return;
        }
        let now = self.clock.now();
        let health = |s: &SyncHealth| {
            let h = s.data_health(true, now);
            (h.hot_stale, h.warm_stale, h.stale, h.work_scope)
        };
        let changed = health(&sync) != health(&self.sync);
        self.sync = sync;
        if changed {
            self.reload(cx);
        }
    }

    /// Read `settings.toml [nudges]` off the UI thread (Settings › Nudges),
    /// keep the machine's time zone, and rerun the pass.
    fn load_settings(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            self.reload(cx);
            return;
        };
        let job = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let doc = toml::from_str::<toml::Table>(&text).unwrap_or_default();
            crate::settings::model::parse_nudges(&doc).0
        });
        self._settings_load = Some(cx.spawn(async move |this, cx| {
            let mut settings = job.await;
            this.update(cx, |this, cx| {
                settings.hours.utc_offset_min = this.settings.hours.utc_offset_min;
                this.settings = settings;
                this.reload(cx);
            })
            .ok();
        }));
    }

    /// Runs a pass on the background executor and swaps in the result.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let settings = self.settings.clone();
        let clock = self.clock.clone();
        let sync = self.sync;
        let job =
            cx.background_spawn(async move { model::load(&store, &settings, &*clock, &sync) });
        self._load = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(Some((loaded, generation))) => {
                        this.own_generation = generation;
                        this.error = None;
                        this.show(loaded, cx);
                    }
                    Ok(None) => {
                        this.error = None;
                        this.state = State::NoAccount;
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// Stories and smoke runs: swap in a precomputed day as a pass would
    /// (selection kept by key, the mood's art crossfaded or swapped).
    pub fn set_loaded(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.error = None;
        self.show(loaded, cx);
        cx.notify();
    }

    /// Swap in a computed day: crossfade the art if the mood changed, keep
    /// the selection by key, apply a pending restore, and schedule the next
    /// time-driven recompute (FR-2).
    fn show(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        let art = loaded.day.summary.mood.art();
        match self.art.current {
            // The first day: nothing to swap from; gpui decodes as it paints.
            None => self.art.current = Some(art),
            Some(current) if current == art => {
                if let Some(stale) = self.art.pending.take() {
                    render::evict_art(stale, cx);
                }
                self.art._decode = None;
            }
            Some(_) if self.art.pending == Some(art) => {}
            Some(_) => self.decode_art(art, cx),
        }
        let next = loaded.day.summary.next_change;
        self.state = State::Ready(Arc::new(loaded));
        self.clamp_selection();
        if let Some((restore, asked)) = self.pending_restore.take()
            && asked.elapsed() < RESTORE_TTL
        {
            self.apply_restore(restore, cx);
        }
        let wait = (next - self.clock.now()).max(0) as u64;
        self._tick = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(wait) + Duration::from_millis(50))
                .await;
            this.update(cx, |this, cx| this.reload(cx)).ok();
        }));
    }

    /// Reruns the pass after store commits (debounced) and on a timer.
    fn watch(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let commits = store.subscribe();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<u64>();
        // `subscribe` hands out a blocking receiver; bridge it off the
        // executors. The thread ends at the first commit after the view drops.
        std::thread::Builder::new()
            .name("today-commits".into())
            .spawn(move || {
                while let Ok(generation) = commits.recv() {
                    if tx.unbounded_send(generation).is_err() {
                        break;
                    }
                }
            })
            .ok();
        self._watch = Some(cx.spawn(async move |this, cx| {
            loop {
                let timer = cx.background_executor().timer(PASS_INTERVAL);
                let next = futures::future::select(rx.next(), timer).await;
                let generation = match next {
                    futures::future::Either::Left((None, _)) => return,
                    futures::future::Either::Left((Some(generation), _)) => Some(generation),
                    futures::future::Either::Right(_) => None,
                };
                if generation.is_some() {
                    cx.background_executor().timer(COMMIT_DEBOUNCE).await;
                }
                let mut latest = generation;
                while let Ok(more) = rx.try_recv() {
                    latest = Some(more);
                }
                let go = this
                    .update(cx, |this, cx| {
                        if latest.is_none_or(|g| g > this.own_generation) {
                            this.reload(cx);
                        }
                    })
                    .is_ok();
                if !go {
                    return;
                }
            }
        }));
    }

    /// The day on screen, once a pass has landed (smoke runs, stories).
    pub fn loaded(&self) -> Option<&Loaded> {
        match &self.state {
            State::Ready(loaded) => Some(loaded),
            _ => None,
        }
    }

    /// Two columns at Today's last painted width (before the first paint,
    /// the window's width, recorded by `render`).
    fn wide(&self) -> bool {
        self.width.get().is_none_or(Layout::is_wide)
    }

    /// The rows on screen, in the arrangement last painted.
    pub fn current_layout(&self) -> Layout {
        self.layout(self.wide())
    }

    fn layout(&self, wide: bool) -> Layout {
        self.loaded()
            .map(|loaded| Layout::of(loaded, wide))
            .unwrap_or_default()
    }

    fn selected_row(&self, layout: &Layout) -> Option<Row> {
        let loaded = self.loaded()?;
        self.selected
            .and_then(|key| model::find(loaded, layout, key))
            .or_else(|| layout.first())
    }

    /// Keep the selection on an existing row: its key if it still shows,
    /// else the first row.
    fn clamp_selection(&mut self) {
        let layout = self.layout(true);
        let row = self.selected_row(&layout);
        self.selected = self
            .loaded()
            .zip(row)
            .and_then(|(loaded, row)| model::key(loaded, row));
    }

    /// The selected row (its key's row, else the first).
    pub fn current(&self) -> Option<Row> {
        self.selected_row(&self.layout(self.wide()))
    }

    pub fn pin_subject(&self) -> Option<adoken_model::PinSubject> {
        model::pin_subject(self.loaded()?, self.current()?)
    }

    pub fn pin_title(&self) -> gpui::SharedString {
        let Some(loaded) = self.loaded() else {
            return "".into();
        };
        match self.pin_subject() {
            Some(adoken_model::PinSubject::Pr(id)) => loaded
                .snapshot
                .prs
                .iter()
                .chain(&loaded.snapshot.merged_today)
                .find(|pr| pr.id == id)
                .map(|pr| pr.title.clone())
                .unwrap_or_default()
                .into(),
            Some(adoken_model::PinSubject::WorkItem(id)) => loaded
                .snapshot
                .work_items
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.title.clone())
                .unwrap_or_default()
                .into(),
            Some(adoken_model::PinSubject::Definition { project_id, id }) => loaded
                .snapshot
                .runs
                .iter()
                .find(|run| run.project_id == project_id && run.pipeline_id == id)
                .map(|run| run.pipeline_name.clone())
                .unwrap_or_default()
                .into(),
            None => "".into(),
        }
    }

    fn open_row_menu(
        &mut self,
        position: adoken_ui::action_menu::MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items = vec![
            crate::pin_actions::action("Open", Activate).keys("enter"),
            crate::pin_actions::menu_item(self.pin_subject(), self.pin_title(), cx),
        ];
        if self.selected_nudge().is_some() {
            items.extend([
                crate::pin_actions::action("Snooze until next workday", SnoozeNudge),
                crate::pin_actions::action("Mark done", MarkNudgeDone),
                crate::pin_actions::action("Not for this item", NotForThisItem),
                crate::pin_actions::action("Turn off nudge type", TurnOffNudgeType),
            ]);
        }
        self.row_menu.open(
            "Today",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this| &mut this.row_menu,
            window,
            cx,
        );
    }

    fn select(&mut self, row: Row, cx: &mut Context<Self>) {
        self.cancel_pending_restore();
        if let Some(loaded) = self.loaded() {
            self.selected = model::key(loaded, row);
        }
        self.scroll_into_view(row);
        cx.notify();
    }

    fn move_with(
        &mut self,
        cx: &mut Context<Self>,
        step: impl FnOnce(&Layout, Option<Row>, &HashMap<Row, f32>) -> Option<Row>,
    ) {
        let layout = self.layout(self.wide());
        let current = self.selected_row(&layout);
        let centers: HashMap<Row, f32> = self
            .rows
            .borrow()
            .iter()
            .map(|(row, bounds)| (*row, f32::from(bounds.center().y)))
            .collect();
        if let Some(row) = step(&layout, current, &centers) {
            self.select(row, cx);
        }
    }

    fn vertical(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.move_with(cx, |layout, current, _| {
            Some(layout.vertical(current?, delta))
        });
    }

    fn across(&mut self, right: bool, cx: &mut Context<Self>) {
        self.move_with(cx, |layout, current, centers| {
            Some(layout.across(current?, right, |row| centers.get(&row).copied()))
        });
    }

    fn activate(&mut self, row: Row, cx: &mut Context<Self>) {
        self.cancel_pending_restore();
        let Some(loaded) = self.loaded() else {
            return;
        };
        if let Some(subject) = model::pin_subject(loaded, row) {
            crate::pin_actions::mark_opened(subject, cx);
        }
        match model::target(loaded, row) {
            Some(Target::Pr(id)) => cx.emit(TodayEvent::OpenPr(id)),
            Some(Target::WorkItem(id)) => cx.emit(TodayEvent::OpenWorkItem(id)),
            Some(Target::Run(id)) => cx.emit(TodayEvent::OpenRun(id)),
            None => {}
        }
    }

    fn selected_nudge(&self) -> Option<adoken_nudge::Nudge> {
        model::nudge(self.loaded()?, self.current()?).cloned()
    }

    /// Applies a ledger change, re-derives now, persists in the background.
    /// `resolve` also marks the subject's inbox row done.
    fn triage(
        &mut self,
        resolve: Option<Subject>,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut adoken_nudge::Ledger, &Settings, i64),
    ) {
        self.cancel_pending_restore();
        let settings = self.settings.clone();
        let State::Ready(loaded) = &mut self.state else {
            return;
        };
        let loaded = Arc::make_mut(loaded);
        change(&mut loaded.ledger, &settings, loaded.now);
        loaded.refresh(&settings);
        let ledger = loaded.ledger.clone();
        let now = loaded.now;
        self.clamp_selection();
        cx.notify();
        let Some(store) = self.store.clone() else {
            return;
        };
        let job = cx.background_spawn(async move {
            adoken_nudge::store::save_ledger(&store, &ledger)?;
            if let Some(subject) = resolve {
                adoken_notify::persist::resolve_nudge(&store, &subject, now)?;
            }
            Ok::<_, adoken_store::Error>(store.generation())
        });
        self._save = Some(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| match result {
                Ok(generation) => this.own_generation = this.own_generation.max(generation),
                Err(error) => {
                    this.error = Some(format!("Couldn't save nudge state: {error}"));
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn dismiss_focus(&mut self, cx: &mut Context<Self>) {
        self.triage(None, cx, |ledger, settings, now| {
            ledger.dismiss_today(now, &settings.hours)
        });
    }

    fn snooze(&mut self, cx: &mut Context<Self>) {
        let Some(nudge) = self.selected_nudge() else {
            return;
        };
        self.triage(Some(nudge.subject.clone()), cx, |ledger, settings, now| {
            ledger.snooze(&nudge.subject, settings.hours.next_workday_start(now))
        });
    }

    fn mark_done(&mut self, cx: &mut Context<Self>) {
        let Some(nudge) = self.selected_nudge() else {
            return;
        };
        self.triage(Some(nudge.subject.clone()), cx, |ledger, _, _| {
            ledger.dismiss(&nudge.subject, nudge.kind)
        });
    }

    fn not_for_this_item(&mut self, cx: &mut Context<Self>) {
        let Some(nudge) = self.selected_nudge() else {
            return;
        };
        self.triage(Some(nudge.subject.clone()), cx, |ledger, _, _| {
            ledger.not_for_this_item(&nudge.subject)
        });
    }

    /// FR-8 "Turn off this nudge type": saved to `settings.toml [nudges]`
    /// (Settings › Nudges turns it back on).
    fn turn_off_type(&mut self, cx: &mut Context<Self>) {
        let Some(nudge) = self.selected_nudge() else {
            return;
        };
        self.settings.set_enabled(nudge.kind, false);
        crate::settings::write_table(
            cx,
            crate::settings::model::NUDGES_TABLE,
            crate::settings::model::nudges_table(&self.settings),
        );
        self.triage(Some(nudge.subject.clone()), cx, |_, _, _| {});
    }

    /// FR-9: names only (N4); adoken never messages anyone.
    fn copy_names(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_restore();
        if let Some(nudge) = self.selected_nudge().filter(|n| !n.ping.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(nudge.ping.join(", ")));
        }
    }

    /// Retry from the error banner: "Today: Refresh".
    fn retry(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_restore();
        self.reload(cx);
    }
}

impl Render for TodayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        if self.width.get().is_none() {
            self.width
                .set(Some(f32::from(window.viewport_size().width)));
        }
        let wide = self.wide();
        let body = self.render_body(wide, window, cx);
        div()
            .id("today")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.colors.bg_surface)
            .text_color(theme.colors.text_primary)
            .type_style(typography::BODY, &theme)
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                crate::pin_actions::toggle(this.pin_subject(), this.pin_title(), window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &crate::pin_actions::OpenRowMenu, window, cx| {
                    this.open_row_menu(
                        adoken_ui::action_menu::keyboard_position(&this.row_bounds, window),
                        window,
                        cx,
                    );
                }),
            )
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.across(true, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.across(false, cx)))
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| this.vertical(1, cx)))
            .on_action(cx.listener(|this, _: &SelectUp, _, cx| this.vertical(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| {
                this.move_with(cx, |layout, _, _| layout.first())
            }))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.move_with(cx, |layout, _, _| layout.last())
            }))
            .on_action(cx.listener(|this, _: &Activate, _, cx| {
                if let Some(row) = this.current() {
                    this.activate(row, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &DismissFocus, _, cx| this.dismiss_focus(cx)))
            .on_action(cx.listener(|this, _: &SnoozeNudge, _, cx| this.snooze(cx)))
            .on_action(cx.listener(|this, _: &MarkNudgeDone, _, cx| this.mark_done(cx)))
            .on_action(cx.listener(|this, _: &NotForThisItem, _, cx| this.not_for_this_item(cx)))
            .on_action(cx.listener(|this, _: &TurnOffNudgeType, _, cx| this.turn_off_type(cx)))
            .on_action(cx.listener(|this, _: &CopyReviewerNames, _, cx| this.copy_names(cx)))
            .on_action(cx.listener(|this, _: &RefreshToday, _, cx| this.retry(cx)))
            .child(body)
            .children(self.row_menu.element())
    }
}
