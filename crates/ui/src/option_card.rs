//! Selectable option card (docs/design/onboarding.md §3 screen 1): a radio
//! choice with a title, one-line description, optional badge, a status line
//! under the description, and a trailing shortcut keycap. A disabled card
//! stays visible (its reason is the status line) but cannot be chosen.
//! The status sits under the text rather than beside it so a full
//! "az 2.83.0 · signed in as dana@contoso.com" fits in a 480 px card.

use std::rc::Rc;

use adoken_theme::{Color, ColorTokens, elevation, radius, space, typography};
use gpui::{
    AnyElement, App, ElementId, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px,
};

use crate::theme::{ActiveTheme, TypeStyleExt, hsla};

/// Minimum card height (design §3 screen 1: 480 × 88).
pub const OPTION_CARD_HEIGHT: f32 = 88.0;

type ClickHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// Border and fill for a card in each state, over the opaque `surface`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionCardPalette {
    pub background: Color,
    pub border: Color,
    pub title: Color,
}

pub fn option_card_palette(
    colors: &ColorTokens,
    surface: Color,
    selected: bool,
    disabled: bool,
) -> OptionCardPalette {
    let plate = colors.bg_raised.blend_over(surface);
    OptionCardPalette {
        background: if selected {
            colors.accent_wash.blend_over(plate)
        } else {
            plate
        },
        border: if selected {
            colors.accent_solid
        } else {
            colors.border_default.blend_over(surface)
        },
        title: if disabled {
            colors.text_secondary
        } else {
            colors.text_primary
        },
    }
}

#[derive(IntoElement)]
pub struct OptionCard {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    badge: Option<AnyElement>,
    status: Option<AnyElement>,
    shortcut: Option<AnyElement>,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl OptionCard {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            badge: None,
            status: None,
            shortcut: None,
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Shown next to the title (e.g. a "Recommended" chip).
    pub fn badge(mut self, badge: impl IntoElement) -> Self {
        self.badge = Some(badge.into_any_element());
        self
    }

    /// Status line under the description (glyph + text, links).
    pub fn status(mut self, status: impl IntoElement) -> Self {
        self.status = Some(status.into_any_element());
        self
    }

    /// Keycap that picks this card (e.g. `1`).
    pub fn shortcut(mut self, shortcut: impl IntoElement) -> Self {
        self.shortcut = Some(shortcut.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for OptionCard {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let palette = option_card_palette(
            &theme.raw,
            theme.raw.bg_overlay,
            self.selected,
            self.disabled,
        );
        let hover = hsla(theme.raw.bg_hover.blend_over(palette.background));

        let radio = div()
            .flex_none()
            .size(px(14.0))
            .rounded(px(radius::FULL))
            .border_1()
            .border_color(if self.selected {
                colors.accent_solid
            } else {
                colors.border_strong
            })
            .flex()
            .items_center()
            .justify_center()
            .when(self.selected, |radio| {
                radio.child(
                    div()
                        .size(px(space::S6))
                        .rounded(px(radius::FULL))
                        .bg(colors.accent_solid),
                )
            });

        let text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        div()
                            .text_color(hsla(palette.title))
                            .type_style(typography::BODY_STRONG, theme)
                            .child(self.title),
                    )
                    .children(self.badge),
            )
            .when_some(self.description, |text, description| {
                text.child(
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(description),
                )
            })
            .when_some(self.status, |text, status| {
                text.child(
                    div()
                        .pt(px(space::S4))
                        .flex()
                        .items_center()
                        .gap(px(space::S6))
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(status),
                )
            });

        div()
            .id(self.id)
            .w_full()
            .min_h(px(OPTION_CARD_HEIGHT))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .px(px(space::S16))
            .py(px(space::S12))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(hsla(palette.border))
            .bg(hsla(palette.background))
            .shadow(theme.shadows(elevation::LEVEL_1))
            .child(radio)
            .child(text)
            .children(self.shortcut)
            .when_some(self.on_click, |card, handler| {
                card.cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .on_click(move |_, window, cx| handler(window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::ThemeRegistry;

    #[test]
    fn titles_stay_readable_and_selection_is_visible_in_every_theme() {
        for variant in ThemeRegistry::builtin().variants() {
            let colors = &variant.colors;
            let surface = colors.bg_overlay;
            for disabled in [false, true] {
                for selected in [false, true] {
                    let p = option_card_palette(colors, surface, selected, disabled);
                    let ratio = p.title.contrast(p.background);
                    assert!(
                        ratio >= 4.5,
                        "{} {selected} {disabled}: {ratio:.2}",
                        variant.id
                    );
                }
            }
            let rest = option_card_palette(colors, surface, false, false);
            let chosen = option_card_palette(colors, surface, true, false);
            assert_ne!(rest.border, chosen.border, "{}", variant.id);
        }
    }
}
