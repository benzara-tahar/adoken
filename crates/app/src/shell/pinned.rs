use crate::pins::{
    self, Pins, render,
    snapshot::{PinView, Snapshot},
    status::Describe,
};
use adoken_model::{Millis, PinSubject};
use adoken_theme::{size, space, typography};
use adoken_ui::{
    ActiveTheme, Icon, IconName, PinRailIcon, PinRow, PinRowState, Tooltip, TypeStyleExt,
};
use gpui::{AnyElement, App, div, prelude::*, px};

pub fn describe(view: &PinView, now: Millis) -> (String, String) {
    let description = Describe {
        subject: view.subject(),
        kind: &view.kind,
        label: view.pin.label.as_deref(),
        title: &view.title,
        headline: &view.headline,
        changed: view.changed,
        stale_since: view.stale_since,
        now,
    };
    (description.tooltip(), description.spoken())
}

fn unshadowed_binding<'a>(
    keymap: &'a gpui::Keymap,
    action: &'a dyn gpui::Action,
) -> Option<&'a gpui::KeyBinding> {
    keymap.bindings_for_action(action).rev().find(|candidate| {
        keymap
            .bindings()
            .rev()
            .take_while(|later| !std::ptr::eq(*later, *candidate))
            .all(|later| {
                later.keystrokes() != candidate.keystrokes()
                    || later.predicate() != candidate.predicate()
            })
    })
}

fn empty_hint(window: Option<&Window>, cx: &App) -> String {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let action = pins::TogglePin;
    let contextual =
        window.and_then(|window| window.highest_precedence_binding_for_action(&action));
    let binding = if window.is_some() {
        contextual.as_ref()
    } else {
        unshadowed_binding(&keymap, &action)
    };
    let mut hint = "Pin what you use daily".to_owned();
    if let Some(binding) = binding {
        hint.push_str(" · ");
        for (index, key) in binding.keystrokes().iter().enumerate() {
            if index > 0 {
                hint.push(' ');
            }
            hint.push_str(&adoken_ui::format_keystroke(
                key.inner(),
                adoken_ui::KeyPlatform::current(),
            ));
        }
    }
    hint
}

pub fn row(
    view: &PinView,
    collapsed: bool,
    now: Millis,
    state: PinRowState,
    cx: &App,
) -> AnyElement {
    paint_row(
        view,
        collapsed,
        Paint {
            now,
            state,
            phase: 0.0,
        },
        cx,
    )
}

struct Paint {
    now: Millis,
    state: PinRowState,
    phase: f32,
}

fn paint_row(view: &PinView, collapsed: bool, paint: Paint, cx: &App) -> AnyElement {
    let Paint { now, state, phase } = paint;
    let theme = cx.theme();
    let look = render::look(view, now);
    let (tooltip, spoken) = describe(view, now);
    let subject = view.subject().clone();
    let id = format!("pin:{}", subject.key());
    if collapsed {
        PinRailIcon::new(id, look.glyph, look.tint.color(theme))
            .changed(view.changed)
            .state(state)
            .tooltip(tooltip)
            .aria_label(spoken)
            .on_click(move |event, _, cx| {
                if let Some(pins) = Pins::global(cx) {
                    pins.update(cx, |pins, cx| {
                        if event.modifiers().platform {
                            pins.open_in_browser(&subject, cx);
                        } else {
                            pins.open(&subject, cx);
                        }
                    });
                }
            })
            .into_any_element()
    } else {
        let unpin = subject.clone();
        let unpin_label = match &unpin {
            PinSubject::Pr(id) => format!("Unpin pull request {id}"),
            PinSubject::WorkItem(id) => format!("Unpin work item {id}"),
            PinSubject::Definition { id, .. } => format!("Unpin pipeline {id}"),
        };
        let mut row = PinRow::new(
            id,
            look.glyph,
            look.tint.color(theme),
            view.display_title().to_owned(),
        )
        .muted(matches!(
            view.headline,
            crate::pins::status::Headline::NotAvailable(_)
        ))
        .changed(view.changed)
        .state(state)
        .trailing(look.trailing.resolve(theme, phase))
        .tooltip(tooltip)
        .aria_label(spoken)
        .on_click(move |event, _, cx| {
            if let Some(pins) = Pins::global(cx) {
                pins.update(cx, |pins, cx| {
                    if event.modifiers().platform {
                        pins.open_in_browser(&subject, cx);
                    } else {
                        pins.open(&subject, cx);
                    }
                });
            }
        })
        .on_unpin(unpin_label, move |_, _, cx| {
            if let Some(pins) = Pins::global(cx) {
                pins.update(cx, |pins, cx| pins.unpin(&unpin, cx));
            }
        });
        if let Some(reference) = look.fetching {
            row = row.fetching(reference);
        }
        row.into_any_element()
    }
}

/// The same group used by the shell and deterministic gallery fixtures.
pub fn group(
    snapshot: &Snapshot,
    collapsed: bool,
    folded: bool,
    now: Millis,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    if collapsed && snapshot.pins.is_empty() {
        return div().into_any_element();
    }
    let folded = folded && !snapshot.pins.is_empty();
    let header = div()
        .id("pinned-header")
        .flex_none()
        .h(px(size::CONTROL_MD))
        .flex()
        .items_center()
        .gap(px(space::S4))
        .border_t_1()
        .border_color(theme.colors.border_subtle)
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::LABEL, theme)
        .when(collapsed, |header| {
            header.pl(px(adoken_ui::ICON_INSET)).child(Shell::rail_icon(
                IconName::Pin,
                theme.colors.text_tertiary,
                folded
                    .then(|| super::sidebar::rail_badge(snapshot.summary().changed))
                    .flatten(),
                theme,
            ))
        })
        .when(!collapsed, |header| {
            header
                .child("Pinned")
                .when(!snapshot.pins.is_empty(), |header| {
                    header
                        .child(Icon::new(if folded {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        }))
                        .child(div().flex_1())
                        .child(if folded {
                            snapshot.summary().full()
                        } else {
                            snapshot.pins.len().to_string()
                        })
                })
        })
        .aria_label(format!("Pinned · {}", snapshot.summary().full()))
        .tooltip(Tooltip::text(format!(
            "Pinned · {}",
            snapshot.summary().full()
        )));
    div()
        .id("pinned-group")
        .min_h_0()
        .flex()
        .flex_col()
        .gap(px(space::S2))
        .child(header)
        .when(!folded, |group| {
            group.child(
                div()
                    .id("pinned-scroll")
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .children(
                        snapshot
                            .pins
                            .iter()
                            .map(|view| row(view, collapsed, now, PinRowState::default(), cx)),
                    )
                    .when(snapshot.pins.is_empty(), |list| {
                        list.child(
                            div()
                                .h(px(size::CONTROL_MD))
                                .text_color(theme.colors.text_tertiary)
                                .child(empty_hint(None, cx)),
                        )
                    }),
            )
        })
        .into_any_element()
}

pub fn selected_subject(snapshot: &Snapshot, index: usize) -> Option<PinSubject> {
    snapshot.pins.get(index).map(|view| view.subject().clone())
}

use super::Shell;
use adoken_ui::action_menu::{BoundsSlot, MenuItem, MenuPosition, MenuSlot, track_bounds};
use adoken_ui::popover::hover_intent::HoverIntent;
use adoken_ui::{TextInput, TextInputEvent};
use gpui::{
    AnimationExt, Context, Entity, FocusHandle, Focusable, MouseButton, Render, Role, Subscription,
    Window,
};
use std::{cell::Cell, rc::Rc};

pub struct State {
    pub focus: FocusHandle,
    pub cursor: usize,
    pub focused: bool,
    pub peek: bool,
    pub drag: Option<PinSubject>,
    hovered_pin: Option<usize>,
    pub intent: HoverIntent<usize>,
    pub trigger: BoundsSlot,
    pub bounds: BoundsSlot,
    pub menu: MenuSlot,
    row_bounds: BoundsSlot,
    scroll: gpui::ScrollHandle,
    rail_scroll: gpui::ScrollHandle,
    rename: Option<(PinSubject, Entity<TextInput>, Vec<Subscription>)>,
}
impl State {
    pub fn new(cx: &mut Context<Shell>) -> Self {
        Self {
            focus: cx.focus_handle(),
            cursor: 0,
            focused: false,
            peek: false,
            drag: None,
            hovered_pin: None,
            intent: HoverIntent::default(),
            trigger: Rc::new(Cell::new(None)),
            bounds: Rc::new(Cell::new(None)),
            row_bounds: Rc::new(Cell::new(None)),
            menu: MenuSlot::default(),
            scroll: gpui::ScrollHandle::default(),
            rail_scroll: gpui::ScrollHandle::default(),
            rename: None,
        }
    }
}

#[derive(Clone)]
struct DragPin(PinSubject, String);
impl Render for DragPin {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .p(px(space::S8))
            .bg(cx.theme().colors.bg_overlay)
            .child(self.1.clone())
    }
}

impl Shell {
    pub(super) fn pin_at_cursor(&self, cx: &App) -> Option<PinSubject> {
        let pins = self.pins.as_ref()?.read(cx).snapshot();
        self.pin_state
            .cursor
            .checked_sub(super::Nav::ALL.len() + 1)
            .and_then(|index| selected_subject(&pins, index))
    }

    pub(super) fn toggle_pin_fold(&mut self, cx: &mut Context<Self>) {
        self.pinned_collapsed = !self.pinned_collapsed;
        self.persist_sidebar(cx);
        cx.notify();
    }

    pub(super) fn sidebar_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pin_state.rename.is_some() {
            return;
        }
        let count = self
            .pins
            .as_ref()
            .map_or(0, |pins| pins.read(cx).snapshot().pins.len());
        let last = super::Nav::ALL.len() + if count == 0 { 0 } else { count + 1 };
        match event.keystroke.key.as_str() {
            "up" => self.pin_state.cursor = self.pin_state.cursor.saturating_sub(1),
            "down" => {
                self.pin_state.cursor = (self.pin_state.cursor + 1).min(last.saturating_sub(1))
            }
            "escape" => {
                self.pin_state.peek = false;
                self.pin_state.intent.reset();
            }
            "enter" => {
                if let Some(nav) = super::Nav::ALL.get(self.pin_state.cursor) {
                    self.show(*nav, window, cx);
                } else if self.pin_state.cursor == super::Nav::ALL.len() {
                    self.toggle_pin_fold(cx);
                } else if let Some(subject) = self.pin_at_cursor(cx) {
                    if let Some(pins) = &self.pins {
                        pins.update(cx, |pins, cx| {
                            if event.keystroke.modifiers.platform {
                                pins.open_in_browser(&subject, cx)
                            } else {
                                pins.open(&subject, cx)
                            }
                        });
                    }
                    self.pin_state.peek = false;
                }
            }
            "backspace" => {
                if let Some(subject) = self.pin_at_cursor(cx)
                    && let Some(pins) = &self.pins
                {
                    pins.update(cx, |pins, cx| pins.unpin(&subject, cx));
                }
            }
            "k" if event.keystroke.modifiers.platform => {
                if let Some(subject) = self.pin_at_cursor(cx) {
                    self.pin_menu(
                        subject,
                        MenuPosition::at_pointer(window.mouse_position()),
                        window,
                        cx,
                    );
                }
            }
            _ => return,
        }
        if let Some(index) = self.pin_state.cursor.checked_sub(super::Nav::ALL.len() + 1) {
            self.pin_state.scroll.scroll_to_item(index);
            if self.sidebar_collapsed {
                self.pin_state.rail_scroll.scroll_to_item(index);
            }
            if !self.sidebar_collapsed && self.pinned_collapsed {
                self.pinned_collapsed = false;
                self.persist_sidebar(cx);
            }
        }
        if self.sidebar_collapsed
            && self.pin_state.cursor >= super::Nav::ALL.len()
            && event.keystroke.key != "escape"
            && event.keystroke.key != "enter"
        {
            self.pin_state.peek = true;
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn move_cursor_pin(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(subject) = self.pin_at_cursor(cx)
            && let Some(pins) = &self.pins
            && let Some(index) = pins.update(cx, |pins, cx| pins.move_by(&subject, delta, cx))
        {
            self.pin_state.cursor = super::Nav::ALL.len() + 1 + index;
        }
    }

    pub(super) fn open_pin_row_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pin_state.focus.contains_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(subject) = self.pin_at_cursor(cx) {
            let position =
                adoken_ui::action_menu::keyboard_position(&self.pin_state.row_bounds, window);
            self.pin_menu(subject, position, window, cx);
        }
    }

    fn pin_menu(
        &mut self,
        subject: PinSubject,
        position: MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self
            .pins
            .as_ref()
            .map(|pins| pins.read(cx).snapshot())
            .unwrap_or_default();
        let index = snapshot
            .pins
            .iter()
            .position(|view| view.subject() == &subject);
        let has_url = index.is_some_and(|index| snapshot.pins[index].web_url.is_some());
        let mut items = Vec::new();
        let open = subject.clone();
        items.push(
            MenuItem::new("Open", move |_, cx| {
                if let Some(pins) = Pins::global(cx) {
                    pins.update(cx, |pins, cx| pins.open(&open, cx));
                }
            })
            .keys("enter"),
        );
        let browser = subject.clone();
        items.push(
            MenuItem::new("Open in browser", move |_, cx| {
                if let Some(pins) = Pins::global(cx) {
                    pins.update(cx, |pins, cx| pins.open_in_browser(&browser, cx));
                }
            })
            .keys("secondary-enter")
            .disabled((!has_url).then_some("No browser link is available for this pin")),
        );
        let weak = cx.entity().downgrade();
        let rename = subject.clone();
        items.push(MenuItem::new("Rename pin…", move |window, cx| {
            weak.update(cx, |shell, cx| {
                shell.start_pin_rename(rename.clone(), window, cx)
            })
            .ok();
        }));
        for (label, delta, keys) in [
            ("Move up", -1, pins::MOVE_UP_KEYS),
            ("Move down", 1, pins::MOVE_DOWN_KEYS),
        ] {
            let disabled = index.is_none_or(|index| {
                (delta < 0 && index == 0) || (delta > 0 && index + 1 == snapshot.pins.len())
            });
            let subject = subject.clone();
            items.push(
                MenuItem::new(label, move |_, cx| {
                    if let Some(pins) = Pins::global(cx) {
                        pins.update(cx, |pins, cx| {
                            pins.move_by(&subject, delta, cx);
                        });
                    }
                })
                .keys(keys)
                .disabled(disabled.then_some("Already at this end of the list")),
            );
        }
        items.push(
            MenuItem::new("Unpin", move |_, cx| {
                if let Some(pins) = Pins::global(cx) {
                    pins.update(cx, |pins, cx| pins.unpin(&subject, cx));
                }
            })
            .keys("backspace"),
        );
        self.pin_state.menu.open(
            "Pin actions",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |shell| &mut shell.pin_state.menu,
            window,
            cx,
        );
    }

    pub(super) fn start_pin_rename(
        &mut self,
        subject: PinSubject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.pins.as_ref().and_then(|pins| {
            pins.read(cx)
                .snapshot()
                .pins
                .iter()
                .find(|view| view.subject() == &subject)
                .cloned()
        }) else {
            return;
        };
        let input = cx.new(|cx| TextInput::new(view.title.clone(), cx));
        input.update(cx, |input, cx| input.set_text(view.display_title(), cx));
        let changed = cx.subscribe(&input, |_, input, _: &TextInputEvent, cx| {
            let text = input.read(cx).text().to_owned();
            if text.chars().count() > adoken_model::MAX_PIN_LABEL_CHARS {
                let text: String = text
                    .chars()
                    .take(adoken_model::MAX_PIN_LABEL_CHARS)
                    .collect();
                input.update(cx, |input, cx| input.set_text(&text, cx));
            }
            cx.notify();
        });
        let blur = cx.on_blur(&input.focus_handle(cx), window, |shell, _, cx| {
            shell.finish_pin_rename(true, cx)
        });
        window.focus(&input.focus_handle(cx), cx);
        let focus = input.focus_handle(cx);
        window.on_next_frame(move |window, cx| {
            window.focus(&focus, cx);
            window.dispatch_action(Box::new(adoken_ui::text_input::SelectAll), cx);
        });
        self.pin_state.rename = Some((subject, input, vec![changed, blur]));
        self.pin_state.peek = self.sidebar_collapsed;
        cx.notify();
    }

    fn finish_pin_rename(&mut self, save: bool, cx: &mut Context<Self>) {
        if let Some((subject, input, _)) = self.pin_state.rename.take() {
            if save && let Some(pins) = &self.pins {
                let text = input.read(cx).text().to_owned();
                pins.update(cx, |pins, cx| pins.rename(&subject, Some(text), cx));
            }
            cx.notify();
        }
    }

    pub(super) fn render_pin_group(
        &self,
        collapsed: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(pins) = &self.pins else {
            return div().into_any_element();
        };
        let snapshot = pins.read(cx).snapshot();
        let now = pins.read(cx).now();
        if collapsed && snapshot.pins.is_empty() {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let folded = self.pinned_collapsed
            && !snapshot.pins.is_empty()
            && (collapsed || !self.pin_state.peek);
        let phase = if !folded
            && !collapsed
            && snapshot
                .pins
                .iter()
                .any(|view| render::look(view, now).trailing.is_running())
        {
            adoken_ui::motion::pulse_phase(
                adoken_ui::motion::ms(adoken_theme::motion::GLYPH_SPIN),
                cx.entity().entity_id(),
                cx,
            )
        } else {
            0.0
        };
        let header = div()
            .id("live-pinned-header")
            .relative()
            .h(px(size::CONTROL_MD))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .pl(px(adoken_ui::ICON_INSET))
            .pr(px(space::S8))
            .type_style(typography::LABEL, &theme)
            .border_t_1()
            .border_color(theme.colors.border_subtle)
            .text_color(theme.colors.text_tertiary)
            .when(collapsed, |header| {
                header.child(Self::rail_icon(
                    IconName::Pin,
                    theme.colors.text_tertiary,
                    folded
                        .then(|| super::sidebar::rail_badge(snapshot.summary().changed))
                        .flatten(),
                    &theme,
                ))
            })
            .when(!collapsed, |header| {
                header
                    .child("Pinned")
                    .when(!snapshot.pins.is_empty(), |header| {
                        header
                            .child(Icon::new(if folded {
                                IconName::ChevronRight
                            } else {
                                IconName::ChevronDown
                            }))
                            .child(div().flex_1())
                            .child(if folded {
                                snapshot.summary().full()
                            } else {
                                snapshot.pins.len().to_string()
                            })
                    })
            })
            .when(!snapshot.pins.is_empty(), |header| {
                header.role(Role::Button)
            })
            .aria_label(format!("Pinned · {}", snapshot.summary().full()))
            .tooltip(Tooltip::text(if snapshot.pins.is_empty() {
                empty_hint(Some(window), cx)
            } else if collapsed {
                format!("Pinned · {}", snapshot.summary().full())
            } else if folded {
                "Unfold pinned".to_owned()
            } else {
                "Fold pinned".to_owned()
            }))
            .when(
                self.pin_state.focused && self.pin_state.cursor == super::Nav::ALL.len(),
                |header| header.child(adoken_ui::focus_ring(&theme)),
            )
            .on_click(cx.listener(|shell, _, window, cx| {
                if shell
                    .pins
                    .as_ref()
                    .is_some_and(|pins| !pins.read(cx).snapshot().pins.is_empty())
                {
                    window.focus(&shell.pin_state.focus, cx);
                    shell.pin_state.cursor = super::Nav::ALL.len();
                    shell.toggle_pin_fold(cx);
                }
            }));
        let hover_color = theme.colors.bg_hover;
        let insertion_color = theme.colors.border_focus;
        let rows: Vec<_> = snapshot
            .pins
            .iter()
            .enumerate()
            .map(|(index, view)| {
                let subject = view.subject().clone();
                let selected = self.pin_state.focused
                    && self.pin_state.cursor == super::Nav::ALL.len() + 1 + index;
                let state = PinRowState {
                    cursor: selected,
                    on_screen: self.current_pin_subject(cx).as_ref() == Some(&subject),
                    linked: !collapsed
                        && self.sidebar_collapsed
                        && self.pin_state.hovered_pin == Some(index),
                    drag_origin: self.pin_state.drag.as_ref() == Some(&subject),
                };
                let content =
                    if let Some((editing, input, _)) = &self.pin_state.rename {
                        if editing == &subject {
                            div()
                                .id(("pin-rename", index))
                                .flex()
                                .items_center()
                                .gap(px(space::S8))
                                .pl(px(adoken_ui::ICON_INSET))
                                .pr(px(space::S8))
                                .h(px(size::CONTROL_MD))
                                .child(
                                    Icon::new(render::look(view, now).glyph)
                                        .color(render::look(view, now).tint.color(&theme)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .bg(theme.colors.bg_input)
                                        .rounded(px(adoken_theme::radius::SM))
                                        .border_2()
                                        .border_color(theme.colors.border_focus)
                                        .child(input.clone()),
                                )
                                .when(input.read(cx).text().chars().count() >= 50, |row| {
                                    row.child(div().type_style(typography::LABEL, &theme).child(
                                        format!("{}/60", input.read(cx).text().chars().count()),
                                    ))
                                })
                                .on_key_down(cx.listener(
                                    |shell, event: &gpui::KeyDownEvent, window, cx| {
                                        match event.keystroke.key.as_str() {
                                            "enter" => shell.finish_pin_rename(true, cx),
                                            "escape" => shell.finish_pin_rename(false, cx),
                                            _ => return,
                                        };
                                        window.focus(&shell.pin_state.focus, cx);
                                        cx.stop_propagation();
                                    },
                                ))
                                .into_any_element()
                        } else {
                            paint_row(view, collapsed, Paint { now, state, phase }, cx)
                        }
                    } else {
                        paint_row(view, collapsed, Paint { now, state, phase }, cx)
                    };
                let menu_subject = subject.clone();
                let weak = cx.entity().downgrade();
                div()
                    .id(("pin-interaction", index))
                    .relative()
                    .flex_none()
                    .child(content)
                    .when(selected, |row| {
                        row.child(track_bounds(self.pin_state.row_bounds.clone()))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |shell, _, window, cx| {
                            if shell.pin_state.rename.is_some() {
                                return;
                            }
                            shell.pin_state.cursor = super::Nav::ALL.len() + 1 + index;
                            window.focus(&shell.pin_state.focus, cx);
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |shell, event: &gpui::MouseDownEvent, window, cx| {
                            shell.pin_menu(
                                menu_subject.clone(),
                                MenuPosition::at_pointer(event.position),
                                window,
                                cx,
                            )
                        }),
                    )
                    .on_hover(cx.listener(move |shell, hovered: &bool, _, cx| {
                        if collapsed {
                            shell.pin_state.hovered_pin = hovered.then_some(index);
                            cx.notify();
                        }
                    }))
                    .on_drag(
                        DragPin(subject, view.display_title().to_owned()),
                        move |drag, _, _, cx| {
                            weak.update(cx, |shell, cx| {
                                shell.pin_state.drag = Some(drag.0.clone());
                                shell.pin_state.intent.cancel();
                                cx.notify();
                            })
                            .ok();
                            cx.new(|_| drag.clone())
                        },
                    )
                    .drag_over::<DragPin>(move |style, _, _, _| {
                        style
                            .bg(hover_color)
                            .border_t_2()
                            .border_color(insertion_color)
                    })
                    .on_drop(cx.listener(move |shell, drag: &DragPin, _, cx| {
                        if let Some(pins) = &shell.pins {
                            pins.update(cx, |pins, cx| {
                                pins.move_to(&drag.0, index, cx);
                            });
                            shell.pin_state.cursor = super::Nav::ALL.len() + 1 + index;
                        }
                        shell.pin_state.drag = None;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        div()
            .id(if collapsed {
                "live-pins-rail"
            } else {
                "live-pins-group"
            })
            .min_h_0()
            .flex_shrink_1()
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .role(Role::List)
            .child(header)
            .when(!folded, |group| {
                group.child(
                    div()
                        .id("live-pins-scroll")
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(if collapsed {
                            &self.pin_state.rail_scroll
                        } else {
                            &self.pin_state.scroll
                        })
                        .flex()
                        .flex_col()
                        .gap(px(space::S2))
                        .children(rows)
                        .when(snapshot.pins.is_empty(), |list| {
                            list.child(empty_hint(Some(window), cx))
                        }),
                )
            })
            .into_any_element()
    }
}

impl Shell {
    pub(super) fn current_pin_subject(&self, cx: &App) -> Option<PinSubject> {
        let views = self.views.as_ref()?;
        match self.nav {
            super::Nav::PullRequests => views.review.read(cx).pin_subject(cx),
            super::Nav::WorkItems => views.workitems.read(cx).pin_subject(),
            super::Nav::Pipelines => views.pipelines.read(cx).pin_subject(),
            super::Nav::Inbox => views.inbox.read(cx).pin_subject(),
            super::Nav::Today => views.today.read(cx).pin_subject(),
            super::Nav::Search => None,
        }
    }

    pub(super) fn toggle_current_pin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let subject = if self.palette.read(cx).is_visible() {
            self.palette.read(cx).selected_pin_subject()
        } else if self.pin_state.focus.contains_focused(window, cx) {
            self.pin_at_cursor(cx)
        } else {
            self.current_pin_subject(cx)
        };
        let title = if self.palette.read(cx).is_visible() {
            self.palette.read(cx).selected_title()
        } else if self.pin_state.focus.contains_focused(window, cx) {
            self.pins
                .as_ref()
                .and_then(|pins| {
                    subject.as_ref().and_then(|subject| {
                        pins.read(cx)
                            .snapshot()
                            .pins
                            .iter()
                            .find(|view| view.subject() == subject)
                            .map(|view| view.title.clone().into())
                    })
                })
                .unwrap_or_default()
        } else if let Some(views) = &self.views {
            match self.nav {
                super::Nav::PullRequests => views.review.read(cx).pin_title(cx),
                super::Nav::WorkItems => views.workitems.read(cx).pin_title(),
                super::Nav::Pipelines => views.pipelines.read(cx).pin_title(),
                super::Nav::Inbox => views.inbox.read(cx).pin_title(),
                super::Nav::Today => views.today.read(cx).pin_title(),
                super::Nav::Search => gpui::SharedString::default(),
            }
        } else {
            gpui::SharedString::default()
        };
        crate::pin_actions::toggle(subject, title, window, cx);
    }

    pub(super) fn render_pin_peek(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.sidebar_collapsed || !self.pin_state.peek {
            return None;
        }
        let theme = cx.theme().clone();
        let top = self
            .pin_state
            .trigger
            .get()
            .map_or(size::CONTROL_MD * super::Nav::ALL.len() as f32, |bounds| {
                f32::from(bounds.origin.y) - size::TITLEBAR
            });
        let available = (f32::from(window.viewport_size().height)
            - size::TITLEBAR
            - super::STATUSBAR_HEIGHT
            - top.max(space::S4)
            - space::S8)
            .max(size::CONTROL_MD);
        Some(
            div()
                .id("pins-peek")
                .absolute()
                .left(px(super::sidebar::RAIL_WIDTH + space::S4))
                .top(px(top.max(space::S4)))
                .w(px(super::sidebar::SIDEBAR_WIDTH))
                .max_h(px(available))
                .flex()
                .flex_col()
                .p(px(space::S8))
                .bg(theme.colors.bg_overlay)
                .border_1()
                .border_color(theme.colors.border_default)
                .rounded(px(adoken_theme::radius::MD))
                .child(track_bounds(self.pin_state.bounds.clone()))
                .child(self.render_pin_group(false, window, cx))
                .with_animation(
                    "pins-peek-in",
                    adoken_ui::motion::animation(
                        adoken_theme::motion::QUICK,
                        adoken_ui::motion::EASE_OUT,
                    ),
                    |peek, t| peek.opacity(t),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_live_pins(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("pins-trigger")
            .relative()
            .min_h_0()
            .flex_shrink_1()
            .flex()
            .flex_col()
            .child(track_bounds(self.pin_state.trigger.clone()))
            .child(self.render_pin_group(self.sidebar_collapsed, window, cx))
            .on_hover(cx.listener(|shell, hovered: &bool, _, cx| {
                if !shell.sidebar_collapsed {
                    return;
                }
                if *hovered && !shell.pin_state.peek {
                    shell.pin_state.intent.dwell(0, cx, |shell, cx| {
                        if shell.sidebar_collapsed && shell.pin_state.intent.pending() == Some(&0) {
                            shell.pin_state.peek = true;
                            cx.notify();
                        }
                    });
                } else if !*hovered && !shell.pin_state.peek {
                    shell.pin_state.intent.leave(&0);
                }
            }))
            .on_mouse_move(cx.listener(|shell, event: &gpui::MouseMoveEvent, _, _| {
                shell.pin_state.intent.record_origin(event.position)
            }))
            .into_any_element()
    }

    pub(super) fn dismiss_peek_outside(
        &mut self,
        event: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.pin_state.peek
            || self.pin_state.rename.is_some()
            || self.pin_state.drag.is_some()
            || self.pin_state.menu.is_open()
        {
            return;
        }
        if let Some(trigger) = self.pin_state.trigger.get() {
            if !self.pin_state.intent.contains_pointer(
                trigger,
                self.pin_state.bounds.get(),
                event.position,
                false,
            ) {
                if !self.pin_state.intent.is_scheduled() {
                    self.pin_state.intent.defer(cx, |shell, cx| {
                        shell.pin_state.peek = false;
                        cx.notify();
                    });
                }
            } else {
                self.pin_state.intent.cancel();
                self.pin_state.intent.record_origin(event.position);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_key_collision_hides_pin_shortcut_but_keeps_an_unshadowed_alias() {
        let toggle = pins::TogglePin;
        let mut keymap = gpui::Keymap::new(vec![gpui::KeyBinding::new(
            "secondary-d",
            toggle.clone(),
            None,
        )]);
        assert!(unshadowed_binding(&keymap, &toggle).is_some());
        keymap.add_bindings(vec![gpui::KeyBinding::new(
            "secondary-d",
            pins::UndoUnpin,
            None,
        )]);
        assert!(unshadowed_binding(&keymap, &toggle).is_none());
        keymap.add_bindings(vec![gpui::KeyBinding::new("alt-d", toggle.clone(), None)]);
        let active = unshadowed_binding(&keymap, &toggle).unwrap();
        assert_eq!(
            active.keystrokes(),
            gpui::KeyBinding::new("alt-d", toggle.clone(), None).keystrokes()
        );
    }
}
