//! Button: click or, when focused, Enter/Space. Optional shortcut hint.

use std::rc::Rc;

use adoken_theme::{Color, ColorTokens, radius, size, space, typography};
use gpui::{
    App, ElementId, Hsla, IntoElement, KeyDownEvent, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};

use crate::Kbd;
use crate::icon::{Icon, IconName};
use crate::status::Spinner;
use crate::theme::{ActiveTheme, TypeStyleExt, hsla};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// The one main action of a view: `accent.solid`.
    Primary,
    #[default]
    Secondary,
    /// Toolbar and inline actions: no plate until hovered.
    Ghost,
    // A destructive variant waits for a `status.danger.solid_hover` token:
    // washing `status.danger.solid` with `bg.active` drops label contrast
    // below 4.5:1 in Dojo Night.
}

impl ButtonVariant {
    pub const ALL: [Self; 3] = [Self::Primary, Self::Secondary, Self::Ghost];

    /// Resolved colors, all opaque except `background` for ghost buttons.
    /// `surface` is the opaque plane the button sits on; washes are flattened
    /// over it so hover never stacks translucency.
    pub fn palette(self, colors: &ColorTokens, surface: Color, disabled: bool) -> ButtonPalette {
        let transparent = Color::TRANSPARENT;
        let over = |wash: Color, base: Color| wash.blend_over(base);
        if disabled {
            let background = match self {
                Self::Ghost => transparent,
                _ => over(colors.bg_active, surface),
            };
            return ButtonPalette {
                background,
                background_hover: background,
                foreground: colors.text_disabled,
                border: over(colors.border_subtle, surface),
            };
        }
        match self {
            Self::Primary => ButtonPalette {
                background: colors.accent_solid,
                background_hover: colors.accent_solid_hover,
                foreground: colors.accent_on,
                border: colors.accent_solid,
            },
            Self::Secondary => {
                let plate = over(colors.bg_raised, surface);
                ButtonPalette {
                    background: plate,
                    background_hover: over(colors.bg_hover, plate),
                    foreground: colors.text_primary,
                    border: over(colors.border_default, surface),
                }
            }
            Self::Ghost => ButtonPalette {
                background: transparent,
                background_hover: over(colors.bg_hover, surface),
                foreground: colors.text_primary,
                border: transparent,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonPalette {
    pub background: Color,
    pub background_hover: Color,
    pub foreground: Color,
    pub border: Color,
}

/// Control height: `size.control.md` (default) or `size.control.lg` for the
/// primary action of a full-window flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    #[default]
    Md,
    Lg,
}

impl ButtonSize {
    fn height(self) -> f32 {
        match self {
            Self::Md => size::CONTROL_MD,
            Self::Lg => size::CONTROL_LG,
        }
    }
}

type PressHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    variant: ButtonVariant,
    size: ButtonSize,
    icon: Option<IconName>,
    disabled: bool,
    loading: bool,
    shortcut: Option<Kbd>,
    on_press: Option<PressHandler>,
    tooltip: Option<SharedString>,
    aria_label: Option<SharedString>,
    aria_pressed: Option<bool>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            icon: None,
            disabled: false,
            loading: false,
            shortcut: None,
            on_press: None,
            tooltip: None,
            aria_label: None,
            aria_pressed: None,
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Leading 14 px glyph.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Work in flight: a spinner replaces the icon and presses are ignored,
    /// but the button keeps its variant colors (it is busy, not unavailable).
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Show the binding that triggers the same action (keyboard-first UI).
    pub fn shortcut(mut self, shortcut: Kbd) -> Self {
        self.shortcut = Some(shortcut);
        self
    }

    pub fn on_press(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Rc::new(handler));
        self
    }

    /// Hover text; shown on disabled buttons too, so it can say why. Pair it
    /// with visible copy: disabled buttons aren't focusable.
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn aria_pressed(mut self, pressed: bool) -> Self {
        self.aria_pressed = Some(pressed);
        self
    }
}

fn is_activation_key(event: &KeyDownEvent) -> bool {
    let keystroke = &event.keystroke;
    !keystroke.modifiers.modified() && matches!(keystroke.key.as_str(), "enter" | "space")
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let palette = self
            .variant
            .palette(&theme.raw, theme.raw.bg_surface, self.disabled);
        let paint = |color: Color| -> Hsla { hsla(color) };
        let hover = paint(palette.background_hover);
        let focus_ring = theme.colors.border_focus;
        let foreground = paint(palette.foreground);
        let leading = if self.loading {
            Some(Spinner::new().color(foreground).into_any_element())
        } else {
            self.icon.map(|icon| {
                Icon::new(icon)
                    .size(size::ICON_SM)
                    .color(foreground)
                    .into_any_element()
            })
        };
        let interactive = !self.disabled && !self.loading;

        div()
            .id(self.id)
            .role(gpui::Role::Button)
            .aria_label(self.aria_label.unwrap_or_else(|| self.label.clone()))
            .when_some(self.aria_pressed, |this, pressed| {
                this.aria_toggled(if pressed {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
            })
            .flex()
            .flex_none()
            .items_center()
            .gap(px(space::S6))
            .h(px(self.size.height()))
            .px(px(space::S12))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(paint(palette.border))
            .bg(paint(palette.background))
            .text_color(paint(palette.foreground))
            .type_style(typography::BODY_STRONG, theme)
            .children(leading)
            .child(self.label)
            .children(self.shortcut)
            .when_some(self.tooltip, |this, text| {
                this.tooltip(crate::tooltip::Tooltip::text(text))
            })
            .when(interactive, |this| {
                this.tab_index(0)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover))
                    .focus(move |style| style.border_color(focus_ring))
                    .when_some(self.on_press, |this, handler| {
                        let on_key = handler.clone();
                        this.on_click(move |_, window, cx| handler(window, cx))
                            .on_key_down(move |event, window, cx| {
                                if is_activation_key(event) {
                                    cx.stop_propagation();
                                    on_key(window, cx);
                                }
                            })
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::{Appearance, ThemeRegistry};

    #[test]
    fn every_variant_keeps_label_contrast_in_every_builtin_theme() {
        for appearance in Appearance::ALL {
            let colors = &ThemeRegistry::builtin().default_variant(appearance).colors;
            for surface in [colors.bg_surface, colors.bg_raised, colors.bg_overlay] {
                for variant in ButtonVariant::ALL {
                    let palette = variant.palette(colors, surface, false);
                    for background in [palette.background, palette.background_hover] {
                        let background = background.blend_over(surface);
                        let ratio = palette.foreground.contrast(background);
                        assert!(ratio >= 4.5, "{appearance:?} {variant:?}: {ratio:.2}");
                    }
                }
            }
        }
    }

    #[test]
    fn hover_is_distinguishable_and_disabled_has_no_hover() {
        let colors = &ThemeRegistry::builtin()
            .default_variant(Appearance::Dark)
            .colors;
        for variant in ButtonVariant::ALL {
            let enabled = variant.palette(colors, colors.bg_surface, false);
            assert_ne!(enabled.background, enabled.background_hover, "{variant:?}");
            let disabled = variant.palette(colors, colors.bg_surface, true);
            assert_eq!(
                disabled.background, disabled.background_hover,
                "{variant:?}"
            );
            assert_eq!(disabled.foreground, colors.text_disabled);
        }
    }

    #[test]
    fn only_unmodified_enter_and_space_activate() {
        let key = |source: &str| KeyDownEvent {
            keystroke: gpui::Keystroke::parse(source).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        assert!(is_activation_key(&key("enter")));
        assert!(is_activation_key(&key("space")));
        assert!(!is_activation_key(&key("cmd-enter")));
        assert!(!is_activation_key(&key("a")));
    }
}
