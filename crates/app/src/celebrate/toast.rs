//! The Milestone toast (ken.md §12.5): leading `sparkles`, the title
//! ("Moved to Done", "Merged into main") and a `type.small` body ("#4821
//! Fix timezone bug…"). Standard toast stack, 5 s, no action; the arcade
//! style adds the 3 px `ken.primary` stripe like badge toasts.

use adoken_theme::{elevation, radius, size, space, typography};
use adoken_ui::{ActiveTheme, Icon, IconName, TypeStyleExt};
use gpui::{App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use super::Anchor;

pub const TOAST_WIDTH: f32 = 340.0;

#[derive(IntoElement, Clone, Debug)]
pub struct CelebrationToast {
    pub id: u64,
    pub title: SharedString,
    pub body: SharedString,
    /// Arcade style: the ken stripe.
    pub stripe: bool,
}

impl RenderOnce for CelebrationToast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        div()
            .id(("celebration-toast", self.id as usize))
            .w(px(TOAST_WIDTH))
            .relative()
            .overflow_hidden()
            .flex()
            .items_start()
            .gap(px(space::S8))
            .pl(px(space::S12 + if self.stripe { 3.0 } else { 0.0 }))
            .pr(px(space::S12))
            .py(px(space::S8))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_2))
            .role(gpui::Role::Status)
            .aria_label(SharedString::from(format!("{}. {}", self.title, self.body)))
            // A burst with no visible anchor rises from here.
            .children(super::track(Anchor::Toast(self.id), cx))
            .when(self.stripe, |toast| {
                toast.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.0))
                        .bg(colors.ken_primary),
                )
            })
            .child(
                div().pt(px(2.0)).flex_none().child(
                    Icon::new(IconName::Sparkles)
                        .size(size::ICON_SM)
                        .color(colors.status_success_fg),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY_STRONG, &theme)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, &theme)
                            .truncate()
                            .child(self.body),
                    ),
            )
    }
}
