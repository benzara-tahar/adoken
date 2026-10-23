//! Header (design §3): the toolbar (back when stacked, type · id,
//! freshness / Sync this item, Pin, Open, ⋯), the editable title, the
//! state chip-menu with priority and tags, the read-only token hint, the
//! Not available banner, and the sticky compact header (§2.5).

use adoken_model::{StateCategory, WorkItem};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{ActiveTheme, Chip, Icon, IconName, Spinner, Theme, Tooltip, TypeStyleExt};
use gpui::{AnyElement, Context, Hsla, SharedString, Window, div, prelude::*, px};

use super::model::Stop;
use super::status::{ghost, kbd, stop_frame};
use super::{Intent, OPEN_KEYS, stop_key};
use crate::workitems::writes::{self, Field as EditField, FreshKind, Gate, ItemSyncState};
use crate::workitems::{WorkItemsView, priority_style, state_tone, type_style};

/// Freshness labels shorten below this pane width (design §2.4).
const SHORT_TOOLBAR: f32 = 720.0;

/// State category glyph (design §3.3): the chip's non-color channel.
pub(super) fn category_icon(category: StateCategory) -> IconName {
    match category {
        StateCategory::Proposed => IconName::CircleDashed,
        StateCategory::InProgress => IconName::CircleDot,
        StateCategory::Resolved => IconName::CircleCheck,
        StateCategory::Completed => IconName::CircleCheckBig,
        StateCategory::Removed => IconName::CircleSlash,
    }
}

/// Category tint for menus and link rows (pins.md §3.3 work-item tints).
pub(super) fn category_tint(category: StateCategory, theme: &Theme) -> Hsla {
    let colors = &theme.colors;
    match category {
        StateCategory::InProgress => colors.accent_fg,
        StateCategory::Resolved => colors.status_success_fg,
        StateCategory::Proposed => colors.text_secondary,
        StateCategory::Completed | StateCategory::Removed => colors.text_tertiary,
    }
}

fn spoken_category(category: StateCategory) -> &'static str {
    match category {
        StateCategory::Proposed => "proposed",
        StateCategory::InProgress => "in progress",
        StateCategory::Resolved => "resolved",
        StateCategory::Completed => "completed",
        StateCategory::Removed => "removed",
    }
}

/// The freshness button's look (design §3.4).
pub struct Badge {
    pub icon: IconName,
    pub spinning: bool,
    pub label: SharedString,
    pub short: SharedString,
    pub glyph: Hsla,
    pub text: Hsla,
    pub tooltip: SharedString,
    pub disabled: bool,
}

impl WorkItemsView {
    /// The pending value of `field`, shown in place of the stored one
    /// while Azure DevOps hasn't answered (spec FR-6).
    pub(super) fn pending_value(&self, field: EditField, cx: &Context<Self>) -> Option<String> {
        let id = self.detail_id()?;
        let edits = Self::edits(cx)?;
        let edits = edits.read(cx);
        let status = edits.status(id, field)?;
        status.is_sending().then(|| status.value())
    }

    /// Design §3.4: the controller's freshness (FR-29, with the item's
    /// sync state) in this button's glyph and colors.
    pub(super) fn freshness_badge(&self, item: &WorkItem, cx: &Context<Self>) -> Badge {
        let detail = self.detail().expect("freshness renders with a detail");
        let freshness = match Self::edits(cx) {
            Some(edits) => edits.read(cx).freshness(&detail.freshness, item.id, cx),
            None => writes::freshness_with(
                &detail.freshness,
                &writes::FreshCtx {
                    now: self.now(),
                    offline: false,
                    demo: self.list.demo,
                    sync: &ItemSyncState::Idle,
                    lane_every: None,
                    // Local time like every other timestamp (FR-29 tooltip).
                    offset: crate::activity::model::local_offset(self.now()),
                },
            ),
        };
        badge(freshness, cx.theme())
    }

    fn render_freshness(&self, item: &WorkItem, short: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let badge = self.freshness_badge(item, cx);
        let disabled = badge.disabled;
        let button = freshness_button(item.id, badge, short, &theme);
        if disabled {
            return button.into_any_element();
        }
        button
            .cursor_pointer()
            .hover(|s| s.bg(theme.colors.bg_hover))
            .on_click(cx.listener(|this, _, window, cx| this.intend(Intent::Sync, window, cx)))
            .into_any_element()
    }
}

/// Design §3.4: a freshness in the button's glyph and colors.
pub fn badge(freshness: writes::Freshness, theme: &Theme) -> Badge {
    let colors = &theme.colors;
    let (icon, glyph, text, disabled) = match freshness.kind {
        FreshKind::Fresh | FreshKind::JustSynced => (
            IconName::RefreshCw,
            colors.text_tertiary,
            colors.text_tertiary,
            false,
        ),
        FreshKind::Stale => (
            IconName::RefreshCw,
            colors.text_secondary,
            colors.text_secondary,
            false,
        ),
        FreshKind::Partly => (
            IconName::CircleDashed,
            colors.text_secondary,
            colors.text_secondary,
            false,
        ),
        FreshKind::Syncing => (
            IconName::LoaderCircle,
            colors.status_info_fg,
            colors.text_tertiary,
            true,
        ),
        FreshKind::Failed => (
            IconName::CircleX,
            colors.status_danger_fg,
            colors.text_secondary,
            false,
        ),
        FreshKind::Waiting => (
            IconName::Clock,
            colors.text_tertiary,
            colors.text_tertiary,
            true,
        ),
        FreshKind::Offline => (
            IconName::WifiOff,
            colors.text_tertiary,
            colors.text_tertiary,
            true,
        ),
        FreshKind::Demo => (
            IconName::RefreshCw,
            colors.text_tertiary,
            colors.text_tertiary,
            true,
        ),
    };
    let short: SharedString = match freshness.kind {
        FreshKind::Failed => "Couldn't sync".into(),
        FreshKind::Offline => "Offline".into(),
        FreshKind::Waiting => "Waiting…".into(),
        _ => short_label(&freshness.label).into(),
    };
    Badge {
        icon,
        spinning: freshness.kind == FreshKind::Syncing,
        label: freshness.label.into(),
        short,
        glyph,
        text,
        tooltip: freshness.tooltip.into(),
        disabled,
    }
}

/// The freshness / Sync this item button for work item `id`.
pub fn freshness_button(
    id: i64,
    badge: Badge,
    short: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let glyph = if badge.spinning {
        Spinner::new()
            .size(size::ICON_SM)
            .color(badge.glyph)
            .into_any_element()
    } else {
        Icon::new(badge.icon)
            .size(size::ICON_SM)
            .color(badge.glyph)
            .into_any_element()
    };
    let label = if short { badge.short } else { badge.label };
    let spoken = format!("Sync work item {id}. {label}");
    div()
        .id(("wi-freshness", id as usize))
        .flex_none()
        .h(px(size::CONTROL_SM))
        .px(px(space::S6))
        .flex()
        .items_center()
        .gap(px(space::S4))
        .rounded(px(radius::SM))
        .text_color(badge.text)
        .type_style(typography::SMALL, theme)
        .role(gpui::Role::Button)
        .aria_label(spoken)
        .tooltip(Tooltip::text(badge.tooltip))
        .child(glyph)
        .child(label)
}

impl WorkItemsView {
    /// Toolbar row (design §3.1).
    pub(in crate::workitems) fn render_toolbar(
        &self,
        item: &WorkItem,
        stacked: bool,
        pane_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let (icon, tint) = type_style(&item.kind, colors);
        let id = item.id;
        let short = pane_width < SHORT_TOOLBAR;
        let open_disabled = self.list.demo || self.list.org_url.is_none();
        let open_tooltip = if self.list.demo {
            crate::review::web::DEMO_OPEN_REASON
        } else {
            "Open in Azure DevOps  o"
        };
        div()
            .h(px(size::CONTROL_MD))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .when(stacked, |row| {
                row.child(
                    ghost(
                        "wi-back",
                        Some(IconName::ChevronLeft),
                        Some("Work items".into()),
                        Some("escape"),
                        false,
                        &theme,
                    )
                    .aria_label("Back to work items, Escape")
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.dispatch_action(Box::new(super::super::FocusWorkItemList), cx);
                        this.stacked_detail = false;
                        cx.notify();
                    })),
                )
            })
            .child(Icon::new(icon).size(size::ICON_MD).color(tint))
            .child(
                div()
                    .id("wi-type-id")
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .cursor_pointer()
                    .role(gpui::Role::Button)
                    .aria_label(format!("{} {id}. Copy id, Shift Y", item.kind.as_str()))
                    .tooltip(Tooltip::text("Copy id  ⇧Y"))
                    .on_click(cx.listener(|this, _, _, cx| this.copy(false, cx)))
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, &theme)
                            .child(format!("{} ·", item.kind.as_str())),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::CODE_INLINE, &theme)
                            .child(format!("#{id}")),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex_none()
                    .child(self.render_freshness(item, short, cx))
                    .children(adoken_ui::tour::anchor(super::anchors::FRESHNESS, cx)),
            )
            .child(crate::pin_actions::button(
                adoken_model::PinSubject::WorkItem(id),
                item.title.clone().into(),
                cx,
            ))
            .child(
                ghost(
                    "wi-open-ado",
                    Some(IconName::ExternalLink),
                    (!short).then(|| "Open".into()),
                    Some(OPEN_KEYS),
                    open_disabled,
                    &theme,
                )
                .aria_label(format!("Open work item {id} in Azure DevOps, O"))
                .tooltip(Tooltip::text(open_tooltip))
                .on_click(cx.listener(|this, _, _, cx| this.open_in_ado(cx))),
            )
            .child(
                ghost(
                    "wi-more",
                    Some(IconName::Ellipsis),
                    None,
                    None,
                    false,
                    &theme,
                )
                .aria_label(format!("More actions for work item {id}, Command K"))
                .tooltip(Tooltip::text("More actions  ⌘K"))
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::pin_actions::OpenRowMenu), cx);
                })),
            )
            .into_any_element()
    }

    /// The title, or its inline editor (design §3.2).
    pub(in crate::workitems) fn render_title(
        &self,
        item: &WorkItem,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let writable = self.gate(cx) != Gate::ReadOnly && self.gate(cx) != Gate::Gone;
        let anchor = adoken_ui::action_menu::track_bounds(self.ui.anchors.title.clone());
        if let Some(editor) = &self.ui.title {
            let text = editor.input.read(cx).text().to_owned();
            let hint = match editor.error {
                Some(error) => div()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .child(
                        Icon::new(IconName::CircleAlert)
                            .size(size::ICON_XS)
                            .color(colors.status_warning_fg),
                    )
                    .child(error)
                    .into_any_element(),
                None => div()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .child(kbd("enter"))
                    .child("Save ·")
                    .child(kbd("escape"))
                    .child("Cancel")
                    .into_any_element(),
            };
            return div()
                .id(format!("wi-stop-{}", stop_key(Stop::Title)))
                .key_context(super::TITLE_EDITOR_CONTEXT)
                .relative()
                .flex()
                .flex_col()
                .gap(px(space::S4))
                .max_w(px(size::DETAIL_PROSE_MAX))
                .child(anchor)
                .child(
                    div()
                        .p(px(space::S4))
                        .rounded(px(radius::SM))
                        .bg(colors.bg_input)
                        .border_1()
                        .border_color(colors.border_focus)
                        .text_color(colors.text_primary)
                        .type_style(typography::TITLE, &theme)
                        .child(editor.input.clone()),
                )
                .child(
                    div()
                        .h(px(space::S16))
                        .flex()
                        .justify_between()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, &theme)
                        .child(hint)
                        .children(
                            super::model::title_counter(&text)
                                .map(|c| div().type_style(typography::SMALL, &theme).child(c)),
                        ),
                )
                .into_any_element();
        }
        let title = self
            .pending_value(EditField::Title, cx)
            .unwrap_or_else(|| item.title.clone());
        let cursor = self.ui.cursor == Some(Stop::Title) && self.detail_focus.is_focused(window);
        let label = format!("Title: {title}. Edit, T");
        let block = div()
            .id(format!("wi-stop-{}", stop_key(Stop::Title)))
            .max_w(px(size::DETAIL_PROSE_MAX))
            .p(px(space::S4))
            .m(px(-space::S4))
            .group("wi-title")
            .child(anchor)
            .child(
                div()
                    .id("wi-title-text")
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap(px(space::S6))
                    .text_color(colors.text_primary)
                    .type_style(typography::TITLE, &theme)
                    .role(gpui::Role::Heading)
                    .aria_level(1)
                    .child(title)
                    .when(writable, |t| {
                        // Opacity, never display toggling, under group hover
                        // (the fork's prepaint pitfall).
                        t.child(
                            div()
                                .pb(px(space::S4))
                                .opacity(if cursor { 1.0 } else { 0.0 })
                                .group_hover("wi-title", |s| s.opacity(1.0))
                                .child(
                                    Icon::new(IconName::Pencil)
                                        .size(size::ICON_SM)
                                        .color(colors.text_tertiary),
                                ),
                        )
                    }),
            );
        let block = stop_frame(block, cursor, writable, &theme);
        let block = if writable {
            block
                .role(gpui::Role::Button)
                .aria_label(label)
                .tooltip(Tooltip::text("Edit title  t"))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.ui.from_list = false;
                    this.intend(Intent::Title, window, cx)
                }))
        } else {
            block
        };
        div()
            .flex()
            .flex_col()
            .child(block)
            .children(self.status_lines(EditField::Title, &theme, cx))
            .into_any_element()
    }

    /// State chip-menu (design §3.3): category glyph + word + chevron.
    pub(super) fn render_state_chip(
        &self,
        item: &WorkItem,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let gate = self.gate(cx);
        let writable = matches!(gate, Gate::Editable | Gate::Offline);
        let state = self
            .pending_value(EditField::State, cx)
            .unwrap_or_else(|| item.state.clone());
        let category = if state == item.state {
            item.state_category
        } else {
            StateCategory::of_state(&state)
        };
        let (fg, bg) = state_tone(category).colors(colors);
        let open = self.ui.state_menu.is_open();
        let cursor = self.ui.cursor == Some(Stop::State) && self.detail_focus.is_focused(window);
        let chip = div()
            .id(format!("wi-stop-{}", stop_key(Stop::State)))
            .relative()
            .flex_none()
            .h(px(size::CONTROL_SM))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .rounded(px(radius::XS))
            .bg(bg)
            .text_color(fg)
            .type_style(typography::LABEL, &theme)
            .child(adoken_ui::action_menu::track_bounds(
                self.ui.anchors.state.clone(),
            ))
            // Spec 0022 FR-8/FR-9: celebrations rise from this chip.
            .children(crate::celebrate::track(
                crate::celebrate::Anchor::WorkItem(item.id),
                cx,
            ))
            // Spec 0020: the work item tour's first anchor.
            .children(adoken_ui::tour::anchor(super::anchors::STATE, cx))
            .child(
                Icon::new(category_icon(category))
                    .size(size::ICON_XS)
                    .color(fg),
            )
            .child(state.clone())
            .children(crate::celebrate::glyph(
                crate::celebrate::Anchor::WorkItem(item.id),
                cx,
            ))
            .when(writable, |chip| {
                chip.child(
                    Icon::new(if open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(size::ICON_XS)
                    .color(fg),
                )
            })
            .when(cursor, |chip| chip.child(adoken_ui::focus_ring(&theme)));
        if !writable {
            return chip
                .role(gpui::Role::Label)
                .aria_label(super::model::state_label(&state, "", false))
                .into_any_element();
        }
        chip.cursor_pointer()
            .hover(|s| s.bg(colors.bg_hover))
            .active(|s| s.bg(colors.bg_active))
            .role(gpui::Role::Button)
            .aria_expanded(open)
            .aria_label(super::model::state_label(
                &state,
                spoken_category(category),
                true,
            ))
            .tooltip(Tooltip::text(if gate == Gate::Offline {
                "Offline — reconnect to send"
            } else {
                "Change state  s"
            }))
            .on_click(cx.listener(|this, _, window, cx| {
                this.ui.from_list = false;
                this.intend(Intent::State, window, cx)
            }))
            .into_any_element()
    }

    /// State (single column only), priority and tags; then the state's
    /// status line and the read-only token hint.
    pub(in crate::workitems) fn render_chip_row(
        &self,
        item: &WorkItem,
        with_state: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let mut row = div().flex().flex_wrap().items_center().gap(px(space::S8));
        if with_state {
            row = row.child(self.render_state_chip(item, window, cx));
        }
        if let Some(p) = item.priority {
            let (p_icon, p_color) = priority_style(p, colors);
            row = row.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S2))
                    .text_color(colors.text_secondary)
                    .type_style(typography::LABEL, &theme)
                    .child(Icon::new(p_icon).size(size::ICON_SM).color(p_color))
                    .child(format!("Priority {p}")),
            );
        }
        row = row.children(item.tags.iter().map(|t| Chip::new(t.clone())));
        div()
            .flex()
            .flex_col()
            .child(row)
            .when(with_state, |col| {
                col.children(self.status_lines(EditField::State, &theme, cx))
            })
            .children(self.render_token_hint(cx))
            .into_any_element()
    }

    /// Design §9.4 / §10.4: one passive line instead of every edit control;
    /// no button, not a keyboard stop (spec 0001 FR-9, FR-14).
    fn render_token_hint(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.gate(cx) != Gate::ReadOnly {
            return None;
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        Some(
            div()
                .id("wi-read-only-hint")
                .pt(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S6))
                .text_color(colors.text_secondary)
                .type_style(typography::SMALL, &theme)
                .role(gpui::Role::Note)
                .aria_label(writes::READ_ONLY_HINT)
                .child(
                    Icon::new(IconName::Lock)
                        .size(size::ICON_SM)
                        .color(colors.text_tertiary),
                )
                .child(writes::READ_ONLY_HINT)
                .into_any_element(),
        )
    }

    /// Design §12 Not available: the 404 banner over the last data.
    pub(in crate::workitems) fn render_not_available(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.gate(cx) != Gate::Gone {
            return None;
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let cursor =
            self.ui.cursor == Some(Stop::NotAvailable) && self.detail_focus.is_focused(window);
        let text = "This work item was deleted or you lost access to it.";
        Some(
            div()
                .id(format!("wi-stop-{}", stop_key(Stop::NotAvailable)))
                .p(px(space::S12))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .rounded(px(radius::SM))
                .bg(colors.status_danger_wash)
                .text_color(colors.text_primary)
                .type_style(typography::BODY, &theme)
                .role(gpui::Role::Alert)
                .aria_label(text)
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size(size::ICON_SM)
                        .color(colors.status_danger_fg),
                )
                .child(div().flex_1().child(text))
                .child(
                    stop_frame(
                        ghost(
                            "wi-gone-open",
                            Some(IconName::ExternalLink),
                            Some("Open in Azure DevOps".into()),
                            Some(OPEN_KEYS),
                            self.list.demo,
                            &theme,
                        ),
                        cursor,
                        false,
                        &theme,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_in_ado(cx))),
                )
                .into_any_element(),
        )
    }

    /// Design §2.5: 40 px bar pinned to the pane top once the title has
    /// scrolled away: glyph, id, title, state chip, freshness, ⋯.
    pub(in crate::workitems) fn render_sticky(
        &self,
        item: &WorkItem,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let (icon, tint) = type_style(&item.kind, colors);
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(40.0))
            .px(px(space::S24))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .bg(colors.bg_surface)
            .border_b_1()
            .border_color(colors.border_subtle)
            .occlude()
            .child(Icon::new(icon).size(size::ICON_MD).color(tint))
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
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY_STRONG, &theme)
                    .child(item.title.clone()),
            )
            .child(self.render_state_chip(item, window, cx))
            .child(self.render_freshness(item, true, cx))
            .child(
                ghost(
                    "wi-sticky-more",
                    Some(IconName::Ellipsis),
                    None,
                    None,
                    false,
                    &theme,
                )
                .tooltip(Tooltip::text("More actions  ⌘K"))
                .on_click(cx.listener(|_, _, window, cx| {
                    window.dispatch_action(Box::new(crate::pin_actions::OpenRowMenu), cx);
                })),
            )
            .into_any_element()
    }
}

/// "Synced 2m ago" → "2m" (design §2.4: the label shortens first).
fn short_label(label: &str) -> String {
    label
        .strip_prefix("Synced ")
        .or_else(|| label.strip_prefix("Updated "))
        .map(|rest| rest.trim_end_matches(" ago").to_owned())
        .unwrap_or_else(|| label.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_labels_shorten_to_the_age() {
        assert_eq!(short_label("Synced 2m ago"), "2m");
        assert_eq!(short_label("Updated 18m ago"), "18m");
        assert_eq!(short_label("Partly synced"), "Partly synced");
    }
}
