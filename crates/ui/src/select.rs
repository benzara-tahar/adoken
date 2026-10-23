//! Dropdown select: one value out of a list too long for [`crate::Segmented`]
//! (views, schedules, thresholds). Click the trigger to open the menu; with
//! the trigger focused (Tab), `↑`/`↓` change the value directly (like a
//! native popup button), `↩`/Space open or close the menu, `Esc` closes it.

use std::rc::Rc;

use adoken_theme::{elevation, radius, size, space, typography, z};
use gpui::{
    App, ElementId, IntoElement, KeyDownEvent, RenderOnce, SharedString, Window, deferred, div,
    prelude::*, px,
};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

pub const SELECT_WIDTH: f32 = 200.0;
const MENU_MAX_HEIGHT: f32 = 280.0;

/// One entry; `detail` is a muted second label (a schedule, a shortcut).
#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub label: SharedString,
    pub detail: Option<SharedString>,
}

impl SelectOption {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            detail: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl<T: Into<SharedString>> From<T> for SelectOption {
    fn from(label: T) -> Self {
        Self::new(label)
    }
}

/// What a key does on the focused trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyIntent {
    Pick(usize),
    Toggle,
    Close,
    Ignore,
}

/// `↑`/`↓` pick the neighbor (clamped), `↩`/Space toggle the menu, `Esc`
/// closes it while open.
pub fn key_intent(key: &str, modified: bool, selected: usize, len: usize, open: bool) -> KeyIntent {
    if modified || len == 0 {
        return KeyIntent::Ignore;
    }
    match key {
        "up" if selected > 0 => KeyIntent::Pick(selected - 1),
        "down" if selected + 1 < len => KeyIntent::Pick(selected + 1),
        "enter" | "space" => KeyIntent::Toggle,
        "escape" if open => KeyIntent::Close,
        _ => KeyIntent::Ignore,
    }
}

#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    options: Vec<SelectOption>,
    selected: usize,
    width: f32,
    disabled: bool,
    on_select: Option<SelectHandler>,
}

impl Select {
    pub fn new(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = impl Into<SelectOption>>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            width: SELECT_WIDTH,
            disabled: false,
            on_select: None,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let open_state = window.use_keyed_state((self.id.clone(), "open"), cx, |_, _| false);
        let theme = cx.theme();
        let colors = &theme.colors;
        let interactive = !self.disabled && self.on_select.is_some() && !self.options.is_empty();
        let open = interactive && *open_state.read(cx);
        let current = self
            .options
            .get(self.selected)
            .map(|option| option.label.clone())
            .unwrap_or_default();
        let focus_ring = colors.border_focus;
        let len = self.options.len();
        let selected = self.selected;

        let trigger = div()
            .id(self.id.clone())
            .flex()
            .items_center()
            .gap(px(space::S6))
            .w(px(self.width))
            .h(px(size::CONTROL_MD))
            .px(px(space::S8))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_control)
            .bg(colors.bg_input)
            .text_color(if self.disabled {
                colors.text_disabled
            } else {
                colors.text_primary
            })
            .type_style(typography::BODY, theme)
            .child(div().flex_1().min_w_0().truncate().child(current))
            .child(
                Icon::new(if open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(size::ICON_SM)
                .color(colors.text_tertiary),
            )
            .when_some(
                self.on_select.clone().filter(|_| interactive),
                |this, handler| {
                    let click_state = open_state.clone();
                    let key_state = open_state.clone();
                    this.tab_index(0)
                        .cursor_pointer()
                        .hover(|style| style.bg(colors.bg_hover))
                        .focus(move |style| style.border_color(focus_ring))
                        .on_click(move |_, _, cx| {
                            click_state.update(cx, |open, cx| {
                                *open = !*open;
                                cx.notify();
                            })
                        })
                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                            let keystroke = &event.keystroke;
                            let open = *key_state.read(cx);
                            match key_intent(
                                &keystroke.key,
                                keystroke.modifiers.modified(),
                                selected,
                                len,
                                open,
                            ) {
                                KeyIntent::Pick(ix) => {
                                    cx.stop_propagation();
                                    handler(ix, window, cx);
                                }
                                KeyIntent::Toggle => {
                                    cx.stop_propagation();
                                    key_state.update(cx, |open, cx| {
                                        *open = !*open;
                                        cx.notify();
                                    });
                                }
                                KeyIntent::Close => {
                                    cx.stop_propagation();
                                    key_state.update(cx, |open, cx| {
                                        *open = false;
                                        cx.notify();
                                    });
                                }
                                KeyIntent::Ignore => {}
                            }
                        })
                },
            );

        let menu = open.then(|| {
            let handler = self.on_select.clone().expect("open implies interactive");
            let rows = self.options.into_iter().enumerate().map(|(ix, option)| {
                let handler = handler.clone();
                let state = open_state.clone();
                let is_selected = ix == selected;
                div()
                    .id(ix)
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .rounded(px(radius::XS))
                    .cursor_pointer()
                    .when(is_selected, |row| row.bg(colors.bg_selected))
                    .when(!is_selected, |row| {
                        row.hover(|style| style.bg(colors.bg_hover))
                    })
                    .on_click(move |_, window, cx| {
                        state.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        });
                        handler(ix, window, cx);
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(option.label),
                    )
                    .children(option.detail.map(|detail| {
                        div()
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, theme)
                            .child(detail)
                    }))
                    .when(is_selected, |row| {
                        row.child(
                            Icon::new(IconName::Check)
                                .size(size::ICON_SM)
                                .color(colors.accent_fg),
                        )
                    })
            });
            let close = open_state.clone();
            deferred(
                div()
                    .id((self.id.clone(), "menu"))
                    .absolute()
                    .top(px(size::CONTROL_MD + space::S4))
                    .left_0()
                    .min_w(px(self.width))
                    .max_h(px(MENU_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .p(px(space::S4))
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .rounded(px(radius::MD))
                    .border_1()
                    .border_color(colors.border_default)
                    .bg(colors.bg_overlay)
                    .shadow(theme.shadows(elevation::LEVEL_2))
                    .type_style(typography::BODY, theme)
                    .occlude()
                    .on_mouse_down_out(move |_, _, cx| {
                        close.update(cx, |open, cx| {
                            *open = false;
                            cx.notify();
                        })
                    })
                    .children(rows),
            )
            .with_priority(usize::from(z::DROPDOWN))
        });

        div().relative().flex_none().child(trigger).children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_pick_neighbors_and_stop_at_the_ends() {
        assert_eq!(key_intent("down", false, 0, 3, false), KeyIntent::Pick(1));
        assert_eq!(key_intent("up", false, 1, 3, false), KeyIntent::Pick(0));
        assert_eq!(key_intent("up", false, 0, 3, false), KeyIntent::Ignore);
        assert_eq!(key_intent("down", false, 2, 3, false), KeyIntent::Ignore);
    }

    #[test]
    fn escape_only_consumed_while_open_and_modifiers_pass_through() {
        assert_eq!(key_intent("escape", false, 0, 3, true), KeyIntent::Close);
        assert_eq!(key_intent("escape", false, 0, 3, false), KeyIntent::Ignore);
        assert_eq!(key_intent("space", false, 0, 3, false), KeyIntent::Toggle);
        assert_eq!(key_intent("down", true, 0, 3, false), KeyIntent::Ignore);
        assert_eq!(key_intent("down", false, 0, 0, false), KeyIntent::Ignore);
    }
}
