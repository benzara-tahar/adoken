//! Tooltip: one line of text on `bg.overlay`, `elevation.2`. gpui owns the
//! 500 ms show delay (tokens.md §7.1). Anything a tooltip says must also be
//! visible without hovering somewhere nearby (principles §1.2).

use adoken_theme::{elevation, radius, space, typography};
use gpui::{AnyView, App, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px};

use crate::theme::{ActiveTheme, TypeStyleExt};

pub struct Tooltip {
    text: SharedString,
}

impl Tooltip {
    /// A builder for `.tooltip(…)` on any stateful element.
    pub fn text(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
        let text = text.into();
        move |_, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px(px(space::S8))
            .py(px(space::S4))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(theme.colors.border_default)
            .bg(theme.colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_2))
            .text_color(theme.colors.text_primary)
            .type_style(typography::SMALL, theme)
            .child(self.text.clone())
    }
}
