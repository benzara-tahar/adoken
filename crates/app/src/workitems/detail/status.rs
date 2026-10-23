//! Shared pieces of the detail: section heads, small ghost buttons, the
//! keyboard cursor's ring, and the edit-state grammar of design §9 (the
//! pending, waiting, failed, conflict and rule lines under an element).

use adoken_model::Millis;
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{Icon, IconName, Kbd, Theme, Tooltip, TypeStyleExt};
use gpui::{
    AnyElement, Context, Div, ElementId, Hsla, SharedString, Stateful, div, prelude::*, px,
};

use super::WorkItemsView;
use crate::workitems::writes::{EditStatus, Field as EditField};

pub(super) fn kbd(keys: &str) -> Kbd {
    Kbd::parse(keys).expect("static binding")
}

/// "Description" + count + trailing action (design §2.1: 28 high).
pub(super) fn section_head(label: &'static str, count: Option<usize>, theme: &Theme) -> Div {
    let colors = &theme.colors;
    div()
        .h(px(size::CONTROL_MD))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .child(
            div()
                .text_color(colors.text_primary)
                .type_style(typography::HEADING, theme)
                .child(label),
        )
        .children(count.map(|n| {
            div()
                .text_color(colors.text_tertiary)
                .type_style(typography::LABEL, theme)
                .child(n.to_string())
        }))
}

/// A titled section (Description, Links, Discussion, History) with its
/// accessible role.
pub(super) fn section(id: &'static str) -> Stateful<Div> {
    div().id(id).flex().flex_col().gap(px(space::S8))
}

/// Ghost button 24 high (toolbar, section heads, notices): glyph, label,
/// keycap. `None` label = icon only (24 × 24).
pub(super) fn ghost(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: Option<SharedString>,
    keys: Option<&str>,
    disabled: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let colors = &theme.colors;
    let fg = if disabled {
        colors.text_disabled
    } else {
        colors.text_secondary
    };
    let mut button = div()
        .id(id)
        .flex_none()
        .h(px(size::CONTROL_SM))
        .min_w(px(size::HIT_MIN))
        .px(px(if label.is_some() { space::S6 } else { 0.0 }))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(space::S4))
        .rounded(px(radius::SM))
        .text_color(fg)
        .type_style(typography::SMALL, theme)
        .role(gpui::Role::Button);
    if !disabled {
        button = button
            .cursor_pointer()
            .hover(|s| s.bg(colors.bg_hover))
            .active(|s| s.bg(colors.bg_active));
    }
    button
        .children(icon.map(|icon| Icon::new(icon).size(size::ICON_SM).color(fg)))
        .children(label)
        .children(keys.map(kbd))
}

thread_local! {
    /// Where the keyboard cursor's stop last painted (the pane's reveal
    /// pass scrolls it into view).
    pub(super) static CURSOR: adoken_ui::action_menu::BoundsSlot = Default::default();
}

/// Rest / hover / keyboard-cursor treatment of a detail stop (design
/// §4.1): `bg.hover` under the cursor plus the 2 px focus ring.
pub(super) fn stop_frame(
    base: Stateful<Div>,
    cursor: bool,
    interactive: bool,
    theme: &Theme,
) -> Stateful<Div> {
    let colors = &theme.colors;
    let base = base.relative().rounded(px(radius::SM));
    let base = if cursor {
        base.bg(colors.bg_hover)
            .child(adoken_ui::action_menu::track_bounds(
                CURSOR.with(Clone::clone),
            ))
            .child(adoken_ui::focus_ring(theme))
    } else {
        base
    };
    if interactive {
        base.cursor_pointer().hover(|s| s.bg(colors.bg_hover))
    } else {
        base
    }
}

/// One status line: glyph 12 + text (`type.small`), `space.4` above.
fn line(icon: IconName, tint: Hsla, text: SharedString, theme: &Theme) -> Div {
    div()
        .pt(px(space::S4))
        .flex()
        .items_start()
        .gap(px(space::S4))
        .type_style(typography::SMALL, theme)
        .child(
            div()
                .pt(px(space::S2))
                .child(Icon::new(icon).size(size::ICON_XS).color(tint)),
        )
        .child(div().flex_1().min_w_0().child(text))
}

/// "Sending…" or, after 3 s queued, "Waiting for Azure DevOps…" (design
/// §9.1). `verb` reads "Removing…" for link removals.
pub(super) fn pending_line(waiting: bool, verb: &'static str, theme: &Theme) -> AnyElement {
    let colors = &theme.colors;
    let text = if waiting {
        "Waiting for Azure DevOps…"
    } else {
        verb
    };
    let line = line(IconName::Clock, colors.text_tertiary, text.into(), theme)
        .text_color(colors.text_tertiary)
        .id(text)
        .role(gpui::Role::Status)
        .aria_label(text);
    if waiting {
        line.tooltip(Tooltip::text(
            "Azure DevOps asked adoken to slow down. Your change is kept and sends as soon as it allows.",
        ))
        .into_any_element()
    } else {
        line.into_any_element()
    }
}

/// The relative time of an Azure DevOps change ("1m ago").
fn ago(now: Millis, at: Millis) -> String {
    let age = crate::review::data::age(now, at);
    if age.chars().any(|c| c.is_ascii_digit()) && !age.contains(' ') {
        format!("{age} ago")
    } else {
        age
    }
}

impl WorkItemsView {
    /// The line(s) under an element whose write isn't plain sailing
    /// (design §9.1–§9.5). `mine` renders my value for the conflict's
    /// "Yours:" line.
    pub(super) fn status_lines(
        &self,
        field: EditField,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let id = self.detail_id()?;
        let edits = Self::edits(cx)?;
        let status = edits.read(cx).status(id, field)?.clone();
        let now = self.now();
        let colors = &theme.colors;
        let key = |part: &str| SharedString::from(format!("wi-status-{field:?}-{part}"));
        Some(match &status {
            EditStatus::Sending { .. } => {
                let verb = if matches!(field, EditField::Link { .. }) {
                    "Removing…"
                } else {
                    "Sending…"
                };
                pending_line(status.waiting(now), verb, theme)
            }
            EditStatus::Failed { cause, .. } => {
                // Spec 0001 FR-15: an expired token says so, with its date.
                let text = match cause {
                    adoken_sync::WiError::Unauthorized => {
                        crate::account::rejected_write_text(cause.to_string(), cx)
                    }
                    other => other.to_string(),
                };
                let text_field = matches!(field, EditField::Title | EditField::Description);
                let message: SharedString = if text_field {
                    format!("{text} Your text is saved.").into()
                } else {
                    text.into()
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        line(
                            IconName::CircleX,
                            colors.status_danger_fg,
                            message.clone(),
                            theme,
                        )
                        .text_color(colors.text_secondary),
                    )
                    .child(
                        div()
                            .pl(px(space::S16))
                            .flex()
                            .gap(px(space::S4))
                            .child(
                                ghost(key("retry"), None, Some("Retry".into()), None, false, theme)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.retry_edit(field, cx)
                                    })),
                            )
                            .when(
                                field == EditField::Description
                                    && self
                                        .detail()
                                        .and_then(|d| {
                                            d.draft(adoken_store::wi::DraftKind::Description)
                                        })
                                        .is_some(),
                                |row| {
                                    row.child(
                                        ghost(
                                            key("resume"),
                                            None,
                                            Some("Resume draft".into()),
                                            None,
                                            false,
                                            theme,
                                        )
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.intend(super::Intent::Description, window, cx)
                                            }),
                                        ),
                                    )
                                },
                            )
                            .child(
                                ghost(
                                    key("dismiss"),
                                    None,
                                    Some("Dismiss".into()),
                                    None,
                                    false,
                                    theme,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let (Some(edits), Some(id)) =
                                            (Self::edits(cx), this.detail_id())
                                        {
                                            edits.update(cx, |e, cx| e.dismiss(id, field, cx));
                                        }
                                    },
                                )),
                            ),
                    )
                    .id(key("failed"))
                    .role(gpui::Role::Alert)
                    .aria_label(message)
                    .into_any_element()
            }
            EditStatus::Conflict { conflict, .. } => {
                let mine = status.value();
                let sentence: SharedString = format!(
                    "Changed in Azure DevOps by {} {}. Your edit wasn't saved.",
                    conflict.by_name,
                    ago(now, conflict.at)
                )
                .into();
                let description = field == EditField::Description;
                let actions = if description {
                    vec![
                        ("compare", "Compare"),
                        ("overwrite", "Overwrite with mine"),
                        ("discard", "Discard mine"),
                    ]
                } else {
                    vec![("apply", "Apply mine"), ("keep", "Keep theirs")]
                };
                div()
                    .mt(px(space::S4))
                    .max_w(px(size::DETAIL_PROSE_MAX))
                    .p(px(space::S8))
                    .rounded(px(radius::SM))
                    .bg(colors.status_warning_wash)
                    .border_l_2()
                    .border_color(colors.status_warning_fg)
                    .flex()
                    .flex_col()
                    .gap(px(space::S4))
                    .type_style(typography::SMALL, theme)
                    .text_color(colors.text_primary)
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S4))
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .size(size::ICON_XS)
                                    .color(colors.status_warning_fg),
                            )
                            .child(sentence.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(space::S8))
                            .when(!description, |row| {
                                row.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(colors.text_secondary)
                                        .child(format!("Yours: {mine}")),
                                )
                            })
                            .children(actions.into_iter().map(|(part, label)| {
                                ghost(key(part), None, Some(label.into()), None, false, theme)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.resolve_conflict(field, part, cx)
                                    }))
                            })),
                    )
                    .id(key("conflict"))
                    .role(gpui::Role::Group)
                    .aria_label(sentence)
                    .into_any_element()
            }
            EditStatus::Rule { rule, .. } => {
                let rule = rule.clone();
                self.render_rule(field, &rule, theme, cx)
            }
        })
    }

    /// Apply mine / Keep theirs, or the description's Compare / Overwrite
    /// with mine / Discard mine (spec FR-8, FR-19).
    fn resolve_conflict(&mut self, field: EditField, part: &str, cx: &mut Context<Self>) {
        let (Some(edits), Some(id)) = (Self::edits(cx), self.detail_id()) else {
            return;
        };
        match part {
            "compare" => {
                self.ui.compare = !self.ui.compare;
                cx.notify();
            }
            "apply" | "overwrite" => edits.update(cx, |e, cx| e.apply_mine(id, field, cx)),
            "discard" => {
                edits.update(cx, |e, cx| e.keep_theirs(id, field, cx));
                self.discard_draft(adoken_store::wi::DraftKind::Description, cx);
                self.ui.compare = false;
            }
            _ => edits.update(cx, |e, cx| e.keep_theirs(id, field, cx)),
        }
    }

    /// Design §9.5: what Azure DevOps needs, with an inline control when
    /// adoken can edit that field.
    fn render_rule(
        &self,
        field: EditField,
        rule: &adoken_sync::wi::RuleViolation,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use adoken_model::FieldKind;
        let colors = &theme.colors;
        let target = self.rule_target(field, cx);
        let sentence: SharedString = match &target {
            Some(state) => format!("Azure DevOps needs {} to move to {state}.", rule.field),
            None => format!("Azure DevOps needs {} to save this change.", rule.field),
        }
        .into();
        let apply_label: SharedString = match &target {
            Some(state) => format!("Move to {state}").into(),
            None => "Save".into(),
        };
        let mut body = div()
            .mt(px(space::S4))
            .max_w(px(size::DETAIL_PROSE_MAX))
            .p(px(space::S12))
            .rounded(px(radius::SM))
            .bg(colors.bg_raised)
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .type_style(typography::BODY, theme)
            .text_color(colors.text_primary)
            .child(
                div()
                    .flex()
                    .gap(px(space::S6))
                    .child(
                        div().pt(px(space::S2)).child(
                            Icon::new(IconName::CircleAlert)
                                .size(size::ICON_SM)
                                .color(colors.status_warning_fg),
                        ),
                    )
                    .child(sentence.clone()),
            );
        let cancel = ghost(
            "wi-rule-cancel",
            None,
            Some("Cancel".into()),
            Some("escape"),
            false,
            theme,
        )
        .on_click(cx.listener(|this, _, window, cx| {
            this.cancel_rule(window, cx);
        }));
        match &rule.kind {
            FieldKind::List(values) => {
                body = body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(space::S4))
                        .children(values.iter().enumerate().map(|(ix, value)| {
                            let value = value.clone();
                            ghost(
                                ("wi-rule-value", ix),
                                None,
                                Some(value.clone().into()),
                                None,
                                false,
                                theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.apply_rule_value(value.clone(), cx),
                            ))
                        }))
                        .child(cancel),
                );
            }
            FieldKind::Text | FieldKind::Person => {
                let input = self.ui.rule_input.clone();
                body = body.child(
                    div()
                        .key_context(super::RULE_NOTICE_CONTEXT)
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .child(
                            div()
                                .flex_none()
                                .text_color(colors.text_secondary)
                                .child(rule.field.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h(px(size::CONTROL_MD))
                                .px(px(space::S8))
                                .flex()
                                .items_center()
                                .rounded(px(radius::SM))
                                .border_1()
                                .border_color(colors.border_default)
                                .bg(colors.bg_input)
                                .children(input),
                        )
                        .child(
                            ghost(
                                "wi-rule-apply",
                                None,
                                Some(apply_label),
                                Some("enter"),
                                false,
                                theme,
                            )
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.apply_rule(window, cx);
                                },
                            )),
                        )
                        .child(cancel),
                );
            }
            FieldKind::Unknown => {
                body = body
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(format!("adoken can't edit {} yet.", rule.field)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(space::S4))
                            .child(
                                ghost(
                                    "wi-rule-open",
                                    Some(IconName::ExternalLink),
                                    Some("Open in Azure DevOps".into()),
                                    Some(super::OPEN_KEYS),
                                    self.list.demo,
                                    theme,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.open_in_ado(cx))),
                            )
                            .child(cancel),
                    );
            }
        }
        body.id("wi-rule")
            .role(gpui::Role::Group)
            .aria_label(sentence)
            .into_any_element()
    }

    /// The state a refused state change was going to.
    fn rule_target(&self, field: EditField, cx: &Context<Self>) -> Option<String> {
        if field != EditField::State {
            return None;
        }
        let id = self.detail_id()?;
        let edits = Self::edits(cx)?;
        edits.read(cx).status(id, field).map(|s| s.value())
    }
}
