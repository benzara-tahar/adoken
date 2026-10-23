//! Small label chips ("Recommended", "Read & write", "Write untested") and
//! the checkbox glyph used by multi-select rows.

use adoken_theme::{ColorTokens, radius, size, space, typography};
use gpui::{App, Hsla, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipTone {
    /// `text.secondary` on a `bg.active` wash.
    #[default]
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

impl ChipTone {
    pub const ALL: [Self; 5] = [
        Self::Neutral,
        Self::Accent,
        Self::Success,
        Self::Warning,
        Self::Danger,
    ];

    /// `(foreground, wash)` tokens.
    pub fn colors<C: Copy>(self, colors: &ColorTokens<C>) -> (C, C) {
        match self {
            Self::Neutral => (colors.text_secondary, colors.bg_active),
            Self::Accent => (colors.accent_fg, colors.accent_wash),
            Self::Success => (colors.status_success_fg, colors.status_success_wash),
            Self::Warning => (colors.status_warning_fg, colors.status_warning_wash),
            Self::Danger => (colors.status_danger_fg, colors.status_danger_wash),
        }
    }
}

#[derive(IntoElement)]
pub struct Chip {
    label: SharedString,
    tone: ChipTone,
}

impl Chip {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: ChipTone::default(),
        }
    }

    pub fn tone(mut self, tone: ChipTone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Chip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, wash): (Hsla, Hsla) = self.tone.colors(&theme.colors);
        div()
            .flex_none()
            .px(px(space::S6))
            .py(px(1.0))
            .rounded(px(radius::XS))
            .bg(wash)
            .text_color(fg)
            .type_style(typography::LABEL, theme)
            .child(self.label)
    }
}

/// Checkbox glyph (`square` / `square-check`); the row owns the click and
/// the `Space` binding.
#[derive(IntoElement)]
pub struct Checkbox {
    checked: bool,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self { checked }
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (name, color) = if self.checked {
            (IconName::SquareCheck, colors.accent_solid)
        } else {
            (IconName::Square, colors.text_tertiary)
        };
        Icon::new(name).size(size::ICON_MD).color(color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::ThemeRegistry;

    #[test]
    fn chip_labels_are_readable_on_the_overlay_card_in_every_theme() {
        for variant in ThemeRegistry::builtin().variants() {
            let colors = &variant.colors;
            for tone in ChipTone::ALL {
                let (fg, wash) = tone.colors(colors);
                let background = wash.blend_over(colors.bg_overlay);
                let ratio = fg.contrast(background);
                // type.label is 11 px semibold: hold it to body-text AA.
                assert!(ratio >= 4.5, "{} {tone:?}: {ratio:.2}", variant.id);
            }
        }
    }
}
