//! The main window shell (docs/design/principles.md §2): titlebar with the
//! palette trigger, sidebar ([`sidebar`]: sections, settings, org; ⌘B
//! collapses it to a rail), the section's content, and the statusbar (sync
//! state, org). `?` / ⌘/ open the shortcut cheat sheet ([`crate::help`]).
//!
//! The store opens on a background executor after the first frame; until
//! then the chrome paints and the palette searches commands. If opening
//! takes longer than [`LOADING_GRACE`], the fist splash covers the content
//! until it's ready. Sections whose views haven't shipped show an honest
//! empty state.

pub mod history;
pub mod nav;
pub mod navigate;
pub mod pinned;
pub mod sidebar;

use std::path::PathBuf;
use std::time::Duration;

use adoken_model::{Millis, Person, PrStatus, Vote};
use adoken_store::{PrFilter, Store};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::loaders::{SPLASH_EXIT, splash_overlay};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, EmptyState, Icon, IconName, Kbd, Theme, TypeStyleExt,
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton, Subscription, Task,
    Window, actions, div, prelude::*, px,
};

use crate::avatar;
use crate::background;
use crate::help::{self, CheatSheet};
use crate::inbox::{self, InboxEvent, InboxView};
use crate::palette::catalog::Target;
use crate::palette::{self, Palette, PaletteCommand, PaletteEvent, now_ms};
use crate::pipelines::{self, PipelinesEvent, PipelinesView};
use crate::review::{self, ReviewView};
use crate::sound;
use crate::sync::{ConnectedOrg, LoadOlderRuns, SyncHandle, SyncNow, SyncState};
use crate::today::{self, TodayEvent, TodayView};
use crate::workitems::{self, WorkItemsEvent, WorkItemsView};
pub use nav::Nav;

actions!(
    shell,
    [
        /// Show Today (nudges and my active items).
        ShowToday,
        /// Show the inbox (notification center).
        ShowInbox,
        /// Show pull requests.
        ShowPullRequests,
        /// Show work items.
        ShowWorkItems,
        /// Show pipelines and their runs.
        ShowPipelines,
        /// Show Search (opens the palette over it from the sidebar).
        ShowSearch,
        /// Collapse or expand the sidebar.
        ToggleSidebar,
        /// Go back to where you were before (spec 0018).
        GoBack,
        /// Go forward again after going back (spec 0018).
        GoForward,
    ]
);

/// ⌘B collapses the sidebar to its rail.
pub const TOGGLE_SIDEBAR_KEYS: &str = "secondary-b";
const STATUSBAR_HEIGHT: f32 = 24.0;
/// Room for the macOS traffic lights in the transparent titlebar.
const TRAFFIC_LIGHTS_WIDTH: f32 = 84.0;
/// Loading indicators appear only after this long (principles.md §3.2).
const LOADING_GRACE: Duration = Duration::from_millis(150);
/// How often demo mode appends due fake sync events (the inbox reads them).
const DEMO_FEED_INTERVAL: Duration = Duration::from_secs(30);
/// Statusbar re-render cadence in connected mode (relative times).
const STATUS_TICK: Duration = Duration::from_secs(5);
/// Settings file inside the store's directory (tokens.md §9.4).
pub const SETTINGS_FILE: &str = "settings.toml";

pub fn bindings() -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new(Nav::Today.keys(), ShowToday, Some("Workspace")),
        KeyBinding::new(Nav::Inbox.keys(), ShowInbox, Some("Workspace")),
        KeyBinding::new(
            Nav::PullRequests.keys(),
            ShowPullRequests,
            Some("Workspace"),
        ),
        KeyBinding::new(Nav::WorkItems.keys(), ShowWorkItems, Some("Workspace")),
        KeyBinding::new(Nav::Pipelines.keys(), ShowPipelines, Some("Workspace")),
        KeyBinding::new(Nav::Search.keys(), ShowSearch, Some("Workspace")),
        KeyBinding::new(TOGGLE_SIDEBAR_KEYS, ToggleSidebar, None),
    ];
    bindings.extend(help::bindings());
    bindings.extend(navigate::bindings());
    bindings.extend(crate::sync::bindings());
    bindings.extend(crate::activity::bindings());
    bindings
}

/// Commands the shell contributes to the palette.
pub fn commands() -> Vec<PaletteCommand> {
    let mut commands = vec![
        PaletteCommand {
            title: "Go to Today",
            keys: Some(Nav::Today.keys()),
            action: Box::new(ShowToday),
        },
        PaletteCommand {
            title: "Go to Inbox",
            keys: Some(Nav::Inbox.keys()),
            action: Box::new(ShowInbox),
        },
        PaletteCommand {
            title: "Go to Pull requests",
            keys: Some(Nav::PullRequests.keys()),
            action: Box::new(ShowPullRequests),
        },
        PaletteCommand {
            title: "Go to Work items",
            keys: Some(Nav::WorkItems.keys()),
            action: Box::new(ShowWorkItems),
        },
        PaletteCommand {
            title: "Go to Pipelines",
            keys: Some(Nav::Pipelines.keys()),
            action: Box::new(ShowPipelines),
        },
        PaletteCommand {
            title: "Go to Search",
            keys: Some(Nav::Search.keys()),
            action: Box::new(ShowSearch),
        },
        PaletteCommand {
            title: "Toggle sidebar",
            keys: Some(TOGGLE_SIDEBAR_KEYS),
            action: Box::new(ToggleSidebar),
        },
    ];
    commands.extend(navigate::commands());
    commands.extend(help::commands());
    commands.extend(crate::settings::commands());
    commands.extend(crate::hotkey::commands());
    commands.extend(crate::os_notify::commands());
    commands.extend(today::commands());
    commands.extend(inbox::commands());
    commands.extend(review::palette_commands());
    commands.extend(workitems::palette_commands());
    commands.extend(pipelines::palette_commands());
    commands.extend(crate::browse::palette_commands());
    commands.extend(crate::activity::commands());
    commands.extend(crate::resource::commands());
    commands.extend(crate::ken::commands()); // ken
    commands.extend(crate::celebrate::commands());
    commands.extend(crate::tours::commands());
    commands
}

/// Which store the shell reads.
#[derive(Debug, Clone)]
pub enum StoreSource {
    /// `ADOKEN_DEMO=1`: the fake Contoso org, seeded into its own directory.
    Demo(PathBuf),
    /// The signed-in org's store (`<data_dir>/orgs/<org>/adoken.sqlite`),
    /// kept fresh by the sync scheduler ([`crate::sync`]).
    Connected(ConnectedOrg),
    /// Already open (gallery stories, tests).
    Open { store: Store, demo: bool },
}

impl StoreSource {
    fn is_demo(&self) -> bool {
        match self {
            StoreSource::Demo(_) => true,
            StoreSource::Connected(_) => false,
            StoreSource::Open { demo, .. } => *demo,
        }
    }

    /// The directory the app owns for this source (settings, background art
    /// cache); `None` for stores opened elsewhere (stories).
    fn owned_dir(&self) -> Option<PathBuf> {
        match self {
            StoreSource::Demo(dir) => Some(dir.clone()),
            StoreSource::Connected(org) => Some(org.data_dir.clone()),
            StoreSource::Open { .. } => None,
        }
    }

    fn settings_path(&self, store: &Store) -> PathBuf {
        let dir = self
            .owned_dir()
            .unwrap_or_else(|| store.path().parent().map(PathBuf::from).unwrap_or_default());
        dir.join(SETTINGS_FILE)
    }

    /// Blocking open; call on a background executor.
    fn open(&self) -> Result<Store, String> {
        match self {
            StoreSource::Demo(dir) => adoken_demo::open_store(dir).map_err(|e| e.to_string()),
            StoreSource::Connected(org) => {
                let path = org.store_path();
                crate::sync::prepare_org_store(&org.data_dir, &path).map_err(|e| e.to_string())?;
                Store::open(path).map_err(|e| e.to_string())
            }
            StoreSource::Open { store, .. } => Ok(store.clone()),
        }
    }
}

/// What the statusbar knows once the store is open.
#[derive(Debug, Clone, Default)]
struct Snapshot {
    org: Option<String>,
    /// The signed-in user (sidebar account row, spec 0012 Q-2).
    me: Option<Person>,
    last_sync: Option<Millis>,
    /// Active PRs waiting on my vote (sidebar count: actionable only).
    reviews_waiting: usize,
}

impl Snapshot {
    fn read(store: &Store) -> adoken_store::Result<Self> {
        let me = store.me()?;
        let reviews_waiting = match me.as_ref().map(|me| &me.id) {
            Some(me) => store
                .prs(&PrFilter {
                    status: Some(PrStatus::Active),
                    reviewer: Some(me.clone()),
                    ..PrFilter::default()
                })?
                .iter()
                .filter(|pr| {
                    !pr.is_draft
                        && pr.author != *me
                        && pr
                            .reviewers
                            .iter()
                            .any(|r| r.person == *me && r.vote == Vote::NoVote && !r.has_declined)
                })
                .count(),
            None => 0,
        };
        Ok(Self {
            org: store.org()?.map(|org| org.name),
            me,
            last_sync: store.last_sync()?,
            reviews_waiting,
        })
    }
}

enum Load {
    Opening { show_indicator: bool },
    Ready,
    Failed(String),
}

/// Views hosted by the shell, created once the store is open.
struct Views {
    today: Entity<TodayView>,
    inbox: Entity<InboxView>,
    review: Entity<ReviewView>,
    workitems: Entity<WorkItemsView>,
    pipelines: Entity<PipelinesView>,
}

impl Views {
    fn focus_handle(&self, nav: Nav, cx: &App) -> Option<FocusHandle> {
        match nav {
            Nav::Today => Some(self.today.focus_handle(cx)),
            Nav::Inbox => Some(self.inbox.focus_handle(cx)),
            Nav::PullRequests => Some(self.review.focus_handle(cx)),
            Nav::WorkItems => Some(self.workitems.focus_handle(cx)),
            Nav::Pipelines => Some(self.pipelines.focus_handle(cx)),
            Nav::Search => None,
        }
    }
}

pub struct Shell {
    source: StoreSource,
    nav: Nav,
    sidebar_collapsed: bool,
    /// User toggles so far; keys the width animation (0 = paint at rest).
    sidebar_toggles: usize,
    focus: FocusHandle,
    palette: Entity<Palette>,
    cheat_sheet: Entity<CheatSheet>,
    toasts: Entity<adoken_ui::ToastStack>,
    pins: Option<Entity<crate::pins::Pins>>,
    pinned_collapsed: bool,
    pin_state: pinned::State,
    store: Option<Store>,
    load: Load,
    /// The splash is playing its exit after a slow open.
    splash_fading: bool,
    /// In-flight `[sidebar]` read; a newer read replaces (cancels) it.
    sidebar_read: Option<Task<()>>,
    snapshot: Snapshot,
    views: Option<Views>,
    /// Unread inbox count (sidebar badge), reported by the inbox view.
    inbox_unread: usize,
    /// Ken arcade layer (ken.md), mounted once the store is open. // ken
    ken: Option<crate::ken::KenHandles>,
    /// Connected mode: the running sync scheduler and its latest state.
    sync: Option<SyncHandle>,
    sync_state: Option<SyncState>,
    /// Activity drawer and statusbar items (spec 0016); the drawer's view
    /// is created on first open.
    activity: Entity<crate::activity::Activity>,
    activity_panel: Option<Entity<crate::activity::ActivityPanel>>,
    /// Focus before the drawer opened; `Esc` returns it (FR-3).
    activity_return: Option<FocusHandle>,
    /// Where the sidebar last painted (the drawer never changes its height,
    /// FR-1; smoke runs assert it).
    sidebar_bounds: adoken_ui::action_menu::BoundsSlot,
    /// Drops to background cadence 5 min after the window loses focus.
    background_timer: Option<Task<()>>,
    /// Statusbar resource meter (spec 0023), off unless `[activity]
    /// resource_meter`.
    meter: Entity<crate::resource::ResourceMeter>,
    /// Guided tours (spec 0020): first use, replay, the coach mark layer.
    tours: Entity<crate::tours::Tours>,
    /// Back / forward (spec 0018): one history per main window, in memory.
    history: history::History<navigate::Entry>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Shell {
    pub fn new(
        source: StoreSource,
        commands: Vec<PaletteCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        sound::init(cx);
        let toasts = cx.new(|_| adoken_ui::ToastStack::new(STATUSBAR_HEIGHT));
        adoken_ui::ToastStack::install(toasts.clone(), cx);
        let cheat_sheet = {
            let commands = help::command_infos(&commands);
            cx.new(|cx| CheatSheet::new(commands, cx))
        };
        let palette = cx.new(|cx| Palette::new(commands, cx));
        let palette_events =
            cx.subscribe_in(&palette, window, |this, _, event, window, cx| match event {
                PaletteEvent::Open(target) => this.open_target(target.clone(), window, cx),
            });
        let settings_events = crate::settings::observe(cx, |this, cx| {
            this.load_sidebar(cx);
            this.activity
                .update(cx, |activity, cx| activity.load_settings(cx));
        });
        let activity = cx.new(|cx| {
            let mut activity = crate::activity::Activity::new(source.is_demo(), cx);
            activity.load_settings(cx);
            activity
        });
        let meter = cx.new(|cx| crate::resource::ResourceMeter::new(window, cx));
        let meter_observer = cx.observe(&meter, |_, _, cx| cx.notify());
        let tours = {
            let parts = crate::tours::ShellParts {
                shell: cx.entity().downgrade(),
                palette: palette.clone(),
                cheat_sheet: cheat_sheet.clone(),
            };
            let shell = cx.entity();
            let insets = (size::TITLEBAR, STATUSBAR_HEIGHT);
            cx.new(|cx| crate::tours::Tours::for_shell(parts, &shell, insets, window, cx))
        };
        let activity_observer = cx.observe_in(&activity, window, |this, activity, window, cx| {
            let on = activity.read(cx).settings().resource_meter;
            this.meter
                .update(cx, |meter, cx| meter.set_enabled(on, window, cx));
            cx.notify()
        });
        let activity_events = cx.subscribe_in(&activity, window, Self::on_activity_event);
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            this.activation_changed(window.is_window_active(), cx)
        });
        let shell = cx.entity().downgrade();
        {
            let shell = cx.entity().downgrade();
            let handle = window.window_handle();
            crate::activity::set_opener(
                move |tab, cx| {
                    let shell = shell.clone();
                    handle
                        .update(cx, |_, window, cx| {
                            window.activate_window();
                            shell
                                .update(cx, |this, cx| this.open_activity(tab, false, window, cx))
                                .ok();
                        })
                        .ok();
                },
                cx,
            );
        }
        let handle = window.window_handle();
        crate::os_notify::on_click(cx, move |click, cx| {
            handle
                .update(cx, |_, window, cx| {
                    window.activate_window();
                    shell
                        .update(cx, |this, cx| match click.route {
                            crate::os_notify::Route::Target(target) => {
                                this.open_target(target, window, cx)
                            }
                            crate::os_notify::Route::Inbox => this.show(Nav::Inbox, window, cx),
                        })
                        .ok();
                })
                .ok();
        });

        let job = {
            let source = source.clone();
            cx.background_executor().spawn(async move {
                let store = source.open()?;
                let snapshot = Snapshot::read(&store).map_err(|e| e.to_string())?;
                let settings_text =
                    std::fs::read_to_string(source.settings_path(&store)).unwrap_or_default();
                let pin_settings = crate::pins::PinsSettings::from_toml(&settings_text);
                let pins = crate::pins::refresh_blocking(
                    &store,
                    crate::pins::snapshot::Options {
                        now: now_ms(),
                        auto_unpin: pin_settings.auto_unpin,
                        stale_after: (!source.is_demo())
                            .then_some(crate::pins::snapshot::STALE_AFTER),
                    },
                )
                .map_err(|error| error.to_string())?;
                Ok::<_, String>((store, snapshot, pins, pin_settings))
            })
        };
        let open = cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            this.update_in(cx, |this, window, cx| match result {
                Ok((store, snapshot, pins, pin_settings)) => {
                    this.store_opened(store, snapshot, pins, pin_settings, window, cx)
                }
                Err(error) => {
                    eprintln!("adoken: couldn't open the local store: {error}");
                    this.load = Load::Failed(error);
                    cx.notify();
                }
            })
            .ok();
        });
        let grace = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LOADING_GRACE).await;
            this.update(cx, |this, cx| {
                if let Load::Opening { show_indicator } = &mut this.load {
                    *show_indicator = true;
                    cx.notify();
                }
            })
            .ok();
        });

        let mut shell = Self {
            source,
            nav: Nav::Today,
            sidebar_collapsed: false,
            sidebar_toggles: 0,
            focus: cx.focus_handle(),
            palette,
            cheat_sheet,
            toasts,
            pins: None,
            pinned_collapsed: false,
            pin_state: pinned::State::new(cx),
            store: None,
            load: Load::Opening {
                show_indicator: false,
            },
            splash_fading: false,
            sidebar_read: None,
            snapshot: Snapshot::default(),
            views: None,
            inbox_unread: 0,
            ken: None, // ken
            sync: None,
            sync_state: None,
            activity,
            activity_panel: None,
            activity_return: None,
            sidebar_bounds: Default::default(),
            background_timer: None,
            meter,
            tours,
            history: navigate::launch_history(Nav::Today),
            _tasks: vec![open, grace],
            _subscriptions: vec![
                palette_events,
                settings_events,
                activation,
                activity_observer,
                activity_events,
                meter_observer,
            ],
        };
        // Any key but back / forward cancels a restore still waiting for
        // its view's data (spec 0018 FR-8).
        shell
            ._subscriptions
            .push(cx.observe_keystrokes(|shell, event, _, cx| {
                let history = event
                    .action
                    .as_ref()
                    .is_some_and(|a| a.as_any().is::<GoBack>() || a.as_any().is::<GoForward>());
                if !history {
                    shell.cancel_pending_restores(cx);
                }
            }));
        shell._subscriptions.push(cx.on_focus_in(
            &shell.pin_state.focus,
            window,
            |shell, _, cx| {
                shell.pin_state.focused = true;
                if shell.sidebar_collapsed && shell.pin_state.cursor >= Nav::ALL.len() {
                    shell.pin_state.peek = true;
                }
                cx.notify();
            },
        ));
        shell._subscriptions.push(cx.on_focus_out(
            &shell.pin_state.focus,
            window,
            |shell, _, _, cx| {
                shell.pin_state.focused = false;
                cx.notify();
            },
        ));
        shell.load_sidebar(cx);
        shell
    }

    /// Reads `[sidebar]` off the UI thread (first paint uses the default).
    fn load_sidebar(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        let read = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            sidebar::SidebarSettings::from_toml(&text)
        });
        let task = cx.spawn(async move |this, cx| {
            let settings = read.await;
            this.update(cx, |this, cx| {
                if this.sidebar_collapsed != settings.collapsed
                    || this.pinned_collapsed != settings.pinned_collapsed
                {
                    this.sidebar_collapsed = settings.collapsed;
                    this.pinned_collapsed = settings.pinned_collapsed;
                    cx.notify();
                }
            })
            .ok();
        });
        self.sidebar_read = Some(task);
    }

    fn persist_sidebar(&self, cx: &mut Context<Self>) {
        let settings = sidebar::SidebarSettings {
            collapsed: self.sidebar_collapsed,
            pinned_collapsed: self.pinned_collapsed,
        };
        crate::settings::write_table(cx, sidebar::SETTINGS_TABLE, settings.to_table());
    }

    /// Collapsed (rail) or expanded sidebar, without persisting (stories).
    pub fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.sidebar_collapsed = collapsed;
        cx.notify();
    }

    /// Show `state` as if the scheduler reported it (stories, smoke runs).
    pub fn set_sync_state(&mut self, state: Option<SyncState>, cx: &mut Context<Self>) {
        self.activity.update(cx, |activity, cx| {
            activity.set_sync_state(state.clone(), cx)
        });
        self.sync_state = state;
        cx.notify();
    }

    /// The sidebar's bounds as last painted.
    pub fn sidebar_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.sidebar_bounds.get()
    }

    /// The PR list's selected row (smoke runs).
    pub fn selected_pr_id(&self, cx: &App) -> Option<i64> {
        self.views
            .as_ref()
            .and_then(|views| views.review.read(cx).selected_pr_id())
    }

    /// The shown section's view has the keyboard.
    pub fn content_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.views
            .as_ref()
            .and_then(|views| views.focus_handle(self.nav, cx))
            .is_some_and(|handle| handle.contains_focused(window, cx))
    }

    /// The PR the review view has open (smoke runs).
    pub fn open_review_id(&self, cx: &App) -> Option<i64> {
        self.views
            .as_ref()
            .and_then(|views| views.review.read(cx).open_review_id(cx))
    }

    pub fn cheat_sheet(&self) -> &Entity<CheatSheet> {
        &self.cheat_sheet
    }

    pub fn palette(&self) -> &Entity<Palette> {
        &self.palette
    }

    /// Guided tours (spec 0020).
    pub fn tours(&self) -> &Entity<crate::tours::Tours> {
        &self.tours
    }

    pub fn nav(&self) -> Nav {
        self.nav
    }

    /// The Work items view, once the store is open (smoke tests).
    pub fn work_items(&self) -> Option<&Entity<WorkItemsView>> {
        self.views.as_ref().map(|views| &views.workitems)
    }

    /// The Today view, once the store is open (smoke tests).
    pub fn today_view(&self) -> Option<&Entity<TodayView>> {
        self.views.as_ref().map(|views| &views.today)
    }

    fn store_opened(
        &mut self,
        store: Store,
        snapshot: Snapshot,
        pin_snapshot: crate::pins::snapshot::Snapshot,
        pin_settings: crate::pins::PinsSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let settings = self.source.settings_path(&store);
        let pins = crate::pins::Pins::install_loaded(
            store.clone(),
            crate::pins::PinsConfig {
                demo: self.source.is_demo(),
                scopes: match &self.source {
                    StoreSource::Connected(org) => org.scopes.clone(),
                    _ => Vec::new(),
                },
            },
            pin_snapshot,
            pin_settings,
            cx,
        );
        self._subscriptions
            .push(cx.observe(&pins, |_, _, cx| cx.notify()));
        self._subscriptions.push(
            cx.subscribe_in(&pins, window, |this, _, event, window, cx| match event {
                crate::pins::PinsEvent::Open(subject) => {
                    this.palette
                        .update(cx, |palette, cx| palette.close(window, cx));
                    this.cheat_sheet
                        .update(cx, |sheet, cx| sheet.close(window, cx));
                    this.pin_state.peek = false;
                    this.pin_state.intent.reset();
                    match subject {
                        adoken_model::PinSubject::Pr(id) => {
                            this.open_target(Target::PullRequest(*id), window, cx)
                        }
                        adoken_model::PinSubject::WorkItem(id) => {
                            this.open_target(Target::WorkItem(*id), window, cx)
                        }
                        adoken_model::PinSubject::Definition { project_id, id } => {
                            let project_id = project_id.clone();
                            let id = *id;
                            this.navigate(None, window, cx, |this, window, cx| {
                                this.set_section(Nav::Pipelines, window, cx);
                                if let Some(views) = &this.views {
                                    views.pipelines.update(cx, |view, cx| {
                                        view.open_definition(&project_id, id, window, cx)
                                    });
                                }
                            });
                        }
                    }
                }
                crate::pins::PinsEvent::OpenInBrowser(url) => {
                    this.pin_state.peek = false;
                    cx.open_url(url);
                    cx.notify();
                }
            }),
        );
        self.pins = Some(pins);
        if let Some(dir) = self.source.owned_dir() {
            background::load(settings.clone(), dir, self.source.is_demo(), cx);
        }
        let sound_settings = {
            let path = settings.clone();
            cx.background_executor()
                .spawn(async move { sound::read_settings(&path) })
        };
        self.palette.update(cx, |palette, cx| {
            palette.load_store(store.clone(), settings, cx)
        });
        self.activity
            .update(cx, |activity, cx| activity.set_store(store.clone(), cx));
        if self.source.is_demo() {
            let hub = crate::sync::activity_hub(cx);
            self._tasks.push(demo_feed(store.clone(), hub, cx));
            // Generated pictures for demo people with an image URL (0012 FR-13).
            let reader = store.clone();
            self._tasks.push(cx.spawn(async move |_, cx| {
                let people = cx.background_spawn(async move { reader.people() }).await;
                if let Ok(people) = people {
                    cx.update(|cx| avatar::use_demo(avatar::demo_ids(&people), cx));
                }
            }));
        }
        if let StoreSource::Connected(org) = &self.source {
            self.start_sync(store.clone(), org.clone(), cx);
        }
        // Review writes (spec 0004): the demo org writes locally; a signed-in
        // org through its sync governor when the token has Review access.
        let access = match (&self.source, &self.sync) {
            (StoreSource::Connected(org), Some(sync)) if org.can_write => {
                review::writes::Access::Remote(sync.write_access())
            }
            (source, _) if source.is_demo() => review::writes::Access::Local,
            _ => review::writes::Access::ReadOnly,
        };
        review::writes::set_access(access, cx);
        // Work item edits (spec 0015): the same split; a signed-in org gets
        // per-item sync even with a read-only token.
        let wi_access = match (&self.source, &self.sync) {
            (StoreSource::Connected(_), Some(sync)) => sync.work_item_access(),
            (source, _) if source.is_demo() => workitems::writes::Access::Local,
            _ => workitems::writes::Access::None,
        };
        workitems::writes::set_access(wi_access, cx);
        workitems::writes::WiEdits::install(store.clone(), cx);
        // Review files (spec 0004 FR-1..FR-3): a signed-in org fetches file
        // lists and blobs through its sync governor; the demo store has them.
        let files = match &self.sync {
            Some(sync) => sync.pr_files(),
            None => tokio::sync::watch::channel(None).1,
        };
        review::blobs::set_files(files, cx);
        let today = today::new(store.clone(), window, cx);
        self._subscriptions.push(
            cx.subscribe_in(&today, window, |this, _, event, window, cx| {
                let target = match *event {
                    TodayEvent::OpenPr(id) => Target::PullRequest(id),
                    TodayEvent::OpenWorkItem(id) => Target::WorkItem(id),
                    TodayEvent::OpenRun(id) => Target::Run(id),
                };
                this.open_target(target, window, cx);
            }),
        );
        let inbox = inbox::new(store.clone(), window, cx);
        let sounds_loaded = {
            let inbox = inbox.clone();
            cx.spawn(async move |_, cx| {
                let settings = sound_settings.await;
                cx.update(|cx| {
                    let enabled = settings.enabled;
                    sound::install(settings, cx);
                    inbox.update(cx, |inbox, _| inbox.set_sounds_enabled(enabled));
                });
            })
        };
        self._tasks.push(sounds_loaded);
        self._subscriptions.push(
            cx.subscribe_in(&inbox, window, |this, _, event, window, cx| match event {
                InboxEvent::Open { target, before } => this.open_target_from(
                    target.clone(),
                    Some(navigate::ViewState::Inbox(before.clone())),
                    window,
                    cx,
                ),
                InboxEvent::Counts { unread, badge } => {
                    this.inbox_unread = *unread;
                    crate::ken::inbox_unread(*unread, cx); // ken
                    crate::os_notify::set_badge(*badge, cx);
                    cx.notify();
                }
                InboxEvent::Delivered(banner) => crate::os_notify::post(banner, cx),
                InboxEvent::Sound(cue) => {
                    sound::play_cue(*cue, window.is_window_active(), cx).ok();
                }
            }),
        );
        self.ken = Some(crate::ken::mount(store.clone(), window, cx)); // ken
        let review = review::new(store.clone(), window, cx);
        self._subscriptions.push(
            cx.subscribe_in(&review, window, |this, _, event, window, cx| match event {
                review::ReviewEvent::OpenWorkItem(id) => {
                    this.open_target(Target::WorkItem(*id), window, cx)
                }
                review::ReviewEvent::Navigated(before) => {
                    let before = match (**before).clone() {
                        review::nav::PullRequestsNavState::List(state) => {
                            navigate::ViewState::PullRequests(state)
                        }
                        review::nav::PullRequestsNavState::Review(state) => {
                            navigate::ViewState::Review(state)
                        }
                    };
                    this.view_navigated(before, false, window, cx)
                }
                review::ReviewEvent::BackToList(before) => {
                    this.review_left(before.clone(), window, cx)
                }
                review::ReviewEvent::ThreadOpened(before) => this.view_navigated(
                    navigate::ViewState::Review(before.clone()),
                    true,
                    window,
                    cx,
                ),
            }),
        );
        let workitems = workitems::new(store.clone(), window, cx);
        self._subscriptions.push(cx.subscribe_in(
            &workitems,
            window,
            |this, _, event, window, cx| match event {
                WorkItemsEvent::OpenPr(id) => {
                    this.open_target(Target::PullRequest(*id), window, cx)
                }
                WorkItemsEvent::Navigated(before) => this.view_navigated(
                    navigate::ViewState::WorkItems((**before).clone()),
                    false,
                    window,
                    cx,
                ),
            },
        ));
        let pipelines = pipelines::new(store.clone(), window, cx);
        self._subscriptions.push(cx.subscribe_in(
            &pipelines,
            window,
            |this, _, event, window, cx| {
                let target = match *event {
                    PipelinesEvent::OpenPr(id) => Target::PullRequest(id),
                    PipelinesEvent::OpenWorkItem(id) => Target::WorkItem(id),
                };
                this.open_target(target, window, cx);
            },
        ));
        self.views = Some(Views {
            today,
            inbox,
            review,
            workitems,
            pipelines,
        });
        // Don't pull focus out of an open overlay.
        if !self.palette.read(cx).is_visible() && !self.cheat_sheet.read(cx).is_visible() {
            self.focus_current(window, cx);
        }
        self.snapshot = snapshot;
        self.store = Some(store);
        if let Load::Opening {
            show_indicator: true,
        } = self.load
        {
            self.splash_fading = true;
            self._tasks.push(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(SPLASH_EXIT).await;
                this.update(cx, |this, cx| {
                    this.splash_fading = false;
                    cx.notify();
                })
                .ok();
            }));
        }
        self.load = Load::Ready;
        cx.notify();
    }

    /// Connected mode: run the scheduler on Tokio, mirror its state into
    /// the statusbar, and re-read the statusbar snapshot after each pass.
    fn start_sync(&mut self, store: Store, org: ConnectedOrg, cx: &mut Context<Self>) {
        let handle = crate::sync::start(store.clone(), org, cx);
        let mut states = handle.watch();
        let watch = cx.spawn(async move |this, cx| {
            loop {
                let state = states.borrow_and_update().clone();
                let refresh = state.is_idle().then(|| {
                    let store = store.clone();
                    cx.background_spawn(async move { Snapshot::read(&store) })
                });
                let snapshot = match refresh {
                    Some(job) => job.await.ok(),
                    None => None,
                };
                let offline = matches!(&state, SyncState::Running(s)
                    if s.phase == adoken_sync::Phase::Offline);
                let alive = this
                    .update(cx, |this, cx| {
                        this.activity.update(cx, |activity, cx| {
                            activity.set_sync_state(Some(state.clone()), cx)
                        });
                        if let Some(views) = &this.views {
                            // Spec 0017 FR-10: Today's data health.
                            views
                                .today
                                .update(cx, |today, cx| today.set_sync_state(&state, cx));
                        }
                        this.sync_state = Some(state);
                        if let Some(snapshot) = snapshot {
                            this.snapshot = snapshot;
                        }
                        if let Some(views) = &this.views {
                            views
                                .review
                                .update(cx, |review, cx| review.set_offline(offline, cx));
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive || states.changed().await.is_err() {
                    return;
                }
            }
        });
        // "Synced 12s ago" and throttle countdowns age without new events.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(STATUS_TICK).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        });
        // Pictures through the sync governor once the token is read (0012).
        let mut fetchers = handle.avatars();
        let avatars = cx.spawn(async move |_, cx| {
            loop {
                let fetcher = fetchers.borrow_and_update().clone();
                cx.update(|cx| match fetcher {
                    Some(fetcher) => avatar::use_fetcher(fetcher, cx),
                    None => avatar::clear(cx),
                });
                if fetchers.changed().await.is_err() {
                    return;
                }
            }
        });
        self._tasks.extend([watch, tick, avatars]);
        self.sync = Some(handle);
        self.follow_account(cx);
    }

    /// Spec 0001 FR-16 / AC-21: a replaced token or changed access turns the
    /// features it gates on or off without a restart.
    fn follow_account(&mut self, cx: &mut Context<Self>) {
        let Some(account) = crate::account::model(cx) else {
            return;
        };
        let key = |meta: &adoken_ado::PatMeta| {
            (
                crate::account::grants(meta),
                meta.scopes.clone(),
                meta.created_at,
            )
        };
        let mut applied = account.read(cx).meta().map(key);
        self._subscriptions
            .push(cx.observe(&account, move |this, account, cx| {
                let Some(current) = account.read(cx).meta().map(key) else {
                    return;
                };
                if applied.as_ref() == Some(&current) {
                    return;
                }
                let (grants, scopes, _) = current.clone();
                applied = Some(current);
                this.apply_grants(grants, scopes, cx);
            }));
    }

    fn apply_grants(
        &mut self,
        grants: crate::account::Grants,
        scopes: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(sync) = self.sync.as_mut() else {
            return;
        };
        sync.set_grants(grants, scopes.clone(), cx);
        review::writes::set_access(
            if grants.can_write {
                review::writes::Access::Remote(sync.write_access())
            } else {
                review::writes::Access::ReadOnly
            },
            cx,
        );
        workitems::writes::set_access(sync.work_item_access(), cx);
        if let Some(edits) = workitems::writes::WiEdits::global(cx) {
            edits.update(cx, |edits, cx| edits.token_changed(cx));
        }
        if let Some(pins) = &self.pins {
            pins.update(cx, |pins, cx| pins.set_scopes(scopes, cx));
        }
        // Views read the access globals while rendering.
        cx.refresh_windows();
    }

    fn activation_changed(&mut self, active: bool, cx: &mut Context<Self>) {
        self.meter
            .update(cx, |meter, cx| meter.set_active(active, cx));
        let Some(sync) = &self.sync else { return };
        if active {
            self.background_timer = None;
            sync.set_foreground(true);
        } else if self.background_timer.is_none() {
            self.background_timer = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::sync::BACKGROUND_AFTER)
                    .await;
                this.update(cx, |this, _| {
                    if let Some(sync) = &this.sync {
                        sync.set_foreground(false);
                    }
                })
                .ok();
            }));
        }
    }

    /// A section switch (`⌘1`…`⌘6`, sidebar, palette): recorded in the
    /// history unless it's the section shown (spec 0018 FR-3.5, FR-4).
    pub fn show(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(None, window, cx, |this, window, cx| {
            this.set_section(nav, window, cx)
        });
    }

    /// "Work items: Filter by area…" / "…iteration…" from any view (spec
    /// 0008 FR-8): Work items on that tab with its dropdown open.
    fn open_path_filter(
        &mut self,
        tab: workitems::model::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show(Nav::WorkItems, window, cx);
        if let Some(views) = &self.views {
            views
                .workitems
                .update(cx, |view, cx| view.open_path_filter(tab, true, window, cx));
        }
    }

    /// Shows `nav` without touching the history.
    fn set_section(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        self.nav = nav;
        background::set_view(Some(nav.view_id()), cx);
        self.focus_current(window, cx);
        cx.notify();
    }

    /// Keyboard focus goes to the shown view (its list keys work at once);
    /// the window root calls it when a sheet over the shell closes.
    pub fn focus_current(&self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self
            .views
            .as_ref()
            .and_then(|views| views.focus_handle(self.nav, cx))
            .unwrap_or_else(|| self.focus.clone());
        window.focus(&handle, cx);
    }

    /// Route a palette (or view) result to its canonical place, recorded
    /// in the history (spec 0018 FR-3.1).
    pub fn open_target(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.open_target_from(target, None, window, cx);
    }

    /// [`Self::open_target`] from a view that captured its own state
    /// before the open changed it (`before`).
    fn open_target_from(
        &mut self,
        target: Target,
        before: Option<navigate::ViewState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate(before, window, cx, |this, window, cx| {
            this.route(target, window, cx)
        });
    }

    fn route(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            Target::PullRequest(id) => {
                self.set_section(Nav::PullRequests, window, cx);
                if let Some(views) = &self.views {
                    views
                        .review
                        .update(cx, |review, cx| review.open_pr(id, window, cx));
                }
            }
            Target::WorkItem(id) => {
                self.set_section(Nav::WorkItems, window, cx);
                if let Some(views) = &self.views {
                    views
                        .workitems
                        .update(cx, |view, cx| view.open_work_item(id, window, cx));
                }
            }
            Target::Run(id) => {
                self.set_section(Nav::Pipelines, window, cx);
                if let Some(views) = &self.views {
                    views
                        .pipelines
                        .update(cx, |view, cx| view.open_run(id, window, cx));
                }
            }
        }
    }

    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cheat_sheet
            .update(cx, |sheet, cx| sheet.close(window, cx));
        self.palette
            .update(cx, |palette, cx| palette.toggle(window, cx));
    }

    fn toggle_cheat_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette
            .update(cx, |palette, cx| palette.close(window, cx));
        self.cheat_sheet
            .update(cx, |sheet, cx| sheet.toggle(window, cx));
    }

    fn render_titlebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme.colors;
        let org = self.snapshot.org.clone().unwrap_or_else(|| "adoken".into());
        div()
            .flex_none()
            .h(px(size::TITLEBAR))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .pl(px(TRAFFIC_LIGHTS_WIDTH))
            .pr(px(space::S12))
            .border_b_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_sidebar)
            // ‹ › right after the traffic lights (principles §2, spec 0018).
            .child(self.render_history_buttons(cx))
            .child(
                div()
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY_STRONG, theme)
                    .child(org),
            )
            .child(
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(
                div()
                    .id("titlebar-search")
                    .w(px(320.0))
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(colors.border_default)
                    .bg(colors.bg_raised)
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .cursor_pointer()
                    .hover(|s| s.bg(colors.bg_hover))
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_palette(window, cx)))
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().child("Search…"))
                    .child(Kbd::parse(palette::TOGGLE_KEYS).expect("static binding")),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
    }

    fn nav_count(&self, nav: Nav) -> Option<usize> {
        match nav {
            Nav::PullRequests => Some(self.snapshot.reviews_waiting).filter(|n| *n > 0),
            Nav::Inbox => Some(self.inbox_unread).filter(|n| *n > 0),
            _ => None,
        }
    }

    /// The splash covers the body once opening outlasts [`LOADING_GRACE`],
    /// and stays through its exit.
    fn shows_splash(&self) -> bool {
        self.splash_fading
            || matches!(
                self.load,
                Load::Opening {
                    show_indicator: true
                }
            )
    }

    /// Statusbar (principles §2, activity.md §2): the sync item (a button
    /// opening Activity › Sync), the org, `notice` (a one-time message such
    /// as the code font fallback, `status.warning.fg`), then on the right
    /// the network indicator, the Ken chip, the resource meter (spec 0023)
    /// and the demo pill.
    fn render_statusbar(
        &self,
        theme: &Theme,
        notice: Option<gpui::SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::activity::statusbar::{self as bar, BarParts, LoadState};
        let colors = &theme.colors;
        let demo = self.source.is_demo();
        let activity = self.activity.read(cx);
        let load = match &self.load {
            Load::Opening { .. } => LoadState::Opening,
            Load::Failed(_) => LoadState::Failed,
            Load::Ready => LoadState::Ready,
        };
        let item = bar::sync_item(
            load,
            demo,
            self.sync_state.as_ref(),
            self.snapshot.last_sync,
            crate::sync::prefs(cx).pause,
            matches!(
                crate::account::token_problem(cx),
                Some(crate::account::TokenProblem::Expired(_))
            ),
            activity.offset(),
            activity.now(),
        );
        let view = activity.indicator().clone();
        let (network_value, network_chip) = match &view {
            crate::activity::indicator::IndicatorView::Shown {
                value, problems, ..
            } => (value.clone(), problems.is_some()),
            _ => (None, false),
        };
        let demo_pill = ("Demo mode · fake data, stays on this Mac", "Demo mode");
        let org = self.snapshot.org.clone();
        let meter_text = self.meter.read(cx).text().cloned();
        let meter_strings = meter_text.as_ref().map(|t| (t.full(), t.short()));
        let collapse = bar::collapse(
            f32::from(window.viewport_size().width),
            f32::from(window.rem_size()) / 16.0,
            &BarParts {
                sync_label: &item.label,
                sync_short: &item.short,
                org: org.as_deref(),
                network_value: network_value.as_deref(),
                network_shown: !matches!(view, crate::activity::indicator::IndicatorView::Hidden),
                network_chip,
                ken: self.ken.is_some(),
                demo: demo.then_some(demo_pill),
                meter: meter_strings
                    .as_ref()
                    .map(|(full, short)| (full.as_str(), short.as_str())),
            },
        );
        let busy = matches!(
            view,
            crate::activity::indicator::IndicatorView::Shown { busy: true, .. }
        );
        let showing_sync = activity.showing(crate::activity::Tab::Sync);
        let showing_network = activity.showing(crate::activity::Tab::Network);
        let announcement = activity.announcement().map(str::to_owned);
        let spin = bar::spin_phase(busy, window, cx);
        let model = self.activity.clone();
        let network = bar::render_network(
            &view,
            collapse.network_value_hidden,
            showing_network,
            spin,
            theme,
            move |window, cx| {
                let text = model.read(cx).tooltip();
                adoken_ui::Tooltip::text(text)(window, cx)
            },
            cx.listener(|this, _, window, cx| {
                this.statusbar_click(crate::activity::Tab::Network, window, cx)
            }),
        );
        self.meter.update(cx, |meter, cx| {
            meter.set_collapsed(collapse.meter_hidden, cx)
        });
        let meter = meter_text
            .filter(|_| !collapse.meter_hidden)
            .map(|text| self.render_meter(&text, collapse.meter_short, theme, window, cx));
        div()
            .relative()
            .flex_none()
            .h(px(STATUSBAR_HEIGHT))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .border_t_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_sidebar)
            .text_color(colors.text_secondary)
            .type_style(typography::SMALL, theme)
            .child(bar::render_sync_item(
                &item,
                collapse.sync_short,
                showing_sync,
                showing_sync,
                theme,
                cx.listener(|this, _, window, cx| {
                    this.statusbar_click(crate::activity::Tab::Sync, window, cx)
                }),
            ))
            .children(org.filter(|_| !collapse.org_hidden).map(|org| {
                div()
                    .min_w_0()
                    .when(collapse.org_truncated, |d| d.max_w(px(bar::ORG_MIN)))
                    .truncate()
                    .child(format!("· {org}"))
            }))
            .children(notice.map(|notice| {
                div()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.status_warning_fg)
                    .child(notice)
            }))
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .h_full()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .children(network)
                    .children(self.ken.as_ref().map(|ken| ken.chip.clone())) // ken
                    .children(meter)
                    .when(demo, |bar| {
                        bar.child(
                            div()
                                .px(px(space::S6))
                                .rounded(px(radius::XS))
                                .bg(colors.bg_raised)
                                .text_color(colors.text_secondary)
                                .child(if collapse.demo_short {
                                    demo_pill.1
                                } else {
                                    demo_pill.0
                                }),
                        )
                    }),
            )
            .child(bar::announcement(announcement.as_deref()))
    }

    // ── Resource meter (spec 0023) ──────────────────────────────────────

    pub fn resource_meter(&self) -> &Entity<crate::resource::ResourceMeter> {
        &self.meter
    }

    /// The meter button with its menu; the tooltip also shows while the
    /// meter has keyboard focus (FR-9).
    fn render_meter(
        &self,
        text: &crate::resource::meter::MeterText,
        short: bool,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let meter = self.meter.read(cx);
        let focus = meter.focus.clone();
        let bounds = meter.bounds.clone();
        let expanded = meter.menu_open();
        let menu = meter.menu.element();
        let tooltip_text = meter.tooltip().unwrap_or_default();
        let focus_tooltip = (focus.is_focused(window) && !expanded)
            .then(|| adoken_ui::Tooltip::text(tooltip_text)(window, cx));
        let hover = self.meter.downgrade();
        let open = self.meter.downgrade();
        let activity = self.activity.downgrade();
        div()
            .h_full()
            .child(crate::activity::statusbar::render_meter(
                text,
                short,
                expanded,
                &focus,
                focus_tooltip,
                bounds,
                theme,
                move |window, cx| {
                    let text = hover
                        .upgrade()
                        .and_then(|meter| meter.read(cx).tooltip())
                        .unwrap_or_default();
                    adoken_ui::Tooltip::text(text)(window, cx)
                },
                move |window, cx| {
                    let activity = activity.clone();
                    open.update(cx, |meter, cx| {
                        meter.open_menu(
                            move |_, cx| {
                                activity
                                    .update(cx, |a, cx| a.set_resource_meter(false, cx))
                                    .ok();
                            },
                            window,
                            cx,
                        )
                    })
                    .ok();
                },
            ))
            .children(menu)
            .into_any_element()
    }

    // ── Activity drawer (spec 0016) ─────────────────────────────────────

    pub fn activity(&self) -> &Entity<crate::activity::Activity> {
        &self.activity
    }

    pub fn activity_panel(&self) -> Option<&Entity<crate::activity::ActivityPanel>> {
        self.activity_panel.as_ref()
    }

    fn drawer_has_focus(&self, window: &Window, cx: &App) -> bool {
        self.activity_panel
            .as_ref()
            .is_some_and(|panel| panel.read(cx).contains_focus(window, cx))
    }

    /// Open the drawer on `tab` and move the keyboard into it (`pointer`:
    /// animate the open; keyboard and palette opens are instant).
    pub fn open_activity(
        &mut self,
        tab: crate::activity::Tab,
        pointer: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.drawer_has_focus(window, cx) {
            self.activity_return = window.focused(cx);
        }
        let activity = self.activity.clone();
        let panel = self
            .activity_panel
            .get_or_insert_with(|| {
                cx.new(|cx| crate::activity::ActivityPanel::new(activity, window, cx))
            })
            .clone();
        self.activity
            .update(cx, |activity, cx| activity.open(tab, pointer, cx));
        panel.update(cx, |panel, cx| panel.focus_tab(window, cx));
        cx.notify();
    }

    /// `⌘⇧A`: open on the last tab, or close.
    pub fn toggle_activity(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.activity.read(cx).is_open() {
            self.activity.update(cx, |activity, cx| activity.close(cx));
        } else {
            let tab = self.activity.read(cx).tab();
            self.open_activity(tab, false, window, cx);
        }
    }

    /// A click on a statusbar item: open its tab, switch to it, or close
    /// the drawer when it already shows (activity.md §2.2).
    fn statusbar_click(
        &mut self,
        tab: crate::activity::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.activity.read(cx).showing(tab) {
            self.activity.update(cx, |activity, cx| activity.close(cx));
        } else {
            self.open_activity(tab, true, window, cx);
        }
    }

    fn on_activity_event(
        &mut self,
        _: &Entity<crate::activity::Activity>,
        event: &crate::activity::ActivityEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::activity::ActivityEvent;
        match event {
            ActivityEvent::Open(target) => {
                // The drawer stays open and keeps the keyboard (FR-13).
                let keep = window.focused(cx);
                self.open_target(target.clone(), window, cx);
                if let Some(keep) = keep {
                    window.focus(&keep, cx);
                }
            }
            ActivityEvent::OpenInbox => self.show(Nav::Inbox, window, cx),
            ActivityEvent::Closed => {
                match self.activity_return.take() {
                    Some(handle) => window.focus(&handle, cx),
                    None => self.focus_current(window, cx),
                }
                if window.focused(cx).is_none() {
                    self.focus_current(window, cx);
                }
                cx.notify();
            }
        }
    }

    /// `F6`: content → drawer (when open) → toasts (when shown) → content.
    fn cycle_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let toasts = self.toasts.read(cx).region_focus();
        let in_toasts = toasts
            .as_ref()
            .is_some_and(|t| t.contains_focused(window, cx));
        let open = self.activity.read(cx).is_open();
        if self.drawer_has_focus(window, cx) {
            match toasts {
                Some(toasts) => window.focus(&toasts, cx),
                None => self.focus_current(window, cx),
            }
        } else if in_toasts {
            self.focus_current(window, cx);
        } else if open {
            if let Some(panel) = self.activity_panel.clone() {
                self.activity_return = window.focused(cx);
                panel.update(cx, |panel, cx| panel.focus_tab(window, cx));
            }
        } else if let Some(toasts) = toasts {
            window.focus(&toasts, cx);
        }
    }

    /// "Network log: Export…" (FR-25): the save panel, then JSON Lines
    /// written on a background executor; filtered rows when a filter is on.
    fn export_network_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let activity = self.activity.read(cx);
        let Some(hub) = activity.hub().cloned() else {
            adoken_ui::ToastStack::show(
                adoken_ui::Toast::new(
                    "activity-export",
                    IconName::Info,
                    "Nothing to export: adoken hasn't sent any requests.",
                ),
                cx,
            );
            return;
        };
        let filtered = activity
            .net
            .filter()
            .is_active()
            .then(|| activity.net.export_rows());
        let now = activity.now();
        let name = crate::activity::export_file_name(now, activity.offset());
        let dir = std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Downloads"))
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let chosen = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = path.clone();
            let result = cx
                .background_spawn(async move {
                    let entries = filtered.unwrap_or_else(|| hub.net.entries());
                    crate::activity::export_to(&written, &entries, now).map(|()| entries.len())
                })
                .await;
            cx.update(|_, cx| crate::activity::export_done(result, path, cx))
                .ok();
        })
        .detach();
    }

    fn render_content(&self) -> gpui::AnyElement {
        match &self.load {
            Load::Opening { .. } => return div().into_any_element(),
            Load::Failed(error) => {
                return EmptyState::new(IconName::CircleAlert, "Couldn't open local data")
                    .body(format!("{error}. Nothing was changed on Azure DevOps."))
                    .into_any_element();
            }
            Load::Ready => {}
        }
        let Some(views) = self.views.as_ref() else {
            return div().into_any_element();
        };
        match self.nav {
            Nav::Today => views.today.clone().into_any_element(),
            Nav::Inbox => views.inbox.clone().into_any_element(),
            Nav::PullRequests => views.review.clone().into_any_element(),
            Nav::WorkItems => views.workitems.clone().into_any_element(),
            Nav::Pipelines => views.pipelines.clone().into_any_element(),
            // Every result opens in its own section; Search is the palette's home.
            Nav::Search => EmptyState::new(IconName::Search, "Find anything")
                .body("Pull requests, work items, pipeline runs, people. Try ! # @ or author:@me.")
                .action(
                    Button::new("search-empty", "Search")
                        .variant(ButtonVariant::Primary)
                        .shortcut(Kbd::parse(palette::TOGGLE_KEYS).expect("static binding"))
                        .on_press(|window, cx| {
                            window.dispatch_action(Box::new(palette::Toggle), cx)
                        }),
                )
                .into_any_element(),
        }
    }
}

/// Demo mode's stand-in for sync: appends the demo org's due events to the
/// local store every [`DEMO_FEED_INTERVAL`]. Local writes only; nothing
/// leaves the machine.
fn demo_feed(
    store: Store,
    hub: Option<crate::sync::ActivityHub>,
    cx: &mut Context<Shell>,
) -> Task<()> {
    let executor = cx.background_executor().clone();
    cx.background_executor().spawn(async move {
        let mut feed = match adoken_demo::LiveFeed::for_store(&store) {
            Ok(Some(feed)) => feed,
            Ok(None) => return,
            Err(error) => {
                eprintln!("adoken: demo feed unavailable: {error}");
                return;
            }
        };
        loop {
            let now = now_ms();
            match feed.tick(&store, now) {
                // Activity › Sync lists each tick that added something as a
                // "Demo feed" pass (spec 0016 FR-15).
                Ok(added) if added > 0 => {
                    if let Some(hub) = &hub {
                        hub.passes.record_demo_tick(Default::default(), added, now);
                    }
                }
                Ok(_) => {}
                Err(error) => eprintln!("adoken: demo feed tick failed: {error}"),
            }
            executor.timer(DEMO_FEED_INTERVAL).await;
        }
    })
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        // The drawer's range and columns follow the window (FR-1).
        let viewport = window.viewport_size();
        let content_width = f32::from(viewport.width) - sidebar::width(self.sidebar_collapsed);
        self.activity.update(cx, |activity, _| {
            activity.set_geometry(f32::from(viewport.height), content_width)
        });
        crate::activity::remember_window_height(f32::from(viewport.height));
        let drawer = self
            .activity_panel
            .clone()
            .filter(|_| self.activity.read(cx).is_open());
        div()
            .id("shell")
            .key_context("Workspace")
            .track_focus(&self.focus)
            // Mouse back / forward buttons and swipes go back / forward
            // anywhere in the window, text fields included (FR-11); any
            // other click cancels a waiting restore (FR-8).
            .capture_any_mouse_down(cx.listener(|shell, event: &gpui::MouseDownEvent, _, cx| {
                if !matches!(event.button, MouseButton::Navigate(_)) {
                    shell.cancel_pending_restores(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_button(gpui::NavigationDirection::Back, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|shell, _, window, cx| {
                    shell.on_navigate_button(gpui::NavigationDirection::Forward, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(|shell, event: &gpui::MouseMoveEvent, _, cx| {
                if let Some(panel) = shell.activity_panel.clone()
                    && panel.read(cx).is_dragging()
                {
                    let y = f32::from(event.position.y);
                    panel.update(cx, |panel, cx| panel.drag_to(y, cx));
                    return;
                }
                shell.dismiss_peek_outside(event, cx)
            }))
            .on_key_down(cx.listener(|shell, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && shell.pin_state.drag.take().is_some() {
                    cx.notify();
                    cx.stop_propagation();
                }
                if event.keystroke.key == "escape" && shell.pin_state.peek {
                    shell.pin_state.peek = false;
                    shell.pin_state.intent.reset();
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _, _, cx| {
                    if shell.pin_state.drag.take().is_some() {
                        cx.notify();
                    }
                    if let Some(panel) = shell.activity_panel.clone() {
                        panel.update(cx, |panel, cx| panel.end_drag(cx));
                    }
                }),
            )
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.bg_app)
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .on_action(
                cx.listener(|this, _: &palette::Toggle, window, cx| {
                    this.toggle_palette(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &palette::ToggleCommands, window, cx| {
                    this.cheat_sheet
                        .update(cx, |sheet, cx| sheet.close(window, cx));
                    this.palette
                        .update(cx, |palette, cx| palette.open_with(">", window, cx))
                }),
            )
            .on_action(cx.listener(|this, _: &GoBack, window, cx| this.go_back(window, cx)))
            .on_action(cx.listener(|this, _: &GoForward, window, cx| this.go_forward(window, cx)))
            .on_action(
                cx.listener(|this, _: &ShowToday, window, cx| this.show(Nav::Today, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowInbox, window, cx| this.show(Nav::Inbox, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowPullRequests, window, cx| {
                this.show(Nav::PullRequests, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowWorkItems, window, cx| {
                this.show(Nav::WorkItems, window, cx)
            }))
            // 0008 FR-8 from another view: Work items, the tab, its dropdown.
            .on_action(
                cx.listener(|this, _: &workitems::FilterWorkItemsByArea, window, cx| {
                    this.open_path_filter(workitems::model::Tab::Area, window, cx)
                }),
            )
            .on_action(cx.listener(
                |this, _: &workitems::FilterWorkItemsByIteration, window, cx| {
                    this.open_path_filter(workitems::model::Tab::Iteration, window, cx)
                },
            ))
            .on_action(cx.listener(|this, _: &ShowPipelines, window, cx| {
                this.show(Nav::Pipelines, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowSearch, window, cx| this.show(Nav::Search, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &help::ToggleShortcuts, window, cx| {
                this.toggle_cheat_sheet(window, cx)
            }))
            .on_action(cx.listener(|this, _: &SyncNow, _, _| {
                if let Some(sync) = &this.sync {
                    sync.sync_now();
                }
            }))
            .on_action(cx.listener(|this, _: &LoadOlderRuns, _, _| {
                if let Some(sync) = &this.sync {
                    sync.load_older_runs();
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::pins::OpenPinsPalette, window, cx| {
                    this.cheat_sheet
                        .update(cx, |sheet, cx| sheet.close(window, cx));
                    this.palette
                        .update(cx, |palette, cx| palette.open_with("pin:", window, cx));
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::palette::OpenUnpinPalette, window, cx| {
                    this.cheat_sheet
                        .update(cx, |sheet, cx| sheet.close(window, cx));
                    this.palette
                        .update(cx, |palette, cx| palette.open_unpin(window, cx));
                }),
            )
            .on_action(cx.listener(|this, _: &crate::pins::UndoUnpin, _, cx| {
                if let Some(pins) = &this.pins {
                    pins.update(cx, |pins, cx| {
                        pins.undo_last(cx);
                    });
                }
            }))
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                this.toggle_current_pin(window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::pin_actions::OpenRowMenu, window, cx| {
                    this.open_pin_row_menu(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::pins::MovePinUp, _, cx| this.move_cursor_pin(-1, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::pins::MovePinDown, _, cx| {
                    this.move_cursor_pin(1, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &crate::pins::RenamePin, window, cx| {
                let subject = if this.palette.read(cx).is_visible() {
                    this.palette.read(cx).selected_pin_subject()
                } else if this.pin_state.focus.contains_focused(window, cx) {
                    this.pin_at_cursor(cx)
                } else {
                    this.current_pin_subject(cx)
                };
                if let Some(subject) = subject {
                    this.palette
                        .update(cx, |palette, cx| palette.close(window, cx));
                    this.start_pin_rename(subject, window, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &crate::activity::Toggle, window, cx| {
                    this.toggle_activity(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::activity::ShowSync, window, cx| {
                    this.open_activity(crate::activity::Tab::Sync, false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::activity::ShowNetwork, window, cx| {
                    this.open_activity(crate::activity::Tab::Network, false, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &crate::activity::Hide, _, cx| {
                this.activity.update(cx, |activity, cx| activity.close(cx))
            }))
            .on_action(
                cx.listener(|this, _: &crate::resource::ToggleResourceMeter, _, cx| {
                    let on = !crate::resource::shown(cx);
                    this.activity
                        .update(cx, |activity, cx| activity.set_resource_meter(on, cx))
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::activity::CycleRegion, window, cx| {
                    this.cycle_region(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::activity::ExportNetworkLog, window, cx| {
                    this.export_network_log(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::activity::ClearFilters, _, cx| {
                    match this.activity_panel.clone() {
                        Some(panel) => panel.update(cx, |panel, cx| panel.clear_filters(cx)),
                        None => this.activity.update(cx, |activity, cx| {
                            activity.net.update_filter(|f| *f = Default::default());
                            cx.notify();
                        }),
                    }
                }),
            )
            .child(self.render_titlebar(&theme, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .h_full()
                            .child(self.render_sidebar(&theme, window, cx))
                            .child(adoken_ui::action_menu::track_bounds(
                                self.sidebar_bounds.clone(),
                            )),
                    )
                    // The content column: the drawer docks at its bottom;
                    // the sidebar keeps its full height (FR-1).
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(div().flex_1().min_h_0().child(self.render_content()))
                            .children(drawer),
                    )
                    .children(self.render_pin_peek(window, cx))
                    .when(self.shows_splash(), |body| {
                        body.child(splash_overlay("Opening local data…").fading(self.splash_fading))
                    }),
            )
            .child(self.render_statusbar(&theme, crate::code_font::notice(cx), window, cx))
            .child(self.toasts.clone())
            .child(self.palette.clone())
            .child(self.cheat_sheet.clone())
            .children(self.pin_state.menu.element())
            .children(self.ken.as_ref().map(|ken| ken.layer.clone())) // ken
            // Guided tours (spec 0020): coach mark and spotlight, last.
            .child(self.tours.clone())
    }
}
