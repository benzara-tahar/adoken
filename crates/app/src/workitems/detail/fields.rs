//! Fields (design §4): the 2 × 2 property grid (or one column when cells
//! would drop under 240 px), the properties rail on wide panes, values with
//! avatars and breadcrumb paths, and the Created / Changed meta line.

use adoken_model::{ClassNode, WorkItem};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::picker::{DayRange, TreeNode};
use adoken_ui::{ActiveTheme, Icon, IconName, Theme, Tooltip, TypeStyleExt};
use gpui::{AnyElement, Context, Div, SharedString, Window, div, prelude::*, px};

use super::model::{self, Crumb, Field, Stop};
use super::status::stop_frame;
use super::{Intent, stop_key};
use crate::review::data::age;
use crate::workitems::WorkItemsView;
use crate::workitems::writes::{Field as EditField, Gate};

/// Characters a breadcrumb keeps before collapsing its middle, per px of
/// cell width (`type.body` averages ~7 px per character).
const PX_PER_CHAR: f32 = 7.0;
/// Rail rows: label column (design §2.3).
const RAIL_LABEL: f32 = 96.0;

/// Pre-ordered classification nodes → the tree picker's nodes (parents
/// before children; node 0 is the root).
pub(super) fn tree_nodes(nodes: &[ClassNode]) -> Vec<TreeNode> {
    let mut out: Vec<TreeNode> = Vec::with_capacity(nodes.len());
    let mut index: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::with_capacity(nodes.len());
    for node in nodes {
        let parent = node.parent_path().and_then(|p| index.get(p).copied());
        let day = |ms: i64| ms.div_euclid(adoken_model::time::DAY);
        let dates = node
            .start_ms
            .zip(node.finish_ms)
            .map(|(start, finish)| DayRange {
                start: day(start),
                finish: day(finish),
            });
        index.insert(node.path.as_str(), out.len());
        out.push(TreeNode {
            name: node.name.clone().into(),
            parent,
            dates,
        });
    }
    out
}

impl WorkItemsView {
    /// A path as breadcrumbs (design §4.3) sized for `width` px.
    fn breadcrumb(&self, path: &str, width: f32, theme: &Theme) -> Div {
        let colors = &theme.colors;
        let max_chars = (width / PX_PER_CHAR).max(8.0) as usize;
        let crumbs = model::breadcrumb(path, max_chars);
        let mut row = div().flex().items_center().min_w_0().overflow_hidden();
        for (ix, crumb) in crumbs.iter().enumerate() {
            if ix > 0 {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(colors.text_tertiary)
                        .child(model::SEPARATOR),
                );
            }
            row = row.child(match crumb {
                Crumb::Ancestor(text) => div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .child(text.clone()),
                Crumb::Ellipsis => div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .child("…"),
                Crumb::Leaf(text) => div()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_primary)
                    .child(text.clone()),
            });
        }
        row
    }

    /// One field's value and spoken text (design §4.2).
    fn field_value(
        &self,
        item: &WorkItem,
        project: &str,
        field: Field,
        width: f32,
        cx: &Context<Self>,
    ) -> (AnyElement, String) {
        let theme = cx.theme();
        let colors = &theme.colors;
        let icon = |name: IconName| {
            Icon::new(name)
                .size(size::ICON_MD)
                .color(colors.text_tertiary)
        };
        let row = || div().flex().items_center().gap(px(space::S6)).min_w_0();
        match field {
            Field::AssignedTo => {
                let pending = self.pending_value(EditField::AssignedTo, cx);
                let (id, name) = match (&pending, &item.assigned_to) {
                    (Some(name), _) if name.is_empty() => (None, None),
                    (Some(name), _) => (self.person_id_by_name(name), Some(name.clone())),
                    (None, Some(id)) => (Some(id.clone()), Some(self.list.name(id))),
                    (None, None) => (None, None),
                };
                match name {
                    Some(name) => (
                        row()
                            .child(
                                crate::avatar::Avatar::new(
                                    id.clone().unwrap_or_else(|| name.clone()),
                                    name.clone(),
                                )
                                .size(crate::avatar::AvatarSize::Md),
                            )
                            .child(div().min_w_0().truncate().child(name.clone()))
                            .into_any_element(),
                        name,
                    ),
                    None => (
                        row()
                            .text_color(colors.text_tertiary)
                            .child(icon(IconName::User))
                            .child("Unassigned")
                            .into_any_element(),
                        "Unassigned".into(),
                    ),
                }
            }
            Field::Project => (
                row()
                    .child(icon(IconName::FolderKanban))
                    .child(div().min_w_0().truncate().child(project.to_owned()))
                    .into_any_element(),
                project.to_owned(),
            ),
            Field::Area => {
                let path = self
                    .pending_value(EditField::Area, cx)
                    .unwrap_or_else(|| item.area_path.clone());
                (
                    row()
                        .child(icon(IconName::FolderTree))
                        .child(self.breadcrumb(&path, width - 40.0, theme))
                        .into_any_element(),
                    model::spoken_path(&path),
                )
            }
            Field::Iteration => {
                let path = self
                    .pending_value(EditField::Iteration, cx)
                    .unwrap_or_else(|| item.iteration_path.clone());
                let is_root = !path.contains('\\');
                let leaf = model::leaf(&path).to_owned();
                let (dates, current) = self.iteration_dates(item, &path, cx);
                let value = if is_root {
                    row()
                        .child(icon(IconName::CalendarRange))
                        .child(div().min_w_0().truncate().child(leaf.clone()))
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_tertiary)
                                .child("· No sprint"),
                        )
                } else {
                    row()
                        .child(icon(IconName::CalendarRange))
                        .child(div().min_w_0().truncate().child(leaf.clone()))
                        .children(dates.clone().map(|d| {
                            div()
                                .flex_none()
                                .text_color(colors.text_tertiary)
                                .child(format!("· {d}"))
                        }))
                        .when(current, |r| {
                            r.child(
                                div()
                                    .flex_none()
                                    .h(px(space::S16))
                                    .px(px(space::S4))
                                    .flex()
                                    .items_center()
                                    .rounded(px(radius::XS))
                                    .bg(colors.bg_active)
                                    .text_color(colors.text_secondary)
                                    .type_style(typography::LABEL, theme)
                                    .child("Current"),
                            )
                        })
                };
                let spoken = format!(
                    "{}{}{}",
                    model::spoken_path(&path),
                    dates.map(|d| format!(", {d}")).unwrap_or_default(),
                    if current { ", current" } else { "" }
                );
                (value.into_any_element(), spoken)
            }
        }
    }

    fn person_id_by_name(&self, name: &str) -> Option<String> {
        self.list
            .people
            .iter()
            .find(|p| p.display_name == name)
            .map(|p| p.id.clone())
    }

    /// "Sep 29 – Oct 10" and whether today is inside, when the project's
    /// iteration tree is cached with dates.
    fn iteration_dates(
        &self,
        item: &WorkItem,
        path: &str,
        cx: &Context<Self>,
    ) -> (Option<String>, bool) {
        let Some(tree) = crate::workitems::writes::cached_tree(&item.project_id, cx) else {
            return (None, false);
        };
        let Some(node) = tree.iterations.iter().find(|n| n.path == path) else {
            return (None, false);
        };
        let (Some(start), Some(finish)) = (node.start_ms, node.finish_ms) else {
            return (None, false);
        };
        let now = self.now();
        let day = adoken_model::time::DAY;
        let current = start <= now && now < finish + day;
        (
            Some(format!("{} – {}", month_day(start), month_day(finish))),
            current,
        )
    }

    /// One editable property (grid cell or rail row).
    #[allow(clippy::too_many_arguments)]
    fn render_cell(
        &self,
        item: &WorkItem,
        project: &str,
        field: Field,
        rail: bool,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let gate = self.gate(cx);
        // Project moves in slice C (v0.2): shown, not edited.
        let editable = field != Field::Project && matches!(gate, Gate::Editable | Gate::Offline);
        let cursor =
            self.ui.cursor == Some(Stop::Field(field)) && self.detail_focus.is_focused(window);
        let open = match field {
            Field::AssignedTo => self.ui.people.is_open(),
            Field::Area | Field::Iteration => {
                self.ui.tree.is_open() && self.ui.tree_field == Some(field)
            }
            Field::Project => false,
        };
        let (value, spoken) = self.field_value(item, project, field, width, cx);
        let label = model::cell_label(field, &spoken, editable);
        let tip = model::cell_tooltip(field);
        let chevron = div()
            .flex_none()
            .opacity(if cursor || open { 1.0 } else { 0.0 })
            .group_hover(SharedString::from(format!("wi-cell-{field:?}")), |s| {
                s.opacity(1.0)
            })
            .child(
                Icon::new(if open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .size(size::ICON_XS)
                .color(colors.text_tertiary),
            );
        let label_el = div()
            .flex_none()
            .text_color(colors.text_tertiary)
            .type_style(
                if rail {
                    typography::SMALL
                } else {
                    typography::LABEL
                },
                &theme,
            )
            .child(field.label());
        let body = if rail {
            div()
                .min_h(px(size::CONTROL_LG))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(label_el.w(px(RAIL_LABEL)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .type_style(typography::BODY, &theme)
                        .child(value),
                )
                .when(editable, |b| b.child(chevron))
        } else {
            div()
                .min_h(px(size::PROPERTY_CELL))
                .px(px(space::S12))
                .py(px(space::S8))
                .flex()
                .flex_col()
                .gap(px(space::S2))
                .child(label_el)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S6))
                        .type_style(typography::BODY, &theme)
                        .text_color(colors.text_primary)
                        .child(div().flex_1().min_w_0().child(value))
                        .when(editable, |b| b.child(chevron)),
                )
        };
        let cell = div()
            .id(format!("wi-stop-{}", stop_key(Stop::Field(field))))
            .group(SharedString::from(format!("wi-cell-{field:?}")))
            .child(adoken_ui::action_menu::track_bounds(
                self.ui.anchors.cell(field).clone(),
            ))
            .child(body)
            .when(open, |c| c.bg(colors.bg_selected_muted));
        let cell = stop_frame(cell, cursor, editable, &theme)
            .role(gpui::Role::Button)
            .aria_label(label)
            .aria_expanded(open)
            .tooltip(Tooltip::text(if gate == Gate::Offline && editable {
                "Offline — reconnect to send"
            } else {
                tip
            }));
        let cell = if editable {
            let intent = match field {
                Field::AssignedTo => Intent::Assign,
                Field::Area => Intent::Area,
                _ => Intent::Iteration,
            };
            cell.active(|s| s.bg(colors.bg_active))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.ui.from_list = false;
                    this.intend(intent, window, cx)
                }))
        } else {
            cell
        };
        let status = match field {
            Field::AssignedTo => self.status_lines(EditField::AssignedTo, &theme, cx),
            Field::Area => self.status_lines(EditField::Area, &theme, cx),
            Field::Iteration => self.status_lines(EditField::Iteration, &theme, cx),
            Field::Project => None,
        };
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .child(cell)
            .children(status.map(|s| div().px(px(space::S12)).pb(px(space::S8)).child(s)))
            .into_any_element()
    }

    /// The property grid between hairlines (design §4.1).
    pub(in crate::workitems) fn render_grid(
        &self,
        item: &WorkItem,
        project: &str,
        columns: usize,
        width: f32,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let cell_width = if columns == 2 {
            (width - model::GRID_GAP) / 2.0
        } else {
            width
        };
        let cells: Vec<AnyElement> = Field::GRID
            .iter()
            .map(|field| self.render_cell(item, project, *field, false, cell_width, window, cx))
            .collect();
        let mut rows = div()
            .id("wi-fields")
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .py(px(space::S4))
            .border_t_1()
            .border_b_1()
            .border_color(colors.border_subtle)
            .max_w(px(size::DETAIL_PROSE_MAX))
            .role(gpui::Role::Group)
            .aria_label("Fields");
        let mut cells = cells.into_iter();
        while let Some(first) = cells.next() {
            let mut row = div()
                .flex()
                .gap(px(space::S8))
                .child(div().flex_1().min_w_0().child(first));
            if columns == 2
                && let Some(second) = cells.next()
            {
                row = row.child(div().flex_1().min_w_0().child(second));
            }
            rows = rows.child(row);
        }
        rows.into_any_element()
    }

    /// The properties rail (design §2.3): state, the four fields, then
    /// Created / Changed below a hairline.
    pub(in crate::workitems) fn render_rail(
        &self,
        item: &WorkItem,
        project: &str,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let width = size::DETAIL_PROPERTIES - 2.0 * space::S16 - RAIL_LABEL;
        let meta_row = |label: &'static str, text: String| {
            div()
                .min_h(px(size::CONTROL_LG))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(
                    div()
                        .w(px(RAIL_LABEL))
                        .flex_none()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, &theme)
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, &theme)
                        .child(text),
                )
        };
        let now = self.now();
        let mut rail = div()
            .id("wi-rail")
            .w(px(size::DETAIL_PROPERTIES))
            .flex_none()
            .p(px(space::S16))
            .border_l_1()
            .border_color(colors.border_subtle)
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .role(gpui::Role::Group)
            .aria_label("Fields")
            .child(
                div()
                    .min_h(px(size::CONTROL_LG))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        div()
                            .w(px(RAIL_LABEL))
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child("State"),
                    )
                    .child(self.render_state_chip(item, window, cx)),
            )
            .children(self.status_lines(EditField::State, &theme, cx));
        for field in Field::GRID {
            rail = rail.child(self.render_cell(item, project, field, true, width, window, cx));
        }
        rail.child(
            div()
                .my(px(space::S8))
                .h(px(size::BORDER))
                .bg(colors.border_subtle),
        )
        .child(meta_row(
            "Created",
            format!(
                "{} · {}",
                age(now, item.created),
                self.list.name(&item.created_by)
            ),
        ))
        .child(meta_row(
            "Changed",
            format!(
                "{} · {}",
                age(now, item.changed),
                self.list.name(&item.changed_by)
            ),
        ))
        .into_any_element()
    }

    /// "Created Apr 22 by Kwame Mensah · Changed 4d ago by Mei Tanaka".
    pub(in crate::workitems) fn render_meta(
        &self,
        item: &WorkItem,
        cx: &Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let now = self.now();
        let created = format!(
            "Created {} by {}",
            age(now, item.created),
            self.list.name(&item.created_by)
        );
        let changed = format!(
            "Changed {} by {}",
            changed_age(now, item.changed),
            self.list.name(&item.changed_by)
        );
        div()
            .id("wi-meta")
            .pt(px(space::S8))
            .text_color(theme.colors.text_tertiary)
            .type_style(typography::SMALL, theme)
            .role(gpui::Role::Label)
            .aria_label(format!("{created}. {changed}"))
            .child(format!("{created} · {changed}"))
            .into_any_element()
    }
}

/// "just now", "4d ago" for recent changes, the date for older ones.
fn changed_age(now: i64, at: i64) -> String {
    let text = age(now, at);
    if text == "now" {
        "just now".to_owned()
    } else if text.chars().last().is_some_and(|c| c.is_ascii_alphabetic()) && text.len() <= 4 {
        format!("{text} ago")
    } else {
        text
    }
}

/// "Sep 29" from epoch milliseconds (UTC).
pub(super) fn month_day(ms: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = ms.div_euclid(adoken_model::time::DAY);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {day}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::NodeKind;

    fn node(path: &str, start: Option<i64>, finish: Option<i64>) -> ClassNode {
        ClassNode {
            kind: NodeKind::Iteration,
            path: path.into(),
            name: model::leaf(path).into(),
            depth: path.matches('\\').count() as u16,
            start_ms: start,
            finish_ms: finish,
        }
    }

    #[test]
    fn classification_nodes_keep_parent_links_and_day_ranges() {
        let day = adoken_model::time::DAY;
        let nodes = tree_nodes(&[
            node("Storefront", None, None),
            node("Storefront\\Sprint 41", Some(10 * day), Some(21 * day)),
            node(
                "Storefront\\Sprint 41\\Week 1",
                Some(10 * day),
                Some(14 * day),
            ),
        ]);
        assert_eq!(nodes[0].parent, None);
        assert_eq!(nodes[1].parent, Some(0));
        assert_eq!(nodes[2].parent, Some(1));
        assert_eq!(
            nodes[1].dates,
            Some(DayRange {
                start: 10,
                finish: 21
            })
        );
    }

    #[test]
    fn month_day_formats_utc_dates() {
        // 2026-09-29T00:00:00Z.
        assert_eq!(month_day(1_790_640_000_000), "Sep 29");
        assert_eq!(month_day(0), "Jan 1");
    }
}
