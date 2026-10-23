//! Progress and outcome glyphs: [`Spinner`] and [`StatusIcon`]
//! (docs/design/onboarding.md §6 "Spinner → check", tokens.md §7.3).

use adoken_theme::{motion as durations, size};
use gpui::{
    AnimationExt, App, ElementId, Hsla, IntoElement, RenderOnce, Rgba, Transformation, Window,
};

use crate::icon::{Icon, IconName};
use crate::loaders::{MATRIX_SIDE, gradient_spinner};
use crate::motion::{SPRING_SNAPPY, lerp};
use crate::theme::ActiveTheme;

/// Cells across the matrix, gaps included (3 cells + 2 half-cell gaps).
const MATRIX_SPAN: f32 = MATRIX_SIDE as f32 + (MATRIX_SIDE as f32 - 1.0) / 2.0;

/// The app's busy glyph: the [`crate::loaders`] matrix spinner in a
/// `size` square, on the shared 30 fps clock. Theme "sunrise" tints by
/// default; [`color`](Self::color) makes it one color (button labels).
/// Under reduce motion it holds one still frame; pair it with text
/// ("Looking for az…").
#[derive(IntoElement)]
pub struct Spinner {
    size: f32,
    color: Option<Hsla>,
}

impl Spinner {
    /// 14 px (`size.icon.sm`).
    pub fn new() -> Self {
        Self {
            size: size::ICON_SM,
            color: None,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let spinner = gradient_spinner(self.size / MATRIX_SPAN);
        match self.color {
            Some(color) => spinner.mono(color),
            None => spinner,
        }
    }
}

/// What a checklist row or detection card is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Not started: `circle-dashed`, `text.tertiary`.
    Pending,
    /// In flight: spinner.
    Running,
    /// Succeeded: `circle-check` with the check morph, `status.success.fg`.
    Done,
    /// Needs attention (signed out, too old, didn't respond): `circle-alert`.
    Attention,
    /// Optional thing missing: `circle-x`, `status.warning.fg`.
    Missing,
    /// Required thing missing or failed: `circle-x`, `status.danger.fg`.
    Failed,
    /// Not available at all (az not found): `circle-x`, `text.tertiary`.
    Unavailable,
}

impl Status {
    pub const ALL: [Self; 7] = [
        Self::Pending,
        Self::Running,
        Self::Done,
        Self::Attention,
        Self::Missing,
        Self::Failed,
        Self::Unavailable,
    ];

    pub fn icon(self) -> IconName {
        match self {
            Self::Pending => IconName::CircleDashed,
            Self::Running => IconName::LoaderCircle,
            Self::Done => IconName::CircleCheck,
            Self::Attention => IconName::CircleAlert,
            Self::Missing | Self::Failed | Self::Unavailable => IconName::CircleX,
        }
    }
}

/// Status glyph. `Done` plays the check morph the first time an element with
/// this id renders as done: scale 0.6→1 on `spring.snappy`, color
/// `text.tertiary`→`status.success.fg` over `motion.quick` (140 ms).
#[derive(IntoElement)]
pub struct StatusIcon {
    id: ElementId,
    status: Status,
    size: f32,
}

impl StatusIcon {
    pub fn new(id: impl Into<ElementId>, status: Status) -> Self {
        Self {
            id: id.into(),
            status,
            size: size::ICON_MD,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

/// Straight-alpha RGBA interpolation (HSL hue would take the long way round).
pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(from), Rgba::from(to));
    Rgba {
        r: lerp(a.r, b.r, t),
        g: lerp(a.g, b.g, t),
        b: lerp(a.b, b.b, t),
        a: lerp(a.a, b.a, t),
    }
    .into()
}

impl RenderOnce for StatusIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let color = match self.status {
            Status::Pending | Status::Running | Status::Unavailable => colors.text_tertiary,
            Status::Done => colors.status_success_fg,
            Status::Attention | Status::Missing => colors.status_warning_fg,
            Status::Failed => colors.status_danger_fg,
        };
        match self.status {
            Status::Running => Spinner::new().size(self.size).into_any_element(),
            Status::Done => {
                let (from, to) = (colors.text_tertiary, color);
                let spring_ms = SPRING_SNAPPY.duration().as_millis() as f32;
                let color_share = durations::QUICK as f32 / spring_ms;
                Icon::new(IconName::CircleCheck)
                    .size(self.size)
                    .color(to)
                    .with_animation(self.id, SPRING_SNAPPY.animation(), move |icon, t| {
                        let scale = lerp(0.6, 1.0, SPRING_SNAPPY.at(t));
                        icon.color(mix(from, to, (t / color_share).min(1.0)))
                            .transform(Transformation::scale(gpui::size(scale, scale)))
                    })
                    .into_any_element()
            }
            status => Icon::new(status.icon())
                .size(self.size)
                .color(color)
                .into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_hits_endpoints_and_midpoint() {
        let a: Hsla = Rgba {
            r: 0.0,
            g: 0.2,
            b: 1.0,
            a: 1.0,
        }
        .into();
        let b: Hsla = Rgba {
            r: 1.0,
            g: 0.6,
            b: 0.0,
            a: 0.5,
        }
        .into();
        let at = |t| Rgba::from(mix(a, b, t));
        let close = |x: f32, y: f32| (x - y).abs() < 1e-3;
        assert!(close(at(0.0).r, 0.0) && close(at(0.0).a, 1.0));
        assert!(close(at(1.0).r, 1.0) && close(at(1.0).a, 0.5));
        let mid = at(0.5);
        assert!(close(mid.r, 0.5) && close(mid.g, 0.4) && close(mid.b, 0.5));
    }
}
