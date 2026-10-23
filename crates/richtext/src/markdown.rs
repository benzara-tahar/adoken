//! Markdown (CommonMark + GFM tables, task lists, strikethrough) → [`Doc`].
//!
//! Azure DevOps specifics: mentions are `@<identity id>`; underline has no
//! Markdown syntax, so `<u>…</u>` inline HTML is honored (adoken writes it,
//! see `write_md`), as are the other inline formatting tags and `<br>`.
//! Block HTML goes through the HTML parser; other inline tags are dropped
//! and their text kept (FR-3: unknown tags render their text).

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::model::{Block, Doc, Inline, Link, List, ListItem, Marks, Span, Table, is_guid};

pub(crate) fn parse(source: &str) -> Doc {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut b = Builder::default();
    for event in Parser::new_ext(source, options) {
        b.event(event);
    }
    while b.frames.len() > 1 {
        b.end_frame();
    }
    let Some(Frame::Blocks(blocks)) = b.frames.pop() else {
        unreachable!("the root frame stays at the bottom")
    };
    Doc::from_blocks(blocks)
}

/// `@<id>`: how a mention is written in Markdown text (FR-26).
pub fn mention_token(id: &str) -> String {
    format!("@<{id}>")
}

/// Byte range and id of every `@<guid>` token in `text` (composer chips).
pub fn mention_ranges(text: &str) -> Vec<(Range<usize>, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("@<").map(|i| from + i) {
        let id_start = at + 2;
        match text.get(id_start..id_start + 36) {
            Some(id) if is_guid(id) && text[id_start + 36..].starts_with('>') => {
                out.push((at..id_start + 37, id));
                from = id_start + 37;
            }
            _ => from = id_start,
        }
    }
    out
}

enum Frame {
    /// Root and block quotes.
    Blocks(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    /// Tight list items hold inline content without a paragraph.
    Item {
        task: Option<bool>,
        blocks: Vec<Block>,
        inline: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    Heading(u8, Vec<Inline>),
    Code(String),
    Html(String),
    Table {
        header: bool,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Row(Vec<Vec<Inline>>),
    Cell(Vec<Inline>),
    Image {
        src: String,
        alt: String,
    },
}

struct Builder {
    frames: Vec<Frame>,
    bold: u32,
    italic: u32,
    strike: u32,
    underline: u32,
    code: u32,
    links: Vec<Option<Link>>,
    /// Inside an inline `<a data-vss-mention>`: its text is the name.
    in_mention: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            frames: vec![Frame::Blocks(Vec::new())],
            bold: 0,
            italic: 0,
            strike: 0,
            underline: 0,
            code: 0,
            links: Vec::new(),
            in_mention: false,
        }
    }
}

impl Builder {
    fn marks(&self) -> Marks {
        let mut m = Marks::NONE;
        for (count, mark) in [
            (self.bold, Marks::BOLD),
            (self.italic, Marks::ITALIC),
            (self.strike, Marks::STRIKE),
            (self.underline, Marks::UNDERLINE),
            (self.code, Marks::CODE),
        ] {
            if count > 0 {
                m |= mark;
            }
        }
        m
    }

    fn link(&self) -> Option<Link> {
        self.links.iter().rev().find_map(Clone::clone)
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(text) => {
                let span = Span {
                    text: text.to_string(),
                    marks: self.marks() | Marks::CODE,
                    link: self.link(),
                };
                self.inline(Inline::Text(span));
            }
            Event::Html(html) => match self.frames.last_mut() {
                Some(Frame::Html(buf)) => buf.push_str(&html),
                _ => self.inline_html(&html),
            },
            Event::InlineHtml(html) => self.inline_html(&html),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.inline(Inline::Break),
            Event::Rule => self.block(Block::Rule),
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item { task, .. }) = self.frames.last_mut() {
                    *task = Some(checked);
                }
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t),
            Event::FootnoteReference(t) => self.text(&t),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.open_block(Frame::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.open_block(Frame::Heading(level as u8, Vec::new())),
            Tag::BlockQuote(_) => self.open_block(Frame::Blocks(Vec::new())),
            Tag::CodeBlock(_) => self.open_block(Frame::Code(String::new())),
            Tag::HtmlBlock => self.open_block(Frame::Html(String::new())),
            Tag::List(start) => self.open_block(Frame::List {
                start,
                items: Vec::new(),
            }),
            Tag::Item => self.frames.push(Frame::Item {
                task: None,
                blocks: Vec::new(),
                inline: Vec::new(),
            }),
            Tag::Table(_) => self.open_block(Frame::Table {
                header: false,
                rows: Vec::new(),
            }),
            Tag::TableHead | Tag::TableRow => self.frames.push(Frame::Row(Vec::new())),
            Tag::TableCell => self.frames.push(Frame::Cell(Vec::new())),
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                let url = dest_url.trim();
                self.links
                    .push((!url.is_empty()).then(|| Link::Url(url.to_owned())));
            }
            Tag::Image { dest_url, .. } => self.frames.push(Frame::Image {
                src: dest_url.to_string(),
                alt: String::new(),
            }),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell
            | TagEnd::Image => self.end_frame(),
            _ => {}
        }
    }

    fn end_frame(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        match frame {
            Frame::Blocks(blocks) => self.block(Block::Quote(blocks)),
            Frame::List { start, items } => self.block(Block::List(List { start, items })),
            Frame::Item {
                task,
                mut blocks,
                mut inline,
            } => {
                flush_inline(&mut blocks, &mut inline);
                if let Some(Frame::List { items, .. }) = self.frames.last_mut() {
                    items.push(ListItem { task, blocks });
                }
            }
            Frame::Paragraph(content) => self.block(Block::Paragraph(content)),
            Frame::Heading(level, content) => self.block(Block::Heading { level, content }),
            Frame::Code(text) => self.block(Block::CodeBlock(text)),
            Frame::Html(html) => {
                for block in crate::html::parse(&html).0.blocks {
                    self.block(block);
                }
            }
            Frame::Table { header, rows } => self.block(Block::Table(Table { header, rows })),
            Frame::Row(cells) => {
                if let Some(Frame::Table { header, rows }) = self.frames.last_mut() {
                    // pulldown-cmark: the head row comes first, every table has one.
                    if rows.is_empty() {
                        *header = true;
                    }
                    rows.push(cells);
                }
            }
            Frame::Cell(content) => {
                if let Some(Frame::Row(cells)) = self.frames.last_mut() {
                    cells.push(content);
                }
            }
            Frame::Image { src, alt } => self.inline(Inline::Image {
                src,
                alt: alt.trim().to_owned(),
            }),
        }
    }

    /// Push a block-level frame, closing a tight item's inline run first.
    fn open_block(&mut self, frame: Frame) {
        if let Some(Frame::Item { blocks, inline, .. }) = self.frames.last_mut() {
            flush_inline(blocks, inline);
        }
        self.frames.push(frame);
    }

    fn block(&mut self, block: Block) {
        match self.frames.last_mut() {
            Some(Frame::Blocks(blocks)) => blocks.push(block),
            Some(Frame::Item { blocks, inline, .. }) => {
                flush_inline(blocks, inline);
                blocks.push(block);
            }
            // A block inside inline content (HTML block in a cell): its text.
            _ => {
                let mut inline = Vec::new();
                crate::html::flatten(std::slice::from_ref(&block), &mut inline);
                for i in inline {
                    self.inline(i);
                }
            }
        }
    }

    fn inline(&mut self, inline: Inline) {
        match self.frames.last_mut() {
            Some(Frame::Paragraph(content))
            | Some(Frame::Heading(_, content))
            | Some(Frame::Cell(content))
            | Some(Frame::Item {
                inline: content, ..
            }) => content.push(inline),
            Some(Frame::Image { alt, .. }) => {
                if let Inline::Text(span) = inline {
                    alt.push_str(&span.text);
                }
            }
            Some(Frame::Code(text)) => {
                if let Inline::Text(span) = inline {
                    text.push_str(&span.text);
                }
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_mention {
            return;
        }
        if let Some(Frame::Code(code)) = self.frames.last_mut() {
            code.push_str(text);
            return;
        }
        let span = Span {
            text: text.to_owned(),
            marks: self.marks(),
            link: self.link(),
        };
        self.inline(Inline::Text(span));
    }

    fn inline_html(&mut self, html: &str) {
        let trimmed = html.trim();
        let Some(inner) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) else {
            return self.text(html);
        };
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        // `@<guid>`: a guid starting with a letter is a valid tag name.
        if is_guid(&name) && !closing {
            return self.text(trimmed);
        }
        let counter = match name.as_str() {
            "b" | "strong" => Some(&mut self.bold),
            "i" | "em" => Some(&mut self.italic),
            "u" | "ins" => Some(&mut self.underline),
            "s" | "del" | "strike" => Some(&mut self.strike),
            "code" => Some(&mut self.code),
            _ => None,
        };
        if let Some(counter) = counter {
            *counter = if closing {
                counter.saturating_sub(1)
            } else {
                *counter + 1
            };
            return;
        }
        match name.as_str() {
            "br" => self.inline(Inline::Break),
            "a" if closing => {
                if std::mem::take(&mut self.in_mention) {
                    return;
                }
                self.links.pop();
            }
            "a" => {
                let parsed = crate::html::parse(&format!("<{inner}>x</a>")).0;
                match parsed.blocks.first() {
                    Some(Block::Paragraph(content)) => match content.first() {
                        Some(Inline::Mention(id)) => {
                            self.inline(Inline::Mention(id.clone()));
                            self.in_mention = true;
                        }
                        Some(Inline::Text(span)) => self.links.push(span.link.clone()),
                        _ => self.links.push(None),
                    },
                    _ => self.links.push(None),
                }
            }
            "img" => {
                if let Some(Block::Paragraph(content)) =
                    crate::html::parse(trimmed).0.blocks.into_iter().next()
                {
                    content.into_iter().for_each(|i| self.inline(i));
                }
            }
            _ => {}
        }
    }
}

fn flush_inline(blocks: &mut Vec<Block>, inline: &mut Vec<Inline>) {
    if !inline.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(inline)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mention_tokens_parse_whatever_the_first_character() {
        let letter = "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607";
        let digit = "6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d";
        let doc = parse(&format!("hi @<{letter}> and @<{}>!", digit.to_uppercase()));
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![
                Inline::Text(Span::plain("hi ")),
                Inline::Mention(letter.into()),
                Inline::Text(Span::plain(" and ")),
                Inline::Mention(digit.into()),
                Inline::Text(Span::plain("!")),
            ])]
        );
    }

    #[test]
    fn mention_ranges_find_tokens_for_chips() {
        let text = "a @<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> b @<x>";
        let ranges = mention_ranges(text);
        assert_eq!(ranges.len(), 1);
        assert_eq!(&text[ranges[0].0.clone()], mention_token(ranges[0].1));
    }

    #[test]
    fn underline_and_breaks_come_from_inline_html() {
        let doc = parse("a <u>b</u><br>c");
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph(vec![
                Inline::Text(Span::plain("a ")),
                Inline::Text(Span {
                    text: "b".into(),
                    marks: Marks::UNDERLINE,
                    link: None
                }),
                Inline::Break,
                Inline::Text(Span::plain("c")),
            ])]
        );
    }

    #[test]
    fn tight_and_loose_lists_are_the_same_document() {
        assert_eq!(parse("- a\n- b\n  - c"), parse("- a\n\n- b\n\n  - c"));
        let Block::List(list) = &parse("- [x] done\n- [ ] todo").blocks[0] else {
            panic!()
        };
        assert_eq!(
            list.items.iter().map(|i| i.task).collect::<Vec<_>>(),
            vec![Some(true), Some(false)]
        );
    }
}
