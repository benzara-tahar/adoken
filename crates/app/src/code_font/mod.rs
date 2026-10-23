//! Code font (spec 0009 §3 Code font, user decision 2026-09-30): the family, size
//! and ligatures of every code surface (diffs, code blocks, inline code).
//!
//! - [`init`] registers the bundled JetBrains Mono with the text system
//!   before the first frame (borrowed bytes, no IO) and paints the default.
//! - [`apply`] takes `settings.toml [appearance]` as read (launch, outside
//!   edits, our own writes). Size and ligatures apply on the next frame; a
//!   system or installed family is first resolved off the UI thread, and a
//!   family the text system doesn't have falls back to JetBrains Mono (then
//!   the system monospace face) with a one-time statusbar notice.
//! - [`set`] applies and persists through the theme library's
//!   `[appearance]` table, so the theme key and these keys never drop each
//!   other.
//! - The Settings picker lists installed monospace families; the list is
//!   scanned once per session on the background executor ([`load_families`]).

pub mod picker;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adoken_theme::code_font::{DEFAULT_CODE_FONT_SIZE, system_mono_family};
use adoken_theme::{CodeFont, CodeFontSettings};
use adoken_ui::{CodeFontStyle, Theme};
use gpui::{App, Global, SharedString, Task, TextSystem, actions, px};

pub use picker::CodeFontPicker;

use crate::palette::PaletteCommand;
use crate::settings;

actions!(
    code_font,
    [
        /// Open Settings on the code font picker.
        ChooseCodeFont,
        /// Make code text one pixel bigger.
        IncreaseCodeFontSize,
        /// Make code text one pixel smaller.
        DecreaseCodeFontSize,
        /// Code text back to the default size.
        ResetCodeFontSize,
        /// Turn programming ligatures in code on or off.
        ToggleCodeLigatures,
    ]
);

/// What the Settings search box shows for [`ChooseCodeFont`].
pub const SETTINGS_QUERY: &str = "code font";
/// How long the fallback notice stays in the statusbar.
const NOTICE_FOR: Duration = Duration::from_secs(8);

/// `>` commands (keyboard-first: every code font action is here).
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Code font…",
            keys: None,
            action: Box::new(ChooseCodeFont),
        },
        PaletteCommand {
            title: "Increase code font size",
            keys: None,
            action: Box::new(IncreaseCodeFontSize),
        },
        PaletteCommand {
            title: "Decrease code font size",
            keys: None,
            action: Box::new(DecreaseCodeFontSize),
        },
        PaletteCommand {
            title: "Reset code font size",
            keys: None,
            action: Box::new(ResetCodeFontSize),
        },
        PaletteCommand {
            title: "Toggle code ligatures",
            keys: None,
            action: Box::new(ToggleCodeLigatures),
        },
    ]
}

/// Installed monospace families for the picker.
#[derive(Debug, Clone, Default)]
pub enum Families {
    #[default]
    NotLoaded,
    Loading,
    Ready(Arc<[SharedString]>),
}

pub struct CodeFontState {
    /// As configured (`[appearance]`), whether or not the family exists.
    settings: CodeFontSettings,
    /// The bundled faces registered at launch.
    bundled: bool,
    /// The configured family isn't installed: its name. Code renders in
    /// the fallback meanwhile.
    missing: Option<String>,
    /// Families a notice was already shown for (one notice each).
    announced: HashSet<String>,
    /// The statusbar's fallback notice, while it shows.
    notice: Option<SharedString>,
    families: Families,
    /// Bumped per [`apply`]; a family check finishing late is dropped.
    generation: u64,
    _notice_timer: Option<Task<()>>,
}

impl Global for CodeFontState {}

impl CodeFontState {
    fn new(bundled: bool) -> Self {
        Self {
            settings: CodeFontSettings::default(),
            bundled,
            missing: None,
            announced: HashSet::new(),
            notice: None,
            families: Families::NotLoaded,
            generation: 0,
            _notice_timer: None,
        }
    }

    pub fn settings(&self) -> &CodeFontSettings {
        &self.settings
    }

    /// The configured family, when the text system doesn't have it.
    pub fn missing(&self) -> Option<&str> {
        self.missing.as_deref()
    }

    pub fn families(&self) -> &Families {
        &self.families
    }
}

/// Register the bundled fonts, the state, and the actions; paint the
/// default code font. No IO: first-frame budget.
pub fn init(cx: &mut App) {
    let started = Instant::now();
    let bundled = match adoken_ui::fonts::register(cx) {
        Ok(()) => true,
        Err(error) => {
            log::warn!("code font: couldn't register the bundled JetBrains Mono: {error}");
            false
        }
    };
    log::debug!(
        "code font: bundled fonts registered in {:?}",
        started.elapsed()
    );
    install_state(CodeFontState::new(bundled), cx);
    cx.on_action(|_: &ChooseCodeFont, cx| {
        settings::open_with_query(settings::Tab::Appearance, SETTINGS_QUERY, cx)
    });
    cx.on_action(|_: &IncreaseCodeFontSize, cx| step_size(1, cx));
    cx.on_action(|_: &DecreaseCodeFontSize, cx| step_size(-1, cx));
    cx.on_action(|_: &ResetCodeFontSize, cx| {
        update(cx, |settings| settings.size = DEFAULT_CODE_FONT_SIZE)
    });
    cx.on_action(|_: &ToggleCodeLigatures, cx| {
        update(cx, |settings| settings.ligatures = !settings.ligatures)
    });
}

/// Stories and tests: the state without touching the text system, as if
/// `settings` were configured and `missing` not installed.
pub fn install_for_story(settings: CodeFontSettings, missing: Option<&str>, cx: &mut App) {
    let mut state = CodeFontState::new(true);
    state.settings = settings;
    state.missing = missing.map(str::to_owned);
    cx.set_global(state);
}

fn install_state(state: CodeFontState, cx: &mut App) {
    let style = style_for(&state.settings, rendered_family(&state, None));
    cx.set_global(state);
    Theme::set_code_font(style, cx);
}

/// The configured settings (defaults before [`init`]).
pub fn current(cx: &App) -> CodeFontSettings {
    cx.try_global::<CodeFontState>()
        .map(|state| state.settings.clone())
        .unwrap_or_default()
}

/// The statusbar notice, while it shows.
pub fn notice(cx: &App) -> Option<SharedString> {
    cx.try_global::<CodeFontState>()
        .and_then(|state| state.notice.clone())
}

/// Change the settings with `change`, apply them now and save them.
pub fn update(cx: &mut App, change: impl FnOnce(&mut CodeFontSettings)) {
    let mut settings = current(cx);
    change(&mut settings);
    set(settings, cx);
}

fn step_size(steps: i8, cx: &mut App) {
    let settings = current(cx).stepped(steps);
    set(settings, cx);
}

/// Apply `settings` now and persist them to `[appearance]`.
pub fn set(settings: CodeFontSettings, cx: &mut App) {
    if cx.try_global::<CodeFontState>().is_none() || current(cx) == settings {
        return;
    }
    apply(settings.clone(), cx);
    crate::theme_picker::library::update_appearance(cx, |table| settings.write_into(table));
}

/// Render code with `settings` (as read from `settings.toml`).
pub fn apply(settings: CodeFontSettings, cx: &mut App) {
    let Some(state) = cx.try_global::<CodeFontState>() else {
        return;
    };
    if state.settings == settings {
        return;
    }
    let family_changed = state.settings.font != settings.font;
    let state = cx.global_mut::<CodeFontState>();
    state.settings = settings;
    if !family_changed {
        // Size or ligatures: the family painting now stays (a pending
        // family check still lands).
        repaint(cx);
        return;
    }
    state.generation += 1;
    let generation = state.generation;
    let requested = state.settings.font.clone();
    if requested == CodeFont::Bundled {
        let available = state.bundled;
        finish(generation, available, cx);
        return;
    }
    // A system or installed family: loading it can take milliseconds, so
    // it is resolved off the UI thread; size and ligatures apply meanwhile.
    repaint(cx);
    let text_system = cx.text_system().clone();
    let name = requested.family().to_owned();
    let check = cx
        .background_executor()
        .spawn(async move { family_available(&text_system, &name) });
    cx.spawn(async move |cx| {
        let available = check.await;
        cx.update(|cx| finish(generation, available, cx));
    })
    .detach();
}

/// The configured size and ligatures on the family painting now.
fn repaint(cx: &mut App) {
    let Some(theme) = cx.try_global::<Theme>() else {
        return;
    };
    let family = theme.code_font.family.clone();
    let style = style_for(&cx.global::<CodeFontState>().settings, family);
    Theme::set_code_font(style, cx);
}

/// The configured family is (not) `available`: paint it or the fallback,
/// and announce a new fallback once.
fn finish(generation: u64, available: bool, cx: &mut App) {
    let Some(state) = cx.try_global::<CodeFontState>() else {
        return;
    };
    if state.generation != generation {
        return;
    }
    let requested = state.settings.font.clone();
    let missing = (!available).then(|| requested.family().to_owned());
    let family = rendered_family(state, missing.as_deref());
    let style = style_for(&state.settings, family.clone());
    let announce = missing
        .as_ref()
        .filter(|name| !state.announced.contains(*name))
        .map(|name| {
            SharedString::from(format!(
                "Code font “{}” isn't available · showing {}",
                display_name(&requested, name),
                display_family(&family),
            ))
        });
    let state = cx.global_mut::<CodeFontState>();
    state.missing = missing.clone();
    Theme::set_code_font(style, cx);
    if let (Some(text), Some(name)) = (announce, missing) {
        log::warn!("code font: {text}");
        show_notice(name, text, cx);
    }
}

fn show_notice(family: String, text: SharedString, cx: &mut App) {
    let timer = cx.spawn(async move |cx| {
        cx.background_executor().timer(NOTICE_FOR).await;
        cx.update(|cx| {
            if let Some(state) = cx.try_global::<CodeFontState>()
                && state.notice.is_some()
            {
                cx.global_mut::<CodeFontState>().notice = None;
                cx.refresh_windows();
            }
        });
    });
    let state = cx.global_mut::<CodeFontState>();
    state.announced.insert(family);
    state.notice = Some(text);
    state._notice_timer = Some(timer);
    cx.refresh_windows();
}

/// The family code renders in: the configured one unless it is `missing`
/// (or the bundled faces failed to register), then JetBrains Mono, then
/// the system monospace face.
fn rendered_family(state: &CodeFontState, missing: Option<&str>) -> SharedString {
    let requested = &state.settings.font;
    let bundled_ok = state.bundled;
    let family = match requested {
        CodeFont::Bundled if bundled_ok => requested.family(),
        CodeFont::Bundled => system_mono_family(),
        _ if missing.is_none() => requested.family(),
        _ if bundled_ok => CodeFont::Bundled.family(),
        _ => system_mono_family(),
    };
    SharedString::from(family.to_owned())
}

fn style_for(settings: &CodeFontSettings, family: SharedString) -> CodeFontStyle {
    CodeFontStyle::new(family, f32::from(settings.size), settings.ligatures)
}

/// A family name as people know it (the macOS system face has no public
/// name).
pub fn display_family(family: &str) -> String {
    if family == system_mono_family() {
        CodeFont::System.label()
    } else {
        family.to_owned()
    }
}

fn display_name(requested: &CodeFont, family: &str) -> String {
    match requested {
        CodeFont::Installed(_) => family.to_owned(),
        other => other.label(),
    }
}

/// The text system has `family` (not a fallback face standing in).
pub fn family_available(text_system: &TextSystem, family: &str) -> bool {
    let font = gpui::font(family.to_owned());
    let id = text_system.resolve_font(&font);
    text_system
        .get_font_for_id(id)
        .is_some_and(|resolved| resolved.family == font.family)
}

/// Relative spread the sampled advances may show and still count as fixed
/// width: hinting leaves sub-unit differences in real monospace faces,
/// proportional faces differ by far more (zeron `typography.rs`).
const FIXED_WIDTH_TOLERANCE: f32 = 0.01;

/// The family is available and narrow, wide, round and digit glyphs all
/// advance the same (measured: metadata flags can be wrong).
fn is_monospace(text_system: &TextSystem, family: &str) -> bool {
    if !family_available(text_system, family) {
        return false;
    }
    let id = text_system.resolve_font(&gpui::font(family.to_owned()));
    let size = px(12.0);
    let Some(advances) = ['i', 'm', 'W', '0']
        .into_iter()
        .map(|ch| {
            text_system
                .advance(id, size, ch)
                .ok()
                .map(|a| a.width / px(1.0))
        })
        .collect::<Option<Vec<f32>>>()
    else {
        return false;
    };
    let min = advances.iter().copied().fold(f32::INFINITY, f32::min);
    let max = advances.iter().copied().fold(0.0, f32::max);
    fixed_width(min, max)
}

fn fixed_width(min: f32, max: f32) -> bool {
    min > 0.0 && max - min <= FIXED_WIDTH_TOLERANCE * min
}

/// Installed monospace families by name, without hidden system faces and
/// the bundled family (the picker lists it first).
fn scan_families(text_system: &TextSystem) -> Vec<SharedString> {
    let mut names = text_system.all_font_names();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .filter(|name| !name.starts_with('.') && !CodeFont::parse(name).eq(&CodeFont::Bundled))
        .filter(|name| is_monospace(text_system, name))
        .map(SharedString::from)
        .collect()
}

/// Scan installed families once (background executor; about a second on a
/// typical Mac, so only when the picker first shows).
pub fn load_families(cx: &mut App) {
    let Some(state) = cx.try_global::<CodeFontState>() else {
        return;
    };
    if !matches!(state.families, Families::NotLoaded) {
        return;
    }
    cx.global_mut::<CodeFontState>().families = Families::Loading;
    let text_system = cx.text_system().clone();
    let scan = cx
        .background_executor()
        .spawn(async move { scan_families(&text_system) });
    cx.spawn(async move |cx| {
        let families = scan.await;
        cx.update(|cx| {
            if cx.has_global::<CodeFontState>() {
                cx.global_mut::<CodeFontState>().families = Families::Ready(families.into());
            }
        });
    })
    .detach();
}

/// Stories: a fixed family list instead of the scan.
pub fn set_families_for_story(families: &[&str], cx: &mut App) {
    if cx.has_global::<CodeFontState>() {
        cx.global_mut::<CodeFontState>().families =
            Families::Ready(families.iter().map(|f| SharedString::from(*f)).collect());
    }
}

/// The picker's options for `query`: bundled, system, then installed
/// families, each kept when every query word appears in its label.
pub fn choices(installed: &[SharedString], query: &str) -> Vec<CodeFont> {
    [CodeFont::Bundled, CodeFont::System]
        .into_iter()
        .chain(
            installed
                .iter()
                .map(|name| CodeFont::Installed(name.to_string())),
        )
        .filter(|choice| settings::model::matches(query, &choice.label()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(font: CodeFont, bundled: bool) -> CodeFontState {
        let mut state = CodeFontState::new(bundled);
        state.settings.font = font;
        state
    }

    /// Spec 0025 AC-5: ⌘=, ⌘- and ⌘0 change nothing; ⌘P still lists the
    /// three size commands, unbound, on the actions `init` handles.
    #[test]
    fn ac5_size_keys_are_unbound_and_the_palette_keeps_the_commands() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let workspace = [KeyContext::parse("Workspace").expect("context")];
        for keys in ["secondary-=", "secondary--", "secondary-0"] {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &workspace);
            let action = bindings.first().map(|binding| binding.action().name());
            assert!(
                action.is_none_or(|name| !name.starts_with("code_font::")),
                "{keys} runs {action:?}"
            );
        }
        let commands = crate::palette_commands(false);
        for (title, action) in [
            ("Increase code font size", IncreaseCodeFontSize.name()),
            ("Decrease code font size", DecreaseCodeFontSize.name()),
            ("Reset code font size", ResetCodeFontSize.name()),
        ] {
            let command = commands
                .iter()
                .find(|command| command.title == title)
                .unwrap_or_else(|| panic!("⌘P lists {title}"));
            assert_eq!(command.action.name(), action, "{title}");
            assert_eq!(command.keys, None, "{title}");
        }
    }

    #[test]
    fn a_missing_family_falls_back_to_bundled_then_system_mono() {
        let installed = CodeFont::Installed("Fira Code".into());
        assert_eq!(
            rendered_family(&state(installed.clone(), true), None).as_ref(),
            "Fira Code"
        );
        assert_eq!(
            rendered_family(&state(installed.clone(), true), Some("Fira Code")).as_ref(),
            "JetBrains Mono"
        );
        assert_eq!(
            rendered_family(&state(installed, false), Some("Fira Code")).as_ref(),
            system_mono_family()
        );
        assert_eq!(
            rendered_family(&state(CodeFont::Bundled, false), None).as_ref(),
            system_mono_family(),
            "bundled faces that failed to register"
        );
        assert_eq!(
            rendered_family(&state(CodeFont::System, true), Some(system_mono_family())).as_ref(),
            "JetBrains Mono"
        );
    }

    #[test]
    fn choices_list_bundled_and_system_first_and_filter_by_label() {
        let installed: Vec<SharedString> = vec!["Fira Code".into(), "Menlo".into()];
        let all = choices(&installed, "");
        assert_eq!(all[0], CodeFont::Bundled);
        assert_eq!(all[1], CodeFont::System);
        assert_eq!(all.len(), 4);
        assert_eq!(
            choices(&installed, "fira"),
            [CodeFont::Installed("Fira Code".into())]
        );
        // The system entry matches by what people call it.
        assert_eq!(choices(&installed, "sf mono"), [CodeFont::System]);
        assert_eq!(choices(&installed, "bundled"), [CodeFont::Bundled]);
    }
}
