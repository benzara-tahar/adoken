//! Card: a raised plate (`bg.raised`, `border.default`, elevation 1).

use adoken_theme::{elevation, radius, space, typography};
use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Window, div, prelude::*,
    px,
};

use crate::theme::{ActiveTheme, TypeStyleExt};

#[derive(IntoElement)]
pub struct Card {
    title: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl Card {
    pub fn new() -> Self {
        Self {
            title: None,
            children: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .gap(px(space::S12))
            .p(px(space::S16))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(theme.colors.border_default)
            .bg(theme.colors.bg_raised)
            .shadow(theme.shadows(elevation::LEVEL_1))
            .text_color(theme.colors.text_primary)
            .type_style(typography::BODY, theme)
            .when_some(self.title, |this, title| {
                this.child(div().type_style(typography::HEADING, theme).child(title))
            })
            .children(self.children)
    }
}
