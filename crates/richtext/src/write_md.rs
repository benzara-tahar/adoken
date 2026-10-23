//! [`Doc`] → Markdown, for the description editor (FR-17).
//!
//! The output must parse back to the same document (`markdown::parse`), so
//! text is escaped wherever Markdown would read structure into it, and an
//! emphasis whose `*` delimiters wouldn't be recognized next to
//! punctuation is written as inline HTML (`<strong>`, `<em>`, `<s>`), like
//! underline always is (`<u>`). FR-18's gate checks the round trip anyway.

use crate::model::{Block, Doc, Inline, Link, List, Marks, Table};

pub fn to_markdown(doc: &Doc) -> String {
    blocks(&doc.blocks, false)
}

/// `in_item`: a list item's blocks, where a nested list follows its
/// paragraph on the next line (`- a\n  - b`) as people write it.
fn blocks(blocks: &[Block], in_item: bool) -> String {
    let mut out = String::new();
    let mut prev_list: Option<(bool, bool)> = None; // (ordered, alternate marker)
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            // Only lists that may interrupt a paragraph (bullets, or
            // numbers from 1, with a non-empty first item) can sit on the
            // next line.
            let tight = in_item
                && matches!(blocks[i - 1], Block::Paragraph(_))
                && matches!(
                    block,
                    Block::List(List {
                        start: None | Some(1),
                        items,
                    }) if items.first().is_some_and(|item| !item.blocks.is_empty())
                );
            out.push_str(if tight { "\n" } else { "\n\n" });
        }
        let mut this_list = None;
        match block {
            Block::Paragraph(content) => out.push_str(&inlines(content, Ctx::Paragraph)),
            Block::Heading { level, content } => {
                out.push_str(&"#".repeat(usize::from(*level)));
                out.push(' ');
                out.push_str(&inlines(content, Ctx::Heading));
            }
            Block::CodeBlock(text) => {
                let longest = longest_run(text, '`');
                let fence = "`".repeat(longest.max(2) + 1);
                out.push_str(&fence);
                out.push('\n');
                if !text.is_empty() {
                    out.push_str(text);
                    out.push('\n');
                }
                out.push_str(&fence);
            }
            Block::List(list) => {
                let ordered = list.start.is_some();
                // Two lists in a row with the same marker would merge.
                let alternate = matches!(prev_list, Some((o, alt)) if o == ordered && !alt);
                out.push_str(&list_md(list, alternate));
                this_list = Some((ordered, alternate));
            }
            Block::Quote(inner) => {
                let inner = self::blocks(inner, false);
                out.push_str(&prefix_lines(&inner, "> ", ">"));
            }
            // `- ---` would itself be a thematic break, not an item.
            Block::Rule => out.push_str(if in_item && i == 0 { "___" } else { "---" }),
            Block::Table(table) => out.push_str(&table_md(table)),
        }
        prev_list = this_list;
    }
    out
}

fn list_md(list: &List, alternate: bool) -> String {
    let mut out = String::new();
    for (i, item) in list.items.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let marker = match list.start {
            None => (if alternate { "*" } else { "-" }).to_owned(),
            Some(start) => format!("{}{}", start + i as u64, if alternate { ')' } else { '.' }),
        };
        let indent = " ".repeat(marker.len() + 1);
        let task = match item.task {
            Some(true) => "[x] ",
            Some(false) => "[ ] ",
            None => "",
        };
        let body = blocks(&item.blocks, true);
        if body.is_empty() {
            out.push_str(marker.trim_end());
            if !task.is_empty() {
                out.push(' ');
                out.push_str(task.trim_end());
            }
            continue;
        }
        out.push_str(&marker);
        out.push(' ');
        out.push_str(task);
        for (n, line) in body.split('\n').enumerate() {
            if n > 0 {
                out.push('\n');
                if !line.is_empty() {
                    out.push_str(&indent);
                }
            }
            out.push_str(line);
        }
    }
    out
}

fn table_md(table: &Table) -> String {
    let width = table.rows.first().map_or(0, Vec::len);
    let row = |cells: &[Vec<Inline>]| {
        let mut line = String::from("|");
        for cell in cells {
            line.push(' ');
            line.push_str(&inlines(cell, Ctx::Cell));
            line.push_str(" |");
        }
        line
    };
    let mut out = String::new();
    let mut rows = table.rows.iter();
    if table.header {
        if let Some(head) = rows.next() {
            out.push_str(&row(head));
        }
    } else {
        out.push_str(&row(&vec![Vec::new(); width]));
    }
    out.push_str("\n|");
    out.push_str(&" --- |".repeat(width));
    for cells in rows {
        out.push('\n');
        out.push_str(&row(cells));
    }
    out
}

fn prefix_lines(text: &str, prefix: &str, blank: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.is_empty() {
                blank.to_owned()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn longest_run(text: &str, ch: char) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == ch { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

// ── inline ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ctx {
    Paragraph,
    Heading,
    Cell,
}

/// Emphasis marks in nesting order (outermost first). Code is written per
/// span.
const EMPHASIS: [Marks; 4] = [Marks::UNDERLINE, Marks::STRIKE, Marks::BOLD, Marks::ITALIC];

enum Tok {
    Open(Marks),
    Close(Marks),
    /// Escaped text.
    Text(String),
    /// Written as is.
    Raw(String),
}

fn inlines(content: &[Inline], ctx: Ctx) -> String {
    let mut toks: Vec<Tok> = Vec::new();
    let mut line_start = true;
    let mut i = 0;
    while i < content.len() {
        match &content[i] {
            Inline::Text(span) if span.link.is_some() => {
                let link = span.link.clone();
                let end = content[i..]
                    .iter()
                    .position(|x| !matches!(x, Inline::Text(s) if s.link == link))
                    .map_or(content.len(), |n| i + n);
                let url = match &link {
                    Some(Link::Url(url)) => url.clone(),
                    Some(Link::WorkItem(id)) => format!("#{id}"),
                    None => String::new(),
                };
                // A link showing its own URL: `<https://…>`.
                let bare = matches!(&content[i..end], [Inline::Text(s)] if s.marks.is_empty() && s.text == url)
                    && (url.starts_with("https://") || url.starts_with("http://"))
                    && !url.contains([' ', '<', '>', '\\', '&']);
                if bare {
                    toks.push(Tok::Raw(format!("<{url}>")));
                    line_start = false;
                    i = end;
                    continue;
                }
                // `text!` + `[link](…)` would read as an image.
                if let Some(Tok::Text(prev)) = toks.last_mut()
                    && prev.ends_with('!')
                    && !prev.ends_with("\\!")
                {
                    prev.insert(prev.len() - 1, '\\');
                }
                toks.push(Tok::Raw("[".into()));
                emphasis_run(&content[i..end], ctx, &mut toks, &mut false);
                toks.push(Tok::Raw(format!("]({})", link_destination(&url))));
                line_start = false;
                i = end;
            }
            Inline::Text(_) => {
                let end = content[i..]
                    .iter()
                    .position(|x| !matches!(x, Inline::Text(s) if s.link.is_none()))
                    .map_or(content.len(), |n| i + n);
                emphasis_run(&content[i..end], ctx, &mut toks, &mut line_start);
                i = end;
            }
            Inline::Break => {
                toks.push(Tok::Raw(match ctx {
                    Ctx::Paragraph => "\\\n".into(),
                    Ctx::Heading | Ctx::Cell => "<br>".into(),
                }));
                line_start = ctx == Ctx::Paragraph;
                i += 1;
            }
            Inline::Mention(id) => {
                toks.push(Tok::Raw(crate::markdown::mention_token(id)));
                line_start = false;
                i += 1;
            }
            Inline::Image { src, alt } => {
                let mut ls = false;
                toks.push(Tok::Raw(format!(
                    "![{}]({})",
                    escape(alt, ctx, &mut ls),
                    link_destination(src)
                )));
                line_start = false;
                i += 1;
            }
        }
    }
    render(&toks)
}

/// Text spans without links (or all inside one link): emphasis
/// transitions with a stack, so delimiters nest.
fn emphasis_run(spans: &[Inline], ctx: Ctx, toks: &mut Vec<Tok>, line_start: &mut bool) {
    let mut stack: Vec<Marks> = Vec::new();
    for inline in spans {
        let Inline::Text(span) = inline else { continue };
        let want: Vec<Marks> = EMPHASIS
            .iter()
            .copied()
            .filter(|m| span.marks.contains(*m))
            .collect();
        // Close from the first open mark no longer wanted; reopen above it.
        let keep = stack
            .iter()
            .position(|m| !want.contains(m))
            .unwrap_or(stack.len());
        while stack.len() > keep {
            toks.push(Tok::Close(stack.pop().expect("checked length")));
        }
        for m in want {
            if !stack.contains(&m) {
                stack.push(m);
                toks.push(Tok::Open(m));
            }
        }
        if span.marks.contains(Marks::CODE) {
            toks.push(Tok::Raw(code_span(&span.text)));
            *line_start = false;
        } else {
            toks.push(Tok::Text(escape(&span.text, ctx, line_start)));
        }
    }
    while let Some(m) = stack.pop() {
        toks.push(Tok::Close(m));
    }
}

/// Tokens → text, choosing `*`-style delimiters where CommonMark's
/// flanking rules accept them and inline HTML elsewhere.
fn render(toks: &[Tok]) -> String {
    // Pair each open with its close.
    let mut partner = vec![usize::MAX; toks.len()];
    let mut open: Vec<usize> = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Open(_) => open.push(i),
            Tok::Close(_) => {
                if let Some(o) = open.pop() {
                    partner[o] = i;
                    partner[i] = o;
                }
            }
            _ => {}
        }
    }
    let first_char = |from: usize| -> Option<char> {
        match toks.get(from)? {
            Tok::Text(t) | Tok::Raw(t) => t.chars().next(),
            Tok::Open(_) | Tok::Close(_) => Some('*'),
        }
    };
    let last_char = |before: usize| -> Option<char> {
        let tok = toks.get(before.checked_sub(1)?)?;
        match tok {
            Tok::Text(t) | Tok::Raw(t) => t.chars().next_back(),
            Tok::Open(_) | Tok::Close(_) => Some('*'),
        }
    };
    let html = |i: usize| -> bool {
        let (o, c) = match toks[i] {
            Tok::Open(_) => (i, partner[i]),
            _ => (partner[i], i),
        };
        if c == usize::MAX || o == usize::MAX {
            return true;
        }
        let Tok::Open(mark) = toks[o] else {
            return true;
        };
        if mark == Marks::UNDERLINE {
            return true;
        }
        // `*a*` then `**b**` written back to back is one `***` run.
        let star = |t: &Tok| matches!(t, Tok::Close(m) if *m == Marks::BOLD || *m == Marks::ITALIC);
        if (mark == Marks::BOLD || mark == Marks::ITALIC) && o > 0 && star(&toks[o - 1]) {
            return true;
        }
        let ws = |c: Option<char>| c.is_none_or(char::is_whitespace);
        let punct = |c: Option<char>| {
            c.is_some_and(|c| {
                c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace())
            })
        };
        // CommonMark flanking at a delimiter between `before` and `after`.
        let left = |before: Option<char>, after: Option<char>| {
            !ws(after) && (!punct(after) || ws(before) || punct(before))
        };
        let right = |before: Option<char>, after: Option<char>| {
            !ws(before) && (!punct(before) || ws(after) || punct(after))
        };
        let (before_open, after_open) = (last_char(o), first_char(o + 1));
        let (before_close, after_close) = (last_char(c), first_char(c + 1));
        // Only delimiters that can do just one thing: a run that could both
        // open and close (intraword, between punctuation) is ambiguous.
        let opens = left(before_open, after_open) && !right(before_open, after_open);
        let closes = right(before_close, after_close) && !left(before_close, after_close);
        !(opens && closes)
    };
    let mut out = String::new();
    for (i, tok) in toks.iter().enumerate() {
        match tok {
            Tok::Text(t) | Tok::Raw(t) => out.push_str(t),
            Tok::Open(m) | Tok::Close(m) => {
                let closing = matches!(tok, Tok::Close(_));
                let (md, tag) = match *m {
                    Marks::BOLD => ("**", "strong"),
                    Marks::ITALIC => ("*", "em"),
                    Marks::STRIKE => ("~~", "s"),
                    _ => ("", "u"),
                };
                if html(i) {
                    out.push_str(if closing { "</" } else { "<" });
                    out.push_str(tag);
                    out.push('>');
                } else {
                    out.push_str(md);
                }
            }
        }
    }
    out
}

fn code_span(text: &str) -> String {
    let fence = "`".repeat(longest_run(text, '`') + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' '));
    let pad = if pad { " " } else { "" };
    format!("{fence}{pad}{text}{pad}{fence}")
}

fn link_destination(url: &str) -> String {
    if url.is_empty() || url.contains([' ', '(', ')', '<', '>']) {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url.to_owned()
    }
}

/// Backslash-escape what Markdown would read as structure.
fn escape(text: &str, ctx: Ctx, line_start: &mut bool) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let chars: Vec<char> = text.chars().collect();
    // `1.` / `1)` at a line start is a list marker: escape its punctuation.
    let marker_punct = if *line_start {
        let digits = chars.iter().take_while(|c| c.is_ascii_digit()).count();
        (digits > 0 && digits <= 9 && matches!(chars.get(digits), Some('.' | ')')))
            .then_some(digits)
    } else {
        None
    };
    for (i, &c) in chars.iter().enumerate() {
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        let next = chars.get(i + 1).copied();
        let at_line_start = i == 0 && *line_start;
        let escape = match c {
            '\\' | '`' | '*' | '[' | ']' | '<' | '>' | '~' => true,
            '|' => ctx == Ctx::Cell,
            '#' => at_line_start || ctx == Ctx::Heading,
            '-' | '+' | '=' => at_line_start,
            '_' => {
                !(prev.is_some_and(char::is_alphanumeric)
                    && next.is_some_and(char::is_alphanumeric))
            }
            '&' => entity_like(&chars[i + 1..]),
            '!' => next == Some('['),
            _ => marker_punct == Some(i),
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    if !text.is_empty() {
        *line_start = false;
    }
    out
}

fn entity_like(rest: &[char]) -> bool {
    let body: Vec<char> = rest
        .iter()
        .copied()
        .take_while(|c| *c != ';')
        .take(33)
        .collect();
    !body.is_empty()
        && rest.get(body.len()) == Some(&';')
        && body
            .iter()
            .enumerate()
            .all(|(i, c)| c.is_ascii_alphanumeric() || (i == 0 && *c == '#'))
}
