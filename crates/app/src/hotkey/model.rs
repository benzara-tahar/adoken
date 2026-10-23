//! The global summon shortcut's pure model (spec 0021): chords, FR-5
//! validation, `[hotkey]` parsing, the registration controller behind the
//! [`Registrar`] seam (Carbon on macOS, a fake in tests), key-code
//! resolution against a [`Layout`], the summon decision table (FR-6) and
//! the §3.3 copy. No gpui windows, no Carbon, no IO.

use adoken_ui::{KeyPlatform, format_keystroke};
use gpui::{Keystroke, Modifiers};
use toml::{Table, Value};

pub const TABLE: &str = "hotkey";
/// FR-3: Hyper+A.
pub const DEFAULT_SUMMON: &str = "cmd-ctrl-alt-shift-a";
/// FR-6: presses closer than this to the previous one are ignored.
pub const DEBOUNCE_MS: u64 = 300;

/// Carbon modifier masks (`Events.h`: cmdKey, shiftKey, optionKey,
/// controlKey); zeron `appshots/macos.rs`.
pub const COMMAND_KEY: u32 = 1 << 8;
pub const SHIFT_KEY: u32 = 1 << 9;
pub const OPTION_KEY: u32 = 1 << 11;
pub const CONTROL_KEY: u32 = 1 << 12;

/// The `[hotkey]` key of the one configurable shortcut (FR-11). The
/// second chord is gone (spec 0025 FR-12): an old `summon_palette` key is
/// ignored and never written back (FR-14).
pub const KEY: &str = "summon";
/// The Settings row's label.
pub const LABEL: &str = "Show adoken";
/// The cheat sheet's row title (FR-13).
pub const CHEAT_SHEET_TITLE: &str = "Show adoken from any app";

/// A global chord: one key plus modifiers, keyed by its printed character
/// (FR-4).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: String,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

impl Chord {
    /// adoken's keymap syntax (`cmd-ctrl-alt-shift-a`); `None` when it
    /// doesn't parse or has no key.
    pub fn parse(text: &str) -> Option<Self> {
        let stroke = Keystroke::parse(text.trim()).ok()?;
        Self::from_keystroke(&stroke)
    }

    pub fn from_keystroke(stroke: &Keystroke) -> Option<Self> {
        let key = stroke.key.to_lowercase();
        if key.is_empty() || is_modifier_key(&key) {
            return None;
        }
        let m = stroke.modifiers;
        Some(Self {
            key,
            control: m.control,
            alt: m.alt,
            shift: m.shift,
            platform: m.platform,
        })
    }

    /// As written to `settings.toml`: `cmd-ctrl-alt-shift-a`.
    pub fn setting(&self) -> String {
        let mut parts: Vec<&str> = Vec::with_capacity(5);
        for (on, name) in [
            (self.platform, "cmd"),
            (self.control, "ctrl"),
            (self.alt, "alt"),
            (self.shift, "shift"),
        ] {
            if on {
                parts.push(name);
            }
        }
        parts.push(&self.key);
        parts.join("-")
    }

    pub fn keystroke(&self) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                control: self.control,
                alt: self.alt,
                shift: self.shift,
                platform: self.platform,
                function: false,
            },
            key: self.key.clone(),
            key_char: None,
        }
    }

    /// `⌃⌥⇧⌘A` (Apple order).
    pub fn label(&self) -> String {
        format_keystroke(&self.keystroke(), KeyPlatform::Mac)
    }

    pub fn carbon_modifiers(&self) -> u32 {
        (if self.control { CONTROL_KEY } else { 0 })
            | (if self.alt { OPTION_KEY } else { 0 })
            | (if self.shift { SHIFT_KEY } else { 0 })
            | (if self.platform { COMMAND_KEY } else { 0 })
    }

    fn same_keys(&self, stroke: &Keystroke) -> bool {
        let m = stroke.modifiers;
        stroke.key.eq_ignore_ascii_case(&self.key)
            && m.control == self.control
            && m.alt == self.alt
            && m.shift == self.shift
            && m.platform == self.platform
            && !m.function
    }
}

fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "cmd"
            | "command"
            | "ctrl"
            | "control"
            | "alt"
            | "option"
            | "shift"
            | "fn"
            | "function"
            | "secondary"
            | "platform"
            | "super"
            | "win"
    )
}

/// One of adoken's own bindings: its keystroke and what it does.
#[derive(Clone, Debug)]
pub struct OwnBinding {
    pub keystroke: Keystroke,
    /// "opens search", "runs “Toggle sidebar”".
    pub does: String,
}

/// macOS's own chords (FR-5).
const MACOS_RESERVED: [&str; 12] = [
    "cmd-tab",
    "cmd-shift-tab",
    "cmd-space",
    "ctrl-space",
    "cmd-q",
    "cmd-h",
    "cmd-m",
    "cmd-w",
    "cmd-alt-escape",
    "cmd-shift-3",
    "cmd-shift-4",
    "cmd-shift-5",
];

/// Why a chord can't be the global shortcut (§3.3 copy).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    ModifiersOnly,
    NoCommandOrControl,
    MacOs(String),
    Adoken { label: String, does: String },
}

impl Refusal {
    pub fn message(&self) -> String {
        match self {
            Self::ModifiersOnly => "Add a key, like A.".into(),
            Self::NoCommandOrControl => "Add ⌘ or ⌃ to the shortcut.".into(),
            Self::MacOs(label) => format!("{label} is used by macOS. Pick another shortcut."),
            Self::Adoken { label, does } => {
                format!("{label} {does} in adoken. Pick another shortcut.")
            }
        }
    }
}

/// FR-5: at least ⌘ or ⌃ plus one non-modifier key, not one of macOS's
/// chords, not bound anywhere in adoken's keymap.
pub fn validate(stroke: &Keystroke, own: &[OwnBinding]) -> Result<Chord, Refusal> {
    let Some(chord) = Chord::from_keystroke(stroke) else {
        return Err(Refusal::ModifiersOnly);
    };
    if !(chord.platform || chord.control) {
        return Err(Refusal::NoCommandOrControl);
    }
    let label = chord.label();
    if MACOS_RESERVED
        .iter()
        .filter_map(|text| Keystroke::parse(text).ok())
        .any(|reserved| chord.same_keys(&reserved))
    {
        return Err(Refusal::MacOs(label));
    }
    if let Some(binding) = own.iter().find(|b| chord.same_keys(&b.keystroke)) {
        return Err(Refusal::Adoken {
            label,
            does: binding.does.clone(),
        });
    }
    Ok(chord)
}

/// `[hotkey]` as read (FR-11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeySettings {
    pub summon: Option<Chord>,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            summon: Chord::parse(DEFAULT_SUMMON),
        }
    }
}

impl HotkeySettings {
    /// The `[hotkey]` table (`""` = off).
    pub fn table(&self) -> Table {
        let value = self.summon.as_ref().map(Chord::setting).unwrap_or_default();
        Table::from_iter([(KEY.to_owned(), Value::String(value))])
    }
}

/// Parse `[hotkey]` from the whole file. A value that doesn't parse or
/// fails FR-5 falls back to the default and is reported (FR-11).
pub fn parse(document: &Table, own: &[OwnBinding]) -> (HotkeySettings, Vec<String>) {
    let mut settings = HotkeySettings::default();
    let mut issues = Vec::new();
    let Some(table) = document.get(TABLE) else {
        return (settings, issues);
    };
    let Some(table) = table.as_table() else {
        issues.push(format!("{TABLE}: expected a table; using the defaults"));
        return (settings, issues);
    };
    let Some(value) = table.get(KEY) else {
        return (settings, issues);
    };
    let Some(text) = value.as_str() else {
        issues.push(format!(
            "{TABLE}.{KEY}: expected a shortcut like \"{DEFAULT_SUMMON}\"; using \"{DEFAULT_SUMMON}\""
        ));
        return (settings, issues);
    };
    if text.trim().is_empty() {
        settings.summon = None;
        return (settings, issues);
    }
    let checked = Keystroke::parse(text.trim())
        .map_err(|_| None)
        .and_then(|stroke| validate(&stroke, own).map_err(Some));
    match checked {
        Ok(chord) => settings.summon = Some(chord),
        Err(refusal) => {
            let why = refusal.map_or_else(
                || "not a shortcut".to_owned(),
                |refusal| refusal.message().trim_end_matches('.').to_owned(),
            );
            issues.push(format!(
                "{TABLE}.{KEY}: \"{text}\": {why}; using \"{DEFAULT_SUMMON}\""
            ));
        }
    }
    (settings, issues)
}

// ── key codes (FR-4) ────────────────────────────────────────────────────

/// Resolves a printed key to a virtual key code on the current layout.
pub trait Layout {
    /// The characters key `code` types on the current ASCII-capable layout,
    /// unshifted (`shift == false`) or shifted.
    fn translate(&self, code: u16, shift: bool) -> Option<String>;
}

/// The key code for `key`: named keys by fixed code, anything else by
/// searching the layout (unshifted first), so "a" is A on AZERTY too.
/// Ported from zeron `shortcut_keycode` / `layout_keycode`.
pub fn key_code(key: &str, layout: &dyn Layout) -> Option<u32> {
    let named = match key {
        "space" => Some(49),
        "tab" => Some(48),
        "enter" => Some(36),
        "backspace" => Some(51),
        "delete" => Some(117),
        "insert" => Some(114),
        "escape" => Some(53),
        "up" => Some(126),
        "down" => Some(125),
        "left" => Some(123),
        "right" => Some(124),
        "home" => Some(115),
        "end" => Some(119),
        "pageup" => Some(116),
        "pagedown" => Some(121),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(index) = key.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()) {
        const F_KEYS: [u32; 20] = [
            122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80,
            90,
        ];
        return index.checked_sub(1).and_then(|i| F_KEYS.get(i).copied());
    }
    for shift in [false, true] {
        for code in 0..128_u16 {
            if layout
                .translate(code, shift)
                .is_some_and(|typed| typed.to_lowercase() == key)
            {
                return Some(u32::from(code));
            }
        }
    }
    None
}

// ── registration (FR-1, FR-2, FR-8, FR-12) ──────────────────────────────

/// The system-wide registration seam (Carbon `RegisterEventHotKey` on
/// macOS).
pub trait Registrar {
    /// Register the shortcut; `Err` carries the OS status (e.g. −9878).
    fn register(&mut self, key_code: u32, modifiers: u32) -> Result<(), i32>;
    fn unregister(&mut self);
}

impl<R: Registrar + ?Sized> Registrar for Box<R> {
    fn register(&mut self, key_code: u32, modifiers: u32) -> Result<(), i32> {
        (**self).register(key_code, modifiers)
    }

    fn unregister(&mut self) {
        (**self).unregister()
    }
}

/// The shortcut's state for the status line (§3.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Cleared: nothing registered.
    Off,
    /// Registered; `last_used` in the caller's ms clock.
    Registered {
        chord: Chord,
        last_used: Option<u64>,
    },
    /// The OS refused it (`code`), or the key has no code on this layout.
    Failed { chord: Chord, code: Option<i32> },
    /// Not registered yet (before the first frame) or suspended while
    /// recording.
    Waiting { chord: Chord },
}

/// Keeps the OS registration in line with the settings, the first-frame
/// start, the recorder and the keyboard layout. Every change returns the
/// chord whose registration just failed, if any (FR-12: one launch toast
/// per value).
pub struct Controller<R: Registrar> {
    pub registrar: R,
    settings: HotkeySettings,
    started: bool,
    recording: bool,
    /// What's registered: (chord, key code).
    registered: Option<(Chord, u32)>,
    status: Status,
    last_press: Option<u64>,
}

impl<R: Registrar> Controller<R> {
    pub fn new(registrar: R) -> Self {
        let settings = HotkeySettings::default();
        let status = match &settings.summon {
            Some(chord) => Status::Waiting {
                chord: chord.clone(),
            },
            None => Status::Off,
        };
        Self {
            registrar,
            settings,
            started: false,
            recording: false,
            registered: None,
            status,
            last_press: None,
        }
    }

    pub fn settings(&self) -> &HotkeySettings {
        &self.settings
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// After the first frame (FR-2): register what's configured.
    pub fn start(&mut self, layout: &dyn Layout) -> Option<Chord> {
        self.started = true;
        self.sync(layout)
    }

    /// New settings (Settings, a hand edit, the recorder).
    pub fn apply(&mut self, settings: HotkeySettings, layout: &dyn Layout) -> Option<Chord> {
        self.settings = settings;
        self.sync(layout)
    }

    /// FR-8: recording unregisters the shortcut; stopping re-registers
    /// what the settings say by then.
    pub fn set_recording(&mut self, recording: bool, layout: &dyn Layout) -> Option<Chord> {
        self.recording = recording;
        self.sync(layout)
    }

    /// The keyboard input source changed (FR-4): re-resolve, re-register
    /// if the code moved.
    pub fn relayout(&mut self, layout: &dyn Layout) -> Option<Chord> {
        self.sync(layout)
    }

    /// Quit unregisters (FR-2).
    pub fn stop(&mut self) {
        self.unregister();
        self.started = false;
    }

    /// A press at `now`: `false` within [`DEBOUNCE_MS`] of the previous
    /// one (FR-6); otherwise it counts as "last used".
    pub fn press(&mut self, now: u64) -> bool {
        if self
            .last_press
            .is_some_and(|last| now.saturating_sub(last) < DEBOUNCE_MS)
        {
            return false;
        }
        self.last_press = Some(now);
        if let Status::Registered { last_used, .. } = &mut self.status {
            *last_used = Some(now);
        }
        true
    }

    fn unregister(&mut self) {
        if self.registered.take().is_some() {
            self.registrar.unregister();
        }
    }

    fn sync(&mut self, layout: &dyn Layout) -> Option<Chord> {
        let Some(chord) = self.settings.summon.clone() else {
            self.unregister();
            self.status = Status::Off;
            return None;
        };
        if !self.started || self.recording {
            self.unregister();
            self.status = Status::Waiting { chord };
            return None;
        }
        let code = key_code(&chord.key, layout);
        if let (Some(code), Some((was, was_code))) = (code, &self.registered)
            && *was == chord
            && *was_code == code
        {
            return None;
        }
        self.unregister();
        let last_used = match &self.status {
            Status::Registered {
                chord: was,
                last_used,
            } if *was == chord => *last_used,
            _ => None,
        };
        let result = match code {
            Some(code) => self
                .registrar
                .register(code, chord.carbon_modifiers())
                .map(|()| code)
                .map_err(Some),
            None => Err(None),
        };
        match result {
            Ok(code) => {
                self.registered = Some((chord.clone(), code));
                self.status = Status::Registered { chord, last_used };
                None
            }
            Err(code) => {
                self.status = Status::Failed {
                    chord: chord.clone(),
                    code,
                };
                Some(chord)
            }
        }
    }
}

/// "just now", "2 min ago", "3 h ago".
pub fn ago(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// The shortcut's status line (§3.3); `now` in the same clock as presses.
pub fn status_line(status: &Status, now: u64) -> String {
    match status {
        Status::Off => "Off".into(),
        Status::Registered {
            last_used: Some(at),
            ..
        } => format!("Works · last used {}", ago(now.saturating_sub(*at))),
        Status::Registered { chord, .. } | Status::Waiting { chord } => {
            format!("Press {} to test", chord.label())
        }
        Status::Failed { chord, .. } => format!(
            "Couldn't register {}. Another app may use it; pick another shortcut.",
            chord.label()
        ),
    }
}

// ── summon (FR-6, FR-8) ─────────────────────────────────────────────────

/// The main window when the press lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainWindow {
    /// Visible and key.
    Key,
    /// Visible, not key (another app or the Settings window in front).
    Visible,
    Minimized,
    Closed,
}

/// What adoken and its windows look like when a press lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SummonState {
    pub app_active: bool,
    pub app_hidden: bool,
    pub main: MainWindow,
    /// The Settings window is key and shows Keyboard (FR-8).
    pub settings_keyboard_key: bool,
}

/// What a press does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Summon {
    /// Settings › Keyboard is key: update the status line only.
    Test,
    /// adoken is in front with its main window key (Q-1).
    Nothing,
    /// Bring the main window forward and make it key.
    Activate {
        unhide: bool,
        deminiaturize: bool,
        open_main: bool,
    },
}

pub fn decide(state: SummonState) -> Summon {
    if state.app_active && !state.app_hidden && state.settings_keyboard_key {
        return Summon::Test;
    }
    if state.app_active && !state.app_hidden && state.main == MainWindow::Key {
        return Summon::Nothing;
    }
    Summon::Activate {
        unhide: state.app_hidden,
        deminiaturize: state.main == MainWindow::Minimized,
        open_main: state.main == MainWindow::Closed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Partial layouts: code → unshifted character.
    struct Keys(&'static [(u16, &'static str)]);

    impl Layout for Keys {
        fn translate(&self, code: u16, shift: bool) -> Option<String> {
            let found = self.0.iter().find(|(c, _)| *c == code)?.1;
            Some(if shift {
                found.to_uppercase()
            } else {
                found.to_owned()
            })
        }
    }

    const US: Keys = Keys(&[(0, "a"), (12, "q"), (13, "w"), (40, "k"), (35, "p")]);
    const AZERTY: Keys = Keys(&[(0, "q"), (12, "a"), (13, "z"), (6, "w"), (40, "k")]);

    #[derive(Default)]
    struct Fake {
        calls: Vec<String>,
        fail_with: Option<i32>,
    }

    impl Registrar for Fake {
        fn register(&mut self, key_code: u32, modifiers: u32) -> Result<(), i32> {
            self.calls
                .push(format!("register {KEY} {key_code} {modifiers}"));
            match self.fail_with {
                Some(code) => Err(code),
                None => Ok(()),
            }
        }

        fn unregister(&mut self) {
            self.calls.push(format!("unregister {KEY}"));
        }
    }

    const HYPER: u32 = COMMAND_KEY | CONTROL_KEY | OPTION_KEY | SHIFT_KEY;

    fn own() -> Vec<OwnBinding> {
        vec![OwnBinding {
            keystroke: Keystroke::parse("cmd-p").unwrap(),
            does: "opens search".into(),
        }]
    }

    fn stroke(text: &str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn default_registers_hyper_a_only_after_the_first_frame() {
        // AC-1.
        let mut c = Controller::new(Fake::default());
        let (settings, _) = parse(&Table::new(), &own());
        c.apply(settings, &US);
        assert!(
            c.registrar.calls.is_empty(),
            "nothing before the first frame"
        );
        assert_eq!(status_line(c.status(), 0), "Press ⌃⌥⇧⌘A to test");
        assert!(c.start(&US).is_none());
        assert_eq!(c.registrar.calls, [format!("register summon 0 {HYPER}")]);
        assert_eq!(status_line(c.status(), 0), "Press ⌃⌥⇧⌘A to test");
    }

    #[test]
    fn summon_matrix_follows_fr6() {
        // AC-2.
        let state = |app_active, app_hidden, main, settings_keyboard_key| SummonState {
            app_active,
            app_hidden,
            main,
            settings_keyboard_key,
        };
        let activate = |unhide, deminiaturize, open_main| Summon::Activate {
            unhide,
            deminiaturize,
            open_main,
        };
        use MainWindow::*;
        assert_eq!(
            decide(state(false, false, Visible, false)),
            activate(false, false, false)
        );
        assert_eq!(
            decide(state(false, true, Visible, false)),
            activate(true, false, false)
        );
        assert_eq!(
            decide(state(false, false, Minimized, false)),
            activate(false, true, false)
        );
        assert_eq!(
            decide(state(false, false, Closed, false)),
            activate(false, false, true)
        );
        // Settings key (not Keyboard): the main window becomes key.
        assert_eq!(
            decide(state(true, false, Visible, false)),
            activate(false, false, false)
        );
        assert_eq!(decide(state(true, false, Key, false)), Summon::Nothing);
        assert_eq!(decide(state(true, false, Visible, true)), Summon::Test);
        // Debounce: 200 ms after a press does nothing, 350 ms acts.
        let mut c = Controller::new(Fake::default());
        c.start(&US);
        assert!(c.press(1_000));
        assert!(!c.press(1_200));
        assert!(c.press(1_350));
    }

    #[test]
    fn recording_suspends_saves_cancels_and_clears() {
        // AC-5.
        let mut c = Controller::new(Fake::default());
        c.start(&US);
        c.registrar.calls.clear();
        c.set_recording(true, &US);
        assert_eq!(c.registrar.calls, ["unregister summon"]);
        c.registrar.calls.clear();
        let chord = validate(&stroke("cmd-ctrl-alt-k"), &own()).unwrap();
        assert_eq!(chord.setting(), "cmd-ctrl-alt-k");
        let mut settings = c.settings().clone();
        settings.summon = Some(chord);
        c.apply(settings, &US);
        c.set_recording(false, &US);
        let k = COMMAND_KEY | CONTROL_KEY | OPTION_KEY;
        assert_eq!(c.registrar.calls, [format!("register summon 40 {k}")]);
        // Record, then Esc: the previous chord comes back unchanged.
        c.set_recording(true, &US);
        c.registrar.calls.clear();
        c.set_recording(false, &US);
        assert_eq!(c.registrar.calls, [format!("register summon 40 {k}")]);
        // ⌫ while recording: off, nothing registered.
        c.set_recording(true, &US);
        let mut settings = c.settings().clone();
        settings.summon = None;
        assert_eq!(settings.table()["summon"].as_str(), Some(""));
        c.apply(settings, &US);
        c.registrar.calls.clear();
        c.set_recording(false, &US);
        assert!(c.registrar.calls.is_empty());
        assert_eq!(status_line(c.status(), 0), "Off");
    }

    #[test]
    fn refusals_name_their_reason() {
        // AC-6.
        let own = own();
        let refused = |text: &str| validate(&stroke(text), &own).unwrap_err().message();
        assert_eq!(refused("alt-shift-k"), "Add ⌘ or ⌃ to the shortcut.");
        assert_eq!(
            refused("cmd-p"),
            "⌘P opens search in adoken. Pick another shortcut."
        );
        assert_eq!(
            refused("cmd-tab"),
            "⌘⇥ is used by macOS. Pick another shortcut."
        );
        let modifiers_only = Keystroke {
            modifiers: Modifiers {
                platform: true,
                alt: true,
                ..Modifiers::default()
            },
            key: String::new(),
            key_char: None,
        };
        assert_eq!(
            validate(&modifiers_only, &own).unwrap_err(),
            Refusal::ModifiersOnly
        );
        assert_eq!(Refusal::ModifiersOnly.message(), "Add a key, like A.");
        assert!(validate(&stroke("ctrl-alt-cmd-k"), &own).is_ok());
    }

    #[test]
    fn failures_show_once_and_a_new_value_retries() {
        // AC-7 (the launch toast's once-per-value memory lives in the
        // caller; the controller reports each failed registration).
        let mut c = Controller::new(Fake {
            fail_with: Some(-9878),
            ..Fake::default()
        });
        let failed = c.start(&US);
        assert_eq!(failed, Chord::parse(DEFAULT_SUMMON));
        assert_eq!(
            status_line(c.status(), 0),
            "Couldn't register ⌃⌥⇧⌘A. Another app may use it; pick another shortcut."
        );
        c.registrar.fail_with = None;
        c.registrar.calls.clear();
        let mut settings = c.settings().clone();
        settings.summon = Chord::parse("cmd-ctrl-k");
        assert!(c.apply(settings, &US).is_none());
        assert_eq!(
            c.registrar.calls,
            [format!("register summon 40 {}", COMMAND_KEY | CONTROL_KEY)]
        );
    }

    #[test]
    fn settings_fall_back_per_key_and_hand_edits_re_register() {
        // AC-8.
        let doc = |text: &str| toml::from_str::<Table>(text).unwrap();
        let (off, issues) = parse(&doc("[hotkey]\nsummon = \"\"\n"), &own());
        assert_eq!(off.summon, None);
        assert!(issues.is_empty());
        let (banana, issues) = parse(&doc("[hotkey]\nsummon = \"banana\"\n"), &own());
        assert_eq!(banana, HotkeySettings::default());
        assert_eq!(issues.len(), 1, "{issues:?}");
        let (refused, issues) = parse(&doc("[hotkey]\nsummon = \"alt-shift-k\"\n"), &own());
        assert_eq!(refused, HotkeySettings::default());
        assert!(issues[0].contains("Add ⌘ or ⌃"), "{issues:?}");
        let mut c = Controller::new(Fake::default());
        c.apply(off, &US);
        c.start(&US);
        assert!(
            c.registrar.calls.is_empty(),
            "summon = \"\" registers nothing"
        );
        let (edited, _) = parse(
            &doc("[hotkey]\nsummon = \"cmd-ctrl-alt-shift-a\"\n"),
            &own(),
        );
        c.apply(edited, &US);
        assert_eq!(c.registrar.calls, [format!("register summon 0 {HYPER}")]);
    }

    #[test]
    fn an_old_second_chord_loads_and_the_next_save_drops_it() {
        // Spec 0025 FR-14 / AC-10 (hotkey part): `summon_palette` is
        // ignored on load, `summon` and the rest of the file are kept, and
        // the next `[hotkey]` write leaves the removed key out.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(adoken_settings::FILE_NAME);
        std::fs::write(
            &path,
            "[hotkey]\nsummon = \"cmd-ctrl-k\"\nsummon_palette = \"cmd-ctrl-alt-shift-f\"\n\n\
             [sidebar]\ncollapsed = true\n",
        )
        .unwrap();
        let read = |path: &std::path::Path| {
            toml::from_str::<Table>(&std::fs::read_to_string(path).unwrap()).unwrap()
        };
        let (settings, issues) = parse(&read(&path), &own());
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(settings.summon, Chord::parse("cmd-ctrl-k"));

        adoken_settings::update(&path, |doc| {
            adoken_settings::set_table(doc, TABLE, &settings.table())
        })
        .unwrap();
        let saved = read(&path);
        let hotkey = saved[TABLE].as_table().unwrap();
        assert_eq!(hotkey.keys().collect::<Vec<_>>(), [KEY]);
        assert_eq!(hotkey[KEY].as_str(), Some("cmd-ctrl-k"));
        assert_eq!(saved["sidebar"]["collapsed"].as_bool(), Some(true));
    }

    #[test]
    fn keys_resolve_against_the_layout_and_re_register_on_change() {
        // AC-9.
        assert_eq!(key_code("a", &US), Some(0));
        assert_eq!(key_code("a", &AZERTY), Some(12));
        assert_eq!(key_code("f5", &US), Some(96));
        assert_eq!(key_code("space", &AZERTY), Some(49));
        let mut c = Controller::new(Fake::default());
        c.start(&US);
        c.registrar.calls.clear();
        c.relayout(&US);
        assert!(c.registrar.calls.is_empty(), "same code: nothing to do");
        c.relayout(&AZERTY);
        assert_eq!(
            c.registrar.calls,
            [
                "unregister summon".to_owned(),
                format!("register summon 12 {HYPER}")
            ]
        );
    }

    #[test]
    fn last_used_reads_as_the_status_line() {
        let mut c = Controller::new(Fake::default());
        c.start(&US);
        c.press(10_000);
        let status = c.status().clone();
        assert_eq!(status_line(&status, 10_500), "Works · last used just now");
        assert_eq!(status_line(&status, 130_000), "Works · last used 2 min ago");
    }
}
