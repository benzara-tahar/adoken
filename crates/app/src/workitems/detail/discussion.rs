//! Discussion (design §7): comments oldest first with the last five shown
//! ("Show {n} earlier comments"), avatars, "You", mentions, pending and
//! failed posts, and the composer with its @mention list.

use adoken_theme::{radius, size, space, typography};
use adoken_ui::{ActiveTheme, Button, ButtonVariant, Chip, Icon, IconName, Tooltip, TypeStyleExt};
use gpui::{AnyElement, Context, Focusable, Window, div, prelude::*, px};

use super::model::{self, Stop};
use super::status::{ghost, kbd, pending_line, section, section_head, stop_frame};
use super::{COMPOSER_CONTEXT_EDITING, COMPOSER_MENTION_CONTEXT, Intent, SAVE_KEYS, stop_key};
use crate::avatar::{Avatar, AvatarSize};
use crate::review::data::age;
use crate::workitems::WorkItemsView;
use crate::workitems::writes::{CommentStatus, Gate};

impl WorkItemsView {
    pub(in crate::workitems) fn render_discussion(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self.detail().expect("rendered with a detail");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let now = self.now();
        let focused = self.detail_focus.is_focused(window);
        let comments = &detail.comments;
        let pending = Self::edits(cx)
            .map(|e| e.read(cx).pending_comments(detail.item.id).to_vec())
            .unwrap_or_default();
        let total = comments.len() + pending.len();
        let first = model::first_visible_comment(comments.len(), self.ui.show_all_comments);
        let mut body = section("wi-discussion")
            .role(gpui::Role::List)
            .aria_label(format!("Discussion, {total} comments"))
            .child(section_head(
                "Discussion",
                (total > 0).then_some(total),
                &theme,
            ));
        if first > 0 {
            let stop = Stop::EarlierComments;
            body = body.child(
                stop_frame(
                    ghost(
                        format!("wi-stop-{}", stop_key(stop)),
                        None,
                        Some(format!("Show {first} earlier comments").into()),
                        None,
                        false,
                        &theme,
                    ),
                    focused && self.ui.cursor == Some(stop),
                    false,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ui.show_all_comments = true;
                    cx.notify();
                })),
            );
        }
        if total == 0 {
            body = body.child(
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::BODY, &theme)
                    .child("No comments yet."),
            );
        }
        let mut thread = div().flex().flex_col().gap(px(space::S16));
        for (ix, comment) in comments.iter().enumerate().skip(first) {
            let mine = comment.author == self.list.me;
            let name = if mine {
                "You".to_owned()
            } else {
                comment.name.clone()
            };
            let cursor = focused && self.ui.cursor == Some(Stop::Comment(ix));
            let spoken = format!("{name}, {}: {}", age(now, comment.at), comment.plain);
            let row = div()
                .id(format!("wi-stop-{}", stop_key(Stop::Comment(ix))))
                .flex()
                .gap(px(space::S12))
                .p(px(space::S4))
                .m(px(-space::S4))
                .role(gpui::Role::ListItem)
                .aria_label(spoken)
                .child(div().w(px(size::AVATAR_LG)).flex_none().child(
                    Avatar::new(comment.author.clone(), comment.name.clone()).size(AvatarSize::Lg),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .max_w(px(size::DETAIL_PROSE_MAX - size::AVATAR_LG - space::S12))
                        .flex()
                        .flex_col()
                        .gap(px(space::S4))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(space::S8))
                                .child(
                                    div()
                                        .text_color(colors.text_primary)
                                        .type_style(typography::BODY_STRONG, &theme)
                                        .child(name),
                                )
                                .child(
                                    div()
                                        .text_color(colors.text_tertiary)
                                        .type_style(typography::SMALL, &theme)
                                        .child(age(now, comment.at))
                                        .when(comment.edited, |d| d.child(" · edited")),
                                ),
                        )
                        .child(self.rich_text(("wi-comment", ix), comment.doc.clone(), false, cx)),
                );
            thread = thread.child(stop_frame(row, cursor, false, &theme));
        }
        for comment in &pending {
            thread = thread.child(self.render_pending_comment(comment, cx));
        }
        body = body.child(thread);
        if matches!(self.gate(cx), Gate::Editable | Gate::Offline) {
            body = body.child(self.render_composer(window, cx));
        }
        body.into_any_element()
    }

    /// My comment between `⌘↩` and Azure DevOps' answer (design §7.1), or
    /// its failure with Retry / Edit (design §7.3).
    fn render_pending_comment(
        &self,
        comment: &crate::workitems::writes::PendingComment,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let now = self.now();
        let local = comment.local;
        let me = self.list.me.clone();
        let names = self.list.names.clone();
        let (text, _) = super::atoms_of(&comment.markdown, |id| {
            names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "Unknown user".into())
        });
        let status: AnyElement = match &comment.status {
            CommentStatus::Sending { since } => {
                pending_line(now - since >= 3_000, "Sending…", &theme)
            }
            CommentStatus::Failed { cause } => div()
                .flex()
                .flex_col()
                .gap(px(space::S4))
                .child(
                    div()
                        .flex()
                        .gap(px(space::S4))
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, &theme)
                        .child(
                            Icon::new(IconName::CircleX)
                                .size(size::ICON_XS)
                                .color(colors.status_danger_fg),
                        )
                        .child(format!(
                            "Couldn't post your comment: {cause}. Your text is saved."
                        )),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(space::S4))
                        .child(
                            ghost(
                                ("wi-comment-retry", local as usize),
                                None,
                                Some("Retry".into()),
                                None,
                                false,
                                &theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let (Some(edits), Some(id)) =
                                        (Self::edits(cx), this.detail_id())
                                    {
                                        edits.update(cx, |e, cx| e.retry_comment(id, local, cx));
                                    }
                                },
                            )),
                        )
                        .child(
                            ghost(
                                ("wi-comment-edit", local as usize),
                                None,
                                Some("Edit".into()),
                                None,
                                false,
                                &theme,
                            )
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.edit_failed_comment(local, window, cx)
                                },
                            )),
                        ),
                )
                .into_any_element(),
        };
        div()
            .id(("wi-pending-comment", local as usize))
            .flex()
            .gap(px(space::S12))
            .role(gpui::Role::ListItem)
            .aria_label(format!("You, sending: {text}"))
            .child(
                div()
                    .w(px(size::AVATAR_LG))
                    .flex_none()
                    .child(Avatar::new(me.clone(), "You").size(AvatarSize::Lg)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(space::S4))
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY_STRONG, &theme)
                            .child("You"),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::BODY, &theme)
                            .child(text),
                    )
                    .child(status),
            )
            .into_any_element()
    }

    /// Edit on a failed post: its text goes back into the composer.
    fn edit_failed_comment(&mut self, local: u64, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(edits), Some(id)) = (Self::edits(cx), self.detail_id()) else {
            return;
        };
        let Some(markdown) = edits.update(cx, |e, cx| e.take_comment(id, local, cx)) else {
            return;
        };
        let names = self.list.names.clone();
        let (text, atoms) = super::atoms_of(&markdown, |id| {
            names
                .get(id)
                .cloned()
                .unwrap_or_else(|| "Unknown user".into())
        });
        if let Some(composer) = &self.ui.composer {
            composer
                .area
                .update(cx, |area, cx| area.set_text(&text, atoms, cx));
        }
        self.intend(Intent::Comment, window, cx);
    }

    /// Design §7.3: collapsed one line, focused three, the footer with the
    /// hint and Comment.
    fn render_composer(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(composer) = &self.ui.composer else {
            return div().into_any_element();
        };
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let focused = composer.area.focus_handle(cx).contains_focused(window, cx);
        let cursor = self.ui.cursor == Some(Stop::Composer) && self.detail_focus.is_focused(window);
        let text = composer.area.read(cx).text().to_owned();
        let empty = text.trim().is_empty();
        let offline = self.gate(cx) == Gate::Offline;
        let has_draft = !empty && !focused;
        let context = if self.ui.mention.is_open() {
            COMPOSER_MENTION_CONTEXT
        } else {
            COMPOSER_CONTEXT_EDITING
        };
        let me = self.list.me.clone();
        let my_name = self.list.name(&me);
        let field = div()
            .id(format!("wi-stop-{}", stop_key(Stop::Composer)))
            .relative()
            .flex_1()
            .min_w_0()
            .min_h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .py(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::MD))
            .bg(colors.bg_input)
            .border_1()
            .border_color(if focused {
                colors.border_focus
            } else {
                colors.border_default
            })
            .text_color(if has_draft {
                colors.text_secondary
            } else {
                colors.text_primary
            })
            .type_style(typography::BODY, &theme)
            .child(adoken_ui::action_menu::track_bounds(
                self.ui.anchors.composer.clone(),
            ))
            .child(div().flex_1().min_w_0().child(composer.area.clone()))
            .when(has_draft, |f| f.child(Chip::new("Draft")))
            .when(cursor, |f| f.child(adoken_ui::focus_ring(&theme)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.ui.from_list = false;
                this.intend(Intent::Comment, window, cx)
            }));
        let footer = div()
            .flex()
            .items_center()
            .justify_between()
            .text_color(colors.text_tertiary)
            .type_style(typography::SMALL, &theme)
            .child("Markdown · @ to mention")
            .child(
                Button::new("wi-comment-post", "Comment")
                    .variant(ButtonVariant::Primary)
                    .shortcut(kbd(SAVE_KEYS))
                    .disabled(empty || offline)
                    .tooltip(if offline {
                        "Offline — reconnect to send"
                    } else {
                        "Comment  ⌘↩"
                    })
                    .on_press(crate::browse::press(cx, |this: &mut Self, window, cx| {
                        this.post_comment(window, cx)
                    })),
            );
        div()
            .id("wi-composer")
            .key_context(context)
            .flex()
            .gap(px(space::S12))
            .child(
                div()
                    .w(px(size::AVATAR_LG))
                    .flex_none()
                    .child(Avatar::new(me, my_name).size(AvatarSize::Lg)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .max_w(px(size::DETAIL_PROSE_MAX - size::AVATAR_LG - space::S12))
                    .flex()
                    .flex_col()
                    .gap(px(space::S8))
                    .child(field)
                    .when(focused, |c| c.child(footer)),
            )
            .children(self.ui.mention.element())
            .tooltip(Tooltip::text("Comment  c"))
            .into_any_element()
    }
}
