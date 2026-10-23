//! A theme's miniature: sidebar, list with a selected row, a diff-ish code
//! block, an accent button and status dots, painted in *that* theme's
//! tokens (not the active theme's) so every card shows its own look.
//! Layout after zeron `crates/ui/src/settings/appearance.rs` (`miniature`)
//! at d1010657d69c9730f8020edc36cc2649b136b6fa (MIT).

use adoken_theme::{ColorTokens, radius, space};
use gpui::{App, Div, Hsla, IntoElement, RenderOnce, Window, div, prelude::*, px, relative};

/// Height of a card's miniature.
pub const MINIATURE_HEIGHT: f32 = 92.0;
const BAR_HEIGHT: f32 = 4.0;
const SIDEBAR_WIDTH: f32 = 34.0;

/// Convert every token once per registry change, not per frame.
pub fn paint_colors(colors: &ColorTokens) -> ColorTokens<Hsla> {
    colors.map(|color| adoken_ui::theme::hsla(*color))
}

#[derive(IntoElement)]
pub struct Miniature {
    colors: ColorTokens<Hsla>,
}

impl Miniature {
    pub fn new(colors: ColorTokens<Hsla>) -> Self {
        Self { colors }
    }
}

fn bar(width: f32, color: Hsla) -> Div {
    div()
        .h(px(BAR_HEIGHT))
        .w(relative(width))
        .rounded(px(radius::XS))
        .bg(color)
}

fn code_line(colors: &ColorTokens<Hsla>, wash: Option<Hsla>, parts: &[(f32, Hsla)]) -> Div {
    div()
        .flex()
        .gap(px(space::S2))
        .px(px(space::S4))
        .py(px(1.0))
        .when_some(wash, |line, wash| line.bg(wash))
        .children(parts.iter().map(|(width, color)| bar(*width, *color)))
        .when(parts.is_empty(), |line| {
            line.child(bar(0.1, colors.syntax_punctuation))
        })
}

impl RenderOnce for Miniature {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let c = self.colors;
        let sidebar = div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(space::S6))
            .px(px(space::S6))
            .pt(px(space::S12))
            .bg(c.bg_sidebar)
            .border_r_1()
            .border_color(c.border_subtle)
            .child(bar(0.8, c.text_primary))
            .child(bar(1.0, c.text_tertiary))
            .child(bar(0.7, c.accent_fg))
            .child(bar(0.9, c.text_tertiary));
        let row = |selected: bool, dot: Hsla, width: f32| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .px(px(space::S4))
                .py(px(space::S2))
                .rounded(px(radius::XS))
                .when(selected, |row| row.bg(c.bg_selected))
                .child(div().size(px(BAR_HEIGHT + 1.0)).rounded_full().bg(dot))
                .child(bar(width, c.text_secondary))
        };
        let list = div()
            .flex()
            .flex_col()
            .gap(px(space::S2))
            .child(row(false, c.pipeline_succeeded, 0.7))
            .child(row(true, c.pr_active, 0.55))
            .child(row(false, c.vote_rejected, 0.62));
        let code = div()
            .flex()
            .flex_col()
            .py(px(space::S4))
            .rounded(px(radius::XS))
            .bg(c.bg_code)
            .border_1()
            .border_color(c.border_default)
            .child(code_line(
                &c,
                None,
                &[
                    (0.14, c.syntax_keyword),
                    (0.22, c.syntax_function),
                    (0.08, c.syntax_punctuation),
                ],
            ))
            .child(code_line(
                &c,
                Some(c.diff_del_bg),
                &[(0.1, c.syntax_keyword), (0.3, c.syntax_string)],
            ))
            .child(code_line(
                &c,
                Some(c.diff_add_bg),
                &[
                    (0.1, c.syntax_keyword),
                    (0.18, c.syntax_type),
                    (0.12, c.syntax_number),
                ],
            ))
            .child(code_line(&c, None, &[(0.36, c.syntax_comment)]));
        let button = div()
            .h(px(space::S12))
            .w(px(space::S32))
            .rounded(px(radius::XS))
            .bg(c.accent_solid)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .h(px(2.0))
                    .w(px(space::S16))
                    .rounded_full()
                    .bg(c.accent_on),
            );
        let pane = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .p(px(space::S6))
            .bg(c.bg_surface)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(bar(0.4, c.text_primary))
                    .child(button),
            )
            .child(list)
            .child(code);
        div()
            .h(px(MINIATURE_HEIGHT))
            .w_full()
            .flex()
            .overflow_hidden()
            .bg(c.bg_app)
            .child(sidebar)
            .child(pane)
    }
}
