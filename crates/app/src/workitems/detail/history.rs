//! History (design §8): newest first, the last five then "Show all {n}",
//! and the note that earlier revisions aren't synced.

use adoken_theme::{size, space, typography};
use adoken_ui::{ActiveTheme, Icon, IconName, TypeStyleExt};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

use super::model::{self, Stop};
use super::status::{ghost, section, section_head, stop_frame};
use super::stop_key;
use crate::review::data::age;
use crate::workitems::WorkItemsView;
use crate::workitems::data::HistoryKind;

impl WorkItemsView {
    pub(in crate::workitems) fn render_history(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let detail = self.detail().expect("rendered with a detail");
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let now = self.now();
        let shown = model::visible_history(detail.history.len(), self.ui.show_all_history);
        let rows = detail
            .history
            .iter()
            .take(shown)
            .enumerate()
            .map(|(ix, h)| {
                let glyph = match h.kind {
                    HistoryKind::Created => IconName::CircleDot,
                    HistoryKind::Changed => IconName::RefreshCw,
                    HistoryKind::Assigned => IconName::User,
                    HistoryKind::Mentioned => IconName::AtSign,
                };
                div()
                    .id(("wi-history", ix))
                    .h(px(size::CONTROL_MD))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .type_style(typography::BODY, &theme)
                    .role(gpui::Role::ListItem)
                    .aria_label(format!("{}, {}", h.text, age(now, h.at)))
                    .child(
                        Icon::new(glyph)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_secondary)
                            .child(h.text.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(age(now, h.at)),
                    )
            });
        let mut body = section("wi-history")
            .role(gpui::Role::List)
            .aria_label("History")
            .max_w(px(size::DETAIL_PROSE_MAX))
            .child(section_head("History", None, &theme))
            .children(rows);
        if shown < detail.history.len() {
            let stop = Stop::HistoryShowAll;
            let cursor = self.ui.cursor == Some(stop) && self.detail_focus.is_focused(window);
            body = body.child(
                stop_frame(
                    ghost(
                        format!("wi-stop-{}", stop_key(stop)),
                        None,
                        Some(format!("Show all {}", detail.history.len()).into()),
                        None,
                        false,
                        &theme,
                    ),
                    cursor,
                    false,
                    &theme,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.ui.show_all_history = true;
                    cx.notify();
                })),
            );
        }
        body.child(
            div()
                .text_color(colors.text_tertiary)
                .type_style(typography::SMALL, &theme)
                .child("Earlier revisions aren't synced; history shows what sync observed."),
        )
        .into_any_element()
    }
}
