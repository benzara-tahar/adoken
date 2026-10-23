//! Description (design §5): the rendered rich text clipped at 480 px with
//! Show all, its editor (Write / Preview, format hint, Draft saved, Save /
//! Cancel), the blocked notice for HTML adoken can't round-trip, the Draft
//! chip, the description conflict's Compare, and the read-only Repro steps
//! and Acceptance criteria (spec Q-10).

use std::sync::Arc;

use adoken_richtext::Doc;
use adoken_store::wi::DraftKind;
use adoken_theme::{radius, size, space, typography};
use adoken_ui::rich_text::{LinkTarget, MentionName, RichText};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Chip, Icon, IconName, Segmented, Theme, Tooltip,
    TypeStyleExt,
};
use gpui::{AnyElement, Context, Div, SharedString, Window, div, prelude::*, px};

use super::model::Stop;
use super::status::{ghost, kbd, section, section_head, stop_frame};
use super::{DESCRIPTION_EDITOR_CONTEXT, DESCRIPTION_KEYS, Intent, OPEN_KEYS, stop_key};
use crate::workitems::data::{Editable, RichDoc};
use crate::workitems::writes::{Field as EditField, Gate};
use crate::workitems::{WorkItemsEvent, WorkItemsView};

/// Long text clips at 24 lines (480 px) until Show all (design §5.1).
pub(super) const CLIP_LINES: usize = 24;
const CLIP_HEIGHT: f32 = 480.0;

impl WorkItemsView {
    /// A `RichText` wired to the detail: mentions resolve to people
    /// (mine highlighted), links open where they belong (FR-3), images open
    /// the item in Azure DevOps (Q-3).
    pub(super) fn rich_text(
        &self,
        id: impl Into<gpui::ElementId>,
        doc: Arc<Doc>,
        clip: bool,
        cx: &mut Context<Self>,
    ) -> RichText {
        let people: std::collections::HashMap<String, (String, Option<String>)> = self
            .list
            .people
            .iter()
            .map(|p| {
                (
                    p.id.to_ascii_lowercase(),
                    (p.display_name.clone(), p.unique_name.clone()),
                )
            })
            .collect();
        let me = self.list.me.to_ascii_lowercase();
        let view = cx.entity().downgrade();
        let images = cx.entity().downgrade();
        RichText::new(id, doc)
            .mentions(move |id| {
                let id = id.to_ascii_lowercase();
                people.get(&id).map(|(name, unique)| MentionName {
                    name: name.clone().into(),
                    unique_name: unique.clone().map(Into::into),
                    is_me: id == me,
                })
            })
            .on_link(move |target, window, cx| {
                let target = target.clone();
                view.update(cx, |this, cx| match target {
                    LinkTarget::WorkItem(id) => this.open_work_item(i64::from(id), window, cx),
                    LinkTarget::PullRequest(id) => cx.emit(WorkItemsEvent::OpenPr(i64::from(id))),
                    LinkTarget::Web(url) => cx.open_url(&url),
                })
                .ok();
            })
            .on_image(move |_, _, cx| {
                images.update(cx, |this, cx| this.open_in_ado(cx)).ok();
            })
            .max_height((clip).then_some(CLIP_HEIGHT))
    }

    /// A rendered long-text block, clipped with "Show all · N lines".
    fn clipped(
        &self,
        id: &'static str,
        doc: &RichDoc,
        expanded: bool,
        show_all: Option<Stop>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme().clone();
        let clip = doc.lines > CLIP_LINES && !expanded;
        let mut block = div()
            .max_w(px(size::DETAIL_PROSE_MAX))
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .child(self.rich_text(id, doc.doc.clone(), clip, cx));
        if clip && let Some(stop) = show_all {
            let cursor = self.ui.cursor == Some(stop) && self.detail_focus.is_focused(window);
            block = block.child(
                stop_frame(
                    ghost(
                        format!("wi-stop-{}", stop_key(stop)),
                        None,
                        Some(format!("Show all · {} lines", doc.lines).into()),
                        None,
                        false,
                        &theme,
                    ),
                    cursor,
                    false,
                    &theme,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if stop == Stop::DescriptionShowAll {
                        this.ui.show_all_description = true;
                    }
                    cx.notify();
                })),
            );
        }
        block
    }

    pub(in crate::workitems) fn render_description(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self.detail().expect("rendered with a detail");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let gate = self.gate(cx);
        let writable = matches!(gate, Gate::Editable | Gate::Offline);
        let draft = detail.draft(DraftKind::Description).cloned();
        let blocked = match &detail.editable {
            Editable::Blocked(what) => Some(what.clone()),
            _ => None,
        };
        let editing = self.ui.description.is_some();
        let cursor =
            self.ui.cursor == Some(Stop::Description) && self.detail_focus.is_focused(window);

        // Head: label, Draft chip, Write/Preview or the Edit button.
        let mut head = section_head("Description", None, &theme).child(div().flex_1());
        if let Some(draft) = draft.as_ref().filter(|_| !editing) {
            head = head.child(
                div()
                    .id("wi-description-draft")
                    .tooltip(Tooltip::text(format!(
                        "Unsaved description draft from {} · d to resume",
                        crate::review::data::age(self.now(), draft.updated)
                    )))
                    .child(Chip::new("Draft")),
            );
        }
        if let Some(editor) = &self.ui.description {
            let selected = usize::from(editor.previewing);
            head = head
                .child(
                    Segmented::new("wi-description-mode", ["Write", "Preview"], selected)
                        .on_select({
                            let view = cx.entity().downgrade();
                            move |ix, window, cx| {
                                view.update(cx, |this, cx| {
                                    let previewing =
                                        this.ui.description.as_ref().is_some_and(|e| e.previewing);
                                    if (ix == 1) != previewing {
                                        this.toggle_preview(window, cx);
                                    }
                                })
                                .ok();
                            }
                        }),
                )
                .child(kbd(super::PREVIEW_KEYS));
        } else if blocked.is_some() {
            head = head.child(
                ghost(
                    "wi-description-open",
                    Some(IconName::ExternalLink),
                    Some("Edit in Azure DevOps".into()),
                    Some(OPEN_KEYS),
                    self.list.demo,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| this.open_in_ado(cx))),
            );
        } else if writable && detail.description.is_some() {
            let label: SharedString = if draft.is_some() {
                "Resume draft".into()
            } else {
                "Edit".into()
            };
            head = head.child(
                ghost(
                    "wi-description-edit",
                    Some(IconName::Pencil),
                    Some(label),
                    Some(DESCRIPTION_KEYS),
                    false,
                    &theme,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.ui.from_list = false;
                    this.intend(Intent::Description, window, cx)
                })),
            );
        }

        let mut body = section("wi-description")
            .role(gpui::Role::Article)
            .aria_label("Description")
            .child(head)
            .children(self.status_lines(EditField::Description, &theme, cx));

        if let Some(what) = &blocked {
            let notice_cursor = self.ui.cursor == Some(Stop::DescriptionNotice)
                && self.detail_focus.is_focused(window);
            let copy = format!(
                "This description uses formatting adoken can't edit yet: {what}. Edit it in Azure DevOps."
            );
            body = body.child(
                div()
                    .id(format!("wi-stop-{}", stop_key(Stop::DescriptionNotice)))
                    .max_w(px(size::DETAIL_PROSE_MAX))
                    .p(px(space::S12))
                    .flex()
                    .items_start()
                    .gap(px(space::S8))
                    .rounded(px(radius::SM))
                    .bg(colors.bg_raised)
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, &theme)
                    .role(gpui::Role::Note)
                    .aria_label(copy.clone())
                    .child(
                        div().pt(px(space::S2)).child(
                            Icon::new(IconName::Info)
                                .size(size::ICON_SM)
                                .color(colors.text_tertiary),
                        ),
                    )
                    .child(div().flex_1().child(copy))
                    .child(
                        stop_frame(
                            ghost(
                                "wi-blocked-open",
                                None,
                                Some("Open".into()),
                                Some(OPEN_KEYS),
                                self.list.demo,
                                &theme,
                            ),
                            notice_cursor,
                            false,
                            &theme,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.open_in_ado(cx))),
                    ),
            );
        }

        if let Some(editor) = &self.ui.description {
            return body
                .child(self.render_editor(editor, cx))
                .into_any_element();
        }

        // Compare (FR-19): theirs above, mine below.
        if self.ui.compare
            && let Some(mine) = draft.as_ref()
        {
            let mine = Arc::new(adoken_richtext::parse(
                &mine.body,
                adoken_richtext::Format::Markdown,
            ));
            let caption = |text: &'static str| {
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::LABEL, &theme)
                    .child(text)
            };
            let theirs = detail.description.as_ref().map(|d| d.doc.clone());
            let block = |content: AnyElement| {
                div()
                    .max_w(px(size::DETAIL_PROSE_MAX))
                    .p(px(space::S12))
                    .rounded(px(radius::SM))
                    .bg(colors.bg_raised)
                    .child(content)
            };
            body = body
                .child(caption("Theirs (now in Azure DevOps)"))
                .children(theirs.map(|doc| {
                    block(
                        self.rich_text("wi-compare-theirs", doc, true, cx)
                            .into_any_element(),
                    )
                }))
                .child(caption("Yours"))
                .child(block(
                    self.rich_text("wi-compare-mine", mine, true, cx)
                        .into_any_element(),
                ));
            return body.into_any_element();
        }

        let content = if detail.partly_synced() && detail.description.is_none() {
            skeleton(&theme).into_any_element()
        } else {
            match self
                .pending_description(cx)
                .as_ref()
                .or(detail.description.as_ref())
            {
                Some(doc) => {
                    let rendered = self.clipped(
                        "wi-description-text",
                        doc,
                        self.ui.show_all_description,
                        Some(Stop::DescriptionShowAll),
                        window,
                        cx,
                    );
                    stop_frame(
                        div()
                            .id(format!("wi-stop-{}", stop_key(Stop::Description)))
                            .p(px(space::S4))
                            .m(px(-space::S4))
                            .child(rendered),
                        cursor,
                        false,
                        &theme,
                    )
                    .into_any_element()
                }
                None => {
                    let empty = div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::BODY, &theme)
                        .child("No description.")
                        .when(writable && blocked.is_none(), |row| {
                            row.child(
                                ghost(
                                    "wi-description-add",
                                    None,
                                    Some("Add a description".into()),
                                    Some(DESCRIPTION_KEYS),
                                    false,
                                    &theme,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.ui.from_list = false;
                                        this.intend(Intent::Description, window, cx)
                                    },
                                )),
                            )
                        });
                    stop_frame(
                        div()
                            .id(format!("wi-stop-{}", stop_key(Stop::Description)))
                            .child(empty),
                        cursor,
                        false,
                        &theme,
                    )
                    .into_any_element()
                }
            }
        };
        body.child(content).into_any_element()
    }

    /// The saved-but-unconfirmed description renders at once (design
    /// §5.2 Saving): parsed from the pending Markdown.
    fn pending_description(&self, cx: &Context<Self>) -> Option<RichDoc> {
        let value = self.pending_value(EditField::Description, cx)?;
        let format = match self.detail()?.editable {
            Editable::Markdown(_) => adoken_richtext::Format::Markdown,
            _ => adoken_richtext::Format::Html,
        };
        let doc = Arc::new(adoken_richtext::parse(&value, format));
        let lines = doc.line_estimate(96);
        Some(RichDoc { doc, lines })
    }

    /// Design §5.2: Write / Preview, the format hint, Draft saved, Save and
    /// Cancel.
    fn render_editor(
        &self,
        editor: &super::DescriptionEditor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let unchanged = editor.area.read(cx).text() == editor.base;
        let offline = self.gate(cx) == Gate::Offline;
        let field = if editor.previewing {
            div()
                .id("wi-description-preview")
                .track_focus(&editor.focus)
                .min_h(px(8.0 * 18.0))
                .p(px(space::S12))
                .rounded(px(radius::SM))
                .bg(colors.bg_surface)
                .border_1()
                .border_color(colors.border_default)
                .children(
                    editor
                        .preview
                        .clone()
                        .map(|doc| self.rich_text("wi-description-preview-text", doc, false, cx)),
                )
                .into_any_element()
        } else {
            div()
                .p(px(space::S12))
                .rounded(px(radius::SM))
                .bg(colors.bg_input)
                .border_1()
                .border_color(colors.border_focus)
                .text_color(colors.text_primary)
                .type_style(typography::CODE, &theme)
                .child(editor.area.clone())
                .into_any_element()
        };
        let hint = if editor.markdown_field {
            div().child("Markdown").into_any_element()
        } else {
            div()
                .id("wi-format-hint")
                .flex()
                .items_center()
                .gap(px(space::S4))
                .tooltip(Tooltip::text(
                    "This description is stored as HTML in Azure DevOps. Write Markdown here; adoken saves it back as HTML.",
                ))
                .child("HTML (saved as HTML)")
                .child(
                    Icon::new(IconName::Info)
                        .size(size::ICON_XS)
                        .color(colors.text_tertiary),
                )
                .into_any_element()
        };
        div()
            .id("wi-description-editor")
            .key_context(DESCRIPTION_EDITOR_CONTEXT)
            .max_w(px(size::DETAIL_PROSE_MAX))
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .child(field)
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, &theme)
                    .child(hint)
                    .when(editor.draft_saved, |row| row.child("Draft saved")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(space::S8))
                    .child(
                        Button::new("wi-description-save", "Save")
                            .variant(ButtonVariant::Primary)
                            .shortcut(kbd(super::SAVE_KEYS))
                            .disabled(unchanged || offline)
                            .tooltip(if offline {
                                "Offline — reconnect to send"
                            } else {
                                "Save  ⌘↩"
                            })
                            .on_press(crate::browse::press(cx, |this: &mut Self, window, cx| {
                                this.save_description(window, cx)
                            })),
                    )
                    .child(
                        Button::new("wi-description-cancel", "Cancel")
                            .variant(ButtonVariant::Ghost)
                            .shortcut(kbd("escape"))
                            .on_press(crate::browse::press(cx, |this: &mut Self, window, cx| {
                                this.close_description(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    /// Repro steps / Acceptance criteria: read-only, own heads (Q-10).
    pub(in crate::workitems) fn render_extra_text(
        &self,
        id: &'static str,
        label: &'static str,
        doc: &RichDoc,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        section(id)
            .role(gpui::Role::Article)
            .aria_label(label)
            .child(section_head(label, None, &theme))
            .child(self.clipped(id, doc, false, None, window, cx))
            .into_any_element()
    }
}

/// Three skeleton lines (design §12 Partly synced): widths 100 / 92 / 60 %.
pub(super) fn skeleton(theme: &Theme) -> Div {
    let bar = |ix: usize, width: f32| {
        div()
            .id(("wi-skeleton", ix))
            .h(px(size::PIN_SKELETON_BAR))
            .w(gpui::relative(width))
            .rounded(px(radius::XS))
            .bg(theme.colors.bg_raised)
    };
    div()
        .max_w(px(size::DETAIL_PROSE_MAX))
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .child(bar(0, 1.0))
        .child(bar(1, 0.92))
        .child(bar(2, 0.6))
}
