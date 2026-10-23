//! Guided tours (spec 0020, docs/design/tours.md): a 3–5 step coach mark
//! tour per complex view, shown once on first use and replayed from `⌘P`.
//!
//! - [`defs`]: the five tours as static data (copy mirrors the design).
//! - [`trigger`]: the FR-2 first-use decision, pure.
//! - [`keys`]: key references rendered from the live keymap (FR-12).
//! - [`settings`]: `[tours] auto_start` / `seen` (FR-15).
//! - [`Tours`]: one per main window. It watches the shell, polls the
//!   trigger while the shown view has an unseen tour, runs the tour, and
//!   renders [`adoken_ui::tour`]'s layer (coach mark + spotlight).
//!
//! Input (FR-6): keyboard focus never leaves the view. A keystroke
//! interceptor handles the tour keys (`→` `←` `↩` `Space` `Tab` `⇧Tab`
//! `Esc`) and stops them; any other key ends the tour and continues down the
//! normal dispatch path untouched, so it does exactly what it would have
//! done with no tour. Pointer downs and scrolls outside the mark end it and
//! are delivered normally. `Next` holds the tour's own focus (drawn with
//! the focus ring; `↩`/`Space` activate it), and the mark is announced
//! through a polite live region (FR-18).
//!
//! Anchors record only while a tour shows, a replay or first use is
//! pending, or the palette is open (its commands' gates read them);
//! otherwise each anchor is one flag check (FR-11).

pub mod defs;
pub mod keys;
pub mod settings;
pub mod trigger;

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adoken_theme::motion;
use adoken_ui::tour::{self as ui, CoachButton, CoachMark, Recording, Segment};
use adoken_ui::{ActiveTheme, IconName, Toast, ToastStack};
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, Focusable, Global, KeystrokeEvent, Pixels,
    SharedString, Subscription, Task, WeakEntity, Window, actions, div, point, prelude::*, px,
};

use crate::help::CheatSheet;
use crate::palette::{Palette, PaletteCommand};
use crate::shell::{Nav, Shell};
pub use defs::TourId;
use settings::TourSettings;

actions!(
    tours,
    [
        /// Show the Today tour.
        TourToday,
        /// Show the Inbox tour.
        TourInbox,
        /// Show the pull request review tour.
        TourPullRequestReview,
        /// Show the work item tour.
        TourWorkItem,
        /// Show the command palette tour.
        TourCommandPalette,
        /// Show the tour of the view on screen.
        TourThisView,
        /// Show every tour again the first time its view opens.
        ShowToursAgain,
        /// Show tours the first time each view opens, or stop showing them.
        ToggleFirstUseTours,
    ]
);

pub const SHOW_AGAIN_TITLE: &str = "Tours: Show again on first use";
pub const TOGGLE_TITLE: &str = "First-use tours: turn on or off";
/// Settings › General's button toast (design §6).
pub const SHOW_AGAIN_TOAST: &str = "Tours will show again the first time you open each view";
pub const OPEN_PR_FIRST: &str = "Open a pull request first";
pub const SELECT_WORK_ITEM_FIRST: &str = "Select a work item first";
pub const NO_TOUR_HERE: &str = "No tour for this view";
pub const NOTHING_YET: &str = "Nothing to show yet";
/// Settings › General (spec 0020 FR-15).
pub const SETTING_TITLE: &str = "Show tours the first time you open a view";
pub const SETTING_KEYWORDS: &str = "tours tour guide walkthrough onboarding first use help";
pub const RESET_TITLE: &str = "Show tours again";

/// How often the first-use trigger is re-checked while it could fire.
const POLL: Duration = Duration::from_millis(250);
/// A replay waits this long for its view's anchors to paint.
const REPLAY_WAIT: Duration = Duration::from_secs(2);

/// Palette commands (FR-14, category Help).
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Tour: Today",
            keys: None,
            action: Box::new(TourToday),
        },
        PaletteCommand {
            title: "Tour: Inbox",
            keys: None,
            action: Box::new(TourInbox),
        },
        PaletteCommand {
            title: "Tour: Pull request review",
            keys: None,
            action: Box::new(TourPullRequestReview),
        },
        PaletteCommand {
            title: "Tour: Work item",
            keys: None,
            action: Box::new(TourWorkItem),
        },
        PaletteCommand {
            title: "Tour: Command palette",
            keys: None,
            action: Box::new(TourCommandPalette),
        },
        PaletteCommand {
            title: "Tour: This view",
            keys: None,
            action: Box::new(TourThisView),
        },
        PaletteCommand {
            title: SHOW_AGAIN_TITLE,
            keys: None,
            action: Box::new(ShowToursAgain),
        },
        PaletteCommand {
            title: TOGGLE_TITLE,
            keys: None,
            action: Box::new(ToggleFirstUseTours),
        },
    ]
}

/// The tour of a shell section (the review's only while a PR is open).
fn tour_for(nav: Nav, review_open: bool) -> Option<TourId> {
    match nav {
        Nav::Today => Some(TourId::Today),
        Nav::Inbox => Some(TourId::Inbox),
        Nav::PullRequests => review_open.then_some(TourId::Review),
        Nav::WorkItems => Some(TourId::WorkItem),
        Nav::Pipelines | Nav::Search => None,
    }
}

/// The key context the view's own focus sits in when nothing else (menu,
/// picker, editor, composer) owns the keyboard (FR-2 item 4).
fn quiet_contexts(tour: TourId) -> &'static [&'static str] {
    match tour {
        TourId::Review => &[crate::review::pr::KEY_CONTEXT],
        TourId::Today => &["Today"],
        TourId::Inbox => &["Inbox"],
        TourId::WorkItem => &[
            crate::workitems::detail::LIST_CONTEXT,
            crate::workitems::detail::DETAIL_CONTEXT,
        ],
        TourId::Palette => &[adoken_ui::text_input::KEY_CONTEXT],
    }
}

// ── App-wide state ──────────────────────────────────────────────────────

/// The post-onboarding hint shows (FR-2 item 1).
#[derive(Default)]
struct HintShowing(bool);

impl Global for HintShowing {}

/// The post-onboarding hint appeared or went away.
pub fn set_hint_showing(showing: bool, cx: &mut App) {
    cx.set_global(HintShowing(showing));
}

/// The main window's tours (commands and gates act on it).
struct Active(WeakEntity<Tours>);

impl Global for Active {}

fn active(cx: &App) -> Option<Entity<Tours>> {
    cx.try_global::<Active>()?.0.upgrade()
}

#[derive(Clone, Copy)]
enum Command {
    Replay(TourId),
    ThisView,
    ShowAgain,
    ToggleAutoStart,
}

/// Run a palette command on the main window's tours, after the dispatch
/// that ran it (the window is free again then).
fn run(command: Command, cx: &mut App) {
    cx.defer(move |cx| {
        let Some(tours) = active(cx) else {
            return;
        };
        let handle = tours.read(cx).window;
        handle
            .update(cx, |_, window, cx| {
                tours.update(cx, |tours, cx| match command {
                    Command::Replay(tour) => tours.replay(tour, window, cx),
                    Command::ThisView => {
                        if let Some(tour) = tours.view_tour(cx) {
                            tours.replay(tour, window, cx)
                        }
                    }
                    Command::ShowAgain => tours.show_again(cx),
                    Command::ToggleAutoStart => tours.toggle_auto_start(cx),
                })
            })
            .ok();
    });
}

/// The command handlers and palette gates; once per app.
fn register(cx: &mut App) {
    #[derive(Default)]
    struct Registered;
    impl Global for Registered {}
    if cx.has_global::<Registered>() {
        return;
    }
    cx.set_global(Registered);
    cx.on_action(|_: &TourToday, cx| run(Command::Replay(TourId::Today), cx));
    cx.on_action(|_: &TourInbox, cx| run(Command::Replay(TourId::Inbox), cx));
    cx.on_action(|_: &TourPullRequestReview, cx| run(Command::Replay(TourId::Review), cx));
    cx.on_action(|_: &TourWorkItem, cx| run(Command::Replay(TourId::WorkItem), cx));
    cx.on_action(|_: &TourCommandPalette, cx| run(Command::Replay(TourId::Palette), cx));
    cx.on_action(|_: &TourThisView, cx| run(Command::ThisView, cx));
    cx.on_action(|_: &ShowToursAgain, cx| run(Command::ShowAgain, cx));
    cx.on_action(|_: &ToggleFirstUseTours, cx| run(Command::ToggleAutoStart, cx));

    use crate::palette::gates;
    gates::register(&TourToday, |cx| gate(TourId::Today, cx), cx);
    gates::register(&TourInbox, |cx| gate(TourId::Inbox, cx), cx);
    gates::register(&TourPullRequestReview, |cx| gate(TourId::Review, cx), cx);
    gates::register(&TourWorkItem, |cx| gate(TourId::WorkItem, cx), cx);
    gates::register(
        &TourThisView,
        |cx| {
            let tours = active(cx)?;
            match tours.read(cx).view_tour(cx) {
                Some(tour) => gate(tour, cx),
                None => Some(NO_TOUR_HERE.into()),
            }
        },
        cx,
    );
}

/// Why "Tour: <tour>" can't run now (FR-14 preconditions, §3 States).
fn gate(tour: TourId, cx: &App) -> Option<SharedString> {
    let tours = active(cx)?;
    let tours = tours.read(cx);
    let shell = tours.shell.as_ref()?.upgrade()?;
    let shell = shell.read(cx);
    match tour {
        TourId::Review if shell.open_review_id(cx).is_none() => return Some(OPEN_PR_FIRST.into()),
        TourId::WorkItem
            if !matches!(
                shell
                    .work_items()
                    .and_then(|view| view.read(cx).pin_subject()),
                Some(adoken_model::PinSubject::WorkItem(_))
            ) =>
        {
            return Some(SELECT_WORK_ITEM_FIRST.into());
        }
        _ => {}
    }
    // On screen behind the palette with nothing to point at (inbox zero,
    // Today before its first sync).
    let shown = tour_for(shell.nav(), shell.open_review_id(cx).is_some()) == Some(tour);
    (shown && tours.visible_steps(tour, cx).is_empty()).then(|| NOTHING_YET.into())
}

// ── The engine ──────────────────────────────────────────────────────────

/// The shell parts tours watch.
pub struct ShellParts {
    pub shell: WeakEntity<Shell>,
    pub palette: Entity<Palette>,
    pub cheat_sheet: Entity<CheatSheet>,
}

struct Running {
    tour: TourId,
    /// Indices into the tour's steps whose anchor was visible at start.
    steps: Vec<usize>,
    at: usize,
    focused: CoachButton,
    /// Keys the entrance animation; new per step.
    key: u64,
    progress: Rc<Cell<f32>>,
    body: Vec<Segment>,
}

struct Exiting {
    mark: Running,
    since: Instant,
}

struct Pending {
    tour: TourId,
    /// Stories: start on this step of the definition.
    step: Option<usize>,
    until: Option<Instant>,
}

pub struct Tours {
    window: AnyWindowHandle,
    shell: Option<WeakEntity<Shell>>,
    palette: Option<Entity<Palette>>,
    cheat_sheet: Option<Entity<CheatSheet>>,
    /// Titlebar and statusbar heights (FR-8 content area).
    insets: (f32, f32),
    /// The content area as last rendered (anchor visibility, FR-8).
    painted: Bounds<Pixels>,
    settings: TourSettings,
    running: Option<Running>,
    exiting: Option<Exiting>,
    pending: Option<Pending>,
    last_input: Option<Instant>,
    /// When every gate but time last opened for this tour (FR-2 item 6).
    ready_since: Option<(TourId, Instant)>,
    last_ended: Option<Instant>,
    /// Where the last painted frame put the coach mark (entering, shown or
    /// fading out); `None` when no mark painted.
    placed: Rc<Cell<Option<Bounds<Pixels>>>>,
    recording: Option<Recording>,
    shown: u64,
    poll: Option<Task<()>>,
    _settings_read: Option<Task<()>>,
    _exit: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Tours {
    /// The main window's tours, watching `parts`.
    pub fn for_shell(
        parts: ShellParts,
        shell: &Entity<Shell>,
        insets: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::new(insets, window, cx);
        this._subscriptions.extend([
            cx.observe_in(shell, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
            cx.observe_in(&parts.palette, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
            cx.observe_in(&parts.cheat_sheet, window, |this, _, window, cx| {
                this.refresh(window, cx)
            }),
            crate::settings::observe(cx, |this, cx| this.load_settings(cx)),
        ]);
        this.shell = Some(parts.shell);
        this.palette = Some(parts.palette);
        this.cheat_sheet = Some(parts.cheat_sheet);
        cx.set_global(Active(cx.entity().downgrade()));
        this.load_settings(cx);
        this
    }

    /// Tours over a bare view (gallery stories): no shell, no first use,
    /// the content area is the whole window.
    pub fn new(insets: (f32, f32), window: &mut Window, cx: &mut Context<Self>) -> Self {
        register(cx);
        let handle = window.window_handle();
        let this = cx.entity().downgrade();
        let keys = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != handle {
                return;
            }
            this.update(cx, |tours, cx| tours.on_key(event, window, cx))
                .ok();
        });
        Self {
            window: handle,
            shell: None,
            palette: None,
            cheat_sheet: None,
            insets,
            painted: Bounds::default(),
            settings: TourSettings::default(),
            running: None,
            exiting: None,
            pending: None,
            last_input: None,
            ready_since: None,
            last_ended: None,
            placed: Rc::default(),
            recording: None,
            shown: 0,
            poll: None,
            _settings_read: None,
            _exit: None,
            _subscriptions: vec![keys],
        }
    }

    pub fn settings(&self) -> &TourSettings {
        &self.settings
    }

    /// The tour showing and its step (0-based, among visible steps) and
    /// count; for tests and smoke runs.
    pub fn showing(&self) -> Option<(TourId, usize, usize)> {
        self.running.as_ref().map(|r| (r.tour, r.at, r.steps.len()))
    }

    /// The showing step's title.
    pub fn step_title(&self) -> Option<&'static str> {
        let r = self.running.as_ref()?;
        Some(r.tour.tour().steps[r.steps[r.at]].title)
    }

    /// The coach mark button `↩` would press.
    pub fn focused_button(&self) -> Option<CoachButton> {
        self.running.as_ref().map(|r| r.focused)
    }

    /// The coach mark's bounds in the last painted frame (`None` when no
    /// mark painted); for smoke runs (motion, placement).
    pub fn mark_bounds(&self) -> Option<Bounds<Pixels>> {
        self.placed.get()
    }

    /// Stories: show `tour` on its step `step` (0-based in the definition)
    /// once the anchors paint, ignoring every gate.
    pub fn show_step(&mut self, tour: TourId, step: usize, cx: &mut Context<Self>) {
        self.pending = Some(Pending {
            tour,
            step: Some(step),
            until: None,
        });
        self.ensure_recording(true, cx);
    }

    fn now(cx: &App) -> Instant {
        cx.background_executor().now()
    }

    fn content(&self, window: &Window) -> Bounds<Pixels> {
        let viewport = window.viewport_size();
        let (top, bottom) = self.insets;
        Bounds::new(
            point(px(0.0), px(top)),
            gpui::size(
                viewport.width,
                (viewport.height - px(top) - px(bottom)).max(px(0.0)),
            ),
        )
    }

    // ── settings ────────────────────────────────────────────────────────

    /// Re-read `[tours]` off the UI thread (launch, writes, outside edits).
    fn load_settings(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        let read = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            TourSettings::from_toml(&text)
        });
        self._settings_read = Some(cx.spawn(async move |this, cx| {
            let settings = read.await;
            this.update(cx, |this, cx| {
                // A write still queued is newer than the file.
                if crate::settings::is_writing(cx) {
                    return;
                }
                if this.settings != settings {
                    this.settings = settings;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn write_settings(&mut self, cx: &mut Context<Self>) {
        crate::settings::write_table(cx, settings::TABLE, self.settings.to_table());
        cx.notify();
    }

    fn show_again(&mut self, cx: &mut Context<Self>) {
        self.settings.seen.clear();
        self.write_settings(cx);
        ToastStack::show(
            Toast::new("tours-show-again", IconName::Info, SHOW_AGAIN_TOAST),
            cx,
        );
    }

    fn toggle_auto_start(&mut self, cx: &mut Context<Self>) {
        self.settings.auto_start = !self.settings.auto_start;
        self.write_settings(cx);
    }

    // ── what's on screen ────────────────────────────────────────────────

    /// The tour of the section on screen (behind the palette, if open).
    fn view_tour(&self, cx: &App) -> Option<TourId> {
        let shell = self.shell.as_ref()?.upgrade()?;
        let shell = shell.read(cx);
        tour_for(shell.nav(), shell.open_review_id(cx).is_some())
    }

    fn palette_visible(&self, cx: &App) -> bool {
        self.palette
            .as_ref()
            .is_some_and(|palette| palette.read(cx).is_visible())
    }

    fn cheat_sheet_visible(&self, cx: &App) -> bool {
        self.cheat_sheet
            .as_ref()
            .is_some_and(|sheet| sheet.read(cx).is_visible())
    }

    /// The tour that could start by itself now (FR-2 item 3's view).
    fn candidate(&self, cx: &App) -> Option<TourId> {
        if self.palette_visible(cx) {
            let palette = self.palette.as_ref()?.read(cx);
            return palette.query_is_empty(cx).then_some(TourId::Palette);
        }
        if self.cheat_sheet_visible(cx) {
            return None;
        }
        self.view_tour(cx)
    }

    fn may_auto_start(&self, tour: TourId) -> bool {
        self.settings.auto_start && !self.settings.is_seen(tour.id())
    }

    /// Steps of `tour` whose anchor painted visibly in the last frame.
    fn visible_steps(&self, tour: TourId, cx: &App) -> Vec<usize> {
        let anchors = ui::anchors(self.window.window_id(), cx);
        tour.tour()
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| ui::visible(&anchors, step.anchor, self.painted).is_some())
            .map(|(ix, _)| ix)
            .collect()
    }

    /// Something other than the tour's view owns the keyboard (FR-2 item 4).
    fn overlay(&self, tour: TourId, window: &Window, cx: &App) -> bool {
        if crate::ken::ko_showing(cx) || self.cheat_sheet_visible(cx) {
            return true;
        }
        if (tour == TourId::Palette) != self.palette_visible(cx) {
            return true;
        }
        let focus = window.context_stack();
        let innermost_ok = focus.last().is_some_and(|context| {
            quiet_contexts(tour)
                .iter()
                .any(|name| context.contains(name))
        });
        let in_view = match tour {
            TourId::Palette => focus.iter().any(|c| c.contains("Palette")),
            _ => self
                .shell
                .as_ref()
                .and_then(|shell| shell.upgrade())
                .is_some_and(|shell| shell.read(cx).content_has_focus(window, cx)),
        };
        !(innermost_ok && in_view)
    }

    /// Pull requests shows with the review tour still to auto-start: a PR
    /// opens inside the review view without the shell noticing, so the
    /// poll keeps watching for it (FR-2 item 3).
    fn review_may_open(&self, cx: &App) -> bool {
        self.may_auto_start(TourId::Review)
            && !self.palette_visible(cx)
            && !self.cheat_sheet_visible(cx)
            && self
                .shell
                .as_ref()
                .and_then(|shell| shell.upgrade())
                .is_some_and(|shell| shell.read(cx).nav() == Nav::PullRequests)
    }

    // ── lifecycle ───────────────────────────────────────────────────────

    /// Recording, polling and ending follow what's on screen. Bare tours
    /// (stories) watch no shell or palette, so nothing on screen ends them.
    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shell.is_some()
            && let Some(running) = &self.running
        {
            let tour = running.tour;
            let gone = match tour {
                TourId::Palette => !self.palette_visible(cx),
                _ => {
                    self.palette_visible(cx)
                        || self.cheat_sheet_visible(cx)
                        || self.view_tour(cx) != Some(tour)
                }
            };
            if gone {
                self.end(window, cx);
            }
        }
        let candidate = self.candidate(cx).filter(|tour| self.may_auto_start(*tour));
        if candidate.is_none() {
            self.ready_since = None;
        }
        let wanted = self.running.is_some()
            || self.exiting.is_some()
            || self.pending.is_some()
            || candidate.is_some()
            || self.palette_visible(cx);
        self.ensure_recording(wanted, cx);
        let poll = self.running.is_some() || candidate.is_some() || self.review_may_open(cx);
        match (poll, self.poll.is_some()) {
            (true, false) => {
                self.poll = Some(cx.spawn_in(window, async move |this, cx| {
                    loop {
                        cx.background_executor().timer(POLL).await;
                        let alive = this
                            .update_in(cx, |this, window, cx| this.tick(window, cx))
                            .is_ok();
                        if !alive {
                            return;
                        }
                    }
                }));
            }
            (false, true) => self.poll = None,
            _ => {}
        }
    }

    fn ensure_recording(&mut self, on: bool, cx: &mut Context<Self>) {
        match (on, self.recording.is_some()) {
            (true, false) => {
                self.recording = Some(ui::record(cx));
                // Paint once with recording on, so the anchors exist.
                cx.notify();
            }
            (false, true) => {
                self.recording = None;
                cx.notify();
            }
            _ => {}
        }
    }

    /// The first-use trigger (FR-2), every [`POLL`].
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh(window, cx);
        if self.running.is_some() || self.pending.is_some() {
            return;
        }
        let Some(tour) = self.candidate(cx) else {
            self.ready_since = None;
            return;
        };
        let now = Self::now(cx);
        let mut snapshot = trigger::Snapshot {
            auto_start: self.settings.auto_start,
            seen: self.settings.is_seen(tour.id()),
            hint: cx.try_global::<HintShowing>().is_some_and(|h| h.0),
            overlay: self.overlay(tour, window, cx),
            window_active: window.is_window_active(),
            since_input: None,
            since_tour_ended: self.last_ended.map(|t| now.saturating_duration_since(t)),
            visible_steps: self.visible_steps(tour, cx).len(),
            idle: if tour == TourId::Palette {
                Duration::ZERO
            } else {
                trigger::IDLE
            },
        };
        if matches!(
            trigger::decide(&snapshot),
            trigger::Decision::Never | trigger::Decision::Blocked
        ) {
            self.ready_since = None;
            return;
        }
        // FR-2 item 6 counts from the later of the last input and the
        // moment every other gate opened (the view shown, an overlay
        // closed, the hint gone): "starts 1 s after it closes" (AC-2).
        let ready = match self.ready_since {
            Some((ready_tour, at)) if ready_tour == tour => at,
            _ => {
                self.ready_since = Some((tour, now));
                now
            }
        };
        let quiet = self.last_input.map_or(ready, |input| input.max(ready));
        snapshot.since_input = Some(now.saturating_duration_since(quiet));
        if trigger::decide(&snapshot) == trigger::Decision::Start {
            self.start(tour, None, false, window, cx);
        }
    }

    /// `⌘P` › "Tour: …" (FR-14): switch to the view, then start once its
    /// anchors paint; ignores `seen`, `auto_start` and the cooldown.
    fn replay(&mut self, tour: TourId, window: &mut Window, cx: &mut Context<Self>) {
        if self.running.is_some() {
            self.end(window, cx);
        }
        if let Some(shell) = self.shell.as_ref().and_then(|s| s.upgrade()) {
            let nav = match tour {
                TourId::Today => Some(Nav::Today),
                TourId::Inbox => Some(Nav::Inbox),
                TourId::Review => Some(Nav::PullRequests),
                TourId::WorkItem => Some(Nav::WorkItems),
                TourId::Palette => None,
            };
            match nav {
                Some(nav) => shell.update(cx, |shell, cx| {
                    if shell.nav() != nav {
                        shell.show(nav, window, cx);
                    }
                }),
                None => {
                    if let Some(palette) = &self.palette {
                        palette.update(cx, |palette, cx| palette.open_with("", window, cx));
                    }
                }
            }
        }
        self.pending = Some(Pending {
            tour,
            step: None,
            until: Some(Self::now(cx) + REPLAY_WAIT),
        });
        self.ensure_recording(true, cx);
        cx.notify();
    }

    /// Show `tour` on its visible steps (≥ 2 for first use, ≥ 1 for a
    /// replay) once a frame painted with no view still loading what its
    /// anchors cover (the palette's first results); first use marks it
    /// seen (FR-3).
    fn start(
        &mut self,
        tour: TourId,
        step: Option<usize>,
        replay: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !ui::settled(self.window.window_id(), cx) {
            return false;
        }
        let steps = self.visible_steps(tour, cx);
        let needed = if replay {
            1
        } else {
            trigger::MIN_VISIBLE_STEPS
        };
        if steps.len() < needed {
            return false;
        }
        if self.running.is_some() {
            self.end(window, cx);
        }
        let at = step
            .and_then(|step| steps.iter().position(|s| *s >= step))
            .unwrap_or(0);
        self.shown += 1;
        let mut running = Running {
            tour,
            steps,
            at,
            focused: CoachButton::Next,
            key: self.shown,
            progress: Rc::new(Cell::new(0.0)),
            body: Vec::new(),
        };
        running.body = self.body(&running, cx);
        self.running = Some(running);
        self.exiting = None;
        if !replay && self.settings.mark_seen(tour.id()) {
            self.write_settings(cx);
        }
        self.refresh(window, cx);
        cx.notify();
        true
    }

    fn body(&self, running: &Running, cx: &App) -> Vec<Segment> {
        let def = running.tour.tour();
        let step = &def.steps[running.steps[running.at]];
        let read_only = match running.tour {
            TourId::Review => !crate::review::writes::access(cx).can_write(),
            TourId::WorkItem => matches!(
                crate::workitems::writes::access(cx),
                crate::workitems::writes::Access::Remote {
                    can_write: false,
                    ..
                }
            ),
            _ => false,
        };
        let mut body = step.read_only.filter(|_| read_only).unwrap_or(step.body);
        if running.tour == TourId::Today && step.anchor == crate::today::anchors::UP_NEXT {
            // Single-column Today: no columns to switch (design §5.5).
            if self.today_single_column(cx) {
                body = TODAY_UP_NEXT_NARROW;
            }
        }
        let contexts = keys::contexts(def.contexts);
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keys::segments(body, |action, documented| {
            keys::binding(keymap.bindings(), action, documented, &contexts)
        })
    }

    fn today_single_column(&self, cx: &App) -> bool {
        let anchors = ui::anchors(self.window.window_id(), cx);
        let left = |id: &str| {
            anchors
                .iter()
                .find(|(anchor, _)| *anchor == id)
                .map(|(_, rect)| f32::from(rect.bounds.left()))
        };
        match (
            left(crate::today::anchors::PRS),
            left(crate::today::anchors::UP_NEXT),
        ) {
            (Some(a), Some(b)) => (a - b).abs() < 1.0,
            _ => false,
        }
    }

    /// Move `delta` steps; past the last one is Done.
    fn step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        let target = running.at as isize + delta;
        if target < 0 {
            return;
        }
        if target as usize >= running.steps.len() {
            self.end(window, cx);
            return;
        }
        self.go_to(target as usize, cx);
    }

    fn go_to(&mut self, at: usize, cx: &mut Context<Self>) {
        self.shown += 1;
        let key = self.shown;
        let Some(running) = self.running.as_mut() else {
            return;
        };
        running.at = at;
        running.focused = CoachButton::Next;
        running.key = key;
        running.progress = Rc::new(Cell::new(0.0));
        let running = self.running.take().expect("running");
        let body = self.body(&running, cx);
        self.running = Some(Running { body, ..running });
        cx.notify();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.running.as_ref().map(|r| r.focused) {
            Some(CoachButton::Back) => self.step(-1, window, cx),
            Some(CoachButton::Next) => self.step(1, window, cx),
            Some(CoachButton::Skip) => self.end(window, cx),
            None => {}
        }
    }

    /// End the tour (FR-5): any end counts as seen (Q-5, already written
    /// at start), starts the cooldown, fades out unless reduce motion.
    fn end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = self.running.take() else {
            return;
        };
        let now = Self::now(cx);
        self.last_ended = Some(now);
        if !cx.reduce_motion() {
            self.exiting = Some(Exiting {
                mark: running,
                since: Instant::now(),
            });
            self._exit = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(adoken_ui::motion::ms(motion::QUICK_EXIT))
                    .await;
                this.update(cx, |this, cx| {
                    this.exiting = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        // Focus never left the view; if what had it is gone, the view's
        // main element takes it.
        if window.focused(cx).is_none()
            && let Some(shell) = self.shell.as_ref().and_then(|s| s.upgrade())
        {
            let handle = shell.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
        self.refresh(window, cx);
        cx.notify();
    }

    // ── input ───────────────────────────────────────────────────────────

    /// FR-6 / FR-7: tour keys act and stop; anything else ends the tour
    /// and keeps going down the normal dispatch path.
    fn on_key(&mut self, event: &KeystrokeEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.last_input = Some(Self::now(cx));
        let Some(running) = &self.running else {
            return;
        };
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let plain = !m.modified();
        let palette = running.tour == TourId::Palette;
        let first = running.at == 0;
        let focused = running.focused;
        match keystroke.key.as_str() {
            "right" if plain => self.step(1, window, cx),
            "left" if plain => self.step(-1, window, cx),
            "enter" if plain => self.activate(window, cx),
            // In the palette a space is query text (FR-7).
            "space" if plain && !palette => self.activate(window, cx),
            "tab" if !m.control && !m.alt && !m.platform && !m.function => {
                if let Some(running) = self.running.as_mut() {
                    running.focused = focused.cycle(!m.shift, first);
                    cx.notify();
                }
            }
            "escape" if plain => self.end(window, cx),
            _ => {
                self.end(window, cx);
                return;
            }
        }
        cx.stop_propagation();
    }

    fn on_pointer(&mut self, outside: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.last_input = Some(Self::now(cx));
        if outside && self.running.is_some() {
            self.end(window, cx);
        }
    }

    /// After each painted frame while a tour shows or is pending: follow
    /// hidden anchors (FR-9) and start pending tours.
    fn after_frame(&mut self, placed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(running) = &self.running
            && !placed
        {
            let visible = self.visible_steps(running.tour, cx);
            let next = running.steps[running.at + 1..]
                .iter()
                .position(|step| visible.contains(step))
                .map(|offset| running.at + 1 + offset);
            match next {
                Some(at) => self.go_to(at, cx),
                None => self.end(window, cx),
            }
            return;
        }
        let Some(pending) = &self.pending else {
            return;
        };
        let (tour, step, until) = (pending.tour, pending.step, pending.until);
        if self.start(tour, step, true, window, cx) {
            self.pending = None;
            return;
        }
        if until.is_some_and(|until| Self::now(cx) >= until) {
            self.pending = None;
            ToastStack::show(Toast::new("tours-nothing", IconName::Info, NOTHING_YET), cx);
            self.refresh(window, cx);
        }
    }

    fn mark(
        &self,
        running: &Running,
        exiting: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> CoachMark {
        let def = running.tour.tour();
        let step = &def.steps[running.steps[running.at]];
        // The footer's `?`, like a body key: live binding, else documented.
        let shortcuts = {
            let keymap = cx.key_bindings();
            let keymap = keymap.borrow();
            let contexts = keys::contexts(&["Workspace"]);
            keys::segments(SHORTCUTS_KEY, |action, documented| {
                keys::binding(keymap.bindings(), action, documented, &contexts)
            })
            .into_iter()
            .find_map(|segment| match segment {
                Segment::Key { label, .. } => Some(label),
                _ => None,
            })
        };
        let width = ui::width_for(
            f32::from(window.viewport_size().width),
            f32::from(window.rem_size()),
        );
        let mut mark = CoachMark::new(
            "coach-mark",
            running.tour.name(),
            step.title,
            running.at + 1,
            running.steps.len(),
        )
        .body(running.body.clone())
        .focused(running.focused)
        .width(width)
        .exiting(exiting);
        if let Some(keys) = shortcuts {
            mark = mark.shortcuts(keys);
        }
        if exiting {
            return mark;
        }
        let this = cx.entity().downgrade();
        let back = this.clone();
        let skip = this.clone();
        mark.entrance(("coach-mark-in", running.key), running.progress.clone())
            .on_back(move |window, cx| {
                back.update(cx, |t, cx| t.step(-1, window, cx)).ok();
            })
            .on_next(move |window, cx| {
                this.update(cx, |t, cx| t.step(1, window, cx)).ok();
            })
            .on_skip(move |window, cx| {
                skip.update(cx, |t, cx| t.end(window, cx)).ok();
            })
    }
}

/// Up next's body in single-column Today (no columns to switch).
const TODAY_UP_NEXT_NARROW: &str =
    "The one thing worth doing now. [today::DismissFocus|e] hides it until tomorrow.";

/// The footer hint's key (`help::QUESTION_KEYS`), as a body reference.
const SHORTCUTS_KEY: &str = "[help::ToggleShortcuts|?]";

impl Render for Tours {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.recording.is_none() {
            self.placed.set(None);
            return div().into_any_element();
        }
        let content = self.content(window);
        self.painted = content;
        let mut layer = ui::layer(content);
        let shown = match (&self.running, &self.exiting) {
            (Some(running), _) => Some((running, false, 1.0)),
            (None, Some(exiting)) => {
                let t = exiting.since.elapsed().as_secs_f32() * 1000.0 / motion::QUICK_EXIT as f32;
                Some((&exiting.mark, true, (1.0 - t).clamp(0.0, 1.0)))
            }
            _ => None,
        };
        if let Some((running, exiting, alpha)) = shown {
            let step = &running.tour.tour().steps[running.steps[running.at]];
            let mark = self.mark(running, exiting, window, cx);
            let colors = &cx.theme().colors;
            let mut outline = colors.border_focus;
            outline.a *= alpha;
            layer = layer
                .show(
                    step.anchor,
                    step.side,
                    mark,
                    outline,
                    (colors.bg_overlay, colors.border_default),
                )
                .progress(running.progress.clone());
        }
        let this = cx.entity().downgrade();
        layer = layer.on_pointer(move |outside, window, cx| {
            this.update(cx, |t, cx| t.on_pointer(outside, window, cx))
                .ok();
        });
        let follow = self.running.is_some() || self.pending.is_some();
        let this = cx.entity().downgrade();
        let placed_out = self.placed.clone();
        layer = layer.on_frame(move |frame, window, cx| {
            let placed = frame.placed.map(|placement| placement.bounds);
            placed_out.set(placed);
            if follow {
                window.defer(cx, move |window, cx| {
                    this.update(cx, |t, cx| t.after_frame(placed.is_some(), window, cx))
                        .ok();
                });
            }
        });
        div().child(layer.into_deferred()).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complex_views_have_a_tour() {
        assert_eq!(tour_for(Nav::Today, false), Some(TourId::Today));
        assert_eq!(tour_for(Nav::Inbox, false), Some(TourId::Inbox));
        assert_eq!(tour_for(Nav::PullRequests, false), None, "the PR list");
        assert_eq!(tour_for(Nav::PullRequests, true), Some(TourId::Review));
        assert_eq!(tour_for(Nav::WorkItems, false), Some(TourId::WorkItem));
        assert_eq!(tour_for(Nav::Pipelines, true), None);
        assert_eq!(tour_for(Nav::Search, false), None);
    }
}
