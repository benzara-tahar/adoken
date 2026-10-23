//! Inline notices (errors with retry, policy messages, offline) and
//! skeleton blocks for loading lists.

use std::time::Duration;

use adoken_theme::{radius, size, space, typography};
use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};

use crate::chip::ChipTone;
use crate::icon::{Icon, IconName};
use crate::motion::{lerp, pulse_phase};
use crate::theme::{ActiveTheme, TypeStyleExt};

/// A tinted message row: glyph, text, then action children (buttons).
#[derive(IntoElement)]
pub struct Notice {
    tone: ChipTone,
    icon: Option<IconName>,
    message: SharedString,
    detail: Option<SharedString>,
    actions: Vec<AnyElement>,
}

impl Notice {
    pub fn new(tone: ChipTone, message: impl Into<SharedString>) -> Self {
        Self {
            tone,
            icon: None,
            message: message.into(),
            detail: None,
            actions: Vec::new(),
        }
    }

    /// Defaults per tone: danger/warning `circle-alert`, success
    /// `circle-check`, otherwise none.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Second line in `text.secondary` (e.g. a verbatim error, a hint).
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        let detail = detail.into();
        self.detail = (!detail.is_empty()).then_some(detail);
        self
    }
}

impl ParentElement for Notice {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.actions.extend(elements);
    }
}

impl RenderOnce for Notice {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let (fg, wash) = self.tone.colors(&theme.colors);
        let icon = self.icon.or(match self.tone {
            ChipTone::Danger | ChipTone::Warning => Some(IconName::CircleAlert),
            ChipTone::Success => Some(IconName::CircleCheck),
            ChipTone::Neutral | ChipTone::Accent => None,
        });
        div()
            .w_full()
            .flex()
            .items_start()
            .gap(px(space::S8))
            .px(px(space::S12))
            .py(px(space::S8))
            .rounded(px(radius::SM))
            .bg(wash)
            .children(icon.map(|icon| {
                div()
                    .flex_none()
                    .pt(px(space::S2))
                    .child(Icon::new(icon).size(size::ICON_MD).color(fg))
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY, theme)
                            .child(self.message),
                    )
                    .when_some(self.detail, |text, detail| {
                        text.child(
                            div()
                                .text_color(colors.text_secondary)
                                .type_style(typography::SMALL, theme)
                                .child(detail),
                        )
                    }),
            )
            .when(!self.actions.is_empty(), |row| {
                row.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(space::S6))
                        .children(self.actions),
                )
            })
    }
}

/// Shimmer period (tokens.md §7.3 `skeleton-shimmer`, 1400 ms loop).
const SHIMMER_PERIOD: Duration = Duration::from_millis(1400);

/// A placeholder block for content that is loading. Pulses on the shared
/// 30 fps clock; flat under reduce motion.
#[derive(IntoElement)]
pub struct Skeleton {
    id: ElementId,
    width: Option<f32>,
    height: f32,
}

impl Skeleton {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            width: None,
            height: typography::BODY.line_height,
        }
    }

    /// Fixed width; fills the row when unset.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

/// Opacity of the shimmer at `phase ∈ [0, 1)`: a smooth 0.55 → 1 → 0.55.
pub fn shimmer_opacity(phase: f32) -> f32 {
    lerp(0.55, 1.0, crate::loaders::pulse_wave(phase))
}

impl RenderOnce for Skeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let phase = pulse_phase(SHIMMER_PERIOD, window.current_view(), cx);
        let opacity = if cx.reduce_motion() {
            1.0
        } else {
            shimmer_opacity(phase)
        };
        let colors = &cx.theme().colors;
        div()
            .id(self.id)
            .h(px(self.height))
            .map(|block| match self.width {
                Some(width) => block.w(px(width)),
                None => block.w_full(),
            })
            .rounded(px(radius::XS))
            .bg(colors.bg_active)
            .opacity(opacity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_stays_in_range_and_loops_seamlessly() {
        for i in 0..=100 {
            let o = shimmer_opacity(i as f32 / 100.0);
            assert!((0.55..=1.0).contains(&o), "{o}");
        }
        assert!((shimmer_opacity(0.0) - shimmer_opacity(0.999_9)).abs() < 1e-3);
        assert!((shimmer_opacity(0.5) - 1.0).abs() < 1e-6);
    }
}
