//! Pins (spec 0014): the user's working set, one keystroke away. This module
//! owns the pin list's live state ([`Pins`], one per window, installed by the
//! shell once the store is open), the headline rules ([`status`]), the
//! off-thread status snapshot ([`snapshot`]) and the glyph mapping
//! ([`render`]). The shell paints the sidebar group, rail and peek
//! (`crate::shell::pinned`); views resolve `⌘D` to their subject and call
//! [`Pins::toggle`].
//!
//! Writes and snapshot reads run on the background executor. Every change
//! is applied to the in-memory snapshot first, so the sidebar shows it on
//! the next frame (spec §7); the commit then refreshes the snapshot.

pub mod render;
pub mod snapshot;
pub mod status;

use std::sync::Arc;
use std::time::Duration;

use adoken_model::{MAX_PINS, Millis, Pin, PinSubject, RunResult, RunStatus};
use adoken_store::write::PinOutcome;
use adoken_store::{Store, read, write};
use adoken_ui::{IconName, Toast, ToastAction, ToastEvent, ToastStack, ToastTone};
use gpui::{
    App, AppContext, Context, Entity, EventEmitter, Global, KeyBinding, SharedString, Subscription,
    Task, Window, actions,
};

use crate::palette::{PaletteCommand, now_ms};
use snapshot::{Options, Snapshot};

actions!(
    pins,
    [
        /// Pin or unpin the current item.
        TogglePin,
        /// Open the palette on your pins.
        OpenPinsPalette,
        /// Move the pin under the cursor up.
        MovePinUp,
        /// Move the pin under the cursor down.
        MovePinDown,
        /// Undo the last unpin.
        UndoUnpin,
        /// Rename the pin under the cursor.
        RenamePin,
    ]
);

pub const TOGGLE_KEYS: &str = "secondary-d";
pub const PALETTE_KEYS: &str = "secondary-j";
pub const UNDO_KEYS: &str = "secondary-z";
pub const MOVE_UP_KEYS: &str = "alt-secondary-up";
pub const MOVE_DOWN_KEYS: &str = "alt-secondary-down";
/// `⌘Z` restores an unpin wherever typing isn't going into a text field.
const UNDO_CONTEXT: &str = "Workspace && !TextInput";
/// The shell's sidebar list (sections, the Pinned header, pins).
pub const SIDEBAR_CONTEXT: &str = "Sidebar";
/// Elapsed times, staleness and auto-unpin age without store commits.
const MINUTE_TICK: Duration = Duration::from_secs(60);
/// `settings.toml` table.
pub const SETTINGS_TABLE: &str = "pins";

pub fn bindings() -> Vec<KeyBinding> {
    let sidebar = Some(SIDEBAR_CONTEXT);
    vec![
        KeyBinding::new(TOGGLE_KEYS, TogglePin, None),
        KeyBinding::new(PALETTE_KEYS, OpenPinsPalette, None),
        KeyBinding::new(UNDO_KEYS, UndoUnpin, Some(UNDO_CONTEXT)),
        KeyBinding::new(MOVE_UP_KEYS, MovePinUp, sidebar),
        KeyBinding::new(MOVE_DOWN_KEYS, MovePinDown, sidebar),
    ]
}

/// Palette commands (FR-2): every pin action is reachable from `⌘P`.
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Pin current item",
            keys: Some(TOGGLE_KEYS),
            action: Box::new(TogglePin),
        },
        PaletteCommand {
            title: "Open pins",
            keys: Some(PALETTE_KEYS),
            action: Box::new(OpenPinsPalette),
        },
        PaletteCommand {
            title: "Unpin…",
            keys: None,
            action: Box::new(crate::palette::OpenUnpinPalette),
        },
        PaletteCommand {
            title: "Undo unpin",
            keys: Some(UNDO_KEYS),
            action: Box::new(UndoUnpin),
        },
        PaletteCommand {
            title: "Move pin up",
            keys: Some(MOVE_UP_KEYS),
            action: Box::new(MovePinUp),
        },
        PaletteCommand {
            title: "Move pin down",
            keys: Some(MOVE_DOWN_KEYS),
            action: Box::new(MovePinDown),
        },
        PaletteCommand {
            title: "Rename pin…",
            keys: None,
            action: Box::new(RenamePin),
        },
    ]
}

/// What can be pinned (FR-1); a run pins its definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinKind {
    PullRequest,
    WorkItem,
    Definition,
}

impl PinKind {
    pub fn of(subject: &PinSubject) -> PinKind {
        match subject {
            PinSubject::Pr(_) => PinKind::PullRequest,
            PinSubject::WorkItem(_) => PinKind::WorkItem,
            PinSubject::Definition { .. } => PinKind::Definition,
        }
    }

    /// The optional read scope the kind needs (0001: `vso.code` is required).
    fn scope(self) -> Option<&'static str> {
        match self {
            PinKind::PullRequest => None,
            PinKind::WorkItem => Some("vso.work"),
            PinKind::Definition => Some("vso.build"),
        }
    }

    /// pins.md §6.2 / §6.3 copy.
    fn missing_scope_reason(self) -> &'static str {
        match self {
            PinKind::PullRequest => "",
            PinKind::WorkItem => {
                "Can't pin work items: your token doesn't include Work Items (Read)"
            }
            PinKind::Definition => "Can't pin pipelines: your token doesn't include Build (Read)",
        }
    }
}

/// FR-1 against the token's scopes; empty = unknown (demo, older accounts):
/// everything is allowed and a refusal on refresh explains itself.
pub fn scope_check(kind: PinKind, scopes: &[String]) -> Result<(), SharedString> {
    match kind.scope() {
        Some(scope)
            if !scopes.is_empty()
                && !scopes.iter().any(|s| {
                    s == scope
                        || (scope == "vso.work"
                            && matches!(s.as_str(), "vso.work_write" | "vso.work_full"))
                        || (scope == "vso.build" && s == "vso.build_execute")
                }) =>
        {
            Err(kind.missing_scope_reason().into())
        }
        _ => Ok(()),
    }
}

/// `[pins]` in `settings.toml` (0009 FR-1 amendment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinsSettings {
    /// FR-12: closed items leave 24 h after closing.
    pub auto_unpin: bool,
}

impl Default for PinsSettings {
    fn default() -> Self {
        Self { auto_unpin: true }
    }
}

impl PinsSettings {
    /// Reads `[pins]` from the whole file's text; missing or malformed keys
    /// mean the default.
    pub fn from_toml(text: &str) -> Self {
        let auto_unpin = toml::from_str::<toml::Table>(text)
            .ok()
            .and_then(|root| {
                root.get(SETTINGS_TABLE)?
                    .as_table()?
                    .get("auto_unpin")?
                    .as_bool()
            })
            .unwrap_or(true);
        Self { auto_unpin }
    }
}

/// Where the toast and undo copy refers to a pin: "!4821", "#48213", or the
/// label / definition name.
pub fn short_name(subject: &PinSubject, label: Option<&str>, title: &str) -> String {
    match (subject.reference(), label) {
        (_, Some(label)) => label.to_owned(),
        (Some(reference), None) => reference,
        (None, None) => title.to_owned(),
    }
}

/// Reads the snapshot and removes the pins due for auto-unpin (FR-12,
/// silent). Blocking; call on a background executor.
pub fn refresh_blocking(store: &Store, options: Options) -> adoken_store::Result<Snapshot> {
    let (snapshot, due) = snapshot::read_candidates(store, options)?;
    settle_refresh(store, options, snapshot, due)
}

fn settle_refresh(
    store: &Store,
    options: Options,
    snapshot: Snapshot,
    candidates: Vec<Pin>,
) -> adoken_store::Result<Snapshot> {
    let needs_baseline = snapshot
        .pins
        .iter()
        .any(|view| view.pin.seen_result.is_none() && view.latest_result.is_some());
    if candidates.is_empty() && !needs_baseline {
        return Ok(snapshot);
    }
    store.write(|connection| {
        let (current, due) = snapshot::read_conn_candidates(connection, options)?;
        for pin in due {
            if candidates.iter().any(|candidate| {
                candidate.subject == pin.subject && candidate.pinned_at == pin.pinned_at
            }) {
                write::unpin(connection, &pin.subject)?;
            }
        }
        for view in current.pins {
            if view.pin.seen_result.is_none()
                && let Some(result) = view.latest_result
            {
                write::initialize_pin_result(connection, view.subject(), result)?;
            }
        }
        Ok(())
    })?;
    snapshot::read(store, options).map(|(snapshot, _)| snapshot)
}

/// The latest completed, not canceled, run's result: what a new definition
/// pin records as seen, so its dot lights on the next color change only.
fn latest_result(
    c: &adoken_store::rusqlite::Connection,
    subject: &PinSubject,
) -> adoken_store::Result<Option<RunResult>> {
    let PinSubject::Definition { project_id, id } = subject else {
        return Ok(None);
    };
    Ok(read::definition_runs(c, project_id, *id, 20)?
        .into_iter()
        .filter(|r| r.status == RunStatus::Completed)
        .filter_map(|r| r.result)
        .find(|r| *r != RunResult::Canceled))
}

/// Opens from the shell route to the canonical view (FR-7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinsEvent {
    Open(PinSubject),
    /// `⌘↩` / `⌘`-click: the item's Azure DevOps page.
    OpenInBrowser(String),
}

/// An unpin that Undo can restore while its toast shows.
struct Undo {
    pin: Pin,
    toast: SharedString,
}

/// How the pins entity reads and writes.
#[derive(Clone)]
pub struct PinsConfig {
    /// The demo org never syncs: staleness is off.
    pub demo: bool,
    /// Granted token scopes; empty = unknown.
    pub scopes: Vec<String>,
}

struct Installed(Entity<Pins>);

impl Global for Installed {}

pub struct Pins {
    store: Store,
    config: PinsConfig,
    settings: PinsSettings,
    snapshot: Arc<Snapshot>,
    /// Most recent last; each lives as long as its toast.
    undo: Vec<Undo>,
    /// Writes in flight: a snapshot read before they commit is stale.
    pending_writes: usize,
    write_tail: Option<tokio::sync::oneshot::Receiver<()>>,
    /// Snapshots older than this store generation are stale.
    min_generation: u64,
    clock: fn() -> Millis,
    refresh: Option<Task<()>>,
    settings_read: Option<Task<()>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<PinsEvent> for Pins {}

impl Pins {
    /// Creates the window's pins over `store`, reads the snapshot and keeps
    /// it fresh (store commits, a minute timer, `[pins]` edits).
    pub fn install(store: Store, config: PinsConfig, cx: &mut App) -> Entity<Pins> {
        Self::install_loaded(
            store,
            config,
            Snapshot::default(),
            PinsSettings::default(),
            cx,
        )
    }

    pub(crate) fn install_loaded(
        store: Store,
        config: PinsConfig,
        snapshot: Snapshot,
        settings: PinsSettings,
        cx: &mut App,
    ) -> Entity<Pins> {
        let pins = cx.new(|cx| Pins::new(store, config, snapshot, settings, now_ms, cx));
        cx.set_global(Installed(pins.clone()));
        pins
    }

    fn new(
        store: Store,
        config: PinsConfig,
        snapshot: Snapshot,
        settings: PinsSettings,
        clock: fn() -> Millis,
        cx: &mut Context<Self>,
    ) -> Self {
        let commits =
            crate::browse::reload_on_commit(&store, cx, |this: &mut Pins, _, cx| this.refresh(cx));
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(MINUTE_TICK).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
            }
        });
        let mut subscriptions = vec![crate::settings::observe(cx, |this, cx| {
            this.load_settings(cx)
        })];
        if let Some(toasts) = ToastStack::global(cx) {
            subscriptions.push(cx.subscribe(&toasts, |this, _, event, _| match event {
                ToastEvent::Dismissed(key) => this.undo.retain(|u| &u.toast != key),
            }));
        }
        let mut pins = Self {
            store,
            config,
            settings,
            snapshot: Arc::new(snapshot),
            undo: Vec::new(),
            pending_writes: 0,
            min_generation: 0,
            write_tail: None,
            clock,
            refresh: None,
            settings_read: None,
            _tasks: vec![commits, tick],
            _subscriptions: subscriptions,
        };
        pins.refresh(cx);
        pins.load_settings(cx);
        pins
    }

    pub fn global(cx: &App) -> Option<Entity<Pins>> {
        cx.try_global::<Installed>().map(|g| g.0.clone())
    }

    /// The latest snapshot: pins in order with their status.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.clone()
    }

    pub fn is_pinned(&self, subject: &PinSubject) -> bool {
        self.position(subject).is_some()
    }

    /// 0-based index in pin order.
    pub fn position(&self, subject: &PinSubject) -> Option<usize> {
        self.snapshot
            .pins
            .iter()
            .position(|p| p.subject() == subject)
    }

    /// The `⌘J` footer stat (spec §8).
    pub fn opens_this_week(&self) -> usize {
        self.snapshot.opens_this_week as usize
    }

    pub fn auto_unpin(&self) -> bool {
        self.settings.auto_unpin
    }

    pub fn now(&self) -> Millis {
        (self.clock)()
    }

    /// FR-1 for the installed pins' token; no pins installed → allowed.
    pub fn can_pin(kind: PinKind, cx: &App) -> Result<(), SharedString> {
        match Self::global(cx) {
            Some(pins) => scope_check(kind, &pins.read(cx).config.scopes),
            None => Ok(()),
        }
    }

    /// A replaced token changed the granted scopes (spec 0001 FR-16): pin
    /// kinds gated on them follow without a restart.
    pub fn set_scopes(&mut self, scopes: Vec<String>, cx: &mut Context<Self>) {
        if self.config.scopes != scopes {
            self.config.scopes = scopes;
            cx.notify();
        }
    }

    /// `⌘D` on something that can't be pinned (people, repos, commands).
    pub fn not_pinnable_hint(_window: &mut Window, cx: &mut App) {
        ToastStack::show(
            Toast::new(
                "pins-not-pinnable",
                IconName::Info,
                "Only pull requests, work items and pipelines can be pinned",
            ),
            cx,
        );
    }

    /// `⌘D`: pins `subject` (appended), or unpins it with the Undo toast. At
    /// the cap or without the kind's scope, nothing changes and a toast
    /// says why.
    pub fn toggle(&mut self, subject: PinSubject, title: SharedString, cx: &mut Context<Self>) {
        if self.is_pinned(&subject) {
            self.unpin(&subject, cx);
            return;
        }
        if let Err(reason) = scope_check(PinKind::of(&subject), &self.config.scopes) {
            ToastStack::show(
                Toast::new("pins-scope", IconName::CircleAlert, reason).tone(ToastTone::Warning),
                cx,
            );
            return;
        }
        if self.snapshot.pins.len() >= MAX_PINS {
            Self::limit_toast(cx);
            return;
        }
        let pin = Pin {
            position: self.snapshot.pins.len() as u32,
            subject,
            label: None,
            title: title.to_string(),
            pinned_at: self.now(),
            opened_at: None,
            seen_result: None,
            unavailable: None,
        };
        self.add(pin, false, cx);
    }

    fn limit_toast(cx: &mut App) {
        ToastStack::show(
            Toast::new(
                "pins-limit",
                IconName::CircleAlert,
                format!("{MAX_PINS} pins is the limit · unpin one first"),
            )
            .tone(ToastTone::Warning),
            cx,
        );
    }

    /// Appends (or, for Undo, restores at its old position) and writes.
    fn add(&mut self, pin: Pin, restore: bool, cx: &mut Context<Self>) {
        let snapshot = Arc::make_mut(&mut self.snapshot);
        let at = (pin.position as usize).min(snapshot.pins.len());
        snapshot
            .pins
            .insert(at, snapshot::PinView::provisional(pin.clone()));
        cx.notify();
        let subject = pin.subject.clone();
        self.write(cx, move |c| {
            let mut pin = pin;
            if pin.seen_result.is_none() {
                pin.seen_result = latest_result(c, &pin.subject)?;
            }
            Ok(if restore {
                write::restore_pin(c, &pin)?
            } else {
                write::add_pin(c, &pin)?
            })
        })
        .detach_and_then(cx, move |this, outcome, cx| {
            if outcome == Some(PinOutcome::Full) {
                // Another window filled the list meanwhile.
                Arc::make_mut(&mut this.snapshot)
                    .pins
                    .retain(|p| p.subject() != &subject);
                Self::limit_toast(cx);
                cx.notify();
            }
        });
    }

    /// A user unpin: the row leaves now and the Undo toast shows (FR-2).
    pub fn unpin(&mut self, subject: &PinSubject, cx: &mut Context<Self>) {
        let Some(ix) = self.position(subject) else {
            return;
        };
        let view = Arc::make_mut(&mut self.snapshot).pins.remove(ix);
        let mut pin = view.pin.clone();
        pin.position = ix as u32;
        cx.notify();
        let key: SharedString = format!("pins-unpinned:{}", subject.key()).into();
        self.undo.retain(|u| u.pin.subject != *subject);
        self.undo.push(Undo {
            pin,
            toast: key.clone(),
        });
        let name = short_name(subject, view.pin.label.as_deref(), &view.title);
        let undo_subject = subject.clone();
        ToastStack::show(
            Toast::new(key, IconName::PinOff, format!("Unpinned {name}")).action(
                ToastAction::new("Undo", move |_, cx| {
                    if let Some(pins) = Pins::global(cx) {
                        pins.update(cx, |pins, cx| pins.restore(&undo_subject, cx));
                    }
                })
                .keep_open()
                .keys(UNDO_KEYS, spoken_undo()),
            ),
            cx,
        );
        let subject = subject.clone();
        self.write(cx, move |c| write::unpin(c, &subject).map(|_| ()))
            .detach_and_then(cx, |_, _, _| {});
    }

    /// `⌘Z`: restores the most recent unpin whose toast still shows.
    pub fn undo_last(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(subject) = self.undo.last().map(|u| u.pin.subject.clone()) else {
            return false;
        };
        self.restore(&subject, cx);
        true
    }

    /// The toast's Undo: the pin comes back at its old position with its
    /// label; the toast closes.
    pub fn restore(&mut self, subject: &PinSubject, cx: &mut Context<Self>) {
        let pinned = self.is_pinned(subject);
        let undo = match take_undo(&mut self.undo, subject, pinned, self.snapshot.pins.len()) {
            Ok(Some(undo)) => undo,
            Ok(None) => return,
            Err(()) => {
                Self::limit_toast(cx);
                return;
            }
        };
        ToastStack::dismiss_global(&undo.toast, cx);
        if self.is_pinned(subject) {
            return;
        }
        self.add(undo.pin, true, cx);
    }

    /// Sets (or with `None` / blank, clears) the local label (FR-14).
    pub fn rename(&mut self, subject: &PinSubject, label: Option<String>, cx: &mut Context<Self>) {
        let label = normalized_label(label);
        let Some(ix) = self.position(subject) else {
            return;
        };
        if self.snapshot.pins[ix].pin.label == label {
            return;
        }
        Arc::make_mut(&mut self.snapshot).pins[ix].pin.label = label.clone();
        cx.notify();
        let subject = subject.clone();
        self.write(cx, move |c| {
            write::rename_pin(c, &subject, label.as_deref().unwrap_or("")).map(|_| ())
        })
        .detach_and_then(cx, |_, _, _| {});
    }

    /// Moves the pin `delta` places (clamped); at either end nothing
    /// happens. Returns the new 0-based index when it moved.
    pub fn move_by(
        &mut self,
        subject: &PinSubject,
        delta: isize,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let from = self.position(subject)?;
        let to = move_destination(from, delta, self.snapshot.pins.len());
        self.move_to(subject, to, cx)
    }

    /// Moves the pin to index `to` (drag drop). Returns it when it moved.
    pub fn move_to(
        &mut self,
        subject: &PinSubject,
        to: usize,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let from = self.position(subject)?;
        let pins = &mut Arc::make_mut(&mut self.snapshot).pins;
        let to = to.min(pins.len().saturating_sub(1));
        if to == from {
            return None;
        }
        let view = pins.remove(from);
        pins.insert(to, view);
        cx.notify();
        let subject = subject.clone();
        self.write(cx, move |c| {
            write::move_pin(c, &subject, to as u32).map(|_| ())
        })
        .detach_and_then(cx, |_, _, _| {});
        Some(to)
    }

    /// Click or `↩`: the shell opens the canonical view (FR-7).
    pub fn open(&mut self, subject: &PinSubject, cx: &mut Context<Self>) {
        if !self.is_pinned(subject) {
            return;
        }
        cx.emit(PinsEvent::Open(subject.clone()));
        self.record_open(subject, true, cx);
    }

    /// `⌘↩` / `⌘`-click: the browser; clears the dot like any open.
    pub fn open_in_browser(&mut self, subject: &PinSubject, cx: &mut Context<Self>) {
        let Some(url) = self
            .position(subject)
            .and_then(|ix| self.snapshot.pins[ix].web_url.clone())
        else {
            return;
        };
        cx.emit(PinsEvent::OpenInBrowser(url));
        self.record_open(subject, true, cx);
    }

    /// A view opened `subject`'s canonical view by another path (list,
    /// palette, notification): clears the change dot (FR-10). Not counted
    /// toward the "opened from pins" stat.
    pub fn mark_opened(&mut self, subject: &PinSubject, cx: &mut Context<Self>) {
        self.record_open(subject, false, cx);
    }

    fn record_open(&mut self, subject: &PinSubject, from_pins: bool, cx: &mut Context<Self>) {
        let Some(ix) = self.position(subject) else {
            return;
        };
        let now = self.now();
        let snapshot = Arc::make_mut(&mut self.snapshot);
        let view = &mut snapshot.pins[ix];
        let seen = view.latest_result.or(view.pin.seen_result);
        let was_changed = std::mem::replace(&mut view.changed, false);
        view.pin.opened_at = Some(now);
        view.pin.seen_result = seen;
        if from_pins {
            snapshot.opens_this_week += 1;
        }
        if was_changed || from_pins {
            cx.notify();
        }
        let subject = subject.clone();
        self.write(cx, move |c| {
            write::mark_pin_opened(c, &subject, now, seen, from_pins).map(|_| ())
        })
        .detach_and_then(cx, |_, _, _| {});
    }

    fn load_settings(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        let read = cx.background_executor().spawn(async move {
            PinsSettings::from_toml(&std::fs::read_to_string(path).unwrap_or_default())
        });
        self.settings_read = Some(cx.spawn(async move |this, cx| {
            let settings = read.await;
            this.update(cx, |this, cx| this.set_settings(settings, cx))
                .ok();
        }));
    }

    pub fn set_settings(&mut self, settings: PinsSettings, cx: &mut Context<Self>) {
        let changed = self.settings != settings;
        self.settings = settings;
        if changed || self.refresh.is_none() {
            self.refresh(cx);
        }
    }

    /// Re-reads the snapshot off the UI thread (and applies auto-unpin).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let options = Options {
            now: self.now(),
            auto_unpin: self.settings.auto_unpin,
            stale_after: (!self.config.demo).then_some(snapshot::STALE_AFTER),
        };
        self.refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(40))
                .await;
            let result = cx
                .background_executor()
                .spawn(async move { refresh_blocking(&store, options) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(snapshot) => this.apply(snapshot, cx),
                Err(error) => {
                    eprintln!("adoken: couldn't read pins: {error}");
                    ToastStack::show(
                        Toast::new(
                            "pins-read-error",
                            IconName::CircleAlert,
                            "Couldn't refresh pins · your saved pins are unchanged",
                        )
                        .tone(ToastTone::Warning),
                        cx,
                    );
                }
            })
            .ok();
        }));
    }

    fn apply(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        if !snapshot_is_current(
            self.pending_writes,
            self.min_generation,
            snapshot.generation,
        ) {
            return;
        }
        if *self.snapshot != snapshot {
            self.snapshot = Arc::new(snapshot);
            cx.notify();
        }
    }

    /// Runs `f` in one store write on the background executor; resolves
    /// to its result (`None` on error, logged) once committed.
    fn write<R: Send + 'static>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&adoken_store::rusqlite::Connection) -> adoken_store::Result<R> + Send + 'static,
    ) -> PendingWrite<R> {
        self.pending_writes += 1;
        let store = self.store.clone();
        let previous = self.write_tail.take();
        let (finished, next) = tokio::sync::oneshot::channel();
        self.write_tail = Some(next);
        PendingWrite(
            cx.background_executor()
                .spawn(ordered_write(store, previous, finished, f)),
        )
    }
}

/// A store write in flight (see [`Pins::write`]).
struct PendingWrite<R>(Task<(adoken_store::Result<R>, u64)>);

impl<R: 'static> PendingWrite<R> {
    /// Waits for the commit, settles the bookkeeping, then runs `then`
    /// with the result and refreshes the snapshot.
    fn detach_and_then(
        self,
        cx: &mut Context<Pins>,
        then: impl FnOnce(&mut Pins, Option<R>, &mut Context<Pins>) + 'static,
    ) {
        let task = self.0;
        cx.spawn(async move |this, cx| {
            let (result, generation) = task.await;
            this.update(cx, |this, cx| {
                this.pending_writes = this.pending_writes.saturating_sub(1);
                this.min_generation = this.min_generation.max(generation);
                let out = match result {
                    Ok(out) => Some(out),
                    Err(error) => {
                        eprintln!("adoken: couldn't save pins: {error}");
                        let keys: Vec<_> = this.undo.drain(..).map(|undo| undo.toast).collect();
                        for key in keys {
                            ToastStack::dismiss_global(&key, cx);
                        }
                        ToastStack::show(
                            Toast::new(
                                "pins-write-error",
                                IconName::CircleAlert,
                                "Couldn't save pins · restoring your saved list",
                            )
                            .tone(ToastTone::Warning),
                            cx,
                        );
                        None
                    }
                };
                then(this, out, cx);
                if this.pending_writes == 0 {
                    this.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

fn spoken_undo() -> &'static str {
    if cfg!(target_os = "macos") {
        "Command Z"
    } else {
        "Control Z"
    }
}

#[cfg(test)]
mod tests;

fn normalized_label(label: Option<String>) -> Option<String> {
    label
        .map(|label| {
            label
                .trim()
                .chars()
                .take(adoken_model::MAX_PIN_LABEL_CHARS)
                .collect::<String>()
        })
        .filter(|label| !label.is_empty())
}

fn move_destination(from: usize, delta: isize, len: usize) -> usize {
    from.saturating_add_signed(delta).min(len.saturating_sub(1))
}

fn snapshot_is_current(pending: usize, minimum: u64, generation: u64) -> bool {
    pending == 0 && generation >= minimum
}

async fn ordered_write<R>(
    store: Store,
    previous: Option<tokio::sync::oneshot::Receiver<()>>,
    finished: tokio::sync::oneshot::Sender<()>,
    write: impl FnOnce(&adoken_store::rusqlite::Connection) -> adoken_store::Result<R>,
) -> (adoken_store::Result<R>, u64) {
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    let result = store.write(|connection| write(connection));
    let generation = store.generation();
    let _ = finished.send(());
    (result, generation)
}

fn take_undo(
    undo: &mut Vec<Undo>,
    subject: &PinSubject,
    pinned: bool,
    count: usize,
) -> Result<Option<Undo>, ()> {
    let Some(index) = undo.iter().position(|undo| &undo.pin.subject == subject) else {
        return Ok(None);
    };
    if !pinned && count >= MAX_PINS {
        return Err(());
    }
    Ok(Some(undo.remove(index)))
}
