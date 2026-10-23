//! Inbox: the notification center (spec 0005). One row per subject,
//! filter chips, keyboard triage (`e` `u` `s` `m` `⇧M` `z` `↩`), a snooze
//! menu, and honest empty states. Notification settings live in Settings ›
//! Notifications (spec 0009); `⌘⇧,` opens that tab. The inbox applies
//! `settings.toml [notify]` live and serves its mutes to that tab through
//! [`linked`].
//!
//! The [`Engine`] lives on the UI thread so triage paints on the next frame;
//! reading events (`source::fetch`) and persisting (`persist::save`) run on
//! the background executor every [`POLL`]. Banners are delivered in-app
//! (toast + [`InboxEvent::Delivered`]); no OS notification is posted.

mod demo;
pub mod nav;
pub mod rows;

/// Guided tour anchors (spec 0020 FR-10).
pub mod anchors {
    pub const LIST: &str = "inbox.list";
    pub const FILTERS: &str = "inbox.filters";
    pub const FOOTER: &str = "inbox.footer";
    pub const SETTINGS: &str = "inbox.settings";
}

use std::sync::Arc;
use std::time::Duration;

use adoken_notify::persist::{self, Progress};
use adoken_notify::source::{self, Enricher};
use adoken_notify::{
    Banner, Clock, DEFAULT_MORNING, DeliveryEnv, Effects, Engine, EngineState, Filter, Mute,
    NotificationId, NotifySettings, Priority, SnoozeChoice, SoundCue, Source, StateFilter,
    SystemClock,
};
use adoken_store::Store;
use adoken_theme::{elevation, radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonSize, ButtonVariant, Chip, ChipTone, EmptyState, Icon, IconName,
    Kbd, ListRow, Segmented, Theme, TypeStyleExt,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Global, Hsla,
    KeyBinding, Task, UniformListScrollHandle, WeakEntity, Window, actions, div, prelude::*, px,
    uniform_list,
};

use crate::palette::PaletteCommand;
use crate::palette::catalog::Target;
use rows::Glyph;

actions!(
    inbox,
    [
        /// Open the selected notification's item.
        Open,
        /// Mark the selected notification done.
        MarkDone,
        /// Toggle the selected notification read / unread.
        ToggleRead,
        /// Snooze the selected notification…
        Snooze,
        /// Mute the selected notification's subject.
        MuteSubject,
        /// Mute the repo, pipeline, work item type, or author…
        MuteScope,
        /// Undo the last triage action.
        Undo,
        /// Bring a snoozed or done notification back to the inbox.
        Restore,
        /// Cancel the auto-complete I set on the selected PR (new commits
        /// arrived after it was set).
        CancelAutoComplete,
        SelectNext,
        SelectPrevious,
        SelectFirst,
        SelectLast,
        /// Filter: pull requests.
        ToggleSourcePullRequests,
        /// Filter: work items.
        ToggleSourceWorkItems,
        /// Filter: pipelines.
        ToggleSourcePipelines,
        /// Filter: mentions.
        ToggleSourceMentions,
        /// Filter: nudges.
        ToggleSourceNudges,
        TogglePriority0,
        TogglePriority1,
        TogglePriority2,
        TogglePriority3,
        ShowUnread,
        ShowAll,
        ShowSnoozed,
        ShowDone,
        ClearFilters,
        /// Open Settings › Notifications.
        ToggleRules,
        Confirm,
        Dismiss,
    ]
);

/// Event/queue poll cadence (spec 0005: event → row < 100 ms after commit
/// is bounded by this plus a frame).
const POLL: Duration = Duration::from_secs(2);
/// In-app banner lifetime (principles.md §4 toast: 5 s, 8 s with an action).
const TOAST: Duration = Duration::from_secs(8);
const HEADER_HEIGHT: f32 = 44.0;
const FILTER_HEIGHT: f32 = 40.0;
const FOOTER_HEIGHT: f32 = 28.0;
const MENU_WIDTH: f32 = 300.0;
const TOAST_WIDTH: f32 = 360.0;
const ROW_HEIGHT: f32 = size::PALETTE_ROW_COMFORTABLE;

const CTX_LIST: &str = "Inbox";
const CTX_MENU: &str = "InboxMenu";

pub const OPEN_KEYS: &str = "enter";
pub const DONE_KEYS: &str = "e";
pub const READ_KEYS: &str = "u";
pub const SNOOZE_KEYS: &str = "s";
pub const MUTE_KEYS: &str = "m";
pub const MUTE_SCOPE_KEYS: &str = "shift-m";
pub const UNDO_KEYS: &str = "z";
/// One key on the auto-complete guard alert (spec 0004 FR-12).
pub const CANCEL_AUTO_COMPLETE_KEYS: &str = "c";
pub const RULES_KEYS: &str = "secondary-shift-,";
pub const CLEAR_KEYS: &str = "secondary-backspace";
/// macOS reports `⇧1` as `!` (US layout): bind the character, show `⇧1`.
const PRIORITY_KEYS: [(&str, &str); 4] = [
    ("!", "shift-1"),
    ("@", "shift-2"),
    ("#", "shift-3"),
    ("$", "shift-4"),
];

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(CTX_LIST);
    let menu = Some(CTX_MENU);
    vec![
        KeyBinding::new(OPEN_KEYS, Open, list),
        KeyBinding::new(DONE_KEYS, MarkDone, list),
        KeyBinding::new(READ_KEYS, ToggleRead, list),
        KeyBinding::new(SNOOZE_KEYS, Snooze, list),
        KeyBinding::new(MUTE_KEYS, MuteSubject, list),
        KeyBinding::new(MUTE_SCOPE_KEYS, MuteScope, list),
        KeyBinding::new(UNDO_KEYS, Undo, list),
        KeyBinding::new("r", Restore, list),
        KeyBinding::new(CANCEL_AUTO_COMPLETE_KEYS, CancelAutoComplete, list),
        KeyBinding::new("down", SelectNext, list),
        KeyBinding::new("j", SelectNext, list),
        KeyBinding::new("up", SelectPrevious, list),
        KeyBinding::new("k", SelectPrevious, list),
        KeyBinding::new("home", SelectFirst, list),
        KeyBinding::new("end", SelectLast, list),
        KeyBinding::new("1", ToggleSourcePullRequests, list),
        KeyBinding::new("2", ToggleSourceWorkItems, list),
        KeyBinding::new("3", ToggleSourcePipelines, list),
        KeyBinding::new("4", ToggleSourceMentions, list),
        KeyBinding::new("5", ToggleSourceNudges, list),
        KeyBinding::new(PRIORITY_KEYS[0].0, TogglePriority0, list),
        KeyBinding::new(PRIORITY_KEYS[1].0, TogglePriority1, list),
        KeyBinding::new(PRIORITY_KEYS[2].0, TogglePriority2, list),
        KeyBinding::new(PRIORITY_KEYS[3].0, TogglePriority3, list),
        KeyBinding::new(CLEAR_KEYS, ClearFilters, list),
        KeyBinding::new(RULES_KEYS, ToggleRules, list),
        KeyBinding::new("down", SelectNext, menu),
        KeyBinding::new("up", SelectPrevious, menu),
        KeyBinding::new("enter", Confirm, menu),
        KeyBinding::new("escape", Dismiss, menu),
    ]
}

/// Every inbox action as a palette command (keyboard-first).
pub fn commands() -> Vec<PaletteCommand> {
    let cmd = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        cmd("Inbox: Open notification", Some(OPEN_KEYS), Box::new(Open)),
        cmd("Inbox: Mark done", Some(DONE_KEYS), Box::new(MarkDone)),
        cmd(
            "Inbox: Toggle unread",
            Some(READ_KEYS),
            Box::new(ToggleRead),
        ),
        cmd("Inbox: Snooze…", Some(SNOOZE_KEYS), Box::new(Snooze)),
        cmd(
            "Inbox: Mute this subject",
            Some(MUTE_KEYS),
            Box::new(MuteSubject),
        ),
        cmd(
            "Inbox: Mute repo, pipeline, type, or author…",
            Some(MUTE_SCOPE_KEYS),
            Box::new(MuteScope),
        ),
        cmd("Inbox: Undo", Some(UNDO_KEYS), Box::new(Undo)),
        cmd("Inbox: Move back to inbox", Some("r"), Box::new(Restore)),
        cmd(
            "Inbox: Cancel auto-complete",
            Some(CANCEL_AUTO_COMPLETE_KEYS),
            Box::new(CancelAutoComplete),
        ),
        cmd(
            "Inbox: Filter pull requests",
            Some("1"),
            Box::new(ToggleSourcePullRequests),
        ),
        cmd(
            "Inbox: Filter work items",
            Some("2"),
            Box::new(ToggleSourceWorkItems),
        ),
        cmd(
            "Inbox: Filter pipelines",
            Some("3"),
            Box::new(ToggleSourcePipelines),
        ),
        cmd(
            "Inbox: Filter mentions",
            Some("4"),
            Box::new(ToggleSourceMentions),
        ),
        cmd(
            "Inbox: Filter nudges",
            Some("5"),
            Box::new(ToggleSourceNudges),
        ),
        cmd(
            "Inbox: Filter P0",
            Some(PRIORITY_KEYS[0].1),
            Box::new(TogglePriority0),
        ),
        cmd(
            "Inbox: Filter P1",
            Some(PRIORITY_KEYS[1].1),
            Box::new(TogglePriority1),
        ),
        cmd(
            "Inbox: Filter P2",
            Some(PRIORITY_KEYS[2].1),
            Box::new(TogglePriority2),
        ),
        cmd(
            "Inbox: Filter P3",
            Some(PRIORITY_KEYS[3].1),
            Box::new(TogglePriority3),
        ),
        cmd("Inbox: Show unread", None, Box::new(ShowUnread)),
        cmd("Inbox: Show all", None, Box::new(ShowAll)),
        cmd("Inbox: Show snoozed", None, Box::new(ShowSnoozed)),
        cmd("Inbox: Show done", None, Box::new(ShowDone)),
        cmd(
            "Inbox: Clear filters",
            Some(CLEAR_KEYS),
            Box::new(ClearFilters),
        ),
        cmd(
            "Notification settings",
            Some(RULES_KEYS),
            Box::new(ToggleRules),
        ),
    ]
}

/// What the shell routes.
#[derive(Clone, Debug)]
pub enum InboxEvent {
    /// `↩` on a row: open its canonical view. `before` is the Inbox as it
    /// was just before the open read the row (spec 0018 FR-6).
    Open {
        target: Target,
        before: nav::InboxNavState,
    },
    /// Sidebar count (unread) and dock badge (flow step 6).
    Counts { unread: usize, badge: usize },
    /// A banner the rules let through (in-app delivery).
    Delivered(Banner),
    /// A sound the delivery rules let through (quiet hours, delivery windows
    /// and per-priority delivery already applied); the shell plays it.
    Sound(SoundCue),
}

#[derive(Clone, Debug, PartialEq)]
enum Menu {
    Snooze { selected: usize },
    Mute { scopes: Vec<Mute>, selected: usize },
}

impl Menu {
    fn len(&self) -> usize {
        match self {
            Menu::Snooze { .. } => SnoozeChoice::MENU.len(),
            Menu::Mute { scopes, .. } => scopes.len(),
        }
    }

    fn selected_mut(&mut self) -> &mut usize {
        match self {
            Menu::Snooze { selected } | Menu::Mute { selected, .. } => selected,
        }
    }
}

pub struct InboxView {
    store: Option<Store>,
    engine: Engine,
    env: DeliveryEnv,
    loaded: bool,
    cursor: i64,
    rows: Vec<NotificationId>,
    selected: usize,
    menu: Option<Menu>,
    status: Option<String>,
    toast: Option<Banner>,
    counts: (usize, usize),
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _tasks: Vec<Task<()>>,
    _settings: Option<gpui::Subscription>,
    toast_task: Option<Task<()>>,
    row_menu: adoken_ui::action_menu::MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    _pins: Option<gpui::Subscription>,
    /// An opened row kept listed after back (spec 0018 FR-6).
    sticky: Option<NotificationId>,
    /// Back / forward before the first load landed (spec 0018 FR-8).
    pending_restore: Option<crate::shell::history::Pending<nav::InboxNavState>>,
}

impl EventEmitter<InboxEvent> for InboxView {}

impl Focusable for InboxView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The shell's entry point: loads local notify state off the UI thread,
/// then keeps ingesting new sync events and nudges. The inbox registers
/// itself for Settings › Notifications ([`linked`]).
pub fn new(store: Store, window: &mut Window, cx: &mut App) -> Entity<InboxView> {
    let _ = window;
    let inbox = cx.new(|cx| InboxView::with_store(store, cx));
    cx.set_global(Link(inbox.downgrade()));
    inbox
}

/// The live inbox, for Settings (mutes, live `[notify]` changes).
struct Link(WeakEntity<InboxView>);

impl Global for Link {}

/// The live inbox, once the store is open.
pub fn linked(cx: &App) -> Option<Entity<InboxView>> {
    cx.try_global::<Link>().and_then(|link| link.0.upgrade())
}

/// Active mutes (Settings › Notifications › Muted).
pub fn mutes(cx: &App) -> Vec<Mute> {
    linked(cx).map_or_else(Vec::new, |inbox| inbox.read(cx).engine.mutes().to_vec())
}

/// Unmute the `ix`-th active mute.
pub fn unmute(ix: usize, cx: &mut App) {
    if let Some(inbox) = linked(cx) {
        inbox.update(cx, |inbox, cx| {
            if let Some(mute) = inbox.engine.mutes().get(ix).cloned() {
                inbox.engine.unmute(&mute);
                inbox.refresh(cx);
            }
        });
    }
}

/// Post one account notice (spec 0001 FR-15 Q-6, owner decision
/// 2026-10-01): straight to the inbox, never a banner or a sound. `key`
/// makes it once per token.
pub fn post_account_notice(
    kind: adoken_model::SyncEventKind,
    key: &str,
    summary: String,
    cx: &mut App,
) {
    let Some(inbox) = linked(cx) else {
        return;
    };
    inbox.update(cx, |inbox, cx| {
        let at = inbox.engine.now();
        inbox.engine.ingest_silently(adoken_notify::Incoming {
            dedupe_key: format!("{}|{key}", kind.as_str()),
            kind: adoken_notify::Kind::Sync(kind),
            subject: adoken_model::Subject::Account,
            actor: None,
            self_caused: false,
            at,
            summary,
            ctx: adoken_notify::EventContext {
                title: rows::ACCOUNT_TITLE.into(),
                ..Default::default()
            },
        });
        inbox.refresh(cx);
    });
}

/// A replacement verified: the token's open inbox item is done (Q-6:
/// cleared automatically).
pub fn clear_account_notices(cx: &mut App) {
    let Some(inbox) = linked(cx) else {
        return;
    };
    inbox.update(cx, |inbox, cx| {
        let open: Vec<NotificationId> = inbox
            .engine
            .items()
            .iter()
            .filter(|n| n.subject == adoken_model::Subject::Account && n.in_inbox())
            .map(|n| n.id)
            .collect();
        for id in open {
            inbox.engine.mark_done(id);
        }
        inbox.refresh(cx);
    });
}

/// Apply `[notify]` from Settings now (the inbox persists it as well).
pub fn set_notify_settings(settings: NotifySettings, cx: &mut App) {
    if let Some(inbox) = linked(cx) {
        inbox.update(cx, |inbox, cx| inbox.apply_settings(settings, cx));
    }
}

impl InboxView {
    /// Global sound switch (spec 0007 `[sound] enabled`): notify only emits
    /// [`InboxEvent::Sound`] while it's on.
    pub fn set_sounds_enabled(&mut self, enabled: bool) {
        self.env.sounds_enabled = enabled;
    }

    fn blank(engine: Engine, cx: &mut Context<Self>) -> Self {
        Self {
            store: None,
            engine,
            env: DeliveryEnv::default(),
            loaded: false,
            cursor: 0,
            rows: Vec::new(),
            selected: 0,
            menu: None,
            status: None,
            toast: None,
            counts: (0, 0),
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _tasks: Vec::new(),
            _settings: None,
            toast_task: None,
            row_menu: Default::default(),
            row_bounds: Default::default(),
            _pins: crate::pin_actions::observe(cx),
            sticky: None,
            pending_restore: None,
        }
    }

    fn with_store(store: Store, cx: &mut Context<Self>) -> Self {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let mut this = Self::blank(Engine::new(clock.clone(), Default::default()), cx);
        this.store = Some(store.clone());
        let task = cx.spawn(async move |this, cx| {
            let job = {
                let store = store.clone();
                cx.background_executor()
                    .spawn(async move { first_load(&store, clock) })
            };
            let loaded = match job.await {
                Ok(loaded) => loaded,
                Err(error) => {
                    eprintln!("adoken: couldn't load notifications: {error}");
                    return;
                }
            };
            let Ok(()) = this.update(cx, |this, cx| this.adopt(loaded, cx)) else {
                return;
            };
            poll(this, store, cx).await;
        });
        this._tasks.push(task);
        this._settings = Some(crate::settings::observe(cx, |this, cx| {
            this.reload_settings(cx)
        }));
        this
    }

    /// `settings.toml [notify]` changed (Settings or an outside edit):
    /// re-read it off the UI thread and apply.
    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let job = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(persist::settings_path(&store)).unwrap_or_default();
            NotifySettings::from_settings_toml(&text)
        });
        self._tasks
            .push(cx.spawn(async move |this, cx| match job.await {
                Ok(settings) => {
                    this.update(cx, |this, cx| this.apply_settings(settings, cx))
                        .ok();
                }
                Err(error) => log::warn!("notify: ignoring unreadable settings: {error}"),
            }));
    }

    fn apply_settings(&mut self, settings: NotifySettings, cx: &mut Context<Self>) {
        if *self.engine.settings() != settings {
            self.engine.set_settings(settings);
            self.refresh(cx);
        }
    }

    /// Stories/tests: a ready engine, no store, no IO.
    pub fn from_state(state: EngineState, clock: Arc<dyn Clock>, cx: &mut Context<Self>) -> Self {
        let mut this = Self::blank(Engine::from_state(clock, state), cx);
        this.loaded = true;
        this.refresh(cx);
        this
    }

    /// Stories: the demo inbox at a fixed time.
    pub fn demo(cx: &mut Context<Self>) -> Self {
        Self::from_state(demo::state(), Arc::new(demo::clock()), cx)
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Stories/tests: triage directly, then repaint.
    pub fn update_engine(&mut self, f: impl FnOnce(&mut Engine), cx: &mut Context<Self>) {
        f(&mut self.engine);
        self.refresh(cx);
    }

    pub fn select_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.select(ix, cx);
    }

    pub fn open_snooze_menu(&mut self, cx: &mut Context<Self>) {
        if self.selected_id().is_some() {
            self.menu = Some(Menu::Snooze { selected: 0 });
            cx.notify();
        }
    }

    fn adopt(&mut self, loaded: Loaded, cx: &mut Context<Self>) {
        self.engine = loaded.engine;
        self.cursor = loaded.cursor;
        self.loaded = true;
        self.refresh(cx);
        self.apply_pending_restore(cx);
    }

    fn selected_id(&self) -> Option<NotificationId> {
        self.rows.get(self.selected).copied()
    }

    pub fn pin_subject(&self) -> Option<adoken_model::PinSubject> {
        rows::pin_subject(self.engine.get(self.selected_id()?)?)
    }

    pub fn pin_title(&self) -> gpui::SharedString {
        self.selected_id()
            .and_then(|id| self.engine.get(id))
            .map(|notification| notification.ctx.title.clone())
            .unwrap_or_default()
            .into()
    }

    fn open_row_menu(
        &mut self,
        position: adoken_ui::action_menu::MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.pin_title();
        let mut items = vec![
            crate::pin_actions::action("Open", Open).keys("enter"),
            crate::pin_actions::action("Mark done", MarkDone),
            crate::pin_actions::action("Toggle read / unread", ToggleRead),
            crate::pin_actions::action("Snooze…", Snooze),
            crate::pin_actions::action("Mute subject", MuteSubject),
            crate::pin_actions::action("Mute scope…", MuteScope),
            crate::pin_actions::menu_item(self.pin_subject(), title, cx),
        ];
        if self
            .selected_id()
            .and_then(|id| self.engine.get(id))
            .and_then(rows::auto_complete_guard)
            .is_some()
        {
            items.push(crate::pin_actions::action(
                "Cancel auto-complete",
                CancelAutoComplete,
            ));
        }
        self.row_menu.open(
            "Notification",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this| &mut this.row_menu,
            window,
            cx,
        );
    }

    /// Recompute rows after any change; one sort over the in-memory list.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let keep = self.selected_id();
        self.rows = nav::listed(&self.engine, self.sticky);
        self.selected = rows::clamp_selection(self.selected, self.rows.len());
        // A background update shouldn't move the cursor off the row.
        if let Some(keep) = keep
            && let Some(ix) = self.rows.iter().position(|id| *id == keep)
            && self.status.is_none()
        {
            self.selected = ix;
        }
        let counts = (self.engine.unread_count(), self.engine.badge());
        if counts != self.counts {
            self.counts = counts;
            cx.emit(InboxEvent::Counts {
                unread: counts.0,
                badge: counts.1,
            });
        }
        cx.notify();
    }

    fn deliver(&mut self, effects: Effects, cx: &mut Context<Self>) {
        for banner in effects.banners {
            cx.emit(InboxEvent::Delivered(banner.clone()));
            self.toast = Some(banner);
            self.toast_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TOAST).await;
                this.update(cx, |this, cx| {
                    this.toast = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        for cue in effects.sounds {
            cx.emit(InboxEvent::Sound(cue));
        }
    }

    fn apply_batch(&mut self, batch: source::Batch, cx: &mut Context<Self>) -> Progress {
        let mut effects = Effects::default();
        for incoming in batch.incoming {
            self.engine.ingest(incoming, &self.env, &mut effects);
        }
        for subject in &batch.resolved {
            self.engine.resolve_nudge(subject);
        }
        effects.extend(self.engine.tick(&self.env));
        self.deliver(effects, cx);
        self.refresh(cx);
        batch.progress
    }

    // ── triage ──────────────────────────────────────────────────────────

    fn triage(
        &mut self,
        status: &str,
        f: impl FnOnce(&mut Engine, NotificationId) -> bool,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_id() else {
            return;
        };
        if f(&mut self.engine, id) {
            self.status = Some(status.to_owned());
            self.refresh(cx);
            self.status = Some(status.to_owned());
        }
    }

    fn open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id() else {
            return;
        };
        if let Some(subject) = self.pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        let before = nav::InboxNavState {
            opened: true,
            ..self.nav_state()
        };
        self.engine.open(id);
        if let Some(target) = self.engine.get(id).and_then(rows::target) {
            // The opened row stays listed while it's selected (FR-6).
            self.sticky = Some(id);
            cx.emit(InboxEvent::Open { target, before });
        }
        match self.engine.get(id).and_then(rows::account_open) {
            Some(rows::AccountOpen::ReplaceToken) => crate::account::open_and_replace(cx),
            Some(rows::AccountOpen::Settings) => {
                crate::settings::open(crate::settings::Tab::Account, cx);
            }
            None => {}
        }
        self.refresh(cx);
    }

    fn mark_done(&mut self, _: &MarkDone, _: &mut Window, cx: &mut Context<Self>) {
        self.triage("Marked done · z to undo", |e, id| e.mark_done(id), cx);
    }

    fn toggle_read(&mut self, _: &ToggleRead, _: &mut Window, cx: &mut Context<Self>) {
        self.triage("Updated · z to undo", |e, id| e.toggle_read(id), cx);
    }

    fn restore(&mut self, _: &Restore, _: &mut Window, cx: &mut Context<Self>) {
        self.triage("Moved back to inbox · z to undo", |e, id| e.restore(id), cx);
    }

    /// The guard alert's one-key action: cancel the auto-complete I set, so
    /// the unseen commits don't merge (spec 0004 FR-12, AC-15). Enter
    /// ("Review changes") opens the PR as usual.
    fn cancel_auto_complete(
        &mut self,
        _: &CancelAutoComplete,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(notification) = self.selected_id() else {
            return;
        };
        let Some(pr_id) = self
            .engine
            .get(notification)
            .and_then(rows::auto_complete_guard)
        else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let access = crate::review::writes::access(cx);
        self.status = Some(format!("Cancelling auto-complete on !{pr_id}…"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let reader = store.clone();
            let pr = cx.background_spawn(async move { reader.pr(pr_id) }).await;
            let result = match pr {
                Ok(Some(detail)) => {
                    let write = adoken_sync::ReviewWrite::CancelAutoComplete {
                        pr: adoken_sync::writes::PrRef::of(&detail.pr),
                    };
                    let now = crate::review::now_ms();
                    cx.update(|cx| access.send(Some(store), write, now, cx))
                        .await
                        .map(drop)
                        .map_err(|e| e.to_string())
                }
                Ok(None) => Err(format!("!{pr_id} isn't synced")),
                Err(error) => Err(error.to_string()),
            };
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.engine.mark_done(notification);
                        this.refresh(cx);
                        this.status = Some(format!("Auto-complete cancelled on !{pr_id}"));
                    }
                    Err(error) => {
                        this.status = Some(format!("Auto-complete is still set: {error}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn snooze(&mut self, _: &Snooze, _: &mut Window, cx: &mut Context<Self>) {
        self.open_snooze_menu(cx);
    }

    fn snooze_with(&mut self, choice: SnoozeChoice, cx: &mut Context<Self>) {
        self.menu = None;
        let Some(id) = self.selected_id() else {
            return;
        };
        if self.engine.snooze(id, choice, DEFAULT_MORNING) {
            let until = match self.engine.get(id).and_then(|n| n.snooze) {
                Some(adoken_notify::Snooze::Until(at)) => Some(at),
                _ => None,
            };
            let status = rows::snoozed_status(until, self.engine.now(), self.engine.clock());
            self.refresh(cx);
            self.status = Some(status);
        }
    }

    fn mute_subject(&mut self, _: &MuteSubject, _: &mut Window, cx: &mut Context<Self>) {
        self.triage("Muted · z to undo", |e, id| e.mute_subject(id), cx);
    }

    fn mute_scope(&mut self, _: &MuteScope, _: &mut Window, cx: &mut Context<Self>) {
        let Some(n) = self.selected_id().and_then(|id| self.engine.get(id)) else {
            return;
        };
        let scopes = Mute::scopes_for(n.actor.as_deref(), &n.ctx);
        if !scopes.is_empty() {
            self.menu = Some(Menu::Mute {
                scopes,
                selected: 0,
            });
            cx.notify();
        }
    }

    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.engine.undo() {
            self.status = None;
            self.refresh(cx);
            self.status = Some("Undone".into());
        }
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = rows::clamp_selection(ix, self.rows.len());
        if self.selected_id() != self.sticky {
            self.sticky = None;
        }
        self.scroll
            .scroll_to_item(self.selected, gpui::ScrollStrategy::Top);
        cx.notify();
    }

    fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        match &mut self.menu {
            Some(menu) => {
                let len = menu.len();
                let selected = menu.selected_mut();
                *selected = (*selected + 1).min(len.saturating_sub(1));
                cx.notify();
            }
            None => self.select(self.selected + 1, cx),
        }
    }

    fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        match &mut self.menu {
            Some(menu) => {
                let selected = menu.selected_mut();
                *selected = selected.saturating_sub(1);
                cx.notify();
            }
            None => self.select(self.selected.saturating_sub(1), cx),
        }
    }

    fn set_filter(&mut self, f: impl FnOnce(&mut Filter), cx: &mut Context<Self>) {
        let mut filter = self.engine.filter();
        f(&mut filter);
        self.engine.set_filter(filter);
        self.sticky = None;
        self.selected = 0;
        self.status = None;
        self.refresh(cx);
    }

    fn toggle_source(&mut self, source: Source, cx: &mut Context<Self>) {
        self.set_filter(|f| f.toggle_source(source), cx);
    }

    fn toggle_priority(&mut self, priority: Priority, cx: &mut Context<Self>) {
        self.set_filter(|f| f.toggle_priority(priority), cx);
    }

    fn show_state(&mut self, state: StateFilter, cx: &mut Context<Self>) {
        self.set_filter(|f| f.state = state, cx);
    }

    fn pick(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.menu.clone() {
            Some(Menu::Snooze { .. }) => {
                if let Some(choice) = SnoozeChoice::MENU.get(index) {
                    self.snooze_with(*choice, cx);
                }
            }
            Some(Menu::Mute { scopes, .. }) => {
                self.menu = None;
                if let Some(scope) = scopes.get(index) {
                    self.engine.mute(scope.clone());
                    self.refresh(cx);
                    self.status = Some(format!("Muted {} · z to undo", scope.label()));
                }
            }
            None => {}
        }
    }

    fn confirm(&mut self, _: &Confirm, _: &mut Window, cx: &mut Context<Self>) {
        let index = match &self.menu {
            Some(Menu::Snooze { selected } | Menu::Mute { selected, .. }) => *selected,
            None => return,
        };
        self.pick(index, cx);
    }

    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    fn toggle_rules(&mut self, _: &ToggleRules, _: &mut Window, cx: &mut Context<Self>) {
        crate::settings::open(crate::settings::Tab::Notifications, cx);
    }

    // ── rendering ───────────────────────────────────────────────────────

    fn key_context(&self) -> &'static str {
        if self.menu.is_some() {
            CTX_MENU
        } else {
            CTX_LIST
        }
    }

    fn render_header(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme.colors;
        let unread = self.engine.unread_count();
        div()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .text_color(colors.text_primary)
                    .type_style(typography::HEADING, theme)
                    .child("Inbox"),
            )
            .when(unread > 0, |d| {
                d.child(
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(format!("{unread} unread")),
                )
            })
            .child(div().flex_1())
            .when_some(rows::quiet_label(&self.engine), |d, label| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .px(px(space::S8))
                        .h(px(size::CONTROL_SM))
                        .rounded(px(radius::FULL))
                        .bg(colors.bg_raised)
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(
                            Icon::new(IconName::Moon)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        )
                        .child(label),
                )
            })
            .child(
                div()
                    .flex_none()
                    .child(
                        Button::new("inbox-rules", "Settings")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Md)
                            .icon(IconName::SlidersHorizontal)
                            .shortcut(Kbd::parse(RULES_KEYS).expect("static binding"))
                            .on_press(press(cx, |this, window, cx| {
                                this.toggle_rules(&ToggleRules, window, cx)
                            })),
                    )
                    .children(adoken_ui::tour::anchor(anchors::SETTINGS, cx)),
            )
    }

    fn render_filters(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme.colors;
        let filter = self.engine.filter();
        let chip = |id: String, label: &'static str, keys: &'static str, on: bool| {
            div()
                .id(gpui::ElementId::Name(id.into()))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .h(px(size::CONTROL_SM))
                .pl(px(space::S8))
                .pr(px(space::S4))
                .rounded(px(radius::FULL))
                .border_1()
                .cursor_pointer()
                .type_style(typography::SMALL, theme)
                .when(on, |d| {
                    d.bg(colors.accent_wash)
                        .border_color(colors.accent_fg)
                        .text_color(colors.accent_fg)
                })
                .when(!on, |d| {
                    d.border_color(colors.border_default)
                        .text_color(colors.text_secondary)
                        .hover(|s| s.bg(colors.bg_hover))
                })
                .child(label)
                .child(Kbd::parse(keys).expect("static binding"))
        };
        let sources = Source::ALL.into_iter().enumerate().map(|(i, source)| {
            let keys = ["1", "2", "3", "4", "5"][i];
            chip(
                format!("src-{i}"),
                source.label(),
                keys,
                filter.has_source(source),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_source(source, cx)))
        });
        let priorities = Priority::ALL.into_iter().map(|p| {
            chip(
                format!("pri-{}", p.index()),
                p.label(),
                PRIORITY_KEYS[p.index()].1,
                filter.has_priority(p),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_priority(p, cx)))
        });
        let state_ix = StateFilter::ALL
            .iter()
            .position(|s| *s == filter.state)
            .unwrap_or(1);
        div()
            .flex_none()
            .h(px(FILTER_HEIGHT))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .border_b_1()
            .border_color(colors.border_subtle)
            // Chips clip before the state tabs do on narrow windows; every
            // chip stays reachable by its key and from Cmd+P.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .children(sources)
                    .child(div().flex_none().w(px(space::S8)))
                    .children(priorities),
            )
            .child(
                Segmented::new(
                    "inbox-state",
                    StateFilter::ALL.map(StateFilter::label),
                    state_ix,
                )
                .on_select(select(cx, |this, ix, cx| {
                    if let Some(state) = StateFilter::ALL.get(ix) {
                        this.show_state(*state, cx);
                    }
                })),
            )
    }

    fn render_list(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if !self.loaded {
            return div().flex_1().into_any_element();
        }
        if self.rows.is_empty() {
            let filter = self.engine.filter();
            let (headline, body, clear) = rows::empty_copy(&self.engine, &filter);
            let icon = if clear {
                IconName::Search
            } else {
                IconName::Inbox
            };
            let mut empty = EmptyState::new(icon, headline).body(body);
            if clear {
                empty = empty.action(
                    Button::new("inbox-clear", "Clear filters")
                        .shortcut(Kbd::parse(CLEAR_KEYS).expect("static binding"))
                        .on_press(press(cx, |this, _, cx| this.set_filter(Filter::clear, cx))),
                );
            }
            return div().flex_1().child(empty).into_any_element();
        }
        let theme = theme.clone();
        // The list is the tour's first anchor once it has rows (spec 0020).
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                uniform_list(
                    "inbox-rows",
                    self.rows.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        range
                            .map(|ix| this.render_row(ix, &theme, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .flex_1()
                .py(px(space::S4)),
            )
            .children(adoken_ui::tour::anchor(anchors::LIST, cx))
            .into_any_element()
    }

    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let colors = &theme.colors;
        let Some(n) = self.rows.get(ix).and_then(|id| self.engine.get(*id)) else {
            return div().h(px(ROW_HEIGHT)).into_any_element();
        };
        let text = rows::row_text(n, self.engine.now());
        let (icon, tint) = glyph_style(text.glyph, colors);
        let lead = Icon::new(icon).size(size::ICON_MD).color(if text.unread {
            tint
        } else {
            colors.text_tertiary
        });
        let (p_icon, p_color) = priority_style(text.priority, colors);
        let mut row = ListRow::new(("inbox-row", ix), text.title)
            .height(ROW_HEIGHT)
            .subtitle(text.subtitle)
            .leading(div().relative().child(lead).when(text.unread, |d| {
                d.child(
                    div()
                        .absolute()
                        .top(px(-space::S2))
                        .right(px(-space::S2))
                        .size(px(space::S6))
                        .rounded(px(radius::FULL))
                        .bg(colors.accent_solid),
                )
            }))
            // Who caused it (spec 0012): initials until their picture lands.
            .when_some(
                n.actor.as_deref().zip(n.ctx.actor_name.as_deref()),
                |row, (actor, name)| {
                    row.trailing(
                        crate::avatar::Avatar::new(actor.to_owned(), name.to_owned())
                            .size(crate::avatar::AvatarSize::Sm),
                    )
                },
            )
            .trailing(Icon::new(p_icon).size(size::ICON_SM).color(p_color))
            .trailing(
                div()
                    .w(px(space::S40))
                    .flex()
                    .justify_end()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(text.time),
            )
            .selected(ix == self.selected)
            .track_selected_bounds(self.row_bounds.clone())
            .on_secondary_click(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.select(ix, cx);
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                },
            ))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.select(ix, cx);
                    if event.click_count() > 1 {
                        this.open(&Open, window, cx);
                    }
                }),
            );
        if let Some(count) = text.count {
            row = row.badge(Chip::new(count).tone(ChipTone::Accent));
        }
        if text.snoozed_badge {
            row = row.badge(Chip::new("Snoozed").tone(ChipTone::Neutral));
        }
        if rows::auto_complete_guard(n).is_some() {
            row = row.badge(Chip::new("c Cancel auto-complete").tone(ChipTone::Danger));
        }
        if rows::account_open(n) == Some(rows::AccountOpen::ReplaceToken) {
            row = row.badge(Chip::new("↩ Replace token…").tone(ChipTone::Neutral));
        }
        div()
            .id(("inbox-row-a11y", ix))
            .role(gpui::Role::ListItem)
            .aria_label(text.label)
            .aria_selected(ix == self.selected)
            .child(row)
            .into_any_element()
    }

    fn render_footer(&self, theme: &Theme) -> impl IntoElement {
        let colors = &theme.colors;
        let hint = |keys: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .child(Kbd::parse(keys).expect("static binding"))
                .child(label)
        };
        let footer = div()
            .flex_none()
            .h(px(FOOTER_HEIGHT))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .border_t_1()
            .border_color(colors.border_subtle)
            .text_color(colors.text_tertiary)
            .type_style(typography::SMALL, theme);
        footer
            .child(hint(OPEN_KEYS, "Open"))
            .child(hint(DONE_KEYS, "Done"))
            .child(hint(READ_KEYS, "Unread"))
            .child(hint(SNOOZE_KEYS, "Snooze"))
            .child(hint(MUTE_KEYS, "Mute"))
            .child(hint(MUTE_SCOPE_KEYS, "Mute…"))
            .child(hint(UNDO_KEYS, "Undo"))
            .when(
                self.selected_id()
                    .and_then(|id| self.engine.get(id))
                    .and_then(rows::auto_complete_guard)
                    .is_some(),
                |d| {
                    d.child(hint(CANCEL_AUTO_COMPLETE_KEYS, "Cancel auto-complete"))
                        .child(hint(OPEN_KEYS, "Review changes"))
                },
            )
            .child(div().flex_1())
            .when_some(self.status.clone(), |d, status| {
                d.child(div().text_color(colors.text_secondary).child(status))
            })
    }

    fn render_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let colors = &theme.colors;
        let menu = self.menu.as_ref()?;
        let (title, items, selected): (&str, Vec<String>, usize) = match menu {
            Menu::Snooze { selected } => (
                "Snooze until",
                SnoozeChoice::MENU
                    .iter()
                    .map(|c| c.label(DEFAULT_MORNING))
                    .collect(),
                *selected,
            ),
            Menu::Mute { scopes, selected } => {
                ("Mute", scopes.iter().map(Mute::label).collect(), *selected)
            }
        };
        let rows = items.into_iter().enumerate().map(|(i, label)| {
            div()
                .id(("inbox-menu", i))
                .h(px(size::PALETTE_ROW_COMPACT))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .rounded(px(radius::SM))
                .cursor_pointer()
                .text_color(colors.text_primary)
                .type_style(typography::BODY, theme)
                .when(i == selected, |d| d.bg(colors.bg_selected))
                .when(i != selected, |d| d.hover(|s| s.bg(colors.bg_hover)))
                .role(gpui::Role::MenuItem)
                .aria_label(label.clone())
                .child(div().flex_1().truncate().child(label))
                .on_click(cx.listener(move |this, _, _, cx| this.pick(i, cx)))
        });
        Some(
            div()
                .absolute()
                .top(px(HEADER_HEIGHT + FILTER_HEIGHT + space::S8))
                .right(px(space::S16))
                .w(px(MENU_WIDTH))
                .p(px(space::S4))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_default)
                .bg(colors.bg_overlay)
                .shadow(theme.shadows(elevation::LEVEL_2))
                .id("inbox-menu")
                .role(gpui::Role::Menu)
                .aria_label(title)
                .child(
                    div()
                        .px(px(space::S8))
                        .py(px(space::S4))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, theme)
                        .child(title),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_toast(&self, theme: &Theme) -> Option<AnyElement> {
        let colors = &theme.colors;
        let banner = self.toast.as_ref()?;
        Some(
            div()
                .absolute()
                .bottom(px(FOOTER_HEIGHT + space::S12))
                .right(px(space::S16))
                .w(px(TOAST_WIDTH))
                .p(px(space::S12))
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_default)
                .bg(colors.bg_overlay)
                .shadow(theme.shadows(elevation::LEVEL_2))
                .flex()
                .gap(px(space::S8))
                .id("inbox-toast")
                .role(gpui::Role::Alert)
                .aria_label(format!("{}: {}", banner.title, banner.body))
                .child(
                    Icon::new(IconName::Bell)
                        .size(size::ICON_MD)
                        .color(colors.accent_fg),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_color(colors.text_primary)
                                .type_style(typography::BODY_STRONG, theme)
                                .child(banner.title.clone()),
                        )
                        .child(
                            div()
                                .text_color(colors.text_secondary)
                                .type_style(typography::SMALL, theme)
                                .child(banner.body.clone()),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// A `Segmented::on_select` handler that runs on this view.
fn select(
    cx: &Context<InboxView>,
    f: impl Fn(&mut InboxView, usize, &mut Context<InboxView>) + 'static,
) -> impl Fn(usize, &mut Window, &mut App) + 'static {
    let view = cx.entity().downgrade();
    move |ix, _, cx| {
        view.update(cx, |this, cx| f(this, ix, cx)).ok();
    }
}

/// A `Button::on_press` handler that runs on this view.
fn press(
    cx: &Context<InboxView>,
    f: impl Fn(&mut InboxView, &mut Window, &mut Context<InboxView>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = cx.entity().downgrade();
    move |window, cx| {
        view.update(cx, |this, cx| f(this, window, cx)).ok();
    }
}

fn glyph_style(glyph: Glyph, colors: &adoken_theme::ColorTokens<Hsla>) -> (IconName, Hsla) {
    match glyph {
        Glyph::PullRequest => (IconName::GitPullRequest, colors.pr_active),
        Glyph::WorkItem => (IconName::SquareCheck, colors.wi_task),
        Glyph::BuildFailed => (IconName::CircleX, colors.pipeline_failed),
        Glyph::BuildPassed => (IconName::CircleCheck, colors.pipeline_succeeded),
        Glyph::Mention => (IconName::AtSign, colors.accent_fg),
        Glyph::Nudge => (IconName::Lightbulb, colors.status_warning_fg),
    }
}

/// tokens.md §2.8: notification P0..P3 = `priority.p1..p4`.
fn priority_style(p: Priority, colors: &adoken_theme::ColorTokens<Hsla>) -> (IconName, Hsla) {
    match p {
        Priority::P0 => (IconName::ChevronsUp, colors.priority_p1),
        Priority::P1 => (IconName::ChevronUp, colors.priority_p2),
        Priority::P2 => (IconName::Minus, colors.priority_p3),
        Priority::P3 => (IconName::ChevronDown, colors.priority_p4),
    }
}

impl Render for InboxView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let body = self.render_list(&theme, cx);
        let menu = self.render_menu(&theme, cx);
        let toast = self.render_toast(&theme);
        div()
            .id("inbox")
            .key_context(self.key_context())
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .text_color(theme.colors.text_primary)
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                crate::pin_actions::toggle(
                    this.pin_subject(),
                    this.selected_id()
                        .and_then(|id| this.engine.get(id))
                        .map(|n| n.ctx.title.clone())
                        .unwrap_or_default(),
                    window,
                    cx,
                );
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
            .on_action(cx.listener(Self::open))
            .on_action(cx.listener(Self::mark_done))
            .on_action(cx.listener(Self::toggle_read))
            .on_action(cx.listener(Self::restore))
            .on_action(cx.listener(Self::cancel_auto_complete))
            .on_action(cx.listener(Self::snooze))
            .on_action(cx.listener(Self::mute_subject))
            .on_action(cx.listener(Self::mute_scope))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| this.select(0, cx)))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.select(this.rows.len().saturating_sub(1), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSourcePullRequests, _, cx| {
                this.toggle_source(Source::PullRequests, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSourceWorkItems, _, cx| {
                this.toggle_source(Source::WorkItems, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSourcePipelines, _, cx| {
                this.toggle_source(Source::Pipelines, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSourceMentions, _, cx| {
                this.toggle_source(Source::Mentions, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSourceNudges, _, cx| {
                this.toggle_source(Source::Nudges, cx)
            }))
            .on_action(cx.listener(|this, _: &TogglePriority0, _, cx| {
                this.toggle_priority(Priority::P0, cx)
            }))
            .on_action(cx.listener(|this, _: &TogglePriority1, _, cx| {
                this.toggle_priority(Priority::P1, cx)
            }))
            .on_action(cx.listener(|this, _: &TogglePriority2, _, cx| {
                this.toggle_priority(Priority::P2, cx)
            }))
            .on_action(cx.listener(|this, _: &TogglePriority3, _, cx| {
                this.toggle_priority(Priority::P3, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowUnread, _, cx| this.show_state(StateFilter::Unread, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowAll, _, cx| this.show_state(StateFilter::All, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowSnoozed, _, cx| {
                    this.show_state(StateFilter::Snoozed, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &ShowDone, _, cx| this.show_state(StateFilter::Done, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ClearFilters, _, cx| this.set_filter(Filter::clear, cx)),
            )
            .on_action(cx.listener(Self::toggle_rules))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .child(self.render_header(&theme, cx))
            // Tour anchors (spec 0020) wrap the filters and the footer.
            .child(
                div()
                    .flex_none()
                    .child(self.render_filters(&theme, cx))
                    .children(adoken_ui::tour::anchor(anchors::FILTERS, cx)),
            )
            .child(body)
            .child(
                div()
                    .flex_none()
                    .child(self.render_footer(&theme))
                    .children(adoken_ui::tour::anchor(anchors::FOOTER, cx)),
            )
            .children(menu)
            .children(toast)
            .children(self.row_menu.element())
    }
}

// ── background IO ───────────────────────────────────────────────────────

struct Loaded {
    engine: Engine,
    cursor: i64,
}

/// Load local state; on the very first run, ingest the backlog and the
/// "needs you" baseline silently (no welcome flood), and save.
fn first_load(store: &Store, clock: Arc<dyn Clock>) -> adoken_store::Result<Loaded> {
    let loaded = persist::load(store)?;
    let first_run = loaded.cursor == 0 && loaded.state.items.is_empty();
    let mut engine = Engine::from_state(clock.clone(), loaded.state);
    let mut cursor = loaded.cursor;
    if first_run {
        let enricher = Enricher::load(store)?;
        for incoming in source::baseline(store, &enricher, clock.now())? {
            engine.ingest_silently(incoming);
        }
        let mut progress = Progress::default();
        loop {
            let batch = source::fetch(store, cursor)?;
            for incoming in batch.incoming {
                engine.ingest_silently(incoming);
            }
            for subject in &batch.resolved {
                engine.resolve_nudge(subject);
            }
            cursor = batch.progress.cursor.unwrap_or(cursor);
            progress.cursor = Some(cursor);
            progress.queue_upto = batch.progress.queue_upto.or(progress.queue_upto);
            if !batch.more {
                break;
            }
        }
        persist::save(store, &engine.take_changes(), progress)?;
    }
    engine.prune();
    Ok(Loaded { engine, cursor })
}

/// Poll forever: read → ingest (UI thread) → save. Stops when the view is
/// dropped.
async fn poll(this: gpui::WeakEntity<InboxView>, store: Store, cx: &mut gpui::AsyncApp) {
    loop {
        let Ok(cursor) = this.read_with(cx, |this, _| this.cursor) else {
            return;
        };
        let job = {
            let store = store.clone();
            cx.background_executor()
                .spawn(async move { source::fetch(&store, cursor) })
        };
        let batch = match job.await {
            Ok(batch) => batch,
            Err(error) => {
                eprintln!("adoken: inbox couldn't read events: {error}");
                source::Batch::default()
            }
        };
        let more = batch.more;
        let Ok((changes, progress)) = this.update(cx, |this, cx| {
            let progress = this.apply_batch(batch, cx);
            (this.engine.take_changes(), progress)
        }) else {
            return;
        };
        if !changes.is_empty() || progress != Progress::default() {
            let store = store.clone();
            let saved = cx
                .background_executor()
                .spawn(async move { persist::save(&store, &changes, progress) })
                .await;
            match saved {
                Ok(()) => {
                    if let Some(cursor) = progress.cursor {
                        this.update(cx, |this, _| this.cursor = cursor).ok();
                    }
                }
                Err(error) => eprintln!("adoken: inbox couldn't save: {error}"),
            }
        }
        if !more {
            cx.background_executor().timer(POLL).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_names_an_inbox_action_once() {
        let commands = commands();
        let mut titles: Vec<_> = commands.iter().map(|c| c.title).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), commands.len());
    }

    /// Spec 0025 AC-3: digits pick nothing in the open snooze/mute menu;
    /// ↑/↓/↩ still do, and the list keeps its `1`–`5` source chips.
    #[test]
    fn ac3_digits_pick_nothing_in_the_menu_and_still_filter_the_list() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let resolve = |keys: &str, own: &str| {
            let stack = [
                KeyContext::parse("Workspace").expect("context"),
                KeyContext::parse(own).expect("context"),
            ];
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
            bindings.first().map(|binding| binding.action().name())
        };
        for digit in 1..=9 {
            let action = resolve(&digit.to_string(), CTX_MENU);
            assert!(
                action.is_none_or(|name| !name.starts_with("inbox::")),
                "{digit} in the menu runs {action:?}"
            );
        }
        assert_eq!(resolve("down", CTX_MENU), Some(SelectNext.name()));
        assert_eq!(resolve("enter", CTX_MENU), Some(Confirm.name()));
        assert_eq!(
            resolve("1", CTX_LIST),
            Some(ToggleSourcePullRequests.name())
        );
    }
}
