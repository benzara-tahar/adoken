//! Theme picker stories (spec 0007 §3.1) and the theme strip: a miniature
//! of every built-in variant, so the ported community themes get visual
//! coverage while each story is snapshotted in the house themes only.

use std::path::PathBuf;

use adoken_app::theme_picker::library::{UserFile, install_for_story};
use adoken_app::theme_picker::{ThemePicker, theme_strip};
use adoken_theme::{AppearanceSettings, ThemeRegistry};
use gpui::{AnyElement, App, Context, Window, div, prelude::*};

fn picker(
    key: &'static str,
    settings: AppearanceSettings,
    custom: bool,
    setup: impl FnOnce(&mut ThemePicker, &mut Context<ThemePicker>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let (user, files) = if custom {
            custom_theme()
        } else {
            (Vec::new(), Vec::new())
        };
        install_for_story(settings, user, files, cx);
        let mut picker = ThemePicker::new(window, cx);
        setup(&mut picker, cx);
        picker
    });
    div().size_full().p_4().child(view).into_any_element()
}

/// A user theme (a copy of a built-in under a new id) whose file has a
/// contrast warning, to show the "Custom" chip and the issues list.
fn custom_theme() -> (Vec<adoken_theme::ThemeFamily>, Vec<UserFile>) {
    let mut family = ThemeRegistry::builtin()
        .families()
        .iter()
        .find(|family| family.id == "nord")
        .expect("nord is built in")
        .clone();
    family.id = "fjord".into();
    family.name = "Fjord".into();
    family.author = Some("You".into());
    for variant in &mut family.variants {
        variant.id = "fjord".into();
        variant.name = "Fjord".into();
        variant.family_id = "fjord".into();
    }
    let file = UserFile {
        path: PathBuf::from("fjord.adoken-theme.toml"),
        family_id: Some("fjord".into()),
        issues: vec![
            "fjord: low contrast: text.tertiary on bg.hover is 4.21:1, needs 4.5:1".into(),
        ],
        error: None,
    };
    (vec![family], vec![file])
}

/// Default: Dojo Night applied, every theme in the grid.
pub fn picker_story(window: &mut Window, cx: &mut App) -> AnyElement {
    picker(
        "theme-picker",
        AppearanceSettings::default(),
        false,
        |_, _| {},
        window,
        cx,
    )
}

/// Tokyo Night applied, "or dark" searched and the keyboard highlight on a
/// card, plus a user theme with a reported issue.
pub fn picker_search_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let settings = AppearanceSettings {
        theme: "tokyo-night".into(),
    };
    picker(
        "theme-picker-search",
        settings,
        true,
        |picker, cx| {
            picker.set_query("or dark", cx);
            picker.highlight(1, cx);
        },
        window,
        cx,
    )
}

/// Every built-in variant painted in its own tokens.
pub fn strip_story(_: &mut Window, cx: &mut App) -> AnyElement {
    div().size_full().child(theme_strip(cx)).into_any_element()
}
