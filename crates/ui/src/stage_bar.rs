//! The segmented stage bar of a pull request row (spec 0017 FR-16,
//! `docs/design/today.md` §4.2): four equal segments, Draft · Review ·
//! Approved · Merged, filled up to the PR's stage in its state's tone,
//! empty ones in `border.strong`. No dots, circles or hairlines. Decorative:
//! the row's label and the state glyph beside it carry the state.

use adoken_theme::{radius, space};
use gpui::{App, Hsla, IntoElement, RenderOnce, Window, div, prelude::*, px};

use crate::theme::ActiveTheme;

#[derive(IntoElement)]
pub struct StageBar {
    filled: u8,
    tone: Hsla,
}

impl StageBar {
    pub const SEGMENTS: usize = 4;
    pub const SEGMENT_WIDTH: f32 = 44.0;
    pub const SEGMENT_HEIGHT: f32 = 6.0;
    /// 4 × 44 + 3 gaps of `space.4`.
    pub const WIDTH: f32 =
        Self::SEGMENTS as f32 * Self::SEGMENT_WIDTH + (Self::SEGMENTS as f32 - 1.0) * space::S4;

    /// `filled` of four segments (clamped) in `tone`.
    pub fn new(filled: u8, tone: Hsla) -> Self {
        Self { filled, tone }
    }

    /// Which segments are filled, left to right.
    pub fn fills(&self) -> [bool; Self::SEGMENTS] {
        std::array::from_fn(|i| i < usize::from(self.filled))
    }
}

impl RenderOnce for StageBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let empty = cx.theme().colors.border_strong;
        let tone = self.tone;
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .children(self.fills().map(|filled| {
                div()
                    .w(px(Self::SEGMENT_WIDTH))
                    .h(px(Self::SEGMENT_HEIGHT))
                    .rounded(px(radius::FULL))
                    .bg(if filled { tone } else { empty })
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_from_the_left_and_clamps_at_four() {
        let tone = Hsla::default();
        assert_eq!(StageBar::new(0, tone).fills(), [false; 4]);
        assert_eq!(StageBar::new(2, tone).fills(), [true, true, false, false]);
        assert_eq!(StageBar::new(9, tone).fills(), [true; 4]);
        assert_eq!(StageBar::WIDTH, 188.0, "design §4.2");
    }
}
