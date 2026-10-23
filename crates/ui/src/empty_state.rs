//! Pane empty state (principles.md §3.1): centered, max width 360, a 20 px
//! icon in `text.tertiary`, a one-line headline, one line of body, and at
//! most one action.

use adoken_theme::{size, space, typography};
use gpui::{AnyElement, App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

const MAX_WIDTH: f32 = 360.0;

#[derive(IntoElement)]
pub struct EmptyState {
    icon: IconName,
    headline: SharedString,
    body: Option<SharedString>,
    action: Option<AnyElement>,
}

impl EmptyState {
    pub fn new(icon: IconName, headline: impl Into<SharedString>) -> Self {
        Self {
            icon,
            headline: headline.into(),
            body: None,
            action: None,
        }
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// The single primary action (a `Button` with its shortcut keycap).
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .p(px(space::S24))
            .child(
                div()
                    .max_w(px(MAX_WIDTH))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Icon::new(self.icon)
                            .size(size::ICON_LG)
                            .color(colors.text_tertiary),
                    )
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::HEADING, theme)
                            .text_center()
                            .child(self.headline),
                    )
                    .children(self.body.map(|body| {
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::BODY, theme)
                            .text_center()
                            .child(body)
                    }))
                    .children(
                        self.action
                            .map(|action| div().pt(px(space::S8)).child(action)),
                    ),
            )
    }
}
