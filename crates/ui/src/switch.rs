//! On/off switch for settings rows. Click it, or focus it (Tab) and press
//! Space or Enter. The knob slides on `spring.snappy` (tokens.md §7.3
//! "toggle knob"); reduce motion snaps it.

use std::rc::Rc;

use adoken_theme::{radius, size, space};
use gpui::{
    AnimationExt, App, ElementId, IntoElement, KeyDownEvent, RenderOnce, Window, div, prelude::*,
    px,
};

use crate::motion::{SPRING_SNAPPY, lerp};
use crate::theme::ActiveTheme;

type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

pub const SWITCH_WIDTH: f32 = 32.0;
pub const SWITCH_HEIGHT: f32 = 18.0;
const KNOB: f32 = SWITCH_HEIGHT - 2.0 * (space::S2 + size::BORDER);

/// Knob offset from the track's left inner edge.
pub fn knob_offset(on: bool) -> f32 {
    if on {
        SWITCH_WIDTH - 2.0 * (space::S2 + size::BORDER) - KNOB
    } else {
        0.0
    }
}

#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    disabled: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with the new state.
    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

fn is_activation_key(event: &KeyDownEvent) -> bool {
    let keystroke = &event.keystroke;
    !keystroke.modifiers.modified() && matches!(keystroke.key.as_str(), "enter" | "space")
}

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on = self.on;
        // Animate only a change, never the first paint.
        let last = window.use_keyed_state((self.id.clone(), "last"), cx, |_, _| on);
        let changed = *last.read(cx) != on;
        if changed {
            last.update(cx, |last, _| *last = on);
        }
        let theme = cx.theme();
        let colors = &theme.colors;
        let (track, border, knob) = match (on, self.disabled) {
            (true, false) => (colors.accent_solid, colors.accent_solid, colors.accent_on),
            (true, true) => (
                colors.bg_active,
                colors.border_default,
                colors.text_disabled,
            ),
            (false, false) => (
                colors.bg_input,
                colors.border_control,
                colors.text_secondary,
            ),
            (false, true) => (colors.bg_input, colors.border_subtle, colors.text_disabled),
        };
        let focus_ring = colors.border_focus;
        let knob = div()
            .absolute()
            .top_0()
            .size(px(KNOB))
            .rounded(px(radius::FULL))
            .bg(knob);
        let knob = if changed {
            knob.with_animation(
                ("switch-knob", usize::from(on)),
                SPRING_SNAPPY.animation(),
                move |knob, t| {
                    let t = SPRING_SNAPPY.at(t);
                    knob.left(px(lerp(knob_offset(!on), knob_offset(on), t)))
                },
            )
            .into_any_element()
        } else {
            knob.left(px(knob_offset(on))).into_any_element()
        };
        let interactive = !self.disabled && self.on_toggle.is_some();
        div()
            .id(self.id)
            .flex_none()
            .w(px(SWITCH_WIDTH))
            .h(px(SWITCH_HEIGHT))
            .p(px(space::S2))
            .rounded(px(radius::FULL))
            .border_1()
            .border_color(border)
            .bg(track)
            .child(div().relative().size_full().child(knob))
            .when(interactive, |this| {
                let handler = self.on_toggle.expect("interactive has a handler");
                let on_key = handler.clone();
                this.tab_index(0)
                    .cursor_pointer()
                    .focus(move |style| style.border_color(focus_ring))
                    .on_click(move |_, window, cx| handler(!on, window, cx))
                    .on_key_down(move |event, window, cx| {
                        if is_activation_key(event) {
                            cx.stop_propagation();
                            on_key(!on, window, cx);
                        }
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knob_travels_the_inner_track_exactly() {
        assert_eq!(knob_offset(false), 0.0);
        let inner = SWITCH_WIDTH - 2.0 * (space::S2 + size::BORDER);
        assert_eq!(knob_offset(true) + KNOB, inner);
    }
}
