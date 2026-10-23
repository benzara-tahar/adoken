//! Activity › Network (spec 0016 FR-22–FR-27; activity.md §5): the toolbar
//! and filters, the virtualized requests table (newest at the bottom,
//! following the tail), row detail, Totals, and the privacy footer. Also
//! the drawer's list keys for both tabs.

use adoken_sync::attribution::OriginGroup;
use adoken_sync::ledger::{Entry, Group, Status, StatusClass, TotalsKey, Window as TotalsWindow};
use adoken_theme::{elevation, radius, size, space, typography};
use adoken_ui::action_menu::{MenuItem, MenuPosition};
use adoken_ui::{
    Button, ButtonVariant, EmptyState, Icon, IconName, Kbd, Segmented, Theme, Toast, ToastStack,
    Tooltip, TypeStyleExt,
};
use gpui::{
    AnyElement, ClipboardItem, Context, Focusable, Hsla, Window, div, list, prelude::*, px,
};

use super::net::{
    self, NetFilter, OriginChoice, Pill, TotalsSort, count_text, latency_text, size_text,
    status_text, time_text, tstu_text,
};
use super::panel::ActivityPanel;
use super::settings::Tab;
use super::*;

/// Column widths (activity.md §5.2).
const TIME_W: f32 = 88.0;
const METHOD_W: f32 = 48.0;
const STATUS_W: f32 = 72.0;
const ORIGIN_W: f32 = 120.0;
const LATENCY_W: f32 = 64.0;
const SIZE_W: f32 = 64.0;
const TSTU_W: f32 = 56.0;
/// Rows a page key moves.
const PAGE: isize = 10;

/// Which columns fit the drawer's width (activity.md §5.2 drop order).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Columns {
    pub origin: bool,
    pub path: bool,
    pub size: bool,
    pub tstu: bool,
    /// Purpose and Path share what the fixed columns leave, so every row
    /// and the header line up whatever their text.
    pub purpose_w: f32,
    pub path_w: f32,
}

impl Columns {
    /// `width`: the table's inner width (the drawer minus its padding).
    pub fn for_width(width: f32) -> Self {
        let origin = width >= 1100.0;
        let size = width >= 900.0;
        let tstu = width >= 760.0;
        let path = width >= 600.0;
        let fixed = [
            (true, TIME_W),
            (true, METHOD_W),
            (true, STATUS_W),
            (origin, ORIGIN_W),
            (true, LATENCY_W),
            (size, SIZE_W),
            (tstu, TSTU_W),
        ];
        let shown = fixed.iter().filter(|(on, _)| *on).count() + 1 + usize::from(path);
        let used: f32 = fixed
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, w)| w)
            .sum::<f32>()
            + space::S12 * (shown - 1) as f32;
        let rest = (width - used).max(0.0);
        let (purpose_w, path_w) = if path {
            let purpose = (rest * 0.45).max(120.0).min(rest);
            (purpose, rest - purpose)
        } else {
            (rest, 0.0)
        };
        Self {
            origin,
            path,
            size,
            tstu,
            purpose_w,
            path_w,
        }
    }
}

fn origin_icon(group: OriginGroup) -> IconName {
    match group {
        OriginGroup::Sync => IconName::RefreshCw,
        OriginGroup::You => IconName::User,
        OriginGroup::Avatars => IconName::Image,
        OriginGroup::Startup => IconName::Rocket,
        OriginGroup::Unattributed => IconName::CircleAlert,
    }
}

/// `(wash, glyph, glyph color, text color)` of a status pill.
fn pill_style(pill: Pill, theme: &Theme) -> (Option<Hsla>, Option<IconName>, Hsla, Hsla) {
    let c = &theme.colors;
    match pill {
        Pill::Plain => (None, None, c.text_secondary, c.text_secondary),
        Pill::Canceled => (
            None,
            Some(IconName::CircleSlash),
            c.text_tertiary,
            c.text_tertiary,
        ),
        Pill::InFlight => (
            None,
            Some(IconName::LoaderCircle),
            c.status_info_fg,
            c.text_tertiary,
        ),
        Pill::Delayed => (
            Some(c.status_warning_wash),
            Some(IconName::Timer),
            c.status_warning_fg,
            c.text_primary,
        ),
        Pill::Warning => (
            Some(c.status_warning_wash),
            Some(IconName::CircleAlert),
            c.status_warning_fg,
            c.text_primary,
        ),
        Pill::Auth => (
            Some(c.status_danger_wash),
            Some(IconName::Lock),
            c.status_danger_fg,
            c.text_primary,
        ),
        Pill::Danger => (
            Some(c.status_danger_wash),
            Some(IconName::CircleX),
            c.status_danger_fg,
            c.text_primary,
        ),
        Pill::Offline => (
            Some(c.status_danger_wash),
            Some(IconName::WifiOff),
            c.status_danger_fg,
            c.text_primary,
        ),
    }
}

pub(super) fn status_pill(entry: &Entry, theme: &Theme) -> AnyElement {
    let (wash, icon, glyph_color, text_color) = pill_style(net::pill(entry), theme);
    div()
        .h(px(16.0))
        .px(px(space::S4))
        .flex()
        .items_center()
        .gap(px(space::S2))
        .rounded(px(radius::XS))
        .when_some(wash, |d, wash| d.bg(wash))
        .text_color(text_color)
        .type_style(typography::LABEL, theme)
        .when_some(icon, |d, icon| {
            d.child(Icon::new(icon).size(size::ICON_XS).color(glyph_color))
        })
        .child(status_text(&entry.status))
        .into_any_element()
}

fn cell(width: f32) -> gpui::Div {
    div().flex_none().w(px(width)).min_w_0().truncate()
}

/// The path with a leading ellipsis: the tail names the resource.
fn path_tail(route: &str, max_chars: usize) -> String {
    let count = route.chars().count();
    if count <= max_chars {
        return route.to_owned();
    }
    let tail: String = route.chars().skip(count - max_chars + 1).collect();
    format!("…{tail}")
}

impl ActivityPanel {
    pub(super) fn render_network(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let demo = activity.is_demo();
        let no_token = matches!(activity.sync_state(), Some(crate::sync::SyncState::NoToken));
        let toolbar = self.render_net_toolbar(theme, window, cx);
        let body = if demo {
            EmptyState::new(IconName::ArrowUpDown, "Demo mode sends no requests")
                .body("Everything you see is fake data on this Mac.")
                .into_any_element()
        } else if no_token && self.activity.read(cx).net.held_len() == 0 {
            EmptyState::new(IconName::KeyRound, "No requests")
                .body("adoken has no token, so it sends nothing.")
                .into_any_element()
        } else if self.totals.shown {
            self.render_totals(theme, window, cx)
        } else {
            self.render_requests(theme, window, cx)
        };
        let activity = self.activity.read(cx);
        let kept = activity.net.held_len();
        let since = activity
            .net
            .oldest_at()
            .map(|at| time_text(at, activity.offset(), false)[..5].to_owned());
        let footer_right = if kept >= net::CAPACITY {
            format!(
                "{} of {} kept · cleared when you quit",
                count_text(kept),
                count_text(net::CAPACITY)
            )
        } else {
            match since {
                Some(since) => format!("Kept since {since} · cleared when you quit"),
                None => "Cleared when you quit".to_owned(),
            }
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(toolbar)
            .child(div().flex_1().min_h_0().flex().flex_col().child(body))
            .child(
                // FR-27: always visible.
                div()
                    .flex_none()
                    .h(px(size::ROW_DENSE))
                    .px(px(space::S12))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .border_t_1()
                    .border_color(colors.border_subtle)
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(
                        Icon::new(IconName::Lock)
                            .size(size::ICON_XS)
                            .color(colors.text_tertiary),
                    )
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            "Stays on this Mac. No query strings, bodies, headers or token.",
                        ),
                    )
                    .child(div().flex_none().child(footer_right)),
            )
            .into_any_element()
    }

    fn render_net_toolbar(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let filter = activity.net.filter().clone();
        let total = activity.net.held_len();
        let shown = activity.net.visible().len();
        let unseen = match activity.indicator() {
            super::indicator::IndicatorView::Shown {
                problems: Some(p), ..
            } => p.danger + p.warning,
            _ => 0,
        };
        let filter_focused = self.filter.focus_handle(cx).is_focused(window);
        // Below 1,000 px the status menu and the count give way (the count
        // stays in the table's label).
        let roomy = activity.content_width() >= 1000.0;
        let count = if filter.is_active() {
            format!("{} of {} requests", count_text(shown), count_text(total))
        } else {
            format!("{} requests", count_text(total))
        };
        let origin_label = match filter.origins.as_slice() {
            [] => "All origins".to_owned(),
            [one] => format!("Origin: {}", one.label()),
            many => format!("Origins: {}", many.len()),
        };
        let status_label = match filter.class {
            None => "All statuses".to_owned(),
            Some(class) => format!("Status: {}", class_label(class)),
        };
        let failed_on = filter.failed_only;
        let menu_button =
            |id: &'static str,
             label: String,
             cx: &mut Context<Self>,
             open: fn(&mut Self, &mut Window, &mut Context<Self>)| {
                div()
                    .id(id)
                    .role(gpui::Role::Button)
                    .aria_label(label.clone())
                    .h(px(size::CONTROL_SM))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(colors.border_default)
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .cursor_pointer()
                    .hover(|d| d.bg(colors.bg_hover))
                    .child(label)
                    .child(
                        Icon::new(IconName::ChevronDown)
                            .size(size::ICON_XS)
                            .color(colors.text_tertiary),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| open(this, window, cx)))
            };
        let endpoint_chips: Vec<AnyElement> = filter
            .endpoints
            .iter()
            .enumerate()
            .map(|(ix, (method, route))| {
                let label = format!("Endpoint: {method} {}", path_tail(route, 28));
                div()
                    .id(("activity-endpoint", ix))
                    .role(gpui::Role::Button)
                    .aria_label(format!("{label}, remove"))
                    .h(px(size::CONTROL_SM))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .rounded(px(radius::FULL))
                    .bg(colors.bg_selected_muted)
                    .text_color(colors.text_primary)
                    .type_style(typography::SMALL, theme)
                    .cursor_pointer()
                    .child(label)
                    .child(
                        Icon::new(IconName::X)
                            .size(size::ICON_XS)
                            .color(colors.text_secondary),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.update_filter(
                            |f| {
                                if ix < f.endpoints.len() {
                                    f.endpoints.remove(ix);
                                }
                            },
                            cx,
                        )
                    }))
                    .into_any_element()
            })
            .collect();
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
                Segmented::new(
                    "activity-net-view",
                    ["Requests", "Totals"],
                    usize::from(self.totals.shown),
                )
                .on_select({
                    let weak = cx.entity().downgrade();
                    move |ix, _, cx| {
                        weak.update(cx, |this, cx| this.set_totals(ix == 1, cx))
                            .ok();
                    }
                }),
            )
            .child(
                div()
                    .id("activity-filter")
                    .role(gpui::Role::SearchInput)
                    .aria_label("Filter requests")
                    .w(px(if roomy { 200.0 } else { 140.0 }))
                    .h(px(size::CONTROL_SM))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .rounded(px(radius::SM))
                    .bg(colors.bg_input)
                    .border_1()
                    .border_color(if filter_focused {
                        colors.border_focus
                    } else {
                        colors.border_default
                    })
                    .type_style(typography::SMALL, theme)
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().child(self.filter.clone()))
                    .when(filter.text.is_empty() && !filter_focused, |d| {
                        d.child(Kbd::parse("/").expect("static key"))
                    }),
            )
            .child(
                div()
                    .id("activity-failed-only")
                    .role(gpui::Role::Switch)
                    .aria_label(format!("Failed only, {unseen}"))
                    .aria_toggled(if failed_on {
                        gpui::Toggled::True
                    } else {
                        gpui::Toggled::False
                    })
                    .h(px(size::CONTROL_SM))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .rounded(px(radius::FULL))
                    .border_1()
                    .type_style(typography::SMALL, theme)
                    .cursor_pointer()
                    .map(|d| {
                        if failed_on {
                            d.bg(colors.bg_selected_muted)
                                .border_color(colors.bg_selected_muted)
                                .text_color(colors.text_primary)
                        } else {
                            d.border_color(colors.border_default)
                                .text_color(colors.text_secondary)
                        }
                    })
                    .child(
                        Icon::new(IconName::CircleX)
                            .size(size::ICON_XS)
                            .color(colors.status_danger_fg),
                    )
                    .child("Failed only")
                    .when(unseen > 0, |d| d.child(unseen.to_string()))
                    .tooltip(Tooltip::text("Failed only  F"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_filter(|f| f.failed_only = !f.failed_only, cx)
                    })),
            )
            .child(menu_button(
                "activity-origin",
                origin_label,
                cx,
                Self::open_origin_menu,
            ))
            .when(roomy, |d| {
                d.child(menu_button(
                    "activity-status",
                    status_label,
                    cx,
                    Self::open_status_menu,
                ))
            })
            .children(endpoint_chips)
            .child(div().flex_1())
            .when(roomy, |d| {
                d.child(
                    div()
                        .flex_none()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, theme)
                        .child(count),
                )
            })
            .child(
                div()
                    .id("activity-copy")
                    .role(gpui::Role::Button)
                    .aria_label("Copy request, Command C")
                    .size(px(size::CONTROL_SM))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .cursor_pointer()
                    .hover(|d| d.bg(colors.bg_hover))
                    .child(
                        Icon::new(IconName::Copy)
                            .size(size::ICON_SM)
                            .color(colors.text_secondary),
                    )
                    .tooltip(Tooltip::text("Copy request ⌘C"))
                    .on_click(cx.listener(|this, _, _, cx| this.copy_rows(cx))),
            )
            .child(
                Button::new("activity-export", "Export…")
                    .variant(ButtonVariant::Ghost)
                    .icon(IconName::Download)
                    .on_press(|window, cx| window.dispatch_action(Box::new(ExportNetworkLog), cx)),
            )
    }

    fn render_requests(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let width = activity.content_width() - 2.0 * space::S12;
        let columns = Columns::for_width(width);
        if activity.net.visible().is_empty() {
            let filter = activity.net.filter().clone();
            let empty = if activity.net.held_len() == 0 {
                EmptyState::new(IconName::ArrowUpDown, "No requests yet")
                    .body("Requests to Azure DevOps appear here as adoken makes them.")
            } else {
                let headline = if filter.text.is_empty() {
                    "No requests match".to_owned()
                } else {
                    format!("No requests match \"{}\"", filter.text)
                };
                let mut empty = EmptyState::new(IconName::Search, headline);
                if filter.failed_only && filter.text.is_empty() {
                    let since = activity
                        .net
                        .oldest_at()
                        .map(|at| time_text(at, activity.offset(), false)[..5].to_owned())
                        .unwrap_or_default();
                    empty = empty.body(format!("No failed requests since {since}."));
                }
                empty.action(
                    Button::new("activity-clear-filters", "Clear filters")
                        .shortcut(Kbd::parse(CLEAR_FILTERS_KEYS).expect("static binding"))
                        .on_press(|window, cx| window.dispatch_action(Box::new(ClearFilters), cx)),
                )
            };
            return div().size_full().child(empty).into_any_element();
        }
        let new_count = activity.net.new_count;
        let focused = self.table_focus.is_focused(window);
        let column = |id: &'static str, cell: gpui::Div, label: &'static str| {
            cell.id(id).role(gpui::Role::ColumnHeader).child(label)
        };
        let header = div()
            .id("activity-table-header")
            .flex_none()
            .h(px(size::ROW_DENSE))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .bg(colors.bg_raised)
            .text_color(colors.text_secondary)
            .type_style(typography::LABEL, theme)
            .role(gpui::Role::Row)
            .child(column("col-time", cell(TIME_W), "Time"))
            .child(column("col-method", cell(METHOD_W), "Method"))
            .child(column("col-status", cell(STATUS_W), "Status"))
            .when(columns.origin, |d| {
                d.child(column("col-origin", cell(ORIGIN_W), "Origin"))
            })
            .child(column(
                "col-purpose",
                div().flex_1().min_w(px(0.0)),
                "Purpose",
            ))
            .when(columns.path, |d| {
                d.child(column("col-path", div().flex_1().min_w(px(0.0)), "Path"))
            })
            .child(column(
                "col-latency",
                cell(LATENCY_W).text_right(),
                "Latency",
            ))
            .when(columns.size, |d| {
                d.child(column("col-size", cell(SIZE_W).text_right(), "Size"))
            })
            .when(columns.tstu, |d| {
                d.child(column("col-tstu", cell(TSTU_W).text_right(), "TSTU"))
            });
        let theme_for_rows = theme.clone();
        let rows = list(
            self.table.clone(),
            cx.processor(move |this, ix: usize, window, cx| {
                this.render_request_row(ix, columns, focused, &theme_for_rows, window, cx)
            }),
        )
        .flex_1()
        .min_h_0();
        let total = activity.net.visible().len();
        div()
            .id("activity-table")
            .track_focus(&self.table_focus)
            .role(gpui::Role::Table)
            .aria_label(format!("Requests, {} rows", count_text(total)))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.table_focus, cx)),
            )
            .child(header)
            .child(rows)
            .when(new_count > 0, |d| {
                let label = if new_count == 1 {
                    "1 new request".to_owned()
                } else {
                    format!("{} new requests", count_text(new_count))
                };
                d.child(
                    div()
                        .absolute()
                        .bottom(px(space::S8))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .id("activity-new-pill")
                                .role(gpui::Role::Button)
                                .aria_label(format!("{label}. Jump to latest, End"))
                                .h(px(size::CONTROL_SM))
                                .px(px(space::S8))
                                .flex()
                                .items_center()
                                .gap(px(space::S4))
                                .rounded(px(radius::FULL))
                                .bg(colors.bg_overlay)
                                .shadow(theme.shadows(elevation::LEVEL_2))
                                .text_color(colors.text_primary)
                                .type_style(typography::SMALL, theme)
                                .cursor_pointer()
                                .child(
                                    Icon::new(IconName::ArrowDown)
                                        .size(size::ICON_XS)
                                        .color(colors.text_primary),
                                )
                                .child(label)
                                .on_click(cx.listener(|this, _, _, cx| this.jump_to_latest(cx))),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_request_row(
        &mut self,
        ix: usize,
        columns: Columns,
        focused: bool,
        theme: &Theme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let offset = activity.offset();
        let Some(entry) = activity.net.row(ix) else {
            return div().h(px(size::ROW_DENSE)).into_any_element();
        };
        let seq = entry.seq;
        let selected = activity.net.cursor == Some(seq);
        let open = activity.net.detail == Some(seq);
        let label = format!(
            "{}, row {} of {}",
            net::row_label(entry, offset),
            count_text(ix + 1),
            count_text(activity.net.visible().len())
        );
        let latency = match (entry.status, entry.elapsed_ms) {
            (Status::InFlight, _) => latency_text((activity.now() - entry.at).max(0) as u64),
            (_, Some(ms)) => latency_text(ms),
            _ => "—".into(),
        };
        let subject = entry.subject.clone().map(|s| match s {
            adoken_model::Subject::Pr(id) => format!("!{id}"),
            adoken_model::Subject::WorkItem(id) => format!("#{id}"),
            adoken_model::Subject::Build(id) => format!("run {id}"),
            adoken_model::Subject::Account => String::new(),
        });
        let emphasis = if open {
            colors.text_primary
        } else {
            colors.text_tertiary
        };
        let row = div()
            .id(("activity-request", seq as usize))
            .role(gpui::Role::Row)
            .aria_label(label)
            .aria_selected(selected)
            .w_full()
            .h(px(size::ROW_DENSE))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .type_style(typography::SMALL, theme)
            .when(selected, |d| {
                d.bg(if focused {
                    colors.bg_selected
                } else {
                    colors.bg_selected_muted
                })
            })
            .when(!selected, |d| d.hover(|d| d.bg(colors.bg_hover)))
            .child(
                cell(TIME_W)
                    .text_color(colors.text_tertiary)
                    .child(time_text(entry.at, offset, true)),
            )
            .child(
                cell(METHOD_W)
                    .text_color(colors.text_secondary)
                    .type_style(typography::CODE_INLINE, theme)
                    .child(entry.method),
            )
            .child(cell(STATUS_W).child(status_pill(entry, theme)))
            .when(columns.origin, |d| {
                d.child(
                    cell(ORIGIN_W)
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .text_color(colors.text_secondary)
                        .child(
                            Icon::new(origin_icon(entry.origin.group()))
                                .size(size::ICON_XS)
                                .color(colors.text_secondary),
                        )
                        .child(div().min_w_0().truncate().child(entry.origin.label())),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .overflow_hidden()
                    .when(!columns.origin, |d| {
                        d.child(
                            Icon::new(origin_icon(entry.origin.group()))
                                .size(size::ICON_XS)
                                .color(colors.text_secondary),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(entry.purpose),
                    )
                    .when_some(subject.filter(|s| !s.is_empty()), |d, subject| {
                        d.child(
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .type_style(typography::CODE_INLINE, theme)
                                .child(subject),
                        )
                    }),
            )
            .when(columns.path, |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(emphasis)
                        .type_style(typography::CODE_INLINE, theme)
                        // ~7.2 px per `type.code_inline` character.
                        .child(path_tail(
                            &entry.route,
                            ((columns.path_w / 7.2) as usize).max(8),
                        )),
                )
            })
            .child(
                cell(LATENCY_W)
                    .text_right()
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap(px(space::S2))
                    .text_color(if matches!(entry.status, Status::InFlight) {
                        colors.text_tertiary
                    } else {
                        colors.text_secondary
                    })
                    .when(entry.queued_ms >= 50, |d| {
                        d.child(
                            Icon::new(IconName::Clock)
                                .size(size::ICON_XS)
                                .color(colors.text_tertiary),
                        )
                    })
                    .child(latency),
            )
            .when(columns.size, |d| {
                d.child(
                    cell(SIZE_W)
                        .text_right()
                        .text_color(colors.text_tertiary)
                        .child(size_text(entry.bytes)),
                )
            })
            .when(columns.tstu, |d| {
                d.child(
                    cell(TSTU_W)
                        .text_right()
                        .text_color(colors.text_tertiary)
                        .child(tstu_text(entry.rate.cost)),
                )
            })
            .tooltip(Tooltip::text(format!(
                "{} · {}{} · {}",
                entry.purpose,
                entry.host,
                entry.route,
                status_text(&entry.status)
            )))
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.table_focus, cx);
                this.activity.update(cx, |a, cx| {
                    a.net.cursor = Some(seq);
                    a.net.follow = a.net.visible().last() == Some(&seq);
                    a.net.toggle_detail();
                    cx.notify();
                });
                this.remeasure(cx);
            }));
        if !open {
            return row.into_any_element();
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .child(row)
            .child(self.render_detail(entry.clone(), theme, cx))
            .into_any_element()
    }

    fn render_detail(&self, entry: Entry, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let colors = &theme.colors;
        let activity = self.activity.read(cx);
        let offset = activity.offset();
        let date =
            time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(entry.at) * 1_000_000)
                .map(|t| {
                    let t = t.to_offset(offset);
                    format!("{} {}, {}", month(u8::from(t.month())), t.day(), t.year())
                })
                .unwrap_or_default();
        let rate = &entry.rate;
        let dash = |v: Option<String>| v.unwrap_or_else(|| "—".into());
        let mut lines: Vec<(&'static str, String)> = vec![
            (
                "Started",
                format!("{} · {date}", time_text(entry.at, offset, true)),
            ),
            ("Origin", entry.origin.label()),
            ("Host", entry.host.clone()),
            ("Path", entry.route.clone()),
            ("Attempt", entry.attempt.to_string()),
            (
                "Status",
                match entry.status {
                    Status::Http(code) => format!("{code} {}", reason_phrase(code)),
                    ref other => status_text(other),
                },
            ),
            ("Timing", {
                let mut parts = Vec::new();
                if entry.queued_ms > 0 {
                    parts.push(format!(
                        "waited {} for a slot",
                        latency_text(entry.queued_ms)
                    ));
                }
                parts.push(match entry.elapsed_ms {
                    Some(ms) => format!("{} request", latency_text(ms)),
                    None => "in flight".into(),
                });
                parts.join(" · ")
            }),
            ("Size", size_text(entry.bytes)),
            (
                "Cost",
                match rate.cost {
                    Some(cost) => format!("{cost:.3} TSTU"),
                    None => "— (no X-RateLimit-Cost)".into(),
                },
            ),
        ];
        if let Some(pass) = &entry.pass {
            let pass_text = activity
                .pass(pass.id)
                .map(|p| super::passes::origin_text(p).0)
                .unwrap_or_default();
            lines.push((
                "Pass",
                format!(
                    "Pass {} · {pass_text}",
                    time_text(pass.started, offset, false)
                ),
            ));
        }
        if let Some(error) = &entry.error {
            let mut text = match entry.status {
                Status::Http(401) => "Azure DevOps refused the token (401).".to_owned(),
                Status::Http(403) => "Your token doesn't allow this (403).".to_owned(),
                Status::Http(code) => format!("Azure DevOps returned {code}."),
                Status::Failed { offline: true } => "No response: offline.".to_owned(),
                Status::Failed { offline: false } => "No response.".to_owned(),
                _ => format!("{:?}", error.kind),
            };
            if let Some(code) = &error.code {
                text.push_str(&format!(" {code}"));
            }
            if let Some(message) = &error.message {
                text.push_str(&format!(" {message}"));
            }
            lines.push(("Error", text));
        }
        let has_rate = rate.limit.is_some()
            || rate.remaining.is_some()
            || rate.reset_s.is_some()
            || rate.delay_ms.is_some()
            || rate.resource.is_some();
        if has_rate {
            lines.push((
                "Rate",
                format!(
                    "Limit {} · Remaining {} · Reset {} · Delay {} · Resource {}",
                    dash(rate.limit.map(|v| format!("{v}"))),
                    dash(rate.remaining.map(|v| format!("{v}"))),
                    dash(rate.reset_s.map(|s| time_text(s * 1000, offset, false))),
                    dash(rate.delay_ms.map(|ms| latency_text(ms.max(0) as u64))),
                    dash(rate.resource.clone()),
                ),
            ));
        }
        if let Some(ms) = rate.retry_after_ms {
            lines.push(("Retry-After", latency_text(ms.max(0) as u64)));
        }
        if let Some(id) = &rate.activity_id {
            lines.push(("Activity id", id.clone()));
        }
        let seq = entry.seq;
        let route = (entry.method.to_owned(), entry.route.clone());
        div()
            .id(("activity-detail", seq as usize))
            .role(gpui::Role::Group)
            .aria_label("Request details")
            .ml(px(space::S12))
            .mr(px(space::S12))
            .my(px(space::S4))
            .p(px(space::S12))
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .rounded(px(radius::SM))
            .bg(colors.bg_raised)
            .children(lines.into_iter().enumerate().map(|(ix, (key, value))| {
                div()
                    .id(("detail-line", ix))
                    .flex()
                    .gap(px(space::S8))
                    .role(gpui::Role::Label)
                    .aria_label(format!("{key}, {value}"))
                    .child(
                        div()
                            .flex_none()
                            .w(px(96.0))
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, theme)
                            .child(key),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(colors.text_primary)
                            .type_style(typography::CODE_INLINE, theme)
                            .child(value),
                    )
            }))
            .child(
                div()
                    .pt(px(space::S4))
                    .flex()
                    .gap(px(space::S8))
                    .child(
                        Button::new(("activity-copy-details", seq as usize), "Copy details")
                            .variant(ButtonVariant::Ghost)
                            .icon(IconName::Copy)
                            .shortcut(Kbd::parse("secondary-c").expect("static binding"))
                            .on_press({
                                let weak = cx.entity().downgrade();
                                move |_, cx| {
                                    weak.update(cx, |this, cx| this.copy_rows(cx)).ok();
                                }
                            }),
                    )
                    .child(
                        Button::new(
                            ("activity-show-totals", seq as usize),
                            "Show totals for this endpoint",
                        )
                        .variant(ButtonVariant::Ghost)
                        .on_press({
                            let weak = cx.entity().downgrade();
                            move |_, cx| {
                                let route = route.clone();
                                weak.update(cx, |this, cx| this.show_totals_for(route, cx))
                                    .ok();
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_totals(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        self.refresh_totals(cx);
        let width = self.activity.read(cx).content_width();
        let show_bytes = width >= 900.0;
        let show_mix = width >= 760.0;
        let Some(totals) = self.totals.data.clone() else {
            return div().into_any_element();
        };
        let mut rows = totals.rows.clone();
        net::sort_totals(&mut rows, self.totals.sort, self.totals.ascending);
        let focused = self.table_focus.is_focused(window);
        let window_ix = usize::from(self.totals.window == TotalsWindow::SinceLaunch);
        let group_ix = match self.totals.group {
            Group::Endpoint => 0,
            Group::Origin => 1,
            Group::Tier => 2,
        };
        let weak = cx.entity().downgrade();
        let strip = div()
            .flex_none()
            .h(px(size::CONTROL_MD))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .text_color(colors.text_secondary)
            .type_style(typography::SMALL, theme)
            .child(
                Segmented::new(
                    "activity-totals-window",
                    ["Last hour", "Since launch"],
                    window_ix,
                )
                .on_select({
                    let weak = weak.clone();
                    move |ix, _, cx| {
                        weak.update(cx, |this, cx| {
                            this.totals.window = if ix == 1 {
                                TotalsWindow::SinceLaunch
                            } else {
                                TotalsWindow::LastHour
                            };
                            cx.notify();
                        })
                        .ok();
                    }
                }),
            )
            .child(
                Segmented::new(
                    "activity-totals-group",
                    ["Endpoint", "Origin", "Tier"],
                    group_ix,
                )
                .on_select(move |ix, _, cx| {
                    weak.update(cx, |this, cx| {
                        this.totals.group = match ix {
                            1 => Group::Origin,
                            2 => Group::Tier,
                            _ => Group::Endpoint,
                        };
                        this.totals.cursor = 0;
                        cx.notify();
                    })
                    .ok();
                }),
            )
            .child(div().min_w_0().truncate().child(format!(
                "{} requests · {:.2} TSTU · {} failed",
                count_text(totals.requests),
                totals.tstu,
                count_text(totals.failures)
            )))
            .child(
                div()
                    .flex_none()
                    .text_color(colors.border_default)
                    .child("│"),
            )
            .child(div().flex_none().child(format!(
                "Last 5 min {:.2} TSTU of 200 ({:.2}%)",
                totals.last_5min_tstu, totals.share_of_limit_pct
            )));
        if rows.is_empty() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(strip)
                .child(
                    EmptyState::new(
                        IconName::ArrowUpDown,
                        if self.totals.window == TotalsWindow::LastHour {
                            "No requests in the last hour"
                        } else {
                            "No requests since launch"
                        },
                    )
                    .body("Switch to Since launch to see older totals."),
                )
                .into_any_element();
        }
        let sort_header =
            |label: &'static str, sort: TotalsSort, width: Option<f32>, cx: &mut Context<Self>| {
                let active = self.totals.sort == sort;
                let base = match width {
                    Some(w) => cell(w).flex().justify_end(),
                    None => div().flex_1().min_w(px(200.0)).flex(),
                };
                base.id(label)
                    .role(gpui::Role::ColumnHeader)
                    .items_center()
                    .gap(px(space::S2))
                    .cursor_pointer()
                    .child(label)
                    .when(active, |d| {
                        d.child(
                            Icon::new(if self.totals.ascending {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .size(size::ICON_XS)
                            .color(colors.text_secondary),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.totals.sort == sort {
                            this.totals.ascending = !this.totals.ascending;
                        } else {
                            this.totals.sort = sort;
                            this.totals.ascending = false;
                        }
                        cx.notify();
                    }))
            };
        let header = div()
            .id("activity-totals-header")
            .flex_none()
            .h(px(size::ROW_DENSE))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .bg(colors.bg_raised)
            .text_color(colors.text_secondary)
            .type_style(typography::LABEL, theme)
            .role(gpui::Role::Row)
            .child(
                cell(METHOD_W).child(if self.totals.group == Group::Endpoint {
                    "Method"
                } else {
                    ""
                }),
            )
            .child(sort_header(
                match self.totals.group {
                    Group::Endpoint => "Endpoint",
                    Group::Origin => "Origin",
                    Group::Tier => "Tier",
                },
                TotalsSort::Endpoint,
                None,
                cx,
            ))
            .child(sort_header(
                "Requests",
                TotalsSort::Requests,
                Some(64.0),
                cx,
            ))
            .child(sort_header("Failed", TotalsSort::Failed, Some(64.0), cx))
            .child(cell(64.0).text_right().child("p50"))
            .child(sort_header("p95", TotalsSort::P95, Some(64.0), cx))
            .when(show_bytes, |d| {
                d.child(sort_header("Bytes", TotalsSort::Bytes, Some(72.0), cx))
            })
            .child(sort_header("TSTU", TotalsSort::Tstu, Some(64.0), cx))
            .when(show_mix, |d| d.child(cell(56.0).child("Mix")));
        let cursor = self.totals.cursor.min(rows.len() - 1);
        let body = div()
            .id("activity-totals")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(rows.iter().enumerate().map(|(ix, row)| {
                let selected = ix == cursor;
                let (method, key) = match &row.key {
                    TotalsKey::Endpoint { method, route, .. } => {
                        (method.to_string(), path_tail(route, 60))
                    }
                    other => (String::new(), net::totals_key_text(other)),
                };
                let purpose = match &row.key {
                    TotalsKey::Endpoint { purpose, .. } => purpose.to_string(),
                    _ => String::new(),
                };
                let total = row.requests.max(1) as f32;
                div()
                    .id(("activity-totals-row", ix))
                    .role(gpui::Role::Row)
                    .aria_label(format!(
                        "{method} {key}, {} requests, {} failed, p95 {}, {:.3} TSTU",
                        row.requests,
                        row.failures,
                        latency_text(row.p95_ms),
                        row.tstu
                    ))
                    .aria_selected(selected)
                    .h(px(size::ROW_DENSE))
                    .px(px(space::S12))
                    .flex()
                    .items_center()
                    .gap(px(space::S12))
                    .type_style(typography::SMALL, theme)
                    .when(selected, |d| {
                        d.bg(if focused {
                            colors.bg_selected
                        } else {
                            colors.bg_selected_muted
                        })
                    })
                    .when(!selected, |d| d.hover(|d| d.bg(colors.bg_hover)))
                    .child(
                        cell(METHOD_W)
                            .text_color(colors.text_secondary)
                            .type_style(typography::CODE_INLINE, theme)
                            .child(method),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(colors.text_primary)
                            .type_style(typography::CODE_INLINE, theme)
                            .child(key),
                    )
                    .child(cell(64.0).text_right().child(count_text(row.requests)))
                    .child(
                        cell(64.0)
                            .flex()
                            .justify_end()
                            .items_center()
                            .gap(px(space::S2))
                            .when(row.failures > 0, |d| {
                                d.child(
                                    Icon::new(IconName::CircleX)
                                        .size(size::ICON_XS)
                                        .color(colors.status_danger_fg),
                                )
                                .text_color(colors.text_primary)
                            })
                            .when(row.failures == 0, |d| d.text_color(colors.text_tertiary))
                            .child(count_text(row.failures)),
                    )
                    .child(cell(64.0).text_right().child(latency_text(row.p50_ms)))
                    .child(cell(64.0).text_right().child(latency_text(row.p95_ms)))
                    .when(show_bytes, |d| {
                        d.child(cell(72.0).text_right().child(size_text(Some(row.bytes))))
                    })
                    .child(cell(64.0).text_right().child(format!("{:.3}", row.tstu)))
                    .when(show_mix, |d| {
                        d.child(
                            cell(56.0).child(
                                div()
                                    .h(px(6.0))
                                    .w_full()
                                    .flex()
                                    .rounded(px(radius::FULL))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(row.ok as f32 / total))
                                            .bg(colors.status_success_fg),
                                    )
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(row.warnings as f32 / total))
                                            .bg(colors.status_warning_fg),
                                    )
                                    .child(
                                        div()
                                            .h_full()
                                            .w(gpui::relative(row.dangers as f32 / total))
                                            .bg(colors.status_danger_fg),
                                    ),
                            ),
                        )
                    })
                    .tooltip(Tooltip::text(format!(
                        "{purpose} · {} OK · {} warnings · {} failed",
                        row.ok, row.warnings, row.dangers
                    )))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.table_focus, cx);
                        this.totals.cursor = ix;
                        cx.notify();
                    }))
            }));
        div()
            .id("activity-totals-table")
            .track_focus(&self.table_focus)
            .role(gpui::Role::Table)
            .aria_label("Totals")
            .size_full()
            .flex()
            .flex_col()
            .child(strip)
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// Recompute totals when the log or the grouping changed (< 5 ms over
    /// 10,000 entries, spec §7).
    fn refresh_totals(&mut self, cx: &mut Context<Self>) {
        let activity = self.activity.read(cx);
        let Some(hub) = activity.hub() else {
            return;
        };
        let key = (hub.net.generation(), self.totals.group, self.totals.window);
        if self.totals.generation == Some(key) {
            return;
        }
        self.totals.data = Some(hub.net.totals(
            self.totals.group,
            self.totals.window,
            activity.now(),
        ));
        self.totals.generation = Some(key);
    }

    fn sorted_totals_key(&mut self, cx: &mut Context<Self>) -> Option<TotalsKey> {
        self.refresh_totals(cx);
        let mut rows = self.totals.data.as_ref()?.rows.clone();
        net::sort_totals(&mut rows, self.totals.sort, self.totals.ascending);
        rows.get(self.totals.cursor.min(rows.len().saturating_sub(1)))
            .map(|r| r.key.clone())
    }

    // ── filters ─────────────────────────────────────────────────────────

    pub(super) fn update_filter(
        &mut self,
        change: impl FnOnce(&mut NetFilter),
        cx: &mut Context<Self>,
    ) {
        self.activity.update(cx, |a, cx| {
            a.net.update_filter(change);
            cx.notify();
        });
        self.sync_table(cx);
        cx.notify();
    }

    fn set_totals(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.totals.shown = shown;
        cx.notify();
    }

    fn show_totals_for(&mut self, (method, route): (String, String), cx: &mut Context<Self>) {
        self.totals.shown = true;
        self.totals.group = Group::Endpoint;
        self.refresh_totals(cx);
        if let Some(data) = &self.totals.data {
            let mut rows = data.rows.clone();
            net::sort_totals(&mut rows, self.totals.sort, self.totals.ascending);
            self.totals.cursor = rows
                .iter()
                .position(|r| matches!(&r.key, TotalsKey::Endpoint { method: m, route: r, .. } if *m == method && *r == route))
                .unwrap_or(0);
        }
        cx.notify();
    }

    fn open_origin_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let activity = self.activity.read(cx);
        let current = activity.net.filter().origins.clone();
        let mut labels: Vec<String> = Vec::new();
        for entry in activity.net.held() {
            let label = entry.origin.label();
            if !labels.contains(&label) {
                labels.push(label);
            }
            if labels.len() >= 24 {
                break;
            }
        }
        labels.sort();
        let weak = cx.entity().downgrade();
        let item = |choice: Option<OriginChoice>| {
            let weak = weak.clone();
            let label = match &choice {
                None => "All origins".to_owned(),
                Some(c) => c.label(),
            };
            let on = match &choice {
                None => current.is_empty(),
                Some(c) => current.contains(c),
            };
            MenuItem::new(
                if on { format!("✓ {label}") } else { label },
                move |_, cx| {
                    let choice = choice.clone();
                    weak.update(cx, |this, cx| {
                        this.update_filter(
                            |f| match choice {
                                None => f.origins.clear(),
                                Some(c) => {
                                    if let Some(pos) = f.origins.iter().position(|o| *o == c) {
                                        f.origins.remove(pos);
                                    } else {
                                        f.origins.push(c);
                                    }
                                }
                            },
                            cx,
                        )
                    })
                    .ok();
                },
            )
        };
        let mut items = vec![item(None)];
        for group in [
            OriginGroup::Sync,
            OriginGroup::You,
            OriginGroup::Avatars,
            OriginGroup::Startup,
        ] {
            items.push(item(Some(OriginChoice::Group(group))));
        }
        for label in labels {
            items.push(item(Some(OriginChoice::Label(label))));
        }
        self.menu.open(
            "Origin",
            items,
            MenuPosition::at_pointer(window.mouse_position()),
            adoken_theme::z::DROPDOWN,
            |this: &mut Self| &mut this.menu,
            window,
            cx,
        );
    }

    fn open_status_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let current = self.activity.read(cx).net.filter().class;
        let items = [
            None,
            Some(StatusClass::Success),
            Some(StatusClass::Warning),
            Some(StatusClass::Danger),
            Some(StatusClass::Quiet),
        ]
        .into_iter()
        .map(|class| {
            let weak = weak.clone();
            let label = match class {
                None => "All statuses".to_owned(),
                Some(class) => class_label(class).to_owned(),
            };
            let label = if class == current {
                format!("✓ {label}")
            } else {
                label
            };
            MenuItem::new(label, move |_, cx| {
                weak.update(cx, |this, cx| this.update_filter(|f| f.class = class, cx))
                    .ok();
            })
        })
        .collect();
        self.menu.open(
            "Status",
            items,
            MenuPosition::at_pointer(window.mouse_position()),
            adoken_theme::z::DROPDOWN,
            |this: &mut Self| &mut this.menu,
            window,
            cx,
        );
    }

    // ── copy ────────────────────────────────────────────────────────────

    /// `⌘C`: the selected request as its export line (FR-25); with its
    /// detail open, the detail as `key: value` lines.
    pub(super) fn copy_rows(&mut self, cx: &mut Context<Self>) {
        let activity = self.activity.read(cx);
        let Some(seq) = activity.net.cursor else {
            return;
        };
        let Some(entry) = activity.net.entry(seq) else {
            return;
        };
        let (text, toast) = if activity.net.detail == Some(seq) {
            (
                net::details_text(entry, activity.offset()),
                "Copied request details",
            )
        } else {
            (adoken_sync::ledger::copy_line(entry), "Copied request")
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        ToastStack::show(Toast::new("activity-copy", IconName::Copy, toast), cx);
    }

    fn jump_to_latest(&mut self, cx: &mut Context<Self>) {
        self.activity.update(cx, |a, cx| {
            a.net.jump_to_latest();
            cx.notify();
        });
        self.follow_tail(cx);
        cx.notify();
    }

    fn net_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.totals.shown {
            let len = self.totals.data.as_ref().map_or(0, |t| t.rows.len());
            if len > 0 {
                self.totals.cursor =
                    (self.totals.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
            }
            cx.notify();
            return;
        }
        let follow = self.activity.update(cx, |a, cx| {
            a.net.move_cursor(delta);
            cx.notify();
            a.net.follow
        });
        if follow {
            self.follow_tail(cx);
        } else {
            self.reveal_cursor(cx);
        }
        cx.notify();
    }

    // ── actions (both tabs) ─────────────────────────────────────────────

    fn tab(&self, cx: &Context<Self>) -> Tab {
        self.activity.read(cx).tab()
    }

    pub(super) fn on_select_next(
        &mut self,
        _: &SelectNext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(1, window, cx),
            Tab::Network => self.net_move(1, cx),
        }
    }

    pub(super) fn on_select_previous(
        &mut self,
        _: &SelectPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(-1, window, cx),
            Tab::Network => self.net_move(-1, cx),
        }
    }

    pub(super) fn on_select_first(
        &mut self,
        _: &SelectFirst,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(-10_000, window, cx),
            Tab::Network if self.totals.shown => self.net_move(-10_000, cx),
            Tab::Network => {
                self.activity.update(cx, |a, cx| {
                    a.net.home();
                    cx.notify();
                });
                self.reveal_cursor(cx);
                cx.notify();
            }
        }
    }

    pub(super) fn on_select_last(
        &mut self,
        _: &SelectLast,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(10_000, window, cx),
            Tab::Network if self.totals.shown => self.net_move(10_000, cx),
            Tab::Network => self.jump_to_latest(cx),
        }
    }

    pub(super) fn on_page_down(
        &mut self,
        _: &PageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(PAGE, window, cx),
            Tab::Network => self.net_move(PAGE, cx),
        }
    }

    pub(super) fn on_page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        match self.tab(cx) {
            Tab::Sync => self.sync_move(-PAGE, window, cx),
            Tab::Network => self.net_move(-PAGE, cx),
        }
    }

    pub(super) fn on_confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        match self.tab(cx) {
            Tab::Sync => self.sync_confirm(window, cx),
            Tab::Network if self.totals.shown => {
                // "Show requests": the Requests view with the endpoint chip.
                if let Some(TotalsKey::Endpoint { method, route, .. }) = self.sorted_totals_key(cx)
                {
                    self.totals.shown = false;
                    self.update_filter(
                        |f| f.endpoints = vec![(method.to_string(), route.clone())],
                        cx,
                    );
                }
            }
            Tab::Network => {
                self.activity.update(cx, |a, cx| {
                    a.net.toggle_detail();
                    cx.notify();
                });
                self.remeasure(cx);
                self.reveal_cursor(cx);
            }
        }
    }

    pub(super) fn on_right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        match self.tab(cx) {
            Tab::Sync => self.sync_expand(true, window, cx),
            Tab::Network => self.set_totals(true, cx),
        }
    }

    pub(super) fn on_left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        match self.tab(cx) {
            Tab::Sync => self.sync_expand(false, window, cx),
            Tab::Network => self.set_totals(false, cx),
        }
    }

    /// `⌘↩` on a changed item (Sync tab): its Azure DevOps page.
    pub(super) fn on_open_in_browser(
        &mut self,
        _: &OpenInBrowser,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab(cx) != Tab::Sync || self.pane(window) != super::panel::Pane::Passes {
            return;
        }
        let rows = super::sync_tab::pass_rows(self.activity.read(cx));
        if let Some(super::sync_tab::PassRow::Item { kind, id, .. }) = rows.get(self.pass_cursor) {
            let (kind, id) = (*kind, *id);
            self.activity
                .update(cx, |a, cx| a.open_in_browser(kind, id, cx));
        }
    }

    /// `⌘K`: the row menu (activity.md §6.2).
    pub(super) fn on_row_menu(
        &mut self,
        _: &crate::pin_actions::OpenRowMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let act = |f: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let weak = weak.clone();
            move |window: &mut Window, cx: &mut gpui::App| {
                weak.update(cx, |this, cx| f(this, window, cx)).ok();
            }
        };
        let (title, items): (&str, Vec<MenuItem>) = match self.tab(cx) {
            Tab::Sync if self.pane(window) == super::panel::Pane::Passes => {
                let rows = super::sync_tab::pass_rows(self.activity.read(cx));
                match rows.get(self.pass_cursor) {
                    Some(super::sync_tab::PassRow::Item { .. }) => (
                        "Changed item",
                        vec![
                            MenuItem::new(
                                "Open",
                                act(|this, window, cx| this.sync_confirm(window, cx)),
                            )
                            .keys("enter"),
                            MenuItem::new(
                                "Open in browser",
                                act(|this, window, cx| {
                                    this.on_open_in_browser(&OpenInBrowser, window, cx)
                                }),
                            )
                            .keys("secondary-enter"),
                        ],
                    ),
                    _ => (
                        "Pass",
                        vec![
                            MenuItem::new(
                                "Expand or collapse",
                                act(|this, window, cx| this.sync_confirm(window, cx)),
                            )
                            .keys("enter"),
                        ],
                    ),
                }
            }
            Tab::Sync => (
                "Kind",
                vec![MenuItem::new(
                    "Sync settings for this kind…",
                    |window, cx| {
                        window.dispatch_action(Box::new(crate::sync::OpenSyncSettings), cx)
                    },
                )],
            ),
            Tab::Network if self.totals.shown => (
                "Endpoint totals",
                vec![
                    MenuItem::new(
                        "Show requests",
                        act(|this, window, cx| this.on_confirm(&Confirm, window, cx)),
                    )
                    .keys("enter"),
                    MenuItem::new(
                        "Sort by TSTU",
                        act(|this, _, cx| this.sort_totals(TotalsSort::Tstu, cx)),
                    ),
                    MenuItem::new(
                        "Sort by requests",
                        act(|this, _, cx| this.sort_totals(TotalsSort::Requests, cx)),
                    ),
                    MenuItem::new(
                        "Sort by failed",
                        act(|this, _, cx| this.sort_totals(TotalsSort::Failed, cx)),
                    ),
                    MenuItem::new(
                        "Sort by p95",
                        act(|this, _, cx| this.sort_totals(TotalsSort::P95, cx)),
                    ),
                    MenuItem::new(
                        "Sort by bytes",
                        act(|this, _, cx| this.sort_totals(TotalsSort::Bytes, cx)),
                    ),
                    MenuItem::new(
                        "Sort by endpoint",
                        act(|this, _, cx| this.sort_totals(TotalsSort::Endpoint, cx)),
                    ),
                ],
            ),
            Tab::Network => (
                "Request",
                vec![
                    MenuItem::new("Copy", act(|this, _, cx| this.copy_rows(cx)))
                        .keys("secondary-c"),
                    MenuItem::new(
                        "Copy details",
                        act(|this, _, cx| {
                            this.activity.update(cx, |a, _| {
                                if a.net.detail != a.net.cursor {
                                    a.net.toggle_detail();
                                }
                            });
                            this.copy_rows(cx);
                        }),
                    ),
                    MenuItem::new(
                        "Show totals for this endpoint",
                        act(|this, _, cx| {
                            if let Some(route) = this.cursor_endpoint(cx) {
                                this.show_totals_for(route, cx);
                            }
                        }),
                    ),
                    MenuItem::new(
                        "Filter to this endpoint",
                        act(|this, _, cx| {
                            if let Some(route) = this.cursor_endpoint(cx) {
                                this.update_filter(|f| f.endpoints = vec![route], cx);
                            }
                        }),
                    ),
                ],
            ),
        };
        let slot: adoken_ui::action_menu::BoundsSlot = Default::default();
        let position = adoken_ui::action_menu::keyboard_position(&slot, window);
        self.menu.open(
            title,
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this: &mut Self| &mut this.menu,
            window,
            cx,
        );
    }

    fn sort_totals(&mut self, sort: TotalsSort, cx: &mut Context<Self>) {
        self.totals.sort = sort;
        self.totals.ascending = sort == TotalsSort::Endpoint;
        cx.notify();
    }

    fn cursor_endpoint(&self, cx: &Context<Self>) -> Option<(String, String)> {
        let activity = self.activity.read(cx);
        let entry = activity.net.entry(activity.net.cursor?)?;
        Some((entry.method.to_owned(), entry.route.clone()))
    }

    pub(super) fn on_toggle_failed(
        &mut self,
        _: &ToggleFailedOnly,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab(cx) == Tab::Network {
            self.update_filter(|f| f.failed_only = !f.failed_only, cx);
        }
    }

    pub(super) fn on_toggle_totals(
        &mut self,
        _: &ToggleTotals,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab(cx) == Tab::Network {
            let shown = !self.totals.shown;
            self.set_totals(shown, cx);
        }
    }

    pub(super) fn on_focus_filter(
        &mut self,
        _: &FocusFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab(cx) == Tab::Network {
            self.totals.shown = false;
            window.focus(&self.filter.focus_handle(cx), cx);
            cx.notify();
        }
    }

    pub(super) fn on_copy_rows(&mut self, _: &CopyRows, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab(cx) == Tab::Network {
            self.copy_rows(cx);
        }
    }

    pub(super) fn on_clear_filters(
        &mut self,
        _: &ClearFilters,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_filters(cx);
    }

    pub fn clear_filters(&mut self, cx: &mut Context<Self>) {
        self.filter.update(cx, |input, cx| input.set_text("", cx));
        self.update_filter(|f| *f = NetFilter::default(), cx);
    }
}

fn class_label(class: StatusClass) -> &'static str {
    match class {
        StatusClass::Success => "Success",
        StatusClass::Warning => "Warning",
        StatusClass::Danger => "Failed",
        StatusClass::Quiet => "Canceled or in flight",
    }
}

fn month(m: u8) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][usize::from(m.clamp(1, 12)) - 1]
}

/// HTTP reason phrases for the codes Azure DevOps answers.
fn reason_phrase(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        203 => "Non-Authoritative (sign-in page)",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        412 => "Precondition Failed",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_drop_in_the_design_order() {
        // 900 px window with the sidebar: 668 − 24 = 644 px of table:
        // Time + Method + Status + Latency = 272, five gaps = 60, so
        // Purpose and Path share 312 (activity.md §5.2).
        let narrow = Columns::for_width(644.0);
        assert!(narrow.path && !narrow.origin && !narrow.size && !narrow.tstu);
        assert_eq!(narrow.purpose_w + narrow.path_w, 312.0);
        assert!(narrow.purpose_w >= 120.0);
        let wide = Columns::for_width(1200.0);
        assert!(wide.origin && wide.path && wide.size && wide.tstu);
        assert!(!Columns::for_width(580.0).path);
    }

    #[test]
    fn paths_keep_their_tail() {
        let route = "/{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{n}/threads";
        let tail = path_tail(route, 30);
        assert!(tail.starts_with('…') && tail.ends_with("/pullRequests/{n}/threads"));
        assert_eq!(tail.chars().count(), 30);
        assert_eq!(
            path_tail("/{org}/_apis/projects", 30),
            "/{org}/_apis/projects"
        );
    }
}
