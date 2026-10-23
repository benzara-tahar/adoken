//! Horizontal slider for a bounded number (dim, blur, volume). Click or drag
//! the track; focused (Tab), `←`/`↓` and `→`/`↑` move one step, `⇧` ten
//! steps, `Home`/`End` jump to the ends. Values snap to `step`.

use std::cell::Cell;
use std::ops::RangeInclusive;
use std::rc::Rc;

use adoken_theme::{radius, size};
use gpui::{
    App, Bounds, DispatchPhase, ElementId, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent,
    Pixels, RenderOnce, Window, canvas, div, prelude::*, px,
};

use crate::theme::ActiveTheme;

type ChangeHandler = Rc<dyn Fn(f32, &mut Window, &mut App)>;

const TRACK_HEIGHT: f32 = 4.0;
const THUMB: f32 = 14.0;
pub const SLIDER_WIDTH: f32 = 200.0;

/// Snap `value` to the step grid and clamp it into `range`.
pub fn snap(value: f32, range: &RangeInclusive<f32>, step: f32) -> f32 {
    let (min, max) = (*range.start(), *range.end());
    let snapped = if step > 0.0 {
        min + ((value - min) / step).round() * step
    } else {
        value
    };
    // Kill float noise (0.35000002) so persisted values stay tidy.
    let tidy = (snapped * 1e4).round() / 1e4;
    tidy.clamp(min, max)
}

/// 0.0–1.0 position of `value` along the track.
pub fn fraction(value: f32, range: &RangeInclusive<f32>) -> f32 {
    let span = range.end() - range.start();
    if span <= 0.0 {
        return 0.0;
    }
    ((value - range.start()) / span).clamp(0.0, 1.0)
}

/// The value under a pointer `x` inside a track spanning `left..left+width`.
pub fn value_at(x: f32, left: f32, width: f32, range: &RangeInclusive<f32>, step: f32) -> f32 {
    let t = if width > 0.0 {
        ((x - left) / width).clamp(0.0, 1.0)
    } else {
        0.0
    };
    snap(
        range.start() + t * (range.end() - range.start()),
        range,
        step,
    )
}

/// `value` moved by `steps` (negative = down).
pub fn nudge(value: f32, steps: f32, range: &RangeInclusive<f32>, step: f32) -> f32 {
    let step = if step > 0.0 {
        step
    } else {
        (range.end() - range.start()) / 100.0
    };
    snap(value + steps * step, range, step)
}

#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    range: RangeInclusive<f32>,
    step: f32,
    width: f32,
    disabled: bool,
    on_change: Option<ChangeHandler>,
}

impl Slider {
    pub fn new(id: impl Into<ElementId>, value: f32, range: RangeInclusive<f32>) -> Self {
        Self {
            id: id.into(),
            value,
            range,
            step: 0.0,
            width: SLIDER_WIDTH,
            disabled: false,
            on_change: None,
        }
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Called with each new (snapped) value while dragging or stepping.
    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let dragging = window.use_keyed_state((self.id.clone(), "drag"), cx, |_, _| false);
        let is_dragging = *dragging.read(cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let t = fraction(self.value, &self.range);
        let interactive = !self.disabled && self.on_change.is_some();
        let (fill, thumb, thumb_border) = if self.disabled {
            (colors.text_disabled, colors.bg_raised, colors.border_subtle)
        } else {
            (colors.accent_solid, colors.bg_raised, colors.border_control)
        };
        let focus_ring = colors.border_focus;
        let track_width = self.width - THUMB;
        let bounds: Rc<Cell<Option<Bounds<Pixels>>>> = Rc::new(Cell::new(None));

        let track = div()
            .absolute()
            .left(px(THUMB / 2.0))
            .top(px((size::CONTROL_MD - TRACK_HEIGHT) / 2.0))
            .w(px(track_width))
            .h(px(TRACK_HEIGHT))
            .rounded(px(radius::FULL))
            .bg(colors.bg_active)
            .child(
                div()
                    .h_full()
                    .w(px(track_width * t))
                    .rounded(px(radius::FULL))
                    .bg(fill),
            );
        let knob = div()
            .absolute()
            .left(px(track_width * t))
            .top(px((size::CONTROL_MD - THUMB) / 2.0))
            .size(px(THUMB))
            .rounded(px(radius::FULL))
            .border_1()
            .border_color(thumb_border)
            .bg(thumb)
            .shadow(theme.shadows(adoken_theme::elevation::LEVEL_1));

        let (range, step, value) = (self.range.clone(), self.step, self.value);
        let measure = {
            let bounds = bounds.clone();
            let handler = self.on_change.clone().filter(|_| interactive);
            let dragging = dragging.clone();
            let range = range.clone();
            canvas(
                move |b, _, _| bounds.set(Some(b)),
                move |b, _, window, _| {
                    // While dragging, follow the pointer anywhere in the
                    // window until the button goes up.
                    let Some(handler) = handler.filter(|_| is_dragging) else {
                        return;
                    };
                    let (left, width) = (
                        f32::from(b.left()) + THUMB / 2.0,
                        f32::from(b.size.width) - THUMB,
                    );
                    let on_move = handler.clone();
                    let move_range = range.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Bubble && event.dragging() {
                            let x = f32::from(event.position.x);
                            on_move(value_at(x, left, width, &move_range, step), window, cx);
                        }
                    });
                    let dragging = dragging.clone();
                    window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble {
                            dragging.update(cx, |dragging, cx| {
                                *dragging = false;
                                cx.notify();
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_full()
        };

        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(px(self.width))
            .h(px(size::CONTROL_MD))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(crate::theme::hsla(adoken_theme::Color::TRANSPARENT))
            .child(measure)
            .child(track)
            .child(knob)
            .when(interactive, |this| {
                let handler = self.on_change.expect("interactive has a handler");
                let on_key = handler.clone();
                let key_range = range.clone();
                this.tab_index(0)
                    .cursor_pointer()
                    .focus(move |style| style.border_color(focus_ring))
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        if let Some(b) = bounds.get() {
                            let (left, width) = (
                                f32::from(b.left()) + THUMB / 2.0,
                                f32::from(b.size.width) - THUMB,
                            );
                            let x = f32::from(event.position.x);
                            handler(value_at(x, left, width, &range, step), window, cx);
                        }
                        dragging.update(cx, |dragging, cx| {
                            *dragging = true;
                            cx.notify();
                        });
                    })
                    .on_key_down(move |event, window, cx| {
                        let keystroke = &event.keystroke;
                        let big = if keystroke.modifiers.shift { 10.0 } else { 1.0 };
                        let next = match keystroke.key.as_str() {
                            "left" | "down" => nudge(value, -big, &key_range, step),
                            "right" | "up" => nudge(value, big, &key_range, step),
                            "home" => *key_range.start(),
                            "end" => *key_range.end(),
                            _ => return,
                        };
                        cx.stop_propagation();
                        if next != value {
                            on_key(next, window, cx);
                        }
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_clamps_and_rounds_to_the_step() {
        let range = 0.0..=0.9;
        assert_eq!(snap(0.37, &range, 0.05), 0.35);
        assert_eq!(snap(0.38, &range, 0.05), 0.4);
        assert_eq!(snap(1.4, &range, 0.05), 0.9);
        assert_eq!(snap(-3.0, &range, 0.05), 0.0);
        // Integer steps (blur px).
        assert_eq!(snap(23.6, &(0.0..=64.0), 1.0), 24.0);
    }

    #[test]
    fn pointer_maps_across_the_track_and_clamps_outside_it() {
        let range = 0.0..=64.0;
        assert_eq!(value_at(100.0, 100.0, 200.0, &range, 1.0), 0.0);
        assert_eq!(value_at(200.0, 100.0, 200.0, &range, 1.0), 32.0);
        assert_eq!(value_at(300.0, 100.0, 200.0, &range, 1.0), 64.0);
        assert_eq!(value_at(-50.0, 100.0, 200.0, &range, 1.0), 0.0);
        assert_eq!(value_at(900.0, 100.0, 200.0, &range, 1.0), 64.0);
        assert_eq!(value_at(150.0, 100.0, 0.0, &range, 1.0), 0.0);
    }

    #[test]
    fn keyboard_steps_stop_at_the_ends() {
        let range = 0.0..=1.0;
        assert_eq!(nudge(0.5, 1.0, &range, 0.05), 0.55);
        assert_eq!(nudge(0.5, -10.0, &range, 0.05), 0.0);
        assert_eq!(nudge(0.98, 1.0, &range, 0.05), 1.0);
        // No step: 1 % of the range.
        assert_eq!(nudge(0.5, 1.0, &range, 0.0), 0.51);
    }

    #[test]
    fn fraction_handles_degenerate_ranges() {
        assert_eq!(fraction(0.3, &(0.0..=0.9)), 0.3 / 0.9);
        assert_eq!(fraction(5.0, &(1.0..=1.0)), 0.0);
        assert_eq!(fraction(2.0, &(0.0..=1.0)), 1.0);
    }
}
