//! Shell chrome stories: the sidebar expanded and collapsed to its rail
//! (on Search, whose content doesn't move with the demo feed's clock),
//! the loaders, and the shortcut cheat sheet (generated from the app's real
//! key bindings and palette commands).

use adoken_app::help::{self, CheatSheet};
use adoken_app::shell::Nav;
use adoken_theme::{radius, size, space, typography};
use adoken_ui::loaders::{
    fist_loader, gradient_spinner, matrix_tints, mini_glyph_spinner, mini_mono_spinner,
    progress_ring, splash_overlay,
};
use adoken_ui::{ActiveTheme, Button, ButtonVariant, Spinner, TypeStyleExt};
use gpui::{AnyElement, App, IntoElement, Window, div, prelude::*, px};

use crate::shell::shell_with;

pub fn sidebar_expanded_story(window: &mut Window, cx: &mut App) -> AnyElement {
    shell_with("shell-sidebar-expanded", Nav::Search, false, window, cx)
}

pub fn sidebar_collapsed_story(window: &mut Window, cx: &mut App) -> AnyElement {
    shell_with("shell-sidebar-collapsed", Nav::Search, true, window, cx)
}

fn caption(text: &'static str, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::LABEL, theme)
        .child(text)
}

fn cell(label: &'static str, loader: impl IntoElement, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(space::S8))
        .child(
            div()
                .h(px(64.0))
                .flex()
                .items_center()
                .justify_center()
                .child(loader),
        )
        .child(caption(label, cx))
}

pub fn loaders_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme().clone();
    let colors = &theme.colors;
    let row = || div().flex().items_end().gap(px(space::S24));
    div()
        .size_full()
        .p(px(space::S24))
        .flex()
        .flex_col()
        .gap(px(space::S24))
        .bg(colors.bg_surface)
        .text_color(colors.text_primary)
        .type_style(typography::BODY, &theme)
        .child(
            row()
                .child(cell("fist 20", fist_loader(20.0), cx))
                .child(cell("fist 40", fist_loader(40.0), cx))
                .child(cell("fist 60", fist_loader(60.0), cx))
                .child(cell("matrix 3", gradient_spinner(3.0), cx))
                .child(cell("matrix 6", gradient_spinner(6.0), cx))
                .child(cell(
                    "mini glyph",
                    mini_glyph_spinner(3.0, matrix_tints(&theme)),
                    cx,
                ))
                .child(cell(
                    "mini mono",
                    mini_mono_spinner(3.0, colors.text_tertiary),
                    cx,
                )),
        )
        .child(
            row()
                .child(cell("Spinner", Spinner::new(), cx))
                .child(cell("Spinner 20", Spinner::new().size(size::ICON_LG), cx))
                .child(cell("ring 0 %", progress_ring(0, 44.0), cx))
                .child(cell("ring 42 %", progress_ring(42, 44.0), cx))
                .child(cell("ring 100 %", progress_ring(100, 44.0), cx))
                .child(cell("ring 20 px", progress_ring(65, 20.0), cx))
                .child(cell(
                    "button",
                    Button::new("loaders-busy", "Saving")
                        .variant(ButtonVariant::Primary)
                        .loading(true),
                    cx,
                )),
        )
        .child(
            div()
                .relative()
                .h(px(180.0))
                .overflow_hidden()
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_subtle)
                .child(splash_overlay("Opening local data…")),
        )
        .into_any_element()
}

/// The sheet over the app's real bindings (bound here: stories and
/// snapshots run without `adoken_app::run`).
fn cheat_sheet(
    key: &'static str,
    query: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let bound = cx
        .key_bindings()
        .borrow()
        .bindings()
        .any(|binding| binding.action().name() == "help::ToggleShortcuts");
    if !bound {
        cx.bind_keys(adoken_app::key_bindings());
    }
    let sheet = window.use_keyed_state(key, cx, |window, cx| {
        let commands = help::command_infos(&adoken_app::shell::commands());
        let mut sheet = CheatSheet::new(commands, cx);
        sheet.set_inline(true, cx);
        sheet.open_with(query, window, cx);
        sheet
    });
    div().size_full().child(sheet).into_any_element()
}

pub fn cheat_sheet_story(window: &mut Window, cx: &mut App) -> AnyElement {
    cheat_sheet("cheat-sheet", "", window, cx)
}

pub fn cheat_sheet_search_story(window: &mut Window, cx: &mut App) -> AnyElement {
    cheat_sheet("cheat-sheet-search", "next", window, cx)
}
