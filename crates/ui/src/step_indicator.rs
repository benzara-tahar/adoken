//! Step dots for multi-step flows (docs/design/onboarding.md §2): done
//! dots 6 × 6 `text.secondary`, upcoming 6 × 6 `text.disabled`, the active
//! step an 18 × 6 `accent.solid` pill. On a step change the new pill grows
//! 6→18 and the previous one shrinks 18→6 on `spring.snappy`.

use adoken_theme::{radius, space};
use gpui::{AnimationExt, App, ElementId, IntoElement, RenderOnce, Window, div, prelude::*, px};

use crate::motion::{SPRING_SNAPPY, lerp};
use crate::theme::ActiveTheme;

const DOT: f32 = 6.0;
const PILL: f32 = 18.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotState {
    Done,
    Active,
    Upcoming,
}

/// State of dot `ix` when `active` is the current step.
pub fn dot_state(ix: usize, active: usize) -> DotState {
    match ix.cmp(&active) {
        std::cmp::Ordering::Less => DotState::Done,
        std::cmp::Ordering::Equal => DotState::Active,
        std::cmp::Ordering::Greater => DotState::Upcoming,
    }
}

#[derive(IntoElement)]
pub struct StepIndicator {
    id: ElementId,
    count: usize,
    active: usize,
    previous: Option<usize>,
}

impl StepIndicator {
    pub fn new(id: impl Into<ElementId>, count: usize, active: usize) -> Self {
        Self {
            id: id.into(),
            count,
            active,
            previous: None,
        }
    }

    /// The step shown before this one, so its pill animates closed.
    pub fn previous(mut self, previous: Option<usize>) -> Self {
        self.previous = previous.filter(|p| *p != self.active);
        self
    }
}

impl RenderOnce for StepIndicator {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let animate_from = self.previous;
        div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .gap(px(space::S6))
            .children((0..self.count).map(|ix| {
                let state = dot_state(ix, self.active);
                let color = match state {
                    DotState::Done => colors.text_secondary,
                    DotState::Active => colors.accent_solid,
                    DotState::Upcoming => colors.text_disabled,
                };
                let dot = div()
                    .flex_none()
                    .h(px(DOT))
                    .rounded(px(radius::FULL))
                    .bg(color);
                let key = (self.active, animate_from.unwrap_or(usize::MAX));
                let growing = state == DotState::Active && animate_from.is_some();
                let shrinking = animate_from == Some(ix);
                if growing || shrinking {
                    let (from, to) = if growing { (DOT, PILL) } else { (PILL, DOT) };
                    dot.with_animation(
                        ElementId::NamedInteger(
                            format!("{:?}-dot-{}-{}", self.id, key.0, key.1).into(),
                            ix as u64,
                        ),
                        SPRING_SNAPPY.animation(),
                        move |dot, t| dot.w(px(lerp(from, to, SPRING_SNAPPY.at(t)))),
                    )
                    .into_any_element()
                } else {
                    let width = if state == DotState::Active { PILL } else { DOT };
                    dot.w(px(width)).into_any_element()
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_before_active_are_done_and_after_are_upcoming() {
        let states: Vec<_> = (0..4).map(|ix| dot_state(ix, 2)).collect();
        assert_eq!(
            states,
            [
                DotState::Done,
                DotState::Done,
                DotState::Active,
                DotState::Upcoming
            ]
        );
    }
}
