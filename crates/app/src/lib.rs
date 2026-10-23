//! adoken desktop app: one window that routes between onboarding
//! (docs/specs/0001-onboarding.md) and the main shell (`shell`: sidebar,
//! views, statusbar, and the Cmd+P palette over the local store).
//!
//! `ADOKEN_DEMO=1` skips onboarding and opens the shell on the fake Contoso
//! org (`adoken_demo`), seeded into `ADOKEN_DEMO_DIR` or
//! `$TMPDIR/adoken-demo`: no Keychain, no app data directory, no network.
//!
//! Launch routing never blocks the UI: saved progress and the Keychain are
//! read on the Tokio runtime while the first frame paints. A stored,
//! verified token plus finished onboarding goes straight to the shell, which
//! syncs the org ([`sync`]; `ADOKEN_SYNC_LOG=<file>` appends each request
//! as a redacted JSON line, the Network log export format, spec 0016
//! FR-26); anything else resumes onboarding where it stopped (FR-10).
//! `ADOKEN_ONBOARDING=1` forces the flow from Welcome (debug, demos;
//! `connect` or `create_in_browser` open that screen instead, without saved
//! state or token); `ADOKEN_REDUCE_MOTION=1|0` overrides the macOS Reduce
//! Motion setting (and Settings › General › Motion). `ADOKEN_SETTINGS=<tab>`
//! opens the Settings window on that tab at launch (screenshots).

pub mod account;
pub mod activity;
pub mod avatar;
pub mod background;
pub mod browse;
pub mod celebrate;
pub mod code_font;
pub mod help;
pub mod hotkey;
pub mod inbox;
pub mod ken;
mod keymap;
#[cfg(target_os = "macos")]
mod mac_window;
mod netwatch;
pub mod onboarding;
pub mod os_notify;
pub mod palette;
pub mod pin_actions;
pub mod pins;
pub mod pipelines;
pub mod resource;
pub mod review;
pub mod settings;
pub mod shell;
pub mod sound;
pub mod sync;
pub mod theme_picker; // theme_picker
pub mod today;
pub mod tours;
pub mod workitems;

use std::time::Duration;

use adoken_ado::{
    AccessLevel, AdoClient, ConnectPath, Connector, OnboardingState, Organization, PatMeta, Step,
};
use adoken_theme::{Appearance, motion, space};
use adoken_ui::motion::{EASE_IN_OUT, animation};
use adoken_ui::{ActiveTheme, Card, Kbd};
use gpui::{
    AnimationExt, App, Context, Entity, Focusable, KeyBinding, Menu, MenuItem, Subscription, Task,
    TitlebarOptions, Window, WindowAppearance, WindowBounds, WindowOptions, actions, div, point,
    prelude::*, px, size as gpui_size,
};
use gpui_tokio::Tokio;

use onboarding::{Onboarding, OnboardingEvent, Start};
use palette::PaletteCommand;
use shell::{Shell, StoreSource};
use sync::ConnectedOrg;

actions!(
    adoken,
    [
        /// Quit adoken.
        Quit,
        /// Move keyboard focus to the next control.
        FocusNext,
        /// Move keyboard focus to the previous control.
        FocusPrevious,
        /// Run onboarding again to connect or reconnect Azure DevOps.
        ConnectAzureDevOps,
    ]
);

/// Debug/demo switch: always show onboarding from Welcome.
const FORCE_ONBOARDING_ENV: &str = "ADOKEN_ONBOARDING";
/// Open the shell on the demo org (`1`).
const DEMO_ENV: &str = "ADOKEN_DEMO";
/// Where demo mode seeds its store (default `$TMPDIR/adoken-demo`).
const DEMO_DIR_ENV: &str = "ADOKEN_DEMO_DIR";
/// Start the shell on a section (`today`, `inbox`, `prs`, `search`) and/or
/// with the palette open on a query; for screenshots without input.
const NAV_ENV: &str = "ADOKEN_NAV";
const PALETTE_ENV: &str = "ADOKEN_PALETTE";
/// Override macOS Reduce Motion (`1` on, `0` off).
const REDUCE_MOTION_ENV: &str = "ADOKEN_REDUCE_MOTION";
/// How long the post-onboarding hint stays up.
const HINT_TOAST: Duration = Duration::from_secs(6);

const QUIT_KEYS: &str = "secondary-q";

fn system_appearance(appearance: WindowAppearance) -> Appearance {
    match appearance {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}

/// Every app action, as `>` commands in the palette (keyboard-first).
/// Demo mode hides actions that would talk to Azure DevOps.
pub(crate) fn palette_commands(demo: bool) -> Vec<PaletteCommand> {
    let mut commands = shell::commands();
    if !demo {
        commands.extend(sync::commands());
        commands.extend(account::commands());
        commands.push(PaletteCommand {
            title: "Connect Azure DevOps…",
            keys: None,
            action: Box::new(ConnectAzureDevOps),
        });
    }
    commands.push(PaletteCommand {
        title: "Quit adoken",
        keys: Some(QUIT_KEYS),
        action: Box::new(Quit),
    });
    commands.extend(theme_picker::commands()); // theme_picker
    commands.extend(code_font::commands());
    commands
}

fn demo_enabled(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "yes"))
}

/// The demo store's directory: never the app data directory.
fn demo_dir() -> std::path::PathBuf {
    std::env::var_os(DEMO_DIR_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("adoken-demo"))
}

/// `ADOKEN_ONBOARDING`: `1`/`welcome` → Welcome; `connect` /
/// `create_in_browser` → that screen with nothing saved. Anything else
/// (or unset) → normal launch routing.
fn forced_start(value: Option<&str>) -> Option<Start> {
    let (step, path) = match value? {
        "1" | "welcome" => return Some(Start::Fresh),
        "connect" => (Step::Connect, None),
        "create_in_browser" => (Step::CreateInBrowser, Some(ConnectPath::Paste)),
        _ => return None,
    };
    Some(Start::Resume {
        state: Box::new(OnboardingState {
            step,
            path,
            ..OnboardingState::default()
        }),
        account: None,
    })
}

/// `ADOKEN_REDUCE_MOTION`, else Settings › General › Motion, else the macOS
/// accessibility setting.
pub(crate) fn reduce_motion_preference(cx: &App) -> bool {
    match std::env::var(REDUCE_MOTION_ENV).as_deref() {
        Ok("1") => true,
        Ok("0") => false,
        _ => settings::reduce_motion_override(cx).unwrap_or_else(system_reduce_motion),
    }
}

#[cfg(target_os = "macos")]
// objc 0.2's macros test `feature = "cargo-clippy"` in our crate's cfg space.
#[allow(unexpected_cfgs)]
fn system_reduce_motion() -> bool {
    use objc::runtime::{BOOL, NO, Object};
    use objc::{class, msg_send, sel, sel_impl};
    // SAFETY: NSWorkspace.sharedWorkspace and its
    // accessibilityDisplayShouldReduceMotion getter exist on every macOS
    // gpui supports; both are plain, side-effect-free Objective-C getters.
    unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let reduce: BOOL = msg_send![workspace, accessibilityDisplayShouldReduceMotion];
        reduce != NO
    }
}

#[cfg(not(target_os = "macos"))]
fn system_reduce_motion() -> bool {
    false
}

/// What launch found on disk and in the Keychain (read off the UI thread).
struct Launch {
    state: Option<OnboardingState>,
    account: Option<PatMeta>,
    /// The verified account's client; its PAT was read from the Keychain
    /// once, here, and is handed to sync (no second Keychain prompt).
    client: Option<AdoClient>,
}

impl Launch {
    async fn read(connector: Connector) -> Self {
        // FR-10: a PAT minted but not verified before quitting is verified
        // now, never re-minted.
        if connector.account().is_some_and(|meta| !meta.verified)
            && let Err(error) = connector.resume_pending().await
        {
            eprintln!("adoken: couldn't finish verifying the saved token: {error}");
        }
        let account = connector.account();
        let state = OnboardingState::load(connector.data_dir());
        let client = connector.client().await.unwrap_or_else(|error| {
            eprintln!("adoken: {error}");
            None
        });
        Self {
            state,
            account,
            client,
        }
    }

    /// Onboarding finished and its token is still in the Keychain.
    fn is_connected(&self) -> bool {
        self.client.is_some()
            && self.state.as_ref().is_some_and(|state| {
                matches!(state.step, Step::Done | Step::FirstSync | Step::MakeItYours)
            })
    }

    /// The org to sync: the verified account's, with the projects selected
    /// during onboarding.
    fn connected(self, connector: Connector) -> Option<ConnectedOrg> {
        let meta = self.account.filter(|meta| meta.verified)?;
        let org = meta.organization()?;
        Some(ConnectedOrg {
            data_dir: connector.data_dir().to_path_buf(),
            org,
            projects: self
                .state
                .map(|state| state.projects.into_iter().map(|p| p.id).collect())
                .unwrap_or_default(),
            client: self.client,
            connector,
            can_write: account::grants(&meta).can_write,
            scopes: meta.scopes,
        })
    }

    fn start(self) -> Start {
        if self.state.is_none() && self.account.is_none() {
            return Start::Fresh;
        }
        let has_token = self.client.is_some();
        let account = self.account.filter(|meta| meta.verified && has_token);
        Start::Resume {
            state: Box::new(self.state.unwrap_or_default()),
            account: account.map(Box::new),
        }
    }
}

enum Route {
    /// First frames while launch state is read.
    Starting,
    Onboarding(Entity<Onboarding>),
    Shell(Entity<Shell>),
}

/// The window root: routes between onboarding and the shell, and shows the
/// Replace token sheet over the shell (spec 0001 FR-16, design §10.2).
struct Root {
    connector: Connector,
    /// `ADOKEN_DEMO=1`: fake data, onboarding and ADO actions unavailable.
    demo: bool,
    route: Route,
    /// Onboarding as a sheet over the shell: Replace token without `az`,
    /// Change access….
    sheet: Option<Entity<Onboarding>>,
    /// Bumped when the shell appears after onboarding (exit transition key).
    entered_shell: u64,
    hint_visible: bool,
    _route_subscription: Option<Subscription>,
    _account_subscription: Option<Subscription>,
    _sheet_subscription: Option<Subscription>,
    _launch: Option<Task<()>>,
    _hint: Option<Task<()>>,
    _activation: Subscription,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let connector = Connector::new(adoken_ado::data_dir());
        let demo = demo_enabled(std::env::var(DEMO_ENV).ok().as_deref());
        let forced =
            forced_start(std::env::var(FORCE_ONBOARDING_ENV).ok().as_deref()).filter(|_| !demo);
        let launch = if forced.is_some() || demo {
            None
        } else {
            let job = Tokio::spawn(cx, Launch::read(connector.clone()));
            Some(cx.spawn_in(window, async move |this, cx| {
                let launch = job.await;
                this.update_in(cx, |this, window, cx| match launch {
                    Ok(launch) if launch.is_connected() => {
                        this.install_account(launch.account.clone(), window, cx);
                        let org = launch.connected(this.connector.clone());
                        this.show_shell(org, false, window, cx)
                    }
                    Ok(launch) => this.show_onboarding(launch.start(), window, cx),
                    Err(error) => {
                        eprintln!("adoken: launch check failed: {error}");
                        this.show_onboarding(Start::Fresh, window, cx);
                    }
                })
                .ok();
            }))
        };
        // Reduce Motion can change while adoken runs; re-read on activation.
        let activation = cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                let reduce = reduce_motion_preference(cx);
                if cx.reduce_motion() != reduce {
                    cx.set_reduce_motion(reduce);
                }
            }
        });
        let mut this = Self {
            connector,
            demo,
            route: Route::Starting,
            sheet: None,
            entered_shell: 0,
            hint_visible: false,
            _route_subscription: None,
            _account_subscription: None,
            _sheet_subscription: None,
            _launch: launch,
            _hint: None,
            _activation: activation,
        };
        if let Some(start) = forced {
            this.show_onboarding(start, window, cx);
        } else if demo {
            this.show_shell(None, false, window, cx);
        }
        this
    }

    fn show_onboarding(&mut self, start: Start, window: &mut Window, cx: &mut Context<Self>) {
        if self.demo {
            // Onboarding mints tokens against Azure DevOps; never in demo mode.
            return;
        }
        let connector = self.connector.clone();
        let onboarding = cx.new(|cx| Onboarding::new(connector, start, window, cx));
        self._route_subscription = Some(cx.subscribe_in(
            &onboarding,
            window,
            |this, _, event, window, cx| match event {
                OnboardingEvent::Finished => this.enter_after_onboarding(window, cx),
                OnboardingEvent::Replaced { .. } | OnboardingEvent::Closed => {}
            },
        ));
        window.focus(&onboarding.focus_handle(cx), cx);
        self.route = Route::Onboarding(onboarding);
        cx.notify();
    }

    /// Onboarding saved the account and token: read them back off the UI
    /// thread, then open the shell on the org.
    fn enter_after_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let connector = self.connector.clone();
        let job = Tokio::spawn(cx, Launch::read(connector.clone()));
        self._launch = Some(cx.spawn_in(window, async move |this, cx| {
            let launch = job.await.ok();
            let meta = launch.as_ref().and_then(|launch| launch.account.clone());
            let org = launch.and_then(|launch| launch.connected(connector));
            this.update_in(cx, |this, window, cx| match org {
                Some(org) => {
                    this.install_account(meta, window, cx);
                    this.show_shell(Some(org), true, window, cx)
                }
                None => this.show_onboarding(Start::Fresh, window, cx),
            })
            .ok();
        }));
    }

    /// The connected account (spec 0001 FR-14–FR-18): Settings › Account,
    /// Replace token, the background replacement.
    fn install_account(
        &mut self,
        meta: Option<PatMeta>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = account::install(self.connector.clone(), meta, cx);
        self._account_subscription =
            Some(
                cx.subscribe_in(&model, window, |this, _, event, window, cx| match event {
                    account::AccountEvent::OpenSheet { org, level } => {
                        this.show_sheet(org.clone(), *level, window, cx)
                    }
                }),
            );
    }

    /// The onboarding sheet over the shell (design §10.2): screen 1, org
    /// fixed, `level` preset.
    fn show_sheet(
        &mut self,
        org: Organization,
        level: AccessLevel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.demo || !matches!(self.route, Route::Shell(_)) {
            return;
        }
        let connector = self.connector.clone();
        let sheet =
            cx.new(|cx| Onboarding::new(connector, Start::Sheet { org, level }, window, cx));
        self._sheet_subscription =
            Some(
                cx.subscribe_in(&sheet, window, |this, _, event, window, cx| match event {
                    OnboardingEvent::Replaced { meta, replaced } => {
                        account::sheet_replaced(
                            meta.as_ref().clone(),
                            replaced.as_deref().cloned(),
                            cx,
                        );
                        this.close_sheet(window, cx);
                    }
                    OnboardingEvent::Closed => this.close_sheet(window, cx),
                    OnboardingEvent::Finished => {}
                }),
            );
        window.focus(&sheet.focus_handle(cx), cx);
        self.sheet = Some(sheet);
        cx.notify();
    }

    fn close_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet = None;
        self._sheet_subscription = None;
        // Back to the shown view, not the shell's root: its keys (and the
        // view's first-use tour, spec 0020 FR-2) need its focus.
        if let Route::Shell(shell) = &self.route {
            shell.update(cx, |shell, cx| shell.focus_current(window, cx));
        }
        cx.notify();
    }

    /// The shell (demo when `org` is `None`); after onboarding it fades in
    /// and shows a one-time hint (design §3 exit transition).
    fn show_shell(
        &mut self,
        org: Option<ConnectedOrg>,
        from_onboarding: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = match org {
            Some(org) if !self.demo => StoreSource::Connected(org),
            _ => StoreSource::Demo(demo_dir()),
        };
        let commands = palette_commands(self.demo);
        let shell = cx.new(|cx| {
            let mut shell = Shell::new(source, commands, window, cx);
            if let Some(nav) = std::env::var(NAV_ENV)
                .ok()
                .as_deref()
                .and_then(shell::Nav::parse)
            {
                shell.show(nav, window, cx);
            }
            shell
        });
        window.focus(&shell.focus_handle(cx), cx);
        if let Ok(query) = std::env::var(PALETTE_ENV) {
            shell.update(cx, |shell, cx| {
                shell
                    .palette()
                    .update(cx, |palette, cx| palette.open_with(&query, window, cx))
            });
        }
        self.route = Route::Shell(shell);
        self._route_subscription = None;
        if from_onboarding {
            self.entered_shell += 1;
            self.hint_visible = true;
            // No first-use tour while the hint shows (spec 0020 FR-2).
            tours::set_hint_showing(true, cx);
            self._hint = Some(cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(HINT_TOAST).await;
                this.update(cx, |this, cx| {
                    this.hint_visible = false;
                    tours::set_hint_showing(false, cx);
                    cx.notify();
                })
                .ok();
            }));
        }
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let base = div()
            .id("root")
            .size_full()
            .bg(colors.bg_app)
            .on_action(cx.listener(|this, _: &ConnectAzureDevOps, window, cx| {
                this.show_onboarding(Start::Fresh, window, cx)
            }));
        // Custom background (spec 0007) paints behind every route.
        background::backdrop(match &self.route {
            Route::Starting => base,
            Route::Onboarding(onboarding) => base.child(onboarding.clone()),
            Route::Shell(shell) => {
                let hint = self.hint_visible.then(|| {
                    div()
                        .absolute()
                        .bottom(px(space::S24))
                        .right(px(space::S24))
                        .child(
                            Card::new().child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(space::S8))
                                    .child(
                                        Kbd::parse(palette::TOGGLE_KEYS).expect("static binding"),
                                    )
                                    .child("Press to jump to anything"),
                            ),
                        )
                });
                let shell = div().size_full().child(shell.clone());
                let shell = if self.entered_shell == 0 {
                    shell.into_any_element()
                } else {
                    shell
                        .with_animation(
                            ("enter-shell", self.entered_shell),
                            animation(motion::SLOW, EASE_IN_OUT),
                            |shell, t| shell.opacity(t),
                        )
                        .into_any_element()
                };
                // The Replace token sheet over the shell (design §10.2).
                let sheet = self
                    .sheet
                    .clone()
                    .map(|sheet| div().absolute().inset_0().child(sheet));
                base.relative().child(shell).children(hint).children(sheet)
            }
        })
    }
}

/// Every key binding the app registers, in precedence order (later wins).
/// One list so the shortcut cheat sheet (`help`) and gallery stories see
/// exactly what `run` binds.
pub fn key_bindings() -> Vec<KeyBinding> {
    let mut bindings = vec![
        KeyBinding::new(QUIT_KEYS, Quit, None),
        KeyBinding::new("tab", FocusNext, None),
        KeyBinding::new("shift-tab", FocusPrevious, None),
    ];
    bindings.extend(adoken_ui::text_input::bindings());
    bindings.extend(adoken_ui::text_area::bindings());
    bindings.extend(adoken_ui::picker::bindings());
    bindings.extend(onboarding::bindings());
    bindings.extend(shell::bindings());
    bindings.extend(today::bindings());
    bindings.extend(review::bindings());
    bindings.extend(inbox::bindings());
    bindings.extend(settings::bindings());
    bindings.extend(workitems::bindings());
    bindings.extend(pipelines::bindings());
    bindings.extend(browse::bindings());
    bindings.extend(theme_picker::bindings()); // theme_picker
    bindings.extend(code_font::picker::bindings());
    bindings.extend(ken::bindings());
    bindings.extend(pins::bindings());
    bindings.extend(pin_actions::bindings());
    bindings.extend(palette::bindings());
    bindings.extend(adoken_ui::action_menu::bindings());
    bindings
}

/// Starts the app: one window, routed by [`Root`].
pub fn run() {
    let application = gpui_platform::application().with_assets(adoken_ui::Assets);
    application.run(|cx: &mut App| {
        background::init(cx);
        avatar::init(cx);
        os_notify::init(cx);
        // theme_picker: default built-in now (no IO); `load` below reads the
        // saved choice and user themes off the UI thread. The macOS
        // appearance at launch picks the default until a theme is chosen.
        theme_picker::init(system_appearance(cx.window_appearance()), cx);
        theme_picker::register_actions(cx);
        // Bundled JetBrains Mono before the first frame (borrowed bytes, no
        // IO); `settings::init` applies the configured code font once read.
        code_font::init(cx);
        let reduce = reduce_motion_preference(cx);
        cx.set_reduce_motion(reduce);
        gpui_tokio::init(cx);

        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys(key_bindings());
        let demo = demo_enabled(std::env::var(DEMO_ENV).ok().as_deref());
        // Demo settings live next to the demo store, never in the data dir.
        let settings_dir = if demo {
            demo_dir()
        } else {
            adoken_ado::data_dir()
        };
        sync::init(cx);
        account::init(cx);
        settings::init(settings_dir.join(adoken_settings::FILE_NAME), demo, cx);
        keymap::init(settings_dir.join("keymap.toml"), cx);
        theme_picker::load(cx); // theme_picker
        resource::init(cx);
        hotkey::init(cx);
        let mut items = vec![
            MenuItem::action("Settings…", settings::OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Search…", palette::Toggle),
        ];
        if !demo {
            items.push(MenuItem::action(
                "Connect Azure DevOps…",
                ConnectAzureDevOps,
            ));
        }
        items.extend([MenuItem::separator(), MenuItem::action("Quit adoken", Quit)]);
        cx.set_menus(vec![Menu::new("adoken").items(items)]);

        let main = open_main_window(true, cx);
        hotkey::set_main_window(
            main,
            hotkey::MainWindowHooks {
                reopen: |cx| open_main_window(false, cx),
            },
            cx,
        );
        cx.activate(true);
        settings::open_from_env(cx);
    });
}

/// The main window, routed by [`Root`] (at launch, and when the global
/// shortcut finds it closed: spec 0021 FR-6).
fn open_main_window(launch: bool, cx: &mut App) -> gpui::AnyWindowHandle {
    let bounds = WindowBounds::centered(gpui_size(px(1200.0), px(760.0)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(bounds),
            // Onboarding's minimum (design §2); smaller is full-bleed.
            window_min_size: Some(gpui_size(px(720.0), px(560.0))),
            titlebar: Some(TitlebarOptions {
                title: Some("adoken".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            app_id: Some("dev.adoken.app".into()),
            ..Default::default()
        },
        |window, cx| {
            if launch {
                adoken_perf::cold_start::install(window);
                // The global shortcut registers after the first frame
                // (spec 0021 FR-2: never on the cold-start path).
                window.on_next_frame(|_, cx| hotkey::start(cx));
            }
            cx.new(|cx| Root::new(window, cx))
        },
    )
    .expect("failed to open the main window")
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_appearance_maps_vibrant_variants() {
        assert_eq!(
            system_appearance(WindowAppearance::VibrantDark),
            Appearance::Dark
        );
        assert_eq!(
            system_appearance(WindowAppearance::VibrantLight),
            Appearance::Light
        );
    }

    #[test]
    fn forced_onboarding_values_pick_a_start_screen_or_fall_through() {
        assert!(matches!(forced_start(Some("1")), Some(Start::Fresh)));
        let Some(Start::Resume { state, account }) = forced_start(Some("create_in_browser")) else {
            panic!("expected a resume");
        };
        assert_eq!(state.step, Step::CreateInBrowser);
        assert!(
            account.is_none(),
            "a forced screen never uses a saved token"
        );
        assert!(forced_start(Some("0")).is_none());
        assert!(forced_start(None).is_none());
    }

    #[test]
    fn static_bindings_parse() {
        let palette_keys = palette_commands(false).into_iter().filter_map(|c| c.keys);
        for keys in [
            QUIT_KEYS,
            palette::TOGGLE_KEYS,
            palette::COMMANDS_KEYS,
            "tab",
            "enter",
        ]
        .into_iter()
        .chain(palette_keys)
        {
            assert!(Kbd::parse(keys).is_ok(), "{keys}");
        }
    }

    #[test]
    fn demo_mode_hides_actions_that_reach_azure_devops() {
        assert!(demo_enabled(Some("1")));
        assert!(!demo_enabled(Some("0")));
        assert!(!demo_enabled(None));
        let titles = |demo| {
            palette_commands(demo)
                .into_iter()
                .map(|c| c.title)
                .collect::<Vec<_>>()
        };
        assert!(titles(false).contains(&"Connect Azure DevOps…"));
        assert!(!titles(true).contains(&"Connect Azure DevOps…"));
    }

    /// Spec 0025 AC-7 (palette part) and AC-8 (pairs part): the seven
    /// quieting commands are gone, the two Quiet mode commands are there,
    /// and every merged on/off pair is exactly one row.
    #[test]
    fn quiet_mode_and_toggles_replace_the_removed_commands() {
        let titles: Vec<&str> = palette_commands(false)
            .iter()
            .map(|command| command.title)
            .collect();
        let count = |title: &str| titles.iter().filter(|t| **t == title).count();
        for removed in [
            "Calm mode for 1 hour",
            "Calm mode until tomorrow",
            "End calm mode",
            "Toggle sounds",
            "Mute sounds for 1 hour",
            "Turn celebrations on",
            "Turn celebrations off",
            "Tours: Turn off first-use tours",
            "Tours: Turn on first-use tours",
            "Statusbar: Show memory and CPU",
            "Statusbar: Hide memory and CPU",
            "Ken: Pause streak",
            "Ken: Resume streak",
            "Sync: Pause until resumed",
            "Sync: Resume",
            "Show threads",
            "Show files",
        ] {
            assert_eq!(count(removed), 0, "{removed:?} is still in ⌘P");
        }
        let mut merged = vec![
            ken::QUIET_TOGGLE,
            ken::QUIET_HOUR,
            ken::STREAK_TOGGLE,
            tours::TOGGLE_TITLE,
            "Sync: Pause or resume",
            "Sync: Pause for 1 hour",
            "Threads view: turn on or off",
            "Preview celebration",
        ];
        if resource::source::SUPPORTED {
            merged.push(resource::TOGGLE_TITLE);
        }
        for title in merged {
            assert_eq!(count(title), 1, "{title:?} should be one ⌘P row");
        }
    }
}
