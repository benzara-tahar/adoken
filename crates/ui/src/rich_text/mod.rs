//! `RichText`: renders an [`adoken_richtext::Doc`] (work item descriptions
//! and comments, spec 0015 FR-3) with theme tokens only, per
//! docs/design/work-item-detail.md §5.1 and §7.2. Nothing from the source's
//! styling is applied and nothing is fetched: images are chips.
//!
//! Parse off the UI thread ([`parse_in_background`]) and keep the
//! `Arc<Doc>` per `(id, rev)`. The element flattens the document into
//! styled strings once per document (kept in element state) so a frame
//! only builds highlight runs.

mod prepare;

use std::rc::Rc;
use std::sync::Arc;

use adoken_richtext::Marks;
/// The model types a caller needs next to the element.
pub use adoken_richtext::{Doc, Format, LinkTarget};
use adoken_theme::{radius, size, space, typography};
use gpui::{
    AnyElement, App, AppContext as _, ElementId, FontStyle, FontWeight, HighlightStyle,
    InteractiveText, IntoElement, RenderOnce, SharedString, StrikethroughStyle, StyledText, Task,
    UnderlineStyle, Window, div, prelude::*, px,
};

pub use prepare::MentionName;
use prepare::{Hit, MentionKind, PBlock, PItem, PText, Seg};

use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};
use crate::tooltip::Tooltip;

/// Design §5.1 image chip tooltip.
pub const IMAGE_TOOLTIP: &str = "Images open in Azure DevOps  o";

/// Parse on the background executor (FR-2: never on the UI thread).
pub fn parse_in_background(cx: &App, source: String, format: Format) -> Task<Arc<Doc>> {
    cx.background_spawn(async move { Arc::new(adoken_richtext::parse(&source, format)) })
}

type MentionLookup = Rc<dyn Fn(&str) -> Option<MentionName>>;
type LinkHandler = Rc<dyn Fn(&LinkTarget, &mut Window, &mut App)>;
type ImageHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct RichText {
    id: ElementId,
    doc: Arc<Doc>,
    mentions: Option<MentionLookup>,
    on_link: Option<LinkHandler>,
    on_image: Option<ImageHandler>,
    max_height: Option<f32>,
}

impl RichText {
    pub fn new(id: impl Into<ElementId>, doc: Arc<Doc>) -> Self {
        Self {
            id: id.into(),
            doc,
            mentions: None,
            on_link: None,
            on_image: None,
            max_height: None,
        }
    }

    /// Identity id → display name. Unresolved ids show "@Unknown user".
    pub fn mentions(mut self, lookup: impl Fn(&str) -> Option<MentionName> + 'static) -> Self {
        self.mentions = Some(Rc::new(lookup));
        self
    }

    /// Links, `#123` references and Azure DevOps work item / PR URLs.
    pub fn on_link(
        mut self,
        handler: impl Fn(&LinkTarget, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_link = Some(Rc::new(handler));
        self
    }

    /// An image chip was clicked; gets the image's `src`.
    pub fn on_image(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_image = Some(Rc::new(handler));
        self
    }

    /// Clip taller content (design §5.1: 480). The caller shows "Show all".
    pub fn max_height(mut self, height: Option<f32>) -> Self {
        self.max_height = height;
        self
    }
}

/// Element state: the flattened document and what it was built from.
#[derive(Default)]
struct Prepared {
    doc: Option<Arc<Doc>>,
    names: Vec<Option<MentionName>>,
    blocks: Rc<Vec<PBlock>>,
}

/// Which link (text ix, hit ix) the pointer is over.
type Hover = Option<(usize, usize)>;

struct Ctx {
    hover_state: gpui::Entity<Hover>,
    hover: Hover,
    on_link: Option<LinkHandler>,
    on_image: Option<ImageHandler>,
}

impl RenderOnce for RichText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let lookup: MentionLookup = self.mentions.clone().unwrap_or_else(|| Rc::new(|_| None));
        let id = self.id.clone();
        window.with_element_namespace(id.clone(), |window| {
            let state =
                window.use_keyed_state("rich-text-prepared", cx, |_, _| Prepared::default());
            let hover_state =
                window.use_keyed_state("rich-text-hover", cx, |_, _| None::<(usize, usize)>);
            let names = prepare::resolved_names(&self.doc, &*lookup);
            let stale = {
                let p = state.read(cx);
                !p.doc.as_ref().is_some_and(|d| Arc::ptr_eq(d, &self.doc)) || p.names != names
            };
            if stale {
                let blocks = Rc::new(prepare::prepare(&self.doc, &*lookup));
                state.update(cx, |p, _| {
                    *p = Prepared {
                        doc: Some(self.doc.clone()),
                        names,
                        blocks,
                    }
                });
            }
            let blocks = state.read(cx).blocks.clone();
            let ctx = Ctx {
                hover: *hover_state.read(cx),
                hover_state,
                on_link: self.on_link.clone(),
                on_image: self.on_image.clone(),
            };
            let theme = cx.theme();
            div()
                .id(id)
                .w_full()
                .flex()
                .flex_col()
                .gap(px(space::S8))
                .type_style(typography::BODY, theme)
                .text_color(theme.colors.text_primary)
                .when_some(self.max_height, |this, h| {
                    this.max_h(px(h)).overflow_hidden()
                })
                .children(
                    blocks
                        .iter()
                        .enumerate()
                        .map(|(i, b)| block(b, i == 0, &ctx, theme)),
                )
        })
    }
}

/// `first`: the first block of its column (no space above a heading).
fn block(block: &PBlock, first: bool, ctx: &Ctx, theme: &Theme) -> AnyElement {
    let colors = &theme.colors;
    match block {
        PBlock::Paragraph(segs) => segments(segs, ctx, theme),
        PBlock::Heading(level, segs) => {
            let (style, above) = if *level == 1 {
                (typography::HEADING, space::S16)
            } else {
                (typography::BODY_STRONG, space::S12)
            };
            // The column's gap is `space.8`; the rest of the space above.
            div()
                .when(!first, |this| this.pt(px(above - space::S8)))
                .type_style(style, theme)
                .child(segments(segs, ctx, theme))
                .into_any_element()
        }
        PBlock::Code { ix, text } => div()
            .id(("rich-text-code", *ix))
            .w_full()
            .overflow_x_scroll()
            .p(px(space::S12))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_code)
            .type_style(typography::CODE, theme)
            .text_color(colors.syntax_variable)
            .child(div().whitespace_nowrap().child(text.clone()))
            .into_any_element(),
        PBlock::List { start, items } => div()
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| list_item(*start, i, item, ctx, theme)),
            )
            .into_any_element(),
        PBlock::Quote(inner) => div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .border_l_2()
            .border_color(colors.border_strong)
            .pl(px(space::S12))
            .text_color(colors.text_secondary)
            .children(
                inner
                    .iter()
                    .enumerate()
                    .map(|(i, b)| self::block(b, i == 0, ctx, theme)),
            )
            .into_any_element(),
        PBlock::Rule => div()
            .my(px(space::S16 - space::S8))
            .h(px(size::BORDER))
            .w_full()
            .bg(colors.border_subtle)
            .into_any_element(),
        PBlock::Table {
            ix,
            header,
            columns,
            rows,
        } => table(*ix, *header, *columns, rows, ctx, theme),
    }
}

fn list_item(start: Option<u64>, i: usize, item: &PItem, ctx: &Ctx, theme: &Theme) -> AnyElement {
    let colors = &theme.colors;
    let marker: AnyElement = match (item.task, start) {
        (Some(checked), _) => {
            let (name, color) = if checked {
                (IconName::SquareCheck, colors.accent_solid)
            } else {
                (IconName::Square, colors.text_tertiary)
            };
            // Centered on the first line (line height 20, glyph 14).
            div()
                .pt(px(space::S2))
                .child(Icon::new(name).size(size::ICON_SM).color(color))
                .into_any_element()
        }
        (None, None) => div().child("•").into_any_element(),
        (None, Some(n)) => div()
            .child(SharedString::from(format!("{}.", n + i as u64)))
            .into_any_element(),
    };
    div()
        .flex()
        .flex_row()
        .items_start()
        .child(
            div()
                .flex_none()
                .min_w(px(space::S16))
                .pr(px(space::S4))
                .text_color(colors.text_tertiary)
                .child(marker),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(space::S4))
                .children(
                    item.blocks
                        .iter()
                        .enumerate()
                        .map(|(i, b)| block(b, i == 0, ctx, theme)),
                ),
        )
        .into_any_element()
}

/// A plain grid laid out as columns, each as wide as its widest cell, so
/// a table wider than the prose column scrolls (design §5.1). Cells don't
/// wrap; every cell of a row gets the row's height (its line count, or an
/// image chip) so rows line up across columns.
fn table(
    ix: usize,
    header: bool,
    columns: usize,
    rows: &[Vec<Vec<Seg>>],
    ctx: &Ctx,
    theme: &Theme,
) -> AnyElement {
    let colors = &theme.colors;
    let style = |r: usize| {
        if header && r == 0 {
            typography::LABEL
        } else {
            typography::SMALL
        }
    };
    let heights: Vec<f32> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let content = row
                .iter()
                .map(|segs| cell_height(segs, style(r).line_height))
                .fold(style(r).line_height, f32::max);
            content + 2.0 * space::S4 + size::BORDER
        })
        .collect();
    let column = |c: usize| {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .children(rows.iter().enumerate().map(|(r, row)| {
                let head = header && r == 0;
                div()
                    .h(px(heights[r]))
                    .px(px(space::S8))
                    .py(px(space::S4))
                    .border_r_1()
                    .border_b_1()
                    .border_color(colors.border_subtle)
                    .whitespace_nowrap()
                    .type_style(style(r), theme)
                    .when(head, |this| this.text_color(colors.text_secondary))
                    .children(row.get(c).map(|segs| segments(segs, ctx, theme)))
            }))
    };
    div()
        .id(("rich-text-table", ix))
        .w_full()
        .flex()
        .overflow_x_scroll()
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .border_t_1()
                .border_l_1()
                .border_color(colors.border_subtle)
                .children((0..columns).map(column)),
        )
        .into_any_element()
}

/// A cell's content height: its lines, or an image chip's height.
fn cell_height(segs: &[Seg], line_height: f32) -> f32 {
    segs.iter()
        .map(|seg| match seg {
            Seg::Text(t) => (t.text.matches('\n').count() + 1) as f32 * line_height,
            Seg::Image { .. } => size::CONTROL_SM,
        })
        .fold(0.0, f32::max)
}

fn segments(segs: &[Seg], ctx: &Ctx, theme: &Theme) -> AnyElement {
    match segs {
        [Seg::Text(t)] => text(t, ctx, theme),
        _ => div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(space::S4))
            .children(segs.iter().map(|seg| match seg {
                Seg::Text(t) => text(t, ctx, theme),
                Seg::Image { ix, src, alt } => image_chip(*ix, src, alt, ctx, theme),
            }))
            .into_any_element(),
    }
}

fn image_chip(
    ix: usize,
    src: &SharedString,
    alt: &SharedString,
    ctx: &Ctx,
    theme: &Theme,
) -> AnyElement {
    let colors = &theme.colors;
    let label: SharedString = if alt.is_empty() {
        "Image".into()
    } else {
        format!("Image {alt}").into()
    };
    let on_image = ctx.on_image.clone();
    let src = src.clone();
    div()
        .id(("rich-text-image", ix))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(space::S4))
        .h(px(size::CONTROL_SM))
        .px(px(space::S6))
        .rounded(px(radius::XS))
        .bg(colors.bg_active)
        .text_color(colors.text_secondary)
        .type_style(typography::SMALL, theme)
        .child(
            Icon::new(IconName::Image)
                .size(size::ICON_SM)
                .color(colors.text_secondary),
        )
        .child(label)
        .tooltip(Tooltip::text(IMAGE_TOOLTIP))
        .when_some(on_image, |this, on_image| {
            this.cursor_pointer()
                .on_click(move |_, window, cx| on_image(&src, window, cx))
        })
        .into_any_element()
}

fn text(t: &PText, ctx: &Ctx, theme: &Theme) -> AnyElement {
    let colors = &theme.colors;
    let hovered_hit = ctx.hover.filter(|(ix, _)| *ix == t.ix).map(|(_, hit)| hit);
    let hovered_range = hovered_hit
        .and_then(|h| t.hits.get(h))
        .map(|h| h.range.clone());
    let mut code = Vec::new();
    let highlights = t.runs.iter().map(|(range, style)| {
        let mut h = HighlightStyle::default();
        if style.marks.contains(Marks::BOLD) {
            // Design §5.1: bold is weight 600, the heading step's weight.
            h.font_weight = Some(FontWeight(typography::HEADING.weight));
        }
        if style.marks.contains(Marks::ITALIC) {
            h.font_style = Some(FontStyle::Italic);
        }
        if style.marks.contains(Marks::UNDERLINE) {
            h.underline = Some(UnderlineStyle {
                thickness: px(size::BORDER),
                color: None,
                wavy: false,
            });
        }
        if style.marks.contains(Marks::STRIKE) {
            h.strikethrough = Some(StrikethroughStyle {
                thickness: px(size::BORDER),
                color: None,
            });
        }
        if style.marks.contains(Marks::CODE) {
            h.background_color = Some(colors.bg_code);
            code.push(range.clone());
        }
        if style.link {
            h.color = Some(colors.text_link);
            if hovered_range
                .as_ref()
                .is_some_and(|r| r.start <= range.start && range.end <= r.end)
            {
                h.underline = Some(UnderlineStyle {
                    thickness: px(size::BORDER),
                    color: None,
                    wavy: false,
                });
            }
        }
        match style.mention {
            Some(MentionKind::Other) => {
                h.font_weight = Some(FontWeight(typography::BODY_STRONG.weight));
                h.color = Some(colors.text_link);
            }
            Some(MentionKind::Me) => {
                h.font_weight = Some(FontWeight(typography::BODY_STRONG.weight));
                h.color = Some(colors.text_primary);
                h.background_color = Some(colors.accent_wash);
            }
            Some(MentionKind::Unknown) => h.color = Some(colors.text_tertiary),
            None => {}
        }
        (range.clone(), h)
    });
    let highlights: Vec<_> = highlights.collect();
    let family = theme.code_font.family.clone();
    let styled = StyledText::new(t.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(code.into_iter().map(|r| (r, family.clone())));
    if t.hits.is_empty() {
        return styled.into_any_element();
    }
    let links: Vec<(usize, &Hit)> = t
        .hits
        .iter()
        .enumerate()
        .filter(|(_, h)| h.target.is_some())
        .collect();
    let ranges = links.iter().map(|(_, h)| h.range.clone()).collect();
    let targets: Vec<LinkTarget> = links.iter().filter_map(|(_, h)| h.target.clone()).collect();
    let hits = Rc::new(t.hits.clone());
    let text_ix = t.ix;
    let on_link = ctx.on_link.clone();
    let hover_state = ctx.hover_state.clone();
    let tooltip_hits = hits.clone();
    InteractiveText::new(("rich-text", t.ix), styled)
        .on_click(ranges, move |i, window, cx| {
            if let (Some(on_link), Some(target)) = (&on_link, targets.get(i)) {
                on_link(target, window, cx);
            }
        })
        .on_hover(move |index, _, _, cx| {
            let hit = index.and_then(|ix| {
                hits.iter()
                    .position(|h| h.target.is_some() && h.range.contains(&ix))
            });
            let next = hit.map(|h| (text_ix, h));
            hover_state.update(cx, |hover, cx| {
                let mine = hover.is_some_and(|(ix, _)| ix == text_ix);
                if *hover != next && (next.is_some() || mine) {
                    *hover = next;
                    cx.notify();
                }
            });
        })
        .tooltip(move |index, window, cx| {
            let tip = tooltip_hits
                .iter()
                .find(|h| h.range.contains(&index))?
                .tooltip
                .clone()?;
            Some(Tooltip::text(tip)(window, cx))
        })
        .into_any_element()
}
