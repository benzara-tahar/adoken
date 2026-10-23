//! Row action menu (docs/design/command-palette.md §4.4, pins.md §3.10):
//! the secondary list `⌘K` (or a right-click) opens for a list row, a
//! palette result, a Today card or a pin. 280 wide on `bg.overlay`,
//! `radius.md`, `elevation.2`, `menu-in` motion; each action with its
//! keycap; typing filters (prefix matches first, then substring); `↑`/`↓`
//! move (wrapping), `↩` runs, `Esc` or a click outside closes it and hands
//! the keyboard back to whatever had it. An unavailable action stays listed
//! in `text.disabled` with its reason as a subtitle (command-palette.md
//! §4.3); `↩` on it does nothing.
//!
//! The pure model ([`MenuModel`]) is unit tested; [`ActionMenu`] is the
//! entity a host view keeps in a [`MenuSlot`] and renders anywhere in its
//! tree (the menu paints on a deferred, anchored layer).

use std::cell::Cell;
use std::rc::Rc;

use adoken_theme::{elevation, motion, radius, size, space, typography, z};
use gpui::{
    Anchor, AnimationExt, AnyElement, App, Bounds, Context, DismissEvent, Entity, EventEmitter,
    FocusHandle, Focusable, KeyBinding, Pixels, Point, Role, SharedString, Subscription, Window,
    actions, anchored, canvas, deferred, div, point, prelude::*, px,
};

use crate::icon::{Icon, IconName};
use crate::kbd::Kbd;
use crate::motion::{EASE_OUT, animation, lerp};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{ActiveTheme, TypeStyleExt};

actions!(
    action_menu,
    [
        /// Action menu: next action (wraps).
        SelectNextAction,
        /// Action menu: previous action (wraps).
        SelectPreviousAction,
        /// Action menu: run the highlighted action.
        RunAction,
        /// Close the action menu.
        CloseActionMenu,
    ]
);

pub const KEY_CONTEXT: &str = "ActionMenu";
/// command-palette.md §4.4.
pub const MENU_WIDTH: f32 = size::ACTION_MENU_WIDTH;

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("down", SelectNextAction, cx),
        KeyBinding::new("up", SelectPreviousAction, cx),
        KeyBinding::new("enter", RunAction, cx),
        KeyBinding::new("escape", CloseActionMenu, cx),
    ]
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One action: its label, keycap, and what it does.
#[derive(Clone)]
pub struct MenuItem {
    pub label: SharedString,
    /// gpui keystroke source (`secondary-enter`), shown as a keycap.
    pub keys: Option<SharedString>,
    /// Why the action can't run now; listed, dimmed, inert.
    pub disabled: Option<SharedString>,
    handler: Handler,
}

impl MenuItem {
    pub fn new(
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            keys: None,
            disabled: None,
            handler: Rc::new(handler),
        }
    }

    pub fn keys(mut self, keys: impl Into<SharedString>) -> Self {
        self.keys = Some(keys.into());
        self
    }

    /// Lists the action as unavailable with `reason` (`None` keeps it on).
    pub fn disabled(mut self, reason: Option<impl Into<SharedString>>) -> Self {
        self.disabled = reason.map(Into::into);
        self
    }
}

impl std::fmt::Debug for MenuItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MenuItem")
            .field("label", &self.label)
            .field("keys", &self.keys)
            .field("disabled", &self.disabled)
            .finish()
    }
}

/// Match rank of `label` for `query`: 0 prefix, 1 substring, `None` no
/// match; case-insensitive; an empty query keeps everything at rank 1.
pub fn match_rank(query: &str, label: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(1);
    }
    let label = label.to_lowercase();
    if label.starts_with(&query) {
        Some(0)
    } else if label.contains(&query) {
        Some(1)
    } else {
        None
    }
}

/// Indices of `labels` that match `query`: prefix matches first, then
/// substring matches, each in input order.
pub fn filter_indices<S: AsRef<str>>(query: &str, labels: &[S]) -> Vec<usize> {
    let mut ranked: Vec<(usize, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(ix, label)| match_rank(query, label.as_ref()).map(|rank| (rank, ix)))
        .collect();
    ranked.sort_unstable();
    ranked.into_iter().map(|(_, ix)| ix).collect()
}

/// What `↩` does on the highlighted row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Run the item at this index (into the menu's items).
    Run(usize),
    /// The highlighted action is unavailable: stay open.
    Disabled,
    /// Nothing matches the filter.
    Nothing,
}

/// The menu's keyboard model: filtered rows and the highlight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuModel {
    labels: Vec<SharedString>,
    enabled: Vec<bool>,
    /// Indices into the items, in display order.
    rows: Vec<usize>,
    /// Index into `rows`.
    selected: usize,
}

impl MenuModel {
    pub fn new(items: &[MenuItem]) -> Self {
        let mut model = Self {
            labels: items.iter().map(|i| i.label.clone()).collect(),
            enabled: items.iter().map(|i| i.disabled.is_none()).collect(),
            rows: Vec::new(),
            selected: 0,
        };
        model.set_query("");
        model
    }

    /// Re-filters; the highlight lands on the first enabled row.
    pub fn set_query(&mut self, query: &str) {
        self.rows = filter_indices(query, &self.labels);
        self.selected = self
            .rows
            .iter()
            .position(|&ix| self.enabled[ix])
            .unwrap_or(0);
    }

    pub fn rows(&self) -> &[usize] {
        &self.rows
    }

    /// The highlighted item (index into the items).
    pub fn selected(&self) -> Option<usize> {
        self.rows.get(self.selected).copied()
    }

    /// `↑`/`↓`: one row, wrapping at both ends.
    pub fn step(&mut self, delta: isize) {
        let len = self.rows.len();
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).rem_euclid(len as isize) as usize;
    }

    /// Highlight the row showing item `item` (pointer hover).
    pub fn highlight(&mut self, item: usize) {
        if let Some(row) = self.rows.iter().position(|&ix| ix == item) {
            self.selected = row;
        }
    }

    pub fn confirm(&self) -> Confirm {
        match self.selected() {
            None => Confirm::Nothing,
            Some(ix) if self.enabled[ix] => Confirm::Run(ix),
            Some(_) => Confirm::Disabled,
        }
    }
}

/// Where the menu shows, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuPosition {
    pub point: Point<Pixels>,
    /// The menu corner placed at `point`.
    pub corner: Anchor,
}

impl MenuPosition {
    /// Keyboard (`⌘K`): hangs below the row, its right edge on the row's
    /// right edge (inset like the row's own padding).
    pub fn below_row(row: Bounds<Pixels>) -> Self {
        Self {
            point: point(row.right() - px(space::S8), row.bottom()),
            corner: Anchor::TopRight,
        }
    }

    /// Pointer (right-click): the menu's top-left at the pointer.
    pub fn at_pointer(position: Point<Pixels>) -> Self {
        Self {
            point: position,
            corner: Anchor::TopLeft,
        }
    }
}

/// Where a row last painted, so `⌘K` can anchor the menu to it.
pub type BoundsSlot = Rc<Cell<Option<Bounds<Pixels>>>>;

/// An invisible layer filling its (relative) parent that records the
/// parent's window bounds into `slot` each frame. Put it in the selected
/// row only.
pub fn track_bounds(slot: BoundsSlot) -> impl IntoElement {
    canvas(move |bounds, _, _| slot.set(Some(bounds)), |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// The `⌘K` position for a row tracked by [`track_bounds`]: below that row,
/// else (not painted yet) near the window's top-right.
pub fn keyboard_position(slot: &BoundsSlot, window: &Window) -> MenuPosition {
    match slot.get() {
        Some(bounds) => MenuPosition::below_row(bounds),
        None => {
            let size = window.viewport_size();
            MenuPosition {
                point: point(size.width / 2.0, size.height / 3.0),
                corner: Anchor::TopLeft,
            }
        }
    }
}

pub struct ActionMenu {
    title: SharedString,
    items: Vec<MenuItem>,
    model: MenuModel,
    input: Entity<TextInput>,
    position: MenuPosition,
    layer: u16,
    previous_focus: Option<FocusHandle>,
    _input_events: Subscription,
}

impl EventEmitter<DismissEvent> for ActionMenu {}

impl Focusable for ActionMenu {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl ActionMenu {
    /// Opens over the window at `position` with the keyboard in its filter;
    /// `title` names the menu for assistive technology ("Actions for !4821").
    pub fn new(
        title: impl Into<SharedString>,
        items: Vec<MenuItem>,
        position: MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new("Filter actions", cx));
        let input_events = cx.subscribe(&input, |this, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let query = input.read(cx).text().to_owned();
                this.model.set_query(&query);
                cx.notify();
            }
        });
        let previous_focus = window.focused(cx);
        window.focus(&input.focus_handle(cx), cx);
        Self {
            title: title.into(),
            model: MenuModel::new(&items),
            items,
            input,
            position,
            layer: z::DROPDOWN,
            previous_focus,
            _input_events: input_events,
        }
    }

    /// Paint above another overlay (the palette uses `z.palette` + 1).
    pub fn set_layer(&mut self, layer: u16) {
        self.layer = layer;
    }

    pub fn model(&self) -> &MenuModel {
        &self.model
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.model.step(delta);
        cx.notify();
    }

    /// Closes and hands the keyboard back to where it was.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
        cx.emit(DismissEvent);
    }

    fn run(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handler) = self
            .items
            .get(item)
            .filter(|i| i.disabled.is_none())
            .map(|i| i.handler.clone())
        else {
            return;
        };
        // Close first, so the action sees the focus it was invoked from.
        self.dismiss(window, cx);
        handler(window, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Confirm::Run(item) = self.model.confirm() {
            self.run(item, window, cx);
        }
    }

    fn render_row(&self, row: usize, item: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let entry = &self.items[item];
        let highlighted = self.model.selected == row;
        let enabled = entry.disabled.is_none();
        let label = div()
            .truncate()
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_disabled
            })
            .child(entry.label.clone());
        let body = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(label)
            .children(entry.disabled.clone().map(|reason| {
                div()
                    .truncate()
                    .text_color(colors.text_disabled)
                    .type_style(typography::SMALL, theme)
                    .child(reason)
            }));
        let keycap = entry.keys.as_ref().and_then(|k| Kbd::parse(k).ok());
        div()
            .id(("action-menu-row", item))
            .min_h(px(size::CONTROL_MD))
            .px(px(space::S8))
            .py(px(space::S2))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::XS))
            .when(highlighted, |d| {
                d.bg(colors.bg_selected).aria_active_descendant()
            })
            .when(!highlighted && enabled, |d| {
                d.hover(|s| s.bg(colors.bg_hover)).cursor_pointer()
            })
            .role(Role::MenuItem)
            .aria_label(entry.label.clone())
            .when_some(entry.disabled.clone(), |d, reason| {
                d.aria_description(reason)
            })
            .when_some(entry.keys.clone(), |d, keys| d.aria_keyshortcuts(keys))
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.model.selected() != Some(item) {
                    this.model.highlight(item);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.run(item, window, cx)))
            .child(body)
            .children(keycap)
            .into_any_element()
    }
}

impl Render for ActionMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = self
            .model
            .rows()
            .to_vec()
            .into_iter()
            .enumerate()
            .map(|(row, item)| self.render_row(row, item, cx))
            .collect();
        let theme = cx.theme();
        let colors = &theme.colors;
        let empty = rows.is_empty().then(|| {
            div()
                .h(px(size::CONTROL_MD))
                .px(px(space::S8))
                .flex()
                .items_center()
                .text_color(colors.text_tertiary)
                .child("No matching actions")
        });
        let card = div()
            .id("action-menu")
            .key_context(KEY_CONTEXT)
            .w(px(MENU_WIDTH))
            .p(px(space::S4))
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_2))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, theme)
            .occlude()
            .role(Role::Menu)
            .aria_label(self.title.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNextAction, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPreviousAction, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &RunAction, window, cx| this.confirm(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseActionMenu, window, cx| this.dismiss(window, cx)),
            )
            .child(
                div()
                    .h(px(size::CONTROL_MD))
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .px(px(space::S8))
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().child(self.input.clone())),
            )
            .children(rows)
            .children(empty)
            // `menu-in`: opacity 0 → 1, translate Y −4 → 0 (tokens.md §7.3);
            // gpui snaps it under reduce motion.
            .with_animation(
                "action-menu-in",
                animation(motion::QUICK, EASE_OUT),
                |card, t| card.opacity(t).mt(px(lerp(-space::S4, 0.0, t))),
            );
        deferred(
            anchored()
                .position(self.position.point)
                .anchor(self.position.corner)
                .snap_to_window_with_margin(px(space::S8))
                .child(card),
        )
        .with_priority(usize::from(self.layer))
    }
}

/// A view's open action menu, if any. The host renders [`MenuSlot::element`]
/// anywhere in its tree; the slot empties itself when the menu closes.
#[derive(Default)]
pub struct MenuSlot {
    menu: Option<(Entity<ActionMenu>, Subscription)>,
}

impl MenuSlot {
    /// Opens `items` as a menu at `position`, replacing any open one.
    /// `slot` finds this slot inside the host view.
    #[allow(clippy::too_many_arguments)]
    pub fn open<V: 'static>(
        &mut self,
        title: impl Into<SharedString>,
        items: Vec<MenuItem>,
        position: MenuPosition,
        layer: u16,
        slot: fn(&mut V) -> &mut MenuSlot,
        window: &mut Window,
        cx: &mut Context<V>,
    ) {
        let title = title.into();
        let previous_focus = self
            .menu
            .as_ref()
            .map(|(menu, _)| menu.read(cx).previous_focus.clone());
        let menu = cx.new(|cx| {
            let mut menu = ActionMenu::new(title, items, position, window, cx);
            menu.set_layer(layer);
            if let Some(previous_focus) = previous_focus {
                menu.previous_focus = previous_focus;
            }
            menu
        });
        let closed = cx.subscribe(&menu, move |view, _, _: &DismissEvent, cx| {
            slot(view).menu = None;
            cx.notify();
        });
        self.menu = Some((menu, closed));
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.menu.is_some()
    }

    pub fn menu(&self) -> Option<&Entity<ActionMenu>> {
        self.menu.as_ref().map(|(menu, _)| menu)
    }

    pub fn element(&self) -> Option<AnyElement> {
        self.menu().map(|menu| menu.clone().into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(labels: &[(&str, bool)]) -> Vec<MenuItem> {
        labels
            .iter()
            .map(|(label, enabled)| {
                MenuItem::new(label.to_string(), |_, _| {})
                    .disabled((!enabled).then_some("Not now"))
            })
            .collect()
    }

    #[test]
    fn filter_ranks_prefix_before_substring_and_keeps_order() {
        let labels = ["Open", "Open in browser", "Copy link", "Unpin", "Pin"];
        assert_eq!(filter_indices("", &labels), [0, 1, 2, 3, 4]);
        assert_eq!(filter_indices("pin", &labels), [4, 3]);
        assert_eq!(filter_indices("OPEN", &labels), [0, 1]);
        assert_eq!(filter_indices("brow", &labels), [1]);
        assert!(filter_indices("zzz", &labels).is_empty());
    }

    #[test]
    fn arrows_wrap_and_enter_runs_the_highlighted_item() {
        let mut menu = MenuModel::new(&items(&[
            ("Open", true),
            ("Copy link", true),
            ("Pin", true),
        ]));
        assert_eq!(menu.confirm(), Confirm::Run(0));
        menu.step(-1);
        assert_eq!(menu.confirm(), Confirm::Run(2), "↑ from the top wraps");
        menu.step(1);
        assert_eq!(menu.confirm(), Confirm::Run(0), "↓ from the bottom wraps");
        menu.step(1);
        assert_eq!(menu.confirm(), Confirm::Run(1));
    }

    #[test]
    fn typing_filters_and_resets_the_highlight() {
        let mut menu = MenuModel::new(&items(&[
            ("Open", true),
            ("Open in browser", true),
            ("Unpin", true),
        ]));
        menu.step(1);
        menu.set_query("unp");
        assert_eq!(menu.rows(), [2]);
        assert_eq!(menu.confirm(), Confirm::Run(2));
        menu.set_query("nothing");
        assert_eq!(menu.confirm(), Confirm::Nothing);
        menu.step(1);
        assert_eq!(menu.selected(), None, "stepping an empty menu is a no-op");
    }

    #[test]
    fn disabled_items_stay_listed_but_never_run() {
        let mut menu = MenuModel::new(&items(&[("Pin", false), ("Open", true)]));
        assert_eq!(
            menu.rows(),
            [0, 1],
            "the unavailable action is still listed"
        );
        assert_eq!(
            menu.confirm(),
            Confirm::Run(1),
            "highlight starts on the first enabled"
        );
        menu.step(1);
        assert_eq!(menu.confirm(), Confirm::Disabled);
        menu.highlight(1);
        assert_eq!(menu.confirm(), Confirm::Run(1));
    }

    #[test]
    fn filtered_disabled_pin_remains_inert_across_keyboard_and_pointer_selection() {
        let mut menu = MenuModel::new(&items(&[
            ("Open", true),
            ("Pin", false),
            ("Copy link", true),
        ]));
        menu.set_query("pin");
        assert_eq!(menu.confirm(), Confirm::Disabled);
        menu.step(-1);
        assert_eq!(menu.confirm(), Confirm::Disabled);
        menu.highlight(1);
        assert_eq!(menu.confirm(), Confirm::Disabled);
        menu.set_query("");
        assert_eq!(menu.confirm(), Confirm::Run(0));
        menu.highlight(2);
        assert_eq!(menu.confirm(), Confirm::Run(2));
    }

    #[test]
    fn keyboard_anchor_hangs_below_the_row_right_aligned() {
        let row = Bounds::new(point(px(10.0), px(100.0)), gpui::size(px(400.0), px(44.0)));
        let at = MenuPosition::below_row(row);
        assert_eq!(at.corner, Anchor::TopRight);
        assert_eq!(at.point, point(px(410.0 - space::S8), px(144.0)));
    }
}
