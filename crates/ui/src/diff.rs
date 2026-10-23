//! Diff rows (spec 0004 §3): unified and side-by-side line rows, fold
//! expanders, token-colored code. Stateless `RenderOnce` pieces: the view
//! owns layout ([`adoken_diff::rows`]) and virtualization, and renders only
//! the rows on screen, so a row must be cheap: one flex strip, one
//! `StyledText` with pre-merged highlight runs.
//!
//! Colors: `bg.code` plane, `diff.*` washes flattened over it, `syntax.*`
//! for tokens (tokens.md §2.6–2.7).

use std::ops::Range;

use adoken_diff::LineKind;
use adoken_diff::lex::{self, Lang, TokenKind};
use adoken_theme::{ColorTokens, size, space, typography};
use gpui::{
    App, ElementId, FontStyle, HighlightStyle, Hsla, IntoElement, RenderOnce, SharedString,
    StyledText, Window, combine_highlights, div, prelude::*, px,
};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};

/// Monospace advance as a fraction of the font size (Menlo, JetBrains Mono
/// and Consolas are all 0.6 em); sizes gutters without shaping text.
const MONO_ADVANCE_EM: f32 = 0.6;
/// Gutter bar on changed lines (tokens.md §2.6: "2 px gutter bar").
const GUTTER_BAR: f32 = 2.0;

/// Height of every diff row: `type.code` line height at the configured
/// code font size (spec 0009 §3 Code font). Virtualization uses it as the row
/// height, so a size change must reset the list.
pub fn row_height(theme: &Theme) -> f32 {
    theme.resolve(typography::CODE).line_height
}

/// Digits needed for the widest line number, at least 3 so gutters don't
/// jump between small files.
pub fn gutter_digits(max_line: usize) -> usize {
    max_line.max(1).ilog10() as usize + 1
}

fn number_width(digits: usize, theme: &Theme) -> f32 {
    digits.max(3) as f32 * theme.code_font.size * MONO_ADVANCE_EM + space::S12
}

pub fn token_color<C: Copy>(kind: TokenKind, colors: &ColorTokens<C>) -> C {
    match kind {
        TokenKind::Keyword => colors.syntax_keyword,
        TokenKind::String => colors.syntax_string,
        TokenKind::Number => colors.syntax_number,
        TokenKind::Constant => colors.syntax_constant,
        TokenKind::Comment => colors.syntax_comment,
        TokenKind::Function => colors.syntax_function,
        TokenKind::Type => colors.syntax_type,
        TokenKind::Property => colors.syntax_property,
        TokenKind::Operator => colors.syntax_operator,
        TokenKind::Punctuation => colors.syntax_punctuation,
        TokenKind::Tag => colors.syntax_tag,
        TokenKind::Attribute => colors.syntax_attribute,
    }
}

struct LinePaint {
    background: Hsla,
    word: Option<Hsla>,
    bar: Option<Hsla>,
    sign: &'static str,
}

fn line_paint(kind: Option<LineKind>, cx: &App) -> LinePaint {
    let theme = cx.theme();
    let (raw, colors) = (&theme.raw, &theme.colors);
    match kind {
        Some(LineKind::Add) => LinePaint {
            background: theme.flatten(raw.diff_add_bg, raw.bg_code),
            word: Some(theme.flatten(raw.diff_add_word, raw.diff_add_bg.blend_over(raw.bg_code))),
            bar: Some(colors.diff_add_gutter),
            sign: "+",
        },
        Some(LineKind::Del) => LinePaint {
            background: theme.flatten(raw.diff_del_bg, raw.bg_code),
            word: Some(theme.flatten(raw.diff_del_word, raw.diff_del_bg.blend_over(raw.bg_code))),
            bar: Some(colors.diff_del_gutter),
            sign: "−",
        },
        Some(LineKind::Context) => LinePaint {
            background: colors.bg_code,
            word: None,
            bar: None,
            sign: " ",
        },
        // Empty half of a split row.
        None => LinePaint {
            background: theme.flatten(raw.bg_active, raw.bg_code),
            word: None,
            bar: None,
            sign: " ",
        },
    }
}

/// One line of code: `syntax.*` token colors plus `diff.*.word` washes on
/// the intraline ranges. Plain identifiers stay `syntax.variable`.
#[derive(IntoElement)]
pub struct CodeText {
    text: SharedString,
    lang: Lang,
    words: Vec<Range<usize>>,
    word_color: Option<Hsla>,
}

impl CodeText {
    pub fn new(text: impl Into<SharedString>, lang: Lang) -> Self {
        Self {
            text: text.into(),
            lang,
            words: Vec::new(),
            word_color: None,
        }
    }

    pub fn words(mut self, words: Vec<Range<usize>>, color: Hsla) -> Self {
        self.words = words;
        self.word_color = Some(color);
        self
    }
}

impl RenderOnce for CodeText {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let tokens = lex::tokenize(&self.text, self.lang)
            .into_iter()
            .map(|token| {
                let style = HighlightStyle {
                    color: Some(token_color(token.kind, colors)),
                    font_style: (token.kind == TokenKind::Comment).then_some(FontStyle::Italic),
                    ..HighlightStyle::default()
                };
                (token.range, style)
            });
        let words = self.word_color.into_iter().flat_map(|color| {
            self.words.iter().cloned().map(move |range| {
                (
                    range,
                    HighlightStyle {
                        background_color: Some(color),
                        ..HighlightStyle::default()
                    },
                )
            })
        });
        let highlights: Vec<_> = combine_highlights(tokens, words).collect();
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_color(colors.syntax_variable)
            .child(StyledText::new(self.text).with_highlights(highlights))
    }
}

/// One side of a diff line: what a unified row shows once and a split row
/// shows per column.
#[derive(Clone)]
pub struct DiffCell {
    pub kind: LineKind,
    pub number: Option<u32>,
    pub text: SharedString,
    pub words: Vec<Range<usize>>,
}

fn gutter_number(number: Option<u32>, width: f32, color: Hsla) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(width))
        .pr(px(space::S8))
        .flex()
        .justify_end()
        .text_color(color)
        .children(number.map(|n| n.to_string()))
}

fn comment_marker(show: bool, cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .w(px(size::ICON_SM))
        .flex()
        .items_center()
        .children(show.then(|| {
            Icon::new(IconName::MessageSquare)
                .size(size::ICON_XS)
                .color(cx.theme().colors.diff_comment)
        }))
}

fn strip(
    id: ElementId,
    height: f32,
    background: Hsla,
    bar: Option<Hsla>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .h(px(height))
        .bg(background)
        .border_l(px(GUTTER_BAR))
        .border_color(bar.unwrap_or(background))
}

/// Unified row: `old | new | marker | sign | code`.
#[derive(IntoElement)]
pub struct UnifiedLine {
    id: ElementId,
    kind: LineKind,
    old: Option<u32>,
    new: Option<u32>,
    text: SharedString,
    lang: Lang,
    words: Vec<Range<usize>>,
    digits: usize,
    comment: bool,
}

impl UnifiedLine {
    pub fn new(
        id: impl Into<ElementId>,
        kind: LineKind,
        old: Option<u32>,
        new: Option<u32>,
        text: impl Into<SharedString>,
        lang: Lang,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            old,
            new,
            text: text.into(),
            lang,
            words: Vec::new(),
            digits: 3,
            comment: false,
        }
    }

    pub fn words(mut self, words: Vec<Range<usize>>) -> Self {
        self.words = words;
        self
    }

    pub fn digits(mut self, digits: usize) -> Self {
        self.digits = digits;
        self
    }

    /// A thread is anchored here (`diff.comment` marker).
    pub fn comment(mut self, comment: bool) -> Self {
        self.comment = comment;
        self
    }
}

impl RenderOnce for UnifiedLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let paint = line_paint(Some(self.kind), cx);
        let theme = cx.theme();
        let number = theme.colors.text_tertiary;
        let sign_color = paint.bar.unwrap_or(number);
        let mut code = CodeText::new(self.text, self.lang);
        if let Some(word) = paint.word {
            code = code.words(self.words, word);
        }
        let gutter = number_width(self.digits, theme);
        strip(self.id, row_height(theme), paint.background, paint.bar)
            .w_full()
            .type_style(typography::CODE, theme)
            .child(gutter_number(self.old, gutter, number))
            .child(gutter_number(self.new, gutter, number))
            .child(comment_marker(self.comment, cx))
            .child(
                div()
                    .flex_none()
                    .w(px(space::S16))
                    .text_color(sign_color)
                    .child(paint.sign),
            )
            .child(code)
    }
}

/// Side-by-side row: old column | new column. `None` paints an empty half.
#[derive(IntoElement)]
pub struct SplitLine {
    id: ElementId,
    left: Option<DiffCell>,
    right: Option<DiffCell>,
    lang: Lang,
    digits: usize,
    comment_left: bool,
    comment_right: bool,
}

impl SplitLine {
    pub fn new(
        id: impl Into<ElementId>,
        left: Option<DiffCell>,
        right: Option<DiffCell>,
        lang: Lang,
    ) -> Self {
        Self {
            id: id.into(),
            left,
            right,
            lang,
            digits: 3,
            comment_left: false,
            comment_right: false,
        }
    }

    pub fn digits(mut self, digits: usize) -> Self {
        self.digits = digits;
        self
    }

    pub fn comments(mut self, left: bool, right: bool) -> Self {
        self.comment_left = left;
        self.comment_right = right;
        self
    }
}

fn split_half(
    id: ElementId,
    cell: Option<DiffCell>,
    lang: Lang,
    digits: usize,
    comment: bool,
    cx: &App,
) -> impl IntoElement {
    let paint = line_paint(cell.as_ref().map(|c| c.kind), cx);
    let theme = cx.theme();
    let number = theme.colors.text_tertiary;
    let sign_color = paint.bar.unwrap_or(number);
    let (number_value, code) = match cell {
        Some(cell) => {
            let mut code = CodeText::new(cell.text, lang);
            if let Some(word) = paint.word {
                code = code.words(cell.words, word);
            }
            (cell.number, Some(code))
        }
        None => (None, None),
    };
    strip(id, row_height(theme), paint.background, paint.bar)
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .child(gutter_number(
            number_value,
            number_width(digits, theme),
            number,
        ))
        .child(comment_marker(comment, cx))
        .child(
            div()
                .flex_none()
                .w(px(space::S16))
                .text_color(sign_color)
                .child(paint.sign),
        )
        .children(code)
}

impl RenderOnce for SplitLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let divider = theme.colors.border_subtle;
        let left_id = ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "left".into());
        let right_id = ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "right".into());
        div()
            .flex()
            .w_full()
            .h(px(row_height(theme)))
            .type_style(typography::CODE, theme)
            .child(split_half(
                left_id,
                self.left,
                self.lang,
                self.digits,
                self.comment_left,
                cx,
            ))
            .child(div().w(px(size::BORDER)).h_full().bg(divider))
            .child(split_half(
                right_id,
                self.right,
                self.lang,
                self.digits,
                self.comment_right,
                cx,
            ))
    }
}

type ExpandHandler = Box<dyn Fn(&mut Window, &mut App)>;

/// Collapsed unchanged lines (`diff.hunk.*`): "⋯ 42 unchanged lines".
/// Click or `Enter` on the diff's cursor expands it.
#[derive(IntoElement)]
pub struct FoldLine {
    id: ElementId,
    hidden: u32,
    selected: bool,
    on_expand: Option<ExpandHandler>,
}

impl FoldLine {
    pub fn new(id: impl Into<ElementId>, hidden: u32) -> Self {
        Self {
            id: id.into(),
            hidden,
            selected: false,
            on_expand: None,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_expand(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_expand = Some(Box::new(handler));
        self
    }
}

pub fn fold_label(hidden: u32) -> String {
    match hidden {
        1 => "1 unchanged line".to_owned(),
        n => format!("{n} unchanged lines"),
    }
}

impl RenderOnce for FoldLine {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (raw, colors) = (&theme.raw, &theme.colors);
        let background = theme.flatten(raw.diff_hunk_bg, raw.bg_code);
        let hover = theme.flatten(raw.bg_hover, raw.diff_hunk_bg.blend_over(raw.bg_code));
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(space::S6))
            .h(px(row_height(theme)))
            .px(px(space::S12))
            .bg(if self.selected { hover } else { background })
            .text_color(colors.diff_hunk_fg)
            .type_style(typography::CODE, theme)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(size::ICON_XS)
                    .color(colors.diff_hunk_fg),
            )
            .child(fold_label(self.hidden))
            .when_some(self.on_expand, |this, handler| {
                this.on_click(move |_, window, cx| handler(window, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::ThemeRegistry;

    #[test]
    fn gutter_digits_counts_decimal_width() {
        assert_eq!(gutter_digits(0), 1);
        assert_eq!(gutter_digits(9), 1);
        assert_eq!(gutter_digits(10), 2);
        assert_eq!(gutter_digits(20_000), 5);
    }

    #[test]
    fn syntax_is_readable_on_every_diff_wash_in_every_theme() {
        // tokens.md §2.6: syntax.* passes 4.5:1 on bg.code and each diff.*
        // wash flattened over it; this pins the pairs this component paints.
        let kinds = [
            TokenKind::Keyword,
            TokenKind::String,
            TokenKind::Number,
            TokenKind::Function,
            TokenKind::Type,
            TokenKind::Property,
        ];
        for variant in ThemeRegistry::builtin().variants() {
            let c = &variant.colors;
            for wash in [c.diff_add_bg, c.diff_del_bg] {
                let base = wash.blend_over(c.bg_code);
                for kind in kinds {
                    let ratio = token_color(kind, c).contrast(base);
                    assert!(ratio >= 4.5, "{} {kind:?}: {ratio:.2}", variant.id);
                }
            }
        }
    }
}
