//! Global summon shortcut (spec 0021, one chord since 0025 FR-12): Hyper+A
//! (⌃⌥⇧⌘A, configurable) brings adoken's main window to the front from any
//! app. macOS only: elsewhere the feature is absent (no registration, no
//! Settings section, no command).
//!
//! - [`model`]: chords, validation, `[hotkey]`, the registration
//!   controller behind the `Registrar` seam, the summon decision.
//! - `macos`: Carbon `RegisterEventHotKey`, the keyboard layout, the
//!   LaunchServices activation fallback (ported from zeron).
//! - [`Hotkeys`]: the app-wide entity: settings, statuses, the recorder,
//!   the launch toast. Registration starts after the main window's first
//!   frame (FR-2) and stops on quit.

#[cfg(target_os = "macos")]
mod macos;
pub mod model;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{
    Action, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Global,
    KeyBinding, Subscription, Task, Window, actions,
};

use crate::palette::PaletteCommand;
use model::{Chord, Controller, HotkeySettings, OwnBinding, Refusal, Registrar};

actions!(
    hotkey,
    [
        /// Open Settings › Keyboard at the global shortcut.
        GlobalShortcutSettings,
    ]
);

/// The feature exists on this platform (FR-14: Windows at v0.3).
pub const AVAILABLE: bool = cfg!(target_os = "macos");
pub const SECTION: &str = "Global shortcut";
pub const SECTION_KEYWORDS: &str =
    "global shortcut hotkey hyper summon show adoken from any app focus bring front";
pub const TIP: &str = "Tip: map Caps Lock to Hyper (⌃⌥⇧⌘) with Raycast's Hyper Key or \
                       Karabiner-Elements, then Hyper+A is one finger and a letter.";
/// The file remembering which failing values already got their launch
/// toast (FR-12), next to `settings.toml`.
const NOTIFIED_FILE: &str = "hotkey-notified";
/// FR-7: how long standard activation gets before the fallback.
const ACTIVATION_CHECK: Duration = Duration::from_millis(150);

pub fn commands() -> Vec<PaletteCommand> {
    if !AVAILABLE {
        return Vec::new();
    }
    vec![PaletteCommand {
        title: "Global shortcut settings…",
        keys: None,
        action: Box::new(GlobalShortcutSettings),
    }]
}

/// adoken's own single-keystroke ⌘/⌃ bindings in `bindings` (FR-5), each
/// named by its palette title, else its humanized action.
pub fn own_bindings<'a>(bindings: impl IntoIterator<Item = &'a KeyBinding>) -> Vec<OwnBinding> {
    let commands = crate::palette_commands(false);
    let title = |name: &str| {
        commands
            .iter()
            .find(|c| c.action.name() == name)
            .map(|c| c.title.to_owned())
            .unwrap_or_else(|| crate::help::sheet::humanize_action(name))
    };
    let search = crate::palette::Toggle.name();
    bindings
        .into_iter()
        .filter_map(|binding| {
            let [stroke] = binding.keystrokes() else {
                return None;
            };
            let keystroke = stroke.inner().clone();
            let m = keystroke.modifiers;
            if !(m.platform || m.control) {
                return None;
            }
            let name = binding.action().name();
            let does = if name == search {
                "opens search".to_owned()
            } else {
                format!("runs “{}”", title(name))
            };
            Some(OwnBinding { keystroke, does })
        })
        .collect()
}

/// `[hotkey]` problems for Settings' notice (spec 0009 FR-5).
pub fn parse_issues(document: &toml::Table) -> Vec<String> {
    if !AVAILABLE {
        return Vec::new();
    }
    let bindings = crate::key_bindings();
    model::parse(document, &own_bindings(&bindings)).1
}

/// How the summon reaches the main window; installed by `run`.
#[derive(Clone, Copy)]
pub struct MainWindowHooks {
    /// Open the main window as at launch; its handle.
    pub reopen: fn(&mut App) -> AnyWindowHandle,
}

struct Service(Entity<Hotkeys>);

impl Global for Service {}

/// The app's global shortcut.
pub struct Hotkeys {
    controller: Controller<Box<dyn Registrar>>,
    layout: Box<dyn model::Layout>,
    main: Option<AnyWindowHandle>,
    hooks: Option<MainWindowHooks>,
    /// Failing values that already got their launch toast (FR-12).
    notified: HashSet<String>,
    notified_path: Option<PathBuf>,
    /// Recording (FR-8), and the recorder's keyboard focus.
    recording: bool,
    pub(crate) recorder_focus: FocusHandle,
    /// The last refused chord, until the next action.
    refusal: Option<Refusal>,
    /// Modifiers held while recording, to tell "⌘⌥ alone" apart.
    held: gpui::Modifiers,
    clock: Instant,
    read: Option<Task<()>>,
    /// The notified list has been read once.
    loaded: bool,
    _recording: Vec<Subscription>,
    _settings: Option<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Hotkeys {
    pub fn global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Service>().map(|s| s.0.clone())
    }

    fn now(&self) -> u64 {
        self.clock.elapsed().as_millis() as u64
    }

    pub fn settings(&self) -> &HotkeySettings {
        self.controller.settings()
    }

    pub fn recording(&self) -> bool {
        self.recording
    }

    /// The status line (§3.3), or the recorder's prompt or refusal.
    pub fn status_line(&self) -> String {
        if self.recording {
            return "Press the new shortcut… Esc cancels, ⌫ clears.".into();
        }
        if let Some(refusal) = &self.refusal {
            return refusal.message();
        }
        model::status_line(self.controller.status(), self.now())
    }

    pub fn failed(&self) -> bool {
        self.refusal.is_some() || matches!(self.controller.status(), model::Status::Failed { .. })
    }

    fn apply(&mut self, settings: HotkeySettings, cx: &mut Context<Self>) {
        let failed = self.controller.apply(settings, self.layout.as_ref());
        self.report(failed, cx);
        cx.notify();
    }

    /// FR-12: one launch toast per distinct failing value, remembered
    /// across launches; the status line says it every time.
    fn report(&mut self, failed: Option<Chord>, cx: &mut Context<Self>) {
        let Some(chord) = failed else {
            return;
        };
        if !self.notified.insert(chord.setting()) {
            return;
        }
        let message = format!("Global shortcut {} isn't available.", chord.label());
        self._tasks.push(cx.spawn(async move |_, cx| {
            // The shell installs the toast stack; wait for it briefly.
            for _ in 0..40 {
                let shown = cx.update(|cx| {
                    if adoken_ui::ToastStack::global(cx).is_none() {
                        return false;
                    }
                    adoken_ui::ToastStack::show(
                        adoken_ui::Toast::new(
                            "hotkey-failed",
                            adoken_ui::IconName::CircleAlert,
                            message.clone(),
                        )
                        .tone(adoken_ui::ToastTone::Warning)
                        .action(adoken_ui::ToastAction::new("Settings", |_, cx| {
                            crate::settings::open(crate::settings::Tab::Keyboard, cx)
                        })),
                        cx,
                    );
                    true
                });
                if shown {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
            }
        }));
        if let Some(path) = self.notified_path.clone() {
            let text = self.notified.iter().cloned().collect::<Vec<_>>().join("\n");
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = std::fs::write(&path, text) {
                        log::warn!("hotkey: couldn't remember the failed shortcut: {error}");
                    }
                })
                .detach();
        }
    }

    /// Read `[hotkey]` and the notified list off the UI thread, then apply.
    fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        let notified_path = path.with_file_name(NOTIFIED_FILE);
        self.notified_path = Some(notified_path.clone());
        let own = own_bindings(cx.key_bindings().borrow().bindings());
        let first = !self.loaded;
        let job = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let settings = toml::from_str::<toml::Table>(&text)
                .map(|doc| model::parse(&doc, &own).0)
                .ok();
            let notified = first.then(|| {
                std::fs::read_to_string(&notified_path)
                    .map(|text| text.lines().map(str::to_owned).collect::<HashSet<_>>())
                    .unwrap_or_default()
            });
            (settings, notified)
        });
        self.read = Some(cx.spawn(async move |this, cx| {
            let (settings, notified) = job.await;
            this.update(cx, |this, cx| {
                if let Some(notified) = notified {
                    this.notified.extend(notified);
                    this.loaded = true;
                }
                // A file that doesn't parse keeps the shortcuts as they are.
                if let Some(settings) = settings {
                    this.apply(settings, cx);
                }
            })
            .ok();
        }));
    }

    /// Save `chord`: the file, then the registration (FR-11).
    fn save(&mut self, chord: Option<Chord>, cx: &mut Context<Self>) {
        let settings = HotkeySettings { summon: chord };
        crate::settings::write_table(cx, model::TABLE, settings.table());
        self.apply(settings, cx);
    }

    // ── recorder (§3.2, FR-8) ───────────────────────────────────────────

    pub fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refusal = None;
        self.recording = true;
        self.held = gpui::Modifiers::default();
        self.set_recording(true, cx);
        window.focus(&self.recorder_focus, cx);
        let focus = self.recorder_focus.clone();
        let this = cx.entity().downgrade();
        // Bound actions run before element key listeners: intercept first
        // so a chord adoken binds is refused instead of running (zeron
        // `settings/shortcuts.rs`).
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            if !focus.is_focused(window) {
                return;
            }
            this.update(cx, |this, cx| this.record(&event.keystroke, cx))
                .ok();
            cx.stop_propagation();
        });
        let blur = cx.on_blur(&self.recorder_focus, window, |this, _, cx| {
            this.stop_recording(cx);
        });
        self._recording = vec![intercept, blur];
        cx.notify();
    }

    fn set_recording(&mut self, recording: bool, cx: &mut Context<Self>) {
        let failed = self
            .controller
            .set_recording(recording, self.layout.as_ref());
        self.report(failed, cx);
    }

    /// `Esc`, blur, or a refused chord: the previous chord comes back.
    pub fn stop_recording(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.recording) {
            return;
        }
        self._recording.clear();
        self.set_recording(false, cx);
        cx.notify();
    }

    fn record(&mut self, stroke: &gpui::Keystroke, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        let plain = !stroke.modifiers.modified();
        match stroke.key.as_str() {
            "escape" if plain => self.stop_recording(cx),
            "backspace" if plain => {
                self.recording = false;
                self._recording.clear();
                self.save(None, cx);
                self.set_recording(false, cx);
            }
            _ => {
                let own = own_bindings(cx.key_bindings().borrow().bindings());
                match model::validate(stroke, &own) {
                    Ok(chord) => {
                        self.recording = false;
                        self._recording.clear();
                        self.save(Some(chord), cx);
                        self.set_recording(false, cx);
                    }
                    Err(refusal) => {
                        self.refusal = Some(refusal);
                        self.stop_recording(cx);
                    }
                }
            }
        }
        cx.notify();
    }

    /// Modifier changes while recording: modifiers pressed and released
    /// without a key refuse with "Add a key, like A." (AC-6).
    pub fn modifiers_changed(&mut self, modifiers: gpui::Modifiers, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        if modifiers.modified() {
            let held = &mut self.held;
            held.control |= modifiers.control;
            held.alt |= modifiers.alt;
            held.shift |= modifiers.shift;
            held.platform |= modifiers.platform;
            return;
        }
        if std::mem::take(&mut self.held).modified() {
            self.refusal = Some(Refusal::ModifiersOnly);
            self.stop_recording(cx);
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.refusal = None;
        self.save(None, cx);
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.refusal = None;
        self.save(HotkeySettings::default().summon, cx);
    }

    // ── summon (FR-6, FR-7) ─────────────────────────────────────────────

    #[cfg(target_os = "macos")]
    fn pressed(&mut self, cx: &mut Context<Self>) {
        use model::{MainWindow, Summon, SummonState};
        let now = self.now();
        if !self.controller.press(now) {
            return;
        }
        self.refusal = None;
        cx.notify();
        let main = self.main.and_then(|handle| {
            handle
                .update(cx, |_, window, _| {
                    if window.is_window_active() {
                        MainWindow::Key
                    } else if crate::mac_window::ns_window(window)
                        .is_some_and(crate::mac_window::is_minimized)
                    {
                        MainWindow::Minimized
                    } else {
                        MainWindow::Visible
                    }
                })
                .ok()
        });
        let state = SummonState {
            app_active: crate::mac_window::app_active(),
            app_hidden: crate::mac_window::app_hidden(),
            main: main.unwrap_or(MainWindow::Closed),
            settings_keyboard_key: crate::settings::key_tab(cx)
                == Some(crate::settings::Tab::Keyboard),
        };
        let Some(hooks) = self.hooks else {
            return;
        };
        match model::decide(state) {
            Summon::Test | Summon::Nothing => {}
            Summon::Activate {
                unhide, open_main, ..
            } => {
                if unhide {
                    crate::mac_window::unhide_app();
                }
                let handle = match self.main.filter(|_| !open_main) {
                    Some(handle) => handle,
                    None => {
                        let handle = (hooks.reopen)(cx);
                        self.main = Some(handle);
                        handle
                    }
                };
                cx.activate(true);
                // makeKeyAndOrderFront: deminiaturizes, switches Space.
                handle
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
                self._tasks.push(cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(ACTIVATION_CHECK).await;
                    if !crate::mac_window::app_active() {
                        macos::activate_via_launch_services();
                    }
                }));
            }
        }
    }

    /// The cheat sheet row (FR-13): "Show adoken from any app ·
    /// ⌃⌥⇧⌘A (global)", hidden when cleared.
    pub fn cheat_sheet_rows(&self) -> Vec<(&'static str, String)> {
        self.settings()
            .summon
            .iter()
            .map(|chord| {
                (
                    model::CHEAT_SHEET_TITLE,
                    format!("{} (global)", chord.label()),
                )
            })
            .collect()
    }
}

impl Hotkeys {
    fn new(
        registrar: Box<dyn Registrar>,
        layout: Box<dyn model::Layout>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            controller: Controller::new(registrar),
            layout,
            main: None,
            hooks: None,
            notified: HashSet::new(),
            notified_path: None,
            recording: false,
            recorder_focus: cx.focus_handle().tab_stop(true),
            refusal: None,
            held: gpui::Modifiers::default(),
            clock: Instant::now(),
            read: None,
            loaded: false,
            _recording: Vec::new(),
            // Hand edits and other modules' writes (spec 0009 FR-4).
            _settings: Some(crate::settings::observe(cx, |this, cx| this.reload(cx))),
            _tasks: Vec::new(),
        }
    }
}

/// Register the service (no registration yet: that waits for the first
/// frame, [`start`]). No-op off macOS.
pub fn init(cx: &mut App) {
    if !AVAILABLE {
        return;
    }
    cx.on_action(|_: &GlobalShortcutSettings, cx| {
        crate::settings::open(crate::settings::Tab::Keyboard, cx)
    });
    let entity = cx.new(|cx| {
        let registrar = new_registrar(cx);
        let mut hotkeys = Hotkeys::new(registrar, new_layout(), cx);
        hotkeys.reload(cx);
        hotkeys
    });
    // FR-4: a new input source can move the printed key.
    cx.on_keyboard_layout_change({
        let entity = entity.downgrade();
        move |cx| {
            entity
                .update(cx, |this, cx| {
                    let failed = this.controller.relayout(this.layout.as_ref());
                    this.report(failed, cx);
                    cx.notify();
                })
                .ok();
        }
    })
    .detach();
    cx.on_app_quit({
        let entity = entity.downgrade();
        move |cx| {
            entity.update(cx, |this, _| this.controller.stop()).ok();
            async {}
        }
    })
    .detach();
    cx.set_global(Service(entity));
}

#[cfg(target_os = "macos")]
fn new_registrar(cx: &mut Context<Hotkeys>) -> Box<dyn Registrar> {
    let (tx, mut rx) = futures::channel::mpsc::unbounded();
    let task = cx.spawn(async move |this, cx| {
        use futures::StreamExt as _;
        while rx.next().await.is_some() {
            if this.update(cx, |this, cx| this.pressed(cx)).is_err() {
                return;
            }
        }
    });
    task.detach();
    match macos::CarbonRegistrar::install(tx) {
        Some(registrar) => Box::new(registrar),
        None => Box::new(Unavailable),
    }
}

#[cfg(not(target_os = "macos"))]
fn new_registrar(_: &mut Context<Hotkeys>) -> Box<dyn Registrar> {
    Box::new(Unavailable)
}

#[cfg(target_os = "macos")]
fn new_layout() -> Box<dyn model::Layout> {
    Box::new(macos::CarbonLayout)
}

#[cfg(not(target_os = "macos"))]
fn new_layout() -> Box<dyn model::Layout> {
    struct NoLayout;
    impl model::Layout for NoLayout {
        fn translate(&self, _: u16, _: bool) -> Option<String> {
            None
        }
    }
    Box::new(NoLayout)
}

/// No system registration available: every register fails.
struct Unavailable;

impl Registrar for Unavailable {
    fn register(&mut self, _: u32, _: u32) -> Result<(), i32> {
        Err(-1)
    }

    fn unregister(&mut self) {}
}

/// The main window, and how to reopen it.
pub fn set_main_window(handle: AnyWindowHandle, hooks: MainWindowHooks, cx: &mut App) {
    if let Some(entity) = Hotkeys::global(cx) {
        entity.update(cx, |this, _| {
            this.main = Some(handle);
            this.hooks = Some(hooks);
        });
    }
}

/// After the main window's first frame (FR-2): register.
pub fn start(cx: &mut App) {
    if let Some(entity) = Hotkeys::global(cx) {
        entity.update(cx, |this, cx| {
            let failed = this.controller.start(this.layout.as_ref());
            this.report(failed, cx);
            cx.notify();
        });
    }
}

/// Gallery stories: the Global shortcut section in each state (AC-14),
/// over a fake registrar and a US layout.
pub mod fixtures {
    use super::*;

    /// The states the section shows.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Scene {
        /// Registered, never pressed: "Press ⌃⌥⇧⌘A to test".
        Untested,
        /// Pressed just now.
        Works,
        /// Recording "Show adoken".
        Recording,
        /// A refused chord (⌘P is adoken's search).
        Refused,
        /// The OS refused the chord (−9878).
        Failed,
        /// Cleared.
        Off,
    }

    struct StoryRegistrar(Option<i32>);

    impl Registrar for StoryRegistrar {
        fn register(&mut self, _: u32, _: u32) -> Result<(), i32> {
            self.0.map_or(Ok(()), Err)
        }

        fn unregister(&mut self) {}
    }

    struct UsLayout;

    impl model::Layout for UsLayout {
        fn translate(&self, code: u16, _: bool) -> Option<String> {
            const KEYS: &str = "asdfhgzxcv bqweryt123465=97-80]ou[ip lj'k;\\,/nm.";
            KEYS.chars()
                .nth(usize::from(code))
                .filter(|c| *c != ' ')
                .map(String::from)
        }
    }

    /// Make `scene` the app's global shortcut state.
    pub fn install_for_story(scene: Scene, cx: &mut App) {
        let fail = (scene == Scene::Failed).then_some(-9878);
        let entity = cx.new(|cx| {
            let mut hotkeys = Hotkeys::new(Box::new(StoryRegistrar(fail)), Box::new(UsLayout), cx);
            let mut settings = HotkeySettings::default();
            if scene == Scene::Off {
                settings.summon = None;
            }
            hotkeys.controller.apply(settings, hotkeys.layout.as_ref());
            hotkeys.controller.start(hotkeys.layout.as_ref());
            match scene {
                Scene::Works => {
                    let now = hotkeys.now();
                    hotkeys.controller.press(now);
                }
                Scene::Recording => {
                    hotkeys.recording = true;
                    hotkeys
                        .controller
                        .set_recording(true, hotkeys.layout.as_ref());
                }
                Scene::Refused => {
                    hotkeys.refusal = Some(Refusal::Adoken {
                        label: "⌘P".into(),
                        does: "opens search".into(),
                    })
                }
                _ => {}
            }
            hotkeys
        });
        cx.set_global(Service(entity));
    }

    /// Stories without the section (the plain Keyboard tab).
    pub fn remove_for_story(cx: &mut App) {
        if cx.has_global::<Service>() {
            cx.remove_global::<Service>();
        }
    }
}
