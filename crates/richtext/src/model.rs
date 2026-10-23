//! The document model both parsers produce (spec 0015 FR-3).
//!
//! Equality is the contract: the same content written as Azure DevOps HTML
//! or as Markdown parses to an equal [`Doc`] (AC-1), and the FR-18 gate
//! compares documents, not strings. Every constructor goes through
//! [`Doc::from_blocks`], which normalizes what neither source can see:
//! collapsed whitespace, empty blocks, emphasis on boundary spaces, split
//! text runs.

use std::ops::{BitOr, BitOrAssign};

/// One parsed description or comment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doc {
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    /// `level` is 1–6.
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    /// Monospace, never highlighted. No trailing newline.
    CodeBlock(String),
    List(List),
    Quote(Vec<Block>),
    Rule,
    Table(Table),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List {
    /// `None`: bullets. `Some(n)`: numbered from `n`.
    pub start: Option<u64>,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListItem {
    /// `Some(checked)` for a task list item (read-only checkbox).
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// A plain grid: every row has the same number of cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    /// The first row is a header row.
    pub header: bool,
    pub rows: Vec<Vec<Vec<Inline>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(Span),
    /// A line break inside a block (`<br>`, Markdown hard break).
    Break,
    /// A person or group, by lowercase identity id. The display name comes
    /// from the caller's lookup at render or save time.
    Mention(String),
    /// Never fetched (Q-3): rendered as an "Image" chip.
    Image {
        src: String,
        alt: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub marks: Marks,
    pub link: Option<Link>,
}

impl Span {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            marks: Marks::NONE,
            link: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    Url(String),
    /// A bare `#123` reference in the text (display parse only).
    WorkItem(u32),
}

/// Inline formatting flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Marks(u8);

impl Marks {
    pub const NONE: Self = Self(0);
    pub const BOLD: Self = Self(1);
    pub const ITALIC: Self = Self(1 << 1);
    pub const UNDERLINE: Self = Self(1 << 2);
    pub const STRIKE: Self = Self(1 << 3);
    pub const CODE: Self = Self(1 << 4);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn intersect(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl BitOr for Marks {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Marks {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Doc {
    /// Normalized document from parser output.
    pub fn from_blocks(blocks: Vec<Block>) -> Self {
        Self {
            blocks: normalize_blocks(blocks),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Mentioned identity ids, in document order, without repeats.
    pub fn mentions(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        self.visit_inlines(&mut |inline| {
            if let Inline::Mention(id) = inline
                && !out.contains(&id.as_str())
            {
                out.push(id);
            }
        });
        out
    }

    /// Calls `f` on every inline of every block, tables included.
    pub fn visit_inlines<'a>(&'a self, f: &mut impl FnMut(&'a Inline)) {
        fn walk<'a>(blocks: &'a [Block], f: &mut impl FnMut(&'a Inline)) {
            for block in blocks {
                match block {
                    Block::Paragraph(content) | Block::Heading { content, .. } => {
                        content.iter().for_each(&mut *f)
                    }
                    Block::List(list) => list.items.iter().for_each(|i| walk(&i.blocks, f)),
                    Block::Quote(inner) => walk(inner, f),
                    Block::Table(table) => table.rows.iter().flatten().flatten().for_each(&mut *f),
                    Block::CodeBlock(_) | Block::Rule => {}
                }
            }
        }
        walk(&self.blocks, f);
    }
}

// ── normalization ───────────────────────────────────────────────────────

fn normalize_blocks(blocks: Vec<Block>) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            Block::Paragraph(content) => {
                let content = normalize_inlines(content);
                if !content.is_empty() {
                    out.push(Block::Paragraph(content));
                }
            }
            Block::Heading { level, content } => {
                let content = normalize_inlines(content);
                if !content.is_empty() {
                    out.push(Block::Heading {
                        level: level.clamp(1, 6),
                        content,
                    });
                }
            }
            Block::CodeBlock(text) => {
                let text = text.replace("\r\n", "\n");
                out.push(Block::CodeBlock(
                    text.trim_start_matches('\n').trim_end().to_owned(),
                ));
            }
            Block::List(list) => {
                if list.items.is_empty() {
                    continue;
                }
                let items = list
                    .items
                    .into_iter()
                    .map(|item| ListItem {
                        task: item.task,
                        blocks: normalize_blocks(item.blocks),
                    })
                    .collect();
                out.push(Block::List(List {
                    start: list.start,
                    items,
                }));
            }
            Block::Quote(inner) => {
                let inner = normalize_blocks(inner);
                if !inner.is_empty() {
                    out.push(Block::Quote(inner));
                }
            }
            Block::Rule => out.push(Block::Rule),
            Block::Table(table) => {
                let width = table.rows.iter().map(Vec::len).max().unwrap_or(0);
                if width == 0 {
                    continue;
                }
                let rows = table
                    .rows
                    .into_iter()
                    .map(|row| {
                        let mut row: Vec<_> = row.into_iter().map(normalize_inlines).collect();
                        row.resize_with(width, Vec::new);
                        row
                    })
                    .collect();
                out.push(Block::Table(Table {
                    header: table.header,
                    rows,
                }));
            }
        }
    }
    out
}

type Style = (Marks, Option<Link>);

/// Whitespace collapsed like HTML renders it, boundary spaces carry no
/// emphasis or link, `@<guid>` text becomes mentions, adjacent runs with
/// the same style are merged, no leading/trailing breaks. Idempotent.
pub(crate) fn normalize_inlines(raw: Vec<Inline>) -> Vec<Inline> {
    // 1. Merge runs so patterns split across events are seen whole.
    let mut merged: Vec<Inline> = Vec::with_capacity(raw.len());
    for inline in raw {
        match inline {
            Inline::Text(span) => push_text(&mut merged, &span.text, span.marks, &span.link),
            other => merged.push(other),
        }
    }
    // 2. Mentions written as text (`@<guid>`).
    let mut scanned: Vec<Inline> = Vec::with_capacity(merged.len());
    for inline in merged {
        match inline {
            Inline::Text(span) if !span.marks.contains(Marks::CODE) && span.text.contains("@<") => {
                split_mentions(span, &mut scanned)
            }
            other => scanned.push(other),
        }
    }
    // 3. Collapse whitespace across runs. A collapsed space keeps the
    //    emphasis both neighbours share (bold/italic always; underline,
    //    strike and code only when the space had them) and a link all three
    //    share, so `**a *b* c**` stays one bold run whichever way it was
    //    written.
    let mut out: Vec<Inline> = Vec::with_capacity(scanned.len());
    let mut line_start = true;
    let mut pending: Option<Style> = None;
    let mut last: Option<Style> = None;
    for inline in scanned {
        match inline {
            Inline::Text(span) => {
                let style = (span.marks, span.link);
                for ch in span.text.chars() {
                    if ch.is_whitespace() {
                        if !line_start {
                            pending = Some(match pending.take() {
                                None => style.clone(),
                                Some((marks, link)) => (
                                    marks.intersect(style.0),
                                    if link == style.1 { link } else { None },
                                ),
                            });
                        }
                        continue;
                    }
                    if let Some((space_marks, space_link)) = pending.take() {
                        let space = match &last {
                            Some((last_marks, last_link)) => (
                                last_marks
                                    .intersect(style.0)
                                    .intersect(space_marks | Marks::BOLD | Marks::ITALIC),
                                if *last_link == style.1 && space_link == style.1 {
                                    style.1.clone()
                                } else {
                                    None
                                },
                            ),
                            None => (Marks::NONE, None),
                        };
                        push_char(&mut out, ' ', &space);
                    }
                    push_char(&mut out, ch, &style);
                    line_start = false;
                    last = Some(style.clone());
                }
            }
            Inline::Break => {
                pending = None;
                line_start = true;
                last = None;
                out.push(Inline::Break);
            }
            atom => {
                if pending.take().is_some() {
                    push_char(&mut out, ' ', &(Marks::NONE, None));
                }
                out.push(atom);
                line_start = false;
                last = None;
            }
        }
    }
    while matches!(out.last(), Some(Inline::Break)) {
        out.pop();
    }
    let leading = out
        .iter()
        .take_while(|i| matches!(i, Inline::Break))
        .count();
    out.drain(..leading);
    out
}

fn push_text(out: &mut Vec<Inline>, text: &str, marks: Marks, link: &Option<Link>) {
    if text.is_empty() {
        return;
    }
    if let Some(Inline::Text(last)) = out.last_mut()
        && last.marks == marks
        && last.link == *link
    {
        last.text.push_str(text);
        return;
    }
    out.push(Inline::Text(Span {
        text: text.to_owned(),
        marks,
        link: link.clone(),
    }));
}

fn push_char(out: &mut Vec<Inline>, ch: char, style: &Style) {
    if let Some(Inline::Text(last)) = out.last_mut()
        && last.marks == style.0
        && last.link == style.1
    {
        last.text.push(ch);
        return;
    }
    out.push(Inline::Text(Span {
        text: ch.to_string(),
        marks: style.0,
        link: style.1.clone(),
    }));
}

fn split_mentions(span: Span, out: &mut Vec<Inline>) {
    let mut rest = span.text.as_str();
    while let Some(at) = rest.find("@<") {
        let after = &rest[at + 2..];
        match after.get(..36).filter(|id| is_guid(id)) {
            Some(id) if after[36..].starts_with('>') => {
                push_text(out, &rest[..at], span.marks, &span.link);
                out.push(Inline::Mention(id.to_ascii_lowercase()));
                rest = &after[37..];
            }
            _ => {
                push_text(out, &rest[..at + 2], span.marks, &span.link);
                rest = after;
            }
        }
    }
    push_text(out, rest, span.marks, &span.link);
}

/// `8-4-4-4-12` hex digits.
pub fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str, marks: Marks) -> Inline {
        Inline::Text(Span {
            text: t.into(),
            marks,
            link: None,
        })
    }

    #[test]
    fn whitespace_collapses_across_runs_and_leaves_emphasis() {
        let raw = vec![
            text("  a  ", Marks::NONE),
            text(" b ", Marks::BOLD),
            text("c\n d", Marks::BOLD),
            Inline::Break,
            text("  ", Marks::NONE),
            Inline::Break,
        ];
        assert_eq!(
            normalize_inlines(raw),
            vec![text("a ", Marks::NONE), text("b c d", Marks::BOLD)]
        );
    }

    #[test]
    fn boundary_spaces_lose_emphasis_but_inner_spaces_keep_it() {
        let raw = vec![
            text("x", Marks::NONE),
            text(" bold words ", Marks::BOLD),
            text("y", Marks::NONE),
        ];
        assert_eq!(
            normalize_inlines(raw),
            vec![
                text("x ", Marks::NONE),
                text("bold words", Marks::BOLD),
                text(" y", Marks::NONE),
            ]
        );
    }

    #[test]
    fn mention_tokens_in_text_become_mentions_once() {
        let id = "D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607";
        let raw = vec![
            text("hi @", Marks::NONE),
            text(&format!("<{id}>, @<nope>"), Marks::NONE),
        ];
        let once = normalize_inlines(raw);
        assert_eq!(
            once,
            vec![
                text("hi ", Marks::NONE),
                Inline::Mention(id.to_ascii_lowercase()),
                text(", @<nope>", Marks::NONE),
            ]
        );
        assert_eq!(normalize_inlines(once.clone()), once);
    }
}
