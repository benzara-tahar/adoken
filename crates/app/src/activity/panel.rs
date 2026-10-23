//! The Activity drawer (spec 0016 FR-1–FR-4; activity.md §3, §6, §9):
//! docked at the bottom of the content column, resizable by its top edge
//! (mouse or keys), with the Sync and Network tabs. A region labeled
//! "Activity". Created on first open; the data lives in [`Activity`].

use adoken_sync::ledger::{Group, Totals, Window as TotalsWindow};
use adoken_theme::{motion, radius, size, space, typography};
use adoken_ui::action_menu::MenuSlot;
use adoken_ui::motion::{EASE_OUT, animation, lerp};
use adoken_ui::{
    ActiveTheme, Icon, IconName, TextInput, TextInputEvent, Theme, Tooltip, TypeStyleExt,
};
use gpui::{
    AnimationExt, AnyElement, App, Context, Entity, FocusHandle, Focusable, ListAlignment,
    ListState, MouseButton, MouseDownEvent, Subscription, Window, div, prelude::*, px,
};

use super::indicator::{IndicatorView, ProblemTone};
use super::model::{Activity, ActivityEvent};
use super::net::TotalsSort;
use super::settings::{SplitterKey, Tab};
use super::*;

/// Which list the drawer's keyboard drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pane {
    Kinds,
    Passes,
    Table,
}

/// Network › Totals view state (FR-24).
pub(super) struct TotalsState {
    pub shown: bool,
    pub group: Group,
    pub window: TotalsWindow,
    pub sort: TotalsSort,
    pub ascending: bool,
    pub cursor: usize,
    pub data: Option<Totals>,
    /// Log generation the data was computed at.
    pub generation: Option<(u64, Group, TotalsWindow)>,
}

pub struct ActivityPanel {
    pub(super) activity: Entity<Activity>,
    pub(super) focus: FocusHandle,
    pub(super) kinds_focus: FocusHandle,
    pub(super) passes_focus: FocusHandle,
    pub(super) table_focus: FocusHandle,
    pub(super) splitter_focus: FocusHandle,
    pub(super) filter: Entity<TextInput>,
    pub(super) kinds_cursor: usize,
    pub(super) pass_cursor: usize,
    pub(super) totals: TotalsState,
    pub(super) table: ListState,
    pub(super) menu: MenuSlot,
    /// Splitter drag: pointer y and height at the start.
    drag: Option<(f32, f32)>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for ActivityPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ActivityPanel {
    pub fn new(activity: Entity<Activity>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextInput::new("Filter purpose or path", cx));
        let filter_events = cx.subscribe(&filter, |this, input, event, cx| {
            if matches!(event, TextInputEvent::Changed | TextInputEvent::Pasted) {
                let text = input.read(cx).text().to_owned();
                this.activity.update(cx, |activity, cx| {
                    activity.net.update_filter(|f| f.text = text);
                    cx.notify();
                });
                this.sync_table(cx);
            }
        });
        let observe = cx.observe(&activity, |this, _, cx| {
            this.sync_table(cx);
            cx.notify();
        });
        let table = ListState::new(0, ListAlignment::Top, px(size::ROW_DENSE * 8.0))
            .with_uniform_item_height(px(size::ROW_DENSE));
        let weak = cx.entity().downgrade();
        table.set_scroll_handler(move |_, window, cx| {
            let weak = weak.clone();
            window.defer(cx, move |_, cx| {
                weak.update(cx, |this, cx| this.table_scrolled(cx)).ok();
            });
        });
        let mut this = Self {
            activity,
            focus: cx.focus_handle(),
            kinds_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            passes_focus: cx.focus_handle().tab_index(2).tab_stop(true),
            table_focus: cx.focus_handle().tab_index(1).tab_stop(true),
            splitter_focus: cx.focus_handle().tab_index(3).tab_stop(true),
            filter,
            kinds_cursor: 0,
            pass_cursor: 0,
            totals: TotalsState {
                shown: false,
                group: Group::Endpoint,
                window: TotalsWindow::LastHour,
                sort: TotalsSort::Tstu,
                ascending: false,
                cursor: 0,
                data: None,
                generation: None,
            },
            table,
            menu: MenuSlot::default(),
            drag: None,
            _subscriptions: vec![filter_events, observe],
        };
        this.sync_table(cx);
        let _ = window;
        this
    }

    pub fn activity(&self) -> &Entity<Activity> {
        &self.activity
    }

    /// Move focus into the drawer (activity.md §6.3): Sync focuses the
    /// kinds list on the first kind that isn't Synced; Network the table
    /// on the newest row, following.
    pub fn focus_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.activity.read(cx).tab() {
            Tab::Sync => {
                if let Some((_, rows)) = self.activity.read(cx).rows(cx) {
                    self.kinds_cursor = super::passes::first_attention(&rows);
                }
                window.focus(&self.kinds_focus, cx);
            }
            Tab::Network => {
                self.activity.update(cx, |a, _| a.net.jump_to_latest());
                self.follow_tail(cx);
                window.focus(&self.table_focus, cx);
            }
        }
        cx.notify();
    }

    /// The drawer (or something in it) has the keyboard.
    pub fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.focus.contains_focused(window, cx)
    }

    pub(super) fn pane(&self, window: &Window) -> Pane {
        if self.passes_focus.is_focused(window) {
            Pane::Passes
        } else if self.table_focus.is_focused(window) {
            Pane::Table
        } else {
            Pane::Kinds
        }
    }

    // ── tabs and the drawer ─────────────────────────────────────────────

    /// Splitter drag in progress (the shell routes pointer moves here so
    /// the drag keeps going over the content above).
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The pointer moved to `y` while dragging: 1:1, instant.
    pub fn drag_to(&mut self, y: f32, cx: &mut Context<Self>) {
        if let Some((start_y, start_h)) = self.drag {
            self.activity
                .update(cx, |a, cx| a.set_height(start_h + (start_y - y), cx));
        }
    }

    pub fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    /// Move the keyboard to Recent passes (Tab from the kinds list).
    pub fn focus_passes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.passes_focus, cx);
        cx.notify();
    }

    /// Mirror the Network rows into the table now (stories, smoke runs).
    pub fn sync_table_now(&mut self, cx: &mut Context<Self>) {
        self.sync_table(cx);
        self.reveal_cursor(cx);
    }

    /// Network: show Totals (`T`) or Requests.
    pub fn show_totals(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.totals.shown = shown;
        cx.notify();
    }

    /// Network: type `text` into the filter field.
    pub fn set_filter_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.filter.update(cx, |input, cx| input.set_text(text, cx));
    }

    pub fn switch_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.activity.update(cx, |a, cx| a.set_tab(tab, cx));
        self.focus_tab(window, cx);
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.is_open() {
            return;
        }
        if self.filter.focus_handle(cx).is_focused(window) {
            window.focus(&self.table_focus, cx);
            return;
        }
        let detail = self.activity.read(cx).net.detail.is_some();
        if detail && self.activity.read(cx).tab() == Tab::Network {
            self.activity.update(cx, |a, cx| {
                a.net.detail = None;
                cx.notify();
            });
            self.remeasure(cx);
            return;
        }
        self.activity.update(cx, |a, cx| a.close(cx));
    }

    fn resize(&mut self, key: SplitterKey, cx: &mut Context<Self>) {
        self.activity.update(cx, |a, cx| {
            let height = super::settings::step_height(a.height(), key, a_window(a));
            a.set_height(height, cx);
        });
    }

    // ── table (Network › Requests) ──────────────────────────────────────

    /// Mirror the visible rows into the list state: splice evictions and
    /// appends (the scroll position stays put), reset on filter changes.
    pub(super) fn sync_table(&mut self, cx: &mut Context<Self>) {
        let (delta, len, follow) = self.activity.update(cx, |a, _| {
            (a.net.take_delta(), a.net.visible().len(), a.net.follow)
        });
        let known = self.table.item_count();
        if delta.reset || known + delta.appended < delta.evicted {
            self.table
                .reset_with_uniform_height(len, px(size::ROW_DENSE));
        } else {
            if delta.evicted > 0 {
                self.table.splice(0..delta.evicted.min(known), 0);
            }
            if delta.appended > 0 {
                let at = self.table.item_count();
                self.table.splice(at..at, delta.appended);
            }
            if self.table.item_count() != len {
                self.table
                    .reset_with_uniform_height(len, px(size::ROW_DENSE));
            }
        }
        if follow {
            self.follow_tail(cx);
        } else if delta.reset {
            self.reveal_cursor(cx);
        }
    }

    pub(super) fn follow_tail(&mut self, cx: &mut Context<Self>) {
        // Scroll so the newest row sits at the bottom, from the uniform row
        // height: the tail paints on this frame, before any layout pass
        // measured the list. The model's `follow` keeps it there.
        let count = self.table.item_count();
        let body = self.activity.read(cx).height()
            - 2.0 * size::DRAWER_HEADER
            - 2.0 * size::ROW_DENSE
            - size::BORDER;
        let rows = (body / size::ROW_DENSE).floor().max(1.0) as usize;
        self.table.scroll_to(gpui::ListOffset {
            item_ix: count.saturating_sub(rows),
            offset_in_item: px(0.0),
        });
    }

    pub(super) fn reveal_cursor(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.activity.read(cx).net.cursor_index() {
            self.table.set_follow_mode(gpui::FollowMode::Normal);
            self.table.scroll_to_reveal_item(ix);
        }
    }

    fn table_scrolled(&mut self, cx: &mut Context<Self>) {
        let following =
            self.table.is_following_tail() || self.table.is_scrolled_to_end().unwrap_or(true);
        self.activity.update(cx, |a, cx| {
            let was = a.net.follow;
            a.net.scrolled(following);
            if was != a.net.follow {
                cx.notify();
            }
        });
    }

    pub(super) fn remeasure(&mut self, _cx: &mut Context<Self>) {
        self.table.remeasure();
    }

    // ── rendering ───────────────────────────────────────────────────────

    fn render_splitter(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = &theme.colors;
        let height = self.activity.read(cx).height();
        let focused = self.drag.is_some();
        div()
            .id("activity-splitter")
            .key_context("ActivitySplitter")
            .track_focus(&self.splitter_focus)
            .role(gpui::Role::Splitter)
            .aria_label("Resize activity drawer")
            .aria_numeric_value(f64::from(height))
            .absolute()
            .top(px(-4.0))
            .left_0()
            .right_0()
            .h(px(8.0))
            .cursor_row_resize()
            .group("activity-splitter")
            .child(
                div()
                    .absolute()
                    .top(px(4.0))
                    .left_0()
                    .right_0()
                    .h(px(1.0))
                    .bg(colors.border_default)
                    .when(focused, |d| d.h(px(2.0)).bg(colors.border_strong))
                    .group_hover("activity-splitter", |d| {
                        d.h(px(2.0)).bg(colors.border_strong)
                    }),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    if event.click_count >= 2 {
                        this.activity.update(cx, |a, cx| a.toggle_maximized(cx));
                        return;
                    }
                    let height = this.activity.read(cx).height();
                    this.drag = Some((f32::from(event.position.y), height));
                    window.focus(&this.splitter_focus, cx);
                    cx.stop_propagation();
                }),
            )
            .on_action(cx.listener(|this, _: &GrowSmall, _, cx| this.resize(SplitterKey::Up, cx)))
            .on_action(
                cx.listener(|this, _: &ShrinkSmall, _, cx| this.resize(SplitterKey::Down, cx)),
            )
            .on_action(
                cx.listener(|this, _: &GrowLarge, _, cx| this.resize(SplitterKey::BigUp, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShrinkLarge, _, cx| this.resize(SplitterKey::BigDown, cx)),
            )
            .on_action(cx.listener(|this, _: &Maximize, _, cx| this.resize(SplitterKey::Max, cx)))
            .on_action(cx.listener(|this, _: &Minimize, _, cx| this.resize(SplitterKey::Min, cx)))
    }

    fn render_header(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let tab = activity.tab();
        let maximized = activity.is_maximized();
        let problems = match activity.indicator() {
            IndicatorView::Shown {
                problems: Some(p), ..
            } if tab != Tab::Network => Some(*p),
            _ => None,
        };
        let sync_badge = activity.rows(cx).map(|(_, rows)| {
            let running = rows
                .iter()
                .any(|r| matches!(r.state, adoken_sync::activity::RowState::Syncing(_)));
            let failing = rows
                .iter()
                .any(|r| matches!(r.state, adoken_sync::activity::RowState::Failed { .. }));
            (running, failing)
        });
        let tab_button = |id: Tab, cx: &mut Context<Self>| {
            let active = id == tab;
            let mut label = id.title().to_owned();
            let badge: Option<AnyElement> = match id {
                Tab::Network => problems.map(|p| {
                    let (icon, fg, wash) = match p.tone {
                        ProblemTone::Danger => (
                            IconName::CircleX,
                            colors.status_danger_fg,
                            colors.status_danger_wash,
                        ),
                        ProblemTone::Warning => (
                            IconName::CircleAlert,
                            colors.status_warning_fg,
                            colors.status_warning_wash,
                        ),
                    };
                    label.push_str(&format!(", {} failed", p.count()));
                    div()
                        .h(px(16.0))
                        .px(px(space::S4))
                        .flex()
                        .items_center()
                        .gap(px(space::S2))
                        .rounded(px(radius::FULL))
                        .bg(wash)
                        .text_color(colors.text_primary)
                        .type_style(typography::LABEL, theme)
                        .child(Icon::new(icon).size(size::ICON_XS).color(fg))
                        .child(p.count().to_string())
                        .into_any_element()
                }),
                Tab::Sync => match sync_badge {
                    Some((_, true)) => {
                        label.push_str(", a kind is failing");
                        Some(
                            Icon::new(IconName::CircleX)
                                .size(size::ICON_XS)
                                .color(colors.status_danger_fg)
                                .into_any_element(),
                        )
                    }
                    Some((true, false)) => {
                        label.push_str(", syncing");
                        Some(
                            Icon::new(IconName::LoaderCircle)
                                .size(size::ICON_XS)
                                .color(colors.status_info_fg)
                                .into_any_element(),
                        )
                    }
                    _ => None,
                },
            };
            div()
                .id(("activity-tab", id as usize))
                .role(gpui::Role::Tab)
                .aria_label(label)
                .aria_selected(active)
                .relative()
                .h_full()
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S4))
                .type_style(typography::BODY_STRONG, theme)
                .text_color(if active {
                    colors.text_primary
                } else {
                    colors.text_secondary
                })
                .hover(|d| d.text_color(colors.text_primary))
                .cursor_pointer()
                .child(id.title())
                .children(badge)
                .when(active, |d| {
                    d.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(space::S4))
                            .right(px(space::S4))
                            .h(px(2.0))
                            .bg(colors.accent_solid),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| this.switch_tab(id, window, cx)))
        };
        let icon_button =
            |id: &'static str, icon: IconName, tip: &'static str, label: &'static str| {
                div()
                    .id(id)
                    .role(gpui::Role::Button)
                    .aria_label(label)
                    .size(px(size::CONTROL_SM))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|d| d.bg(colors.bg_hover))
                    .child(
                        Icon::new(icon)
                            .size(size::ICON_SM)
                            .color(colors.text_secondary),
                    )
                    .tooltip(Tooltip::text(tip))
            };
        let _ = window;
        div()
            .flex_none()
            .h(px(size::DRAWER_HEADER))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .id("activity-tabs")
                    .h_full()
                    .flex()
                    .gap(px(space::S8))
                    .role(gpui::Role::TabList)
                    .aria_label("Activity")
                    .child(tab_button(Tab::Sync, cx))
                    .child(tab_button(Tab::Network, cx)),
            )
            .child(div().flex_1())
            .child(
                if maximized {
                    icon_button(
                        "activity-restore",
                        IconName::Minimize2,
                        "Restore ⌘⇧↩",
                        "Restore activity drawer",
                    )
                } else {
                    icon_button(
                        "activity-maximize",
                        IconName::Maximize2,
                        "Maximize ⌘⇧↩",
                        "Maximize activity drawer",
                    )
                }
                .on_click(cx.listener(|this, _, _, cx| {
                    this.activity.update(cx, |a, cx| a.toggle_maximized(cx))
                })),
            )
            .child(
                icon_button(
                    "activity-close",
                    IconName::X,
                    "Close  ⎋",
                    "Close activity drawer",
                )
                .on_click(
                    cx.listener(|this, _, _, cx| this.activity.update(cx, |a, cx| a.close(cx))),
                ),
            )
    }
}

/// The window height the model last saw (the drawer's range depends on it).
fn a_window(activity: &Activity) -> f32 {
    activity.window_height()
}

impl Render for ActivityPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let tab = activity.tab();
        let height = activity.height();
        let animate = activity.opened_by_pointer;
        let opens = activity.opens;
        if tab == Tab::Network {
            // Failures showing are seen; the chip clears after the tab
            // painted, never on the click frame (activity.md §2.3).
            let activity = self.activity.clone();
            window.on_next_frame(move |_, cx| activity.update(cx, |a, cx| a.mark_seen(cx)));
        }
        let body = match tab {
            Tab::Sync => self.render_sync(&theme, window, cx),
            Tab::Network => self.render_network(&theme, window, cx),
        };
        let drawer = div()
            .id("activity-drawer")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .role(gpui::Role::Group)
            .aria_label("Activity")
            .relative()
            .flex_none()
            .w_full()
            .h(px(height))
            .flex()
            .flex_col()
            .bg(colors.bg_surface)
            .border_t_1()
            .border_color(colors.border_default)
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .on_action(cx.listener(|this, _: &NextTab, window, cx| {
                let tab = this.activity.read(cx).tab().other();
                this.switch_tab(tab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousTab, window, cx| {
                let tab = this.activity.read(cx).tab().other();
                this.switch_tab(tab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleMaximize, _, cx| {
                this.activity.update(cx, |a, cx| a.toggle_maximized(cx))
            }))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_select_previous))
            .on_action(cx.listener(Self::on_select_first))
            .on_action(cx.listener(Self::on_select_last))
            .on_action(cx.listener(Self::on_page_down))
            .on_action(cx.listener(Self::on_page_up))
            .on_action(cx.listener(Self::on_confirm))
            .on_action(cx.listener(Self::on_right))
            .on_action(cx.listener(Self::on_left))
            .on_action(cx.listener(Self::on_toggle_failed))
            .on_action(cx.listener(Self::on_toggle_totals))
            .on_action(cx.listener(Self::on_focus_filter))
            .on_action(cx.listener(Self::on_copy_rows))
            .on_action(cx.listener(Self::on_clear_filters))
            .on_action(cx.listener(Self::on_open_in_browser))
            .on_action(cx.listener(Self::on_row_menu))
            .child(self.render_splitter(&theme, cx))
            .child(self.render_header(&theme, window, cx))
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .children(self.menu.element());
        if animate {
            drawer
                .with_animation(
                    ("activity-open", opens),
                    animation(motion::BASE, EASE_OUT),
                    move |d, t| d.h(px(lerp(0.0, height, t))).overflow_hidden(),
                )
                .into_any_element()
        } else {
            drawer.into_any_element()
        }
    }
}

impl ActivityPanel {
    pub(super) fn emit(&self, event: ActivityEvent, cx: &mut Context<Self>) {
        self.activity.update(cx, |a, cx| a.emit(event, cx));
    }
}
