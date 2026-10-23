//! Links (design §6, spec FR-21..FR-23): rows grouped by relation in a
//! fixed order, five per group until "Show {n} more", the Add link row,
//! the remove button on work item rows, the Add link work item picker
//! (spec 0004 FR-17's engine) and the Remove link confirmation.

use adoken_model::PrStatus;
use adoken_theme::{elevation, radius, size, space, typography};
use adoken_ui::{ActiveTheme, Icon, IconName, Theme, Tooltip, TypeStyleExt};
use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px, uniform_list};

use super::model::{self, LinkGroup, Stop};
use super::status::{ghost, kbd, section, section_head, stop_frame};
use super::{Intent, LINK_KEYS, OVERLAY_CONTEXT, PICKER_VISIBLE_ROWS, Removal, stop_key};
use crate::workitems::data::{DetailData, LinkRow, LinkTarget};
use crate::workitems::pick::PickRow;
use crate::workitems::writes::{Field as EditField, Gate};
use crate::workitems::{WorkItemsView, state_tone, type_style};

const PICKER_WIDTH: f32 = 560.0;
const PICKER_ROW: f32 = size::PALETTE_ROW_COMPACT;
const CONFIRM_WIDTH: f32 = 440.0;
/// The state word's max width in a link row (design §6.1).
const STATE_WORD_MAX: f32 = 96.0;

/// Link rows on screen: indices into [`DetailData::links`] in display
/// order, and the groups cut at five with the row their "Show more"
/// follows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::workitems) struct Shown {
    pub rows: Vec<usize>,
    pub more: Vec<(usize, LinkGroup)>,
}

/// Which link rows show: every group's first five unless expanded.
pub(super) fn shown(links: &[LinkRow], expanded: &std::collections::HashSet<LinkGroup>) -> Shown {
    let mut out = Shown::default();
    let mut ix = 0;
    while ix < links.len() {
        let group = links[ix].group;
        let end = links[ix..]
            .iter()
            .position(|r| r.group != group)
            .map_or(links.len(), |n| ix + n);
        let limit = if expanded.contains(&group) {
            end - ix
        } else {
            model::LINK_GROUP_SHOWN
        };
        let take = (end - ix).min(limit);
        out.rows.extend(ix..ix + take);
        if take < end - ix {
            out.more.push((out.rows.len() - 1, group));
        }
        ix = end;
    }
    out
}

fn pr_style(status: PrStatus, draft: bool, theme: &Theme) -> (IconName, gpui::Hsla) {
    let colors = &theme.colors;
    match (status, draft) {
        (PrStatus::Active, true) => (IconName::GitPullRequestDraft, colors.pr_draft),
        (PrStatus::Active, false) => (IconName::GitPullRequest, colors.pr_active),
        (PrStatus::Completed, _) => (IconName::GitMerge, colors.pr_completed),
        (PrStatus::Abandoned, _) => (IconName::GitPullRequestClosed, colors.pr_abandoned),
    }
}

impl WorkItemsView {
    pub(in crate::workitems) fn shown_link_rows(&self, detail: &DetailData) -> Shown {
        shown(&detail.links, &self.ui.expanded_groups)
    }

    pub(in crate::workitems) fn render_links(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self.detail().expect("rendered with a detail");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let gate = self.gate(cx);
        let writable = matches!(gate, Gate::Editable | Gate::Offline);
        let focused = self.detail_focus.is_focused(window);
        let shown = self.shown_link_rows(detail);
        let mut body = section("wi-links")
            .role(gpui::Role::List)
            .aria_label(format!("Links, {}", detail.links.len()))
            .child(section_head(
                "Links",
                (!detail.links.is_empty()).then_some(detail.links.len()),
                &theme,
            ));
        if detail.partly_synced() && detail.links.is_empty() {
            body = body.child(super::description::skeleton(&theme));
        } else if detail.links.is_empty() {
            body = body.child(
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::BODY, &theme)
                    .child("No links."),
            );
        }
        let mut last_group = None;
        for (row, &ix) in shown.rows.iter().enumerate() {
            let link = &detail.links[ix];
            if last_group != Some(link.group) {
                last_group = Some(link.group);
                let count = detail
                    .links
                    .iter()
                    .filter(|r| r.group == link.group)
                    .count();
                body = body.child(
                    div()
                        .h(px(size::CONTROL_SM))
                        .flex()
                        .items_center()
                        .gap(px(space::S4))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::LABEL, &theme)
                        .child(link.group.label())
                        .when(count > 1, |d| d.child(count.to_string())),
                );
            }
            let cursor = focused && self.ui.cursor == Some(Stop::Link(row));
            body = body.child(self.render_link_row(row, ix, link, cursor, writable, &theme, cx));
            for &(after, group) in &shown.more {
                if after == row {
                    let hidden = detail.links.iter().filter(|r| r.group == group).count()
                        - model::LINK_GROUP_SHOWN;
                    let stop = Stop::LinkShowMore(group);
                    body = body.child(
                        stop_frame(
                            ghost(
                                format!("wi-stop-{}", stop_key(stop)),
                                None,
                                Some(format!("Show {hidden} more").into()),
                                None,
                                false,
                                &theme,
                            ),
                            focused && self.ui.cursor == Some(stop),
                            false,
                            &theme,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.ui.expanded_groups.insert(group);
                            cx.notify();
                        })),
                    );
                }
            }
        }
        if writable {
            let cursor = focused && self.ui.cursor == Some(Stop::AddLink);
            let row = div()
                .id(format!("wi-stop-{}", stop_key(Stop::AddLink)))
                .h(px(size::CONTROL_LG))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .text_color(colors.text_secondary)
                .type_style(typography::BODY, &theme)
                .child(adoken_ui::action_menu::track_bounds(
                    self.ui.anchors.add_link.clone(),
                ))
                .child(
                    Icon::new(IconName::Link)
                        .size(size::ICON_SM)
                        .color(colors.text_secondary),
                )
                .child("Add link…")
                .child(kbd(LINK_KEYS));
            body = body.child(
                stop_frame(row, cursor, true, &theme)
                    .role(gpui::Role::Button)
                    .aria_label("Add link, L")
                    .tooltip(Tooltip::text(if gate == Gate::Offline {
                        "Offline — reconnect to send"
                    } else {
                        "Add link  l"
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.ui.from_list = false;
                        this.intend(Intent::AddLink(None), window, cx)
                    })),
            );
        }
        body.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_link_row(
        &self,
        row: usize,
        ix: usize,
        link: &LinkRow,
        cursor: bool,
        writable: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = &theme.colors;
        let pending = link
            .kind
            .zip(link.work_item_id())
            .and_then(|(kind, target)| {
                let field = EditField::Link { kind, target };
                self.status_lines(field, theme, cx)
            });
        let removing = pending.is_some();
        let base = div()
            .id(format!("wi-stop-{}", stop_key(Stop::Link(row))))
            .group(SharedString::from(format!("wi-link-{row}")))
            .h(px(size::CONTROL_LG))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .type_style(typography::BODY, theme);
        let (content, spoken, tooltip): (gpui::Stateful<gpui::Div>, String, Option<String>) =
            match &link.target {
                LinkTarget::WorkItem { id, item } => {
                    let row_el = match item {
                        Some(item) => {
                            let (icon, tint) = type_style(&item.kind, colors);
                            let category_color =
                                super::header::category_tint(item.state_category, theme);
                            base.child(Icon::new(icon).size(size::ICON_SM).color(tint))
                                .child(
                                    div()
                                        .w(px(56.0))
                                        .flex_none()
                                        .text_color(colors.text_secondary)
                                        .type_style(typography::CODE_INLINE, theme)
                                        .child(format!("#{id}")),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(if removing {
                                            colors.text_tertiary
                                        } else {
                                            colors.text_primary
                                        })
                                        .child(item.title.clone()),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .max_w(px(STATE_WORD_MAX))
                                        .flex()
                                        .items_center()
                                        .gap(px(space::S4))
                                        .text_color(colors.text_secondary)
                                        .type_style(typography::SMALL, theme)
                                        .child(
                                            Icon::new(super::header::category_icon(
                                                item.state_category,
                                            ))
                                            .size(size::ICON_XS)
                                            .color(category_color),
                                        )
                                        .child(div().truncate().child(item.state.clone())),
                                )
                                .children(item.assigned_to.as_ref().map(|person| {
                                    crate::avatar::Avatar::new(
                                        person.clone(),
                                        self.list.name(person),
                                    )
                                    .size(crate::avatar::AvatarSize::Sm)
                                }))
                        }
                        None => base
                            .child(
                                Icon::new(IconName::CircleDashed)
                                    .size(size::ICON_SM)
                                    .color(colors.text_tertiary),
                            )
                            .child(
                                div()
                                    .w(px(56.0))
                                    .flex_none()
                                    .text_color(colors.text_secondary)
                                    .type_style(typography::CODE_INLINE, theme)
                                    .child(format!("#{id}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(colors.text_tertiary)
                                    .child("Not synced"),
                            ),
                    };
                    let title = item.as_ref().map_or("not synced".to_owned(), |i| {
                        format!(
                            "{}, {}{}",
                            i.title,
                            i.state,
                            i.assigned_to
                                .as_ref()
                                .map(|p| format!(", assigned to {}", self.list.name(p)))
                                .unwrap_or_default()
                        )
                    });
                    (
                        row_el,
                        format!("{}: work item {id}, {title}", link.group.singular()),
                        None,
                    )
                }
                LinkTarget::PullRequest { id, pr } => {
                    let (icon, tint, status, title) = match pr {
                        Some(pr) => {
                            let (icon, tint) = pr_style(pr.status, pr.is_draft, theme);
                            (
                                icon,
                                tint,
                                crate::palette::catalog::pr_status_label(pr),
                                pr.title.clone(),
                            )
                        }
                        None => (
                            IconName::GitPullRequest,
                            colors.text_tertiary,
                            "Not synced",
                            String::new(),
                        ),
                    };
                    (
                        base.child(Icon::new(icon).size(size::ICON_SM).color(tint))
                            .child(
                                div()
                                    .w(px(56.0))
                                    .flex_none()
                                    .text_color(colors.text_secondary)
                                    .type_style(typography::CODE_INLINE, theme)
                                    .child(format!("!{id}")),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.text_tertiary)
                                    .type_style(typography::SMALL, theme)
                                    .child(status),
                            ),
                        format!("Pull request {id}, {title}, {status}"),
                        Some(format!(
                            "Open !{id} · link or unlink pull requests from the pull request"
                        )),
                    )
                }
                LinkTarget::Commit { sha, .. } => {
                    let short: String = sha.chars().take(7).collect();
                    (
                        base.child(
                            Icon::new(IconName::GitCommitHorizontal)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .type_style(typography::CODE_INLINE, theme)
                                .child(short.clone()),
                        )
                        .child(div().flex_1().min_w_0().truncate().child("Commit")),
                        format!("Commit {short}"),
                        None,
                    )
                }
                LinkTarget::Other { label, web } => (
                    base.child(
                        Icon::new(IconName::Link)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label.clone())),
                    format!("Link, {label}"),
                    web.clone(),
                ),
            };
        // The remove button: hover and keyboard cursor, never a Tab stop
        // (design §6.3); opacity, not display, under group hover.
        let removable = writable && link.group.is_work_item() && !removing;
        let content = if removable {
            let target = link.work_item_id().unwrap_or_default();
            content.child(
                div()
                    .opacity(if cursor { 1.0 } else { 0.0 })
                    .group_hover(SharedString::from(format!("wi-link-{row}")), |s| {
                        s.opacity(1.0)
                    })
                    .child(
                        ghost(
                            ("wi-unlink", ix),
                            Some(IconName::Unlink),
                            None,
                            None,
                            false,
                            theme,
                        )
                        .aria_label(format!("Remove link to work item {target}"))
                        .tooltip(Tooltip::text("Remove link  ⌫"))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.request_remove_row(ix, window, cx);
                            },
                        )),
                    ),
            )
        } else {
            content
        };
        let content = stop_frame(content, cursor, true, theme)
            .role(gpui::Role::ListItem)
            .aria_label(spoken)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.ui.cursor = Some(Stop::Link(row));
                this.open_link_row(row, window, cx)
            }));
        let content = match tooltip {
            Some(tip) => content.tooltip(Tooltip::text(tip)),
            None => content,
        };
        div()
            .flex()
            .flex_col()
            .child(content)
            .children(pending.map(|p| div().px(px(space::S8)).child(p)))
            .into_any_element()
    }

    /// Add link step 2 and Remove link's dialogs (centered, palette-like).
    pub(in crate::workitems) fn render_link_overlays(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if let Some(picker) = &self.ui.link_picker {
            return Some(self.render_link_picker(picker, cx));
        }
        let overlay = self.ui.remove.as_ref()?;
        let detail = self.detail()?;
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let card = div()
            .id("wi-link-overlay")
            .key_context(OVERLAY_CONTEXT)
            .track_focus(&overlay.focus)
            .w(px(CONFIRM_WIDTH))
            .p(px(space::S20))
            .flex()
            .flex_col()
            .gap(px(space::S12))
            .rounded(px(radius::LG))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_3))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)));
        let card = match overlay.what {
            Removal::Confirm { row } => {
                let link = detail.links.get(row)?;
                let target = link.work_item_id()?;
                let title = match &link.target {
                    LinkTarget::WorkItem {
                        item: Some(item), ..
                    } => item.title.clone(),
                    _ => String::new(),
                };
                let heading = format!("Remove link to #{target}?");
                let goes = if link.group == LinkGroup::Parent {
                    "only its parent link goes".to_owned()
                } else {
                    format!(
                        "only this {} link goes",
                        link.group.singular().to_lowercase()
                    )
                };
                card.role(gpui::Role::AlertDialog)
                    .aria_label(heading.clone())
                    .child(div().type_style(typography::HEADING, &theme).child(heading))
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .child(format!("#{target} {title} stays as it is; {goes}.")),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(
                                adoken_ui::Button::new("wi-remove-confirm", "Remove")
                                    .variant(adoken_ui::ButtonVariant::Primary)
                                    .shortcut(kbd("enter"))
                                    .on_press(crate::browse::press(
                                        cx,
                                        |this: &mut Self, window, cx| {
                                            this.confirm_overlay(window, cx)
                                        },
                                    )),
                            )
                            .child(
                                adoken_ui::Button::new("wi-remove-cancel", "Cancel")
                                    .variant(adoken_ui::ButtonVariant::Ghost)
                                    .shortcut(kbd("escape"))
                                    .on_press(crate::browse::press(
                                        cx,
                                        |this: &mut Self, window, cx| {
                                            this.close_overlay(window, cx)
                                        },
                                    )),
                            ),
                    )
            }
            Removal::Choose { selected } => {
                let rows = self.removable_rows();
                card.role(gpui::Role::Dialog)
                    .aria_label("Remove a link")
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, &theme)
                            .child(format!("Remove a link from #{}", detail.item.id)),
                    )
                    .children(rows.iter().enumerate().map(|(n, &ix)| {
                        let link = &detail.links[ix];
                        let id = link.work_item_id().unwrap_or_default();
                        let title = match &link.target {
                            LinkTarget::WorkItem {
                                item: Some(item), ..
                            } => item.title.clone(),
                            _ => "Not synced".into(),
                        };
                        div()
                            .id(("wi-remove-choice", n))
                            .h(px(PICKER_ROW))
                            .px(px(space::S8))
                            .flex()
                            .items_center()
                            .gap(px(space::S8))
                            .rounded(px(radius::SM))
                            .when(n == selected, |d| d.bg(colors.bg_selected))
                            .cursor_pointer()
                            .hover(|s| s.bg(colors.bg_hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if let Some(overlay) = this.ui.remove.as_mut() {
                                    overlay.what = Removal::Choose { selected: n };
                                }
                                this.confirm_overlay(window, cx);
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.text_tertiary)
                                    .type_style(typography::LABEL, &theme)
                                    .child(link.group.singular()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(colors.text_secondary)
                                    .type_style(typography::CODE_INLINE, &theme)
                                    .child(format!("#{id}")),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(title))
                    }))
            }
        };
        Some(scrim(card.into_any_element(), &theme))
    }

    fn render_link_picker(&self, picker: &super::LinkPicker, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let (id, has_parent) = self
            .detail()
            .map_or((0, false), |d| (d.item.id, d.parent().is_some()));
        let title = model::picker_title(picker.kind, id, has_parent);
        let visible = (picker.rows.len() as f32).clamp(1.0, PICKER_VISIBLE_ROWS);
        let rows = if picker.rows.is_empty() {
            let text = if picker.catalog.is_none() {
                "Reading work items…"
            } else {
                "No work items match"
            };
            div()
                .px(px(space::S8))
                .h(px(PICKER_ROW))
                .flex()
                .items_center()
                .text_color(colors.text_tertiary)
                .child(text)
                .into_any_element()
        } else {
            uniform_list(
                "wi-link-picker-rows",
                picker.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| this.render_pick_row(row, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&picker.scroll)
            .h(px(visible * PICKER_ROW))
            .into_any_element()
        };
        let card = div()
            .id("wi-link-picker")
            .key_context(OVERLAY_CONTEXT)
            .w(px(PICKER_WIDTH))
            .p(px(space::S8))
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .rounded(px(radius::LG))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_3))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .occlude()
            .role(gpui::Role::Dialog)
            .aria_label(title.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.close_overlay(window, cx)))
            .child(
                div()
                    .px(px(space::S4))
                    .text_color(colors.text_tertiary)
                    .type_style(typography::LABEL, &theme)
                    .child(title),
            )
            .child(
                div()
                    .h(px(size::CONTROL_LG))
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .px(px(space::S8))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(colors.border_control)
                    .bg(colors.bg_input)
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().child(picker.input.clone())),
            )
            .child(rows)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S12))
                    .px(px(space::S4))
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, &theme)
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S4))
                            .child(kbd("up"))
                            .child(kbd("down"))
                            .child("move"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S4))
                            .child(kbd("enter"))
                            .child("link"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S4))
                            .child(kbd("escape"))
                            .child("close"),
                    ),
            );
        scrim(card.into_any_element(), &theme)
    }

    fn render_pick_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let picker = self
            .ui
            .link_picker
            .as_ref()
            .expect("rows render while open");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let pick = &picker.rows[row];
        let refusal = self.pick_refusal(picker.kind, pick);
        let base = div()
            .id(("wi-pick", row))
            .w_full()
            .h(px(PICKER_ROW))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::SM))
            .when(row == picker.selected, |d| d.bg(colors.bg_selected))
            .when(refusal.is_some(), |d| d.text_color(colors.text_disabled));
        let content = match pick {
            PickRow::Note(text) => {
                return base
                    .text_color(colors.text_secondary)
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(text.clone())
                    .into_any_element();
            }
            PickRow::Item(item) => {
                let (icon, tint) = type_style(&item.kind, colors);
                base.aria_label(format!("{} #{} {}", item.kind, item.id, item.title))
                    .child(Icon::new(icon).size(size::ICON_SM).color(tint))
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_secondary)
                            .type_style(typography::CODE_INLINE, &theme)
                            .child(format!("#{}", item.id)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(item.title.clone()),
                    )
                    .when(refusal.is_none(), |d| {
                        d.child(
                            adoken_ui::Chip::new(item.state.clone())
                                .tone(state_tone(item.state_category)),
                        )
                    })
            }
            PickRow::NotSynced(id) => base
                .child(
                    Icon::new(IconName::CircleDashed)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(format!("#{id}"))
                .child(
                    div()
                        .flex_1()
                        .text_color(colors.text_tertiary)
                        .child("Not synced"),
                ),
        };
        let content = match refusal {
            Some(why) => content.child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, &theme)
                    .child(why),
            ),
            None => content
                .cursor_pointer()
                .hover(|s| s.bg(colors.bg_hover))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(picker) = this.ui.link_picker.as_mut() {
                        picker.selected = row;
                    }
                    this.confirm_overlay(window, cx);
                })),
        };
        content.into_any_element()
    }
}

/// A dialog over the pane's scrim, near the top (palette convention).
fn scrim(card: AnyElement, theme: &Theme) -> AnyElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .justify_center()
        .items_start()
        .pt(px(space::S64))
        .bg(theme.colors.bg_scrim)
        .child(card)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workitems::data::{LinkRow, LinkTarget};

    fn row(group: LinkGroup, id: i64) -> LinkRow {
        LinkRow {
            group,
            kind: None,
            index: None,
            target: LinkTarget::WorkItem { id, item: None },
        }
    }

    #[test]
    fn groups_show_five_rows_then_show_more_until_expanded() {
        let mut links = vec![row(LinkGroup::Parent, 1)];
        links.extend((10..17).map(|id| row(LinkGroup::Children, id)));
        links.push(row(LinkGroup::Related, 30));
        let collapsed = shown(&links, &Default::default());
        assert_eq!(collapsed.rows, vec![0, 1, 2, 3, 4, 5, 8]);
        assert_eq!(collapsed.more, vec![(5, LinkGroup::Children)]);
        let expanded = shown(&links, &[LinkGroup::Children].into_iter().collect());
        assert_eq!(expanded.rows.len(), links.len());
        assert!(expanded.more.is_empty());
    }
}
