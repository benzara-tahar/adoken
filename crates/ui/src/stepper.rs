//! Number stepper: `−` value `+` for small integers (daily cap, thresholds,
//! hours). Focused (Tab), `←`/`↓` and `→`/`↑` step; `Home`/`End` jump to the
//! bounds. The buttons disable at the bounds.

use std::ops::RangeInclusive;
use std::rc::Rc;

use adoken_theme::{radius, size, space, typography};
use gpui::{App, ElementId, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

type ChangeHandler = Rc<dyn Fn(i64, &mut Window, &mut App)>;

const VALUE_WIDTH: f32 = 56.0;

/// `value + delta * step`, clamped into `range`.
pub fn stepped(value: i64, delta: i64, step: i64, range: &RangeInclusive<i64>) -> i64 {
    value
        .saturating_add(delta.saturating_mul(step.max(1)))
        .clamp(*range.start(), *range.end())
}

#[derive(IntoElement)]
pub struct Stepper {
    id: ElementId,
    value: i64,
    range: RangeInclusive<i64>,
    step: i64,
    suffix: Option<SharedString>,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Stepper {
    pub fn new(id: impl Into<ElementId>, value: i64, range: RangeInclusive<i64>) -> Self {
        Self {
            id: id.into(),
            value,
            range,
            step: 1,
            suffix: None,
            disabled: false,
            on_change: None,
        }
    }

    pub fn step(mut self, step: i64) -> Self {
        self.step = step;
        self
    }

    /// Unit shown after the value ("h", "days").
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(i64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Stepper {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let handler = self.on_change.filter(|_| !self.disabled);
        let (value, step, range) = (self.value, self.step, self.range);
        let focus_ring = colors.border_focus;
        let button = |id: &'static str, icon: IconName, delta: i64| {
            let target = stepped(value, delta, step, &range);
            let enabled = handler.is_some() && target != value;
            let handler = handler.clone();
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(size::CONTROL_MD - 2.0 * size::BORDER))
                .rounded(px(radius::XS))
                .child(Icon::new(icon).size(size::ICON_SM).color(if enabled {
                    colors.text_secondary
                } else {
                    colors.text_disabled
                }))
                .when_some(handler.filter(|_| enabled), |this, handler| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(colors.bg_hover))
                        .on_click(move |_, window, cx| handler(target, window, cx))
                })
        };
        let label = match &self.suffix {
            Some(suffix) => format!("{value} {suffix}"),
            None => value.to_string(),
        };
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .h(px(size::CONTROL_MD))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_control)
            .bg(colors.bg_input)
            .type_style(typography::BODY, theme)
            .child(button("minus", IconName::Minus, -1))
            .child(
                div()
                    .min_w(px(VALUE_WIDTH))
                    .px(px(space::S4))
                    .flex()
                    .justify_center()
                    .text_color(if self.disabled {
                        colors.text_disabled
                    } else {
                        colors.text_primary
                    })
                    .child(label),
            )
            .child(button("plus", IconName::Plus, 1))
            .when_some(handler, |this, handler| {
                let key_range = range.clone();
                this.tab_index(0)
                    .focus(move |style| style.border_color(focus_ring))
                    .on_key_down(move |event, window, cx| {
                        let next = match event.keystroke.key.as_str() {
                            "left" | "down" => stepped(value, -1, step, &key_range),
                            "right" | "up" => stepped(value, 1, step, &key_range),
                            "home" => *key_range.start(),
                            "end" => *key_range.end(),
                            _ => return,
                        };
                        cx.stop_propagation();
                        if next != value {
                            handler(next, window, cx);
                        }
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_clamp_to_the_range() {
        let range = 0..=10;
        assert_eq!(stepped(3, 1, 1, &range), 4);
        assert_eq!(stepped(10, 1, 1, &range), 10);
        assert_eq!(stepped(0, -1, 1, &range), 0);
        assert_eq!(stepped(4, 1, 4, &(0..=24)), 8);
        assert_eq!(stepped(22, 1, 4, &(0..=24)), 24);
        // A zero step still moves by one.
        assert_eq!(stepped(3, 1, 0, &range), 4);
        assert_eq!(stepped(i64::MAX, 1, 1, &(0..=i64::MAX)), i64::MAX);
    }
}
