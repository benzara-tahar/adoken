//! Azure DevOps HTML → [`Doc`].
//!
//! A small, tolerant tokenizer and tree builder (not a full HTML5 parser):
//! descriptions are the editor's `<div style="box-sizing:border-box;">`
//! soup, Word/Outlook pastes, and the occasional unclosed tag. Rules:
//!
//! - Block elements (`div`, `p`, …) end the current paragraph; inline
//!   formatting elements set marks; unknown tags render their text (FR-3).
//! - `<!-- comments -->`, `<style>`, `<script>`, `<head>`, `<title>` never
//!   render, with their content.
//! - Source styling is never applied. While converting, [`Features`]
//!   records everything FR-18 must not silently drop (tables, images,
//!   colors, fonts, highlights, other visible styles, attachments).

use crate::model::{Block, Doc, Inline, Link, List, ListItem, Marks, Span, Table, is_guid};

/// What a description holds beyond FR-3-minus-tables-and-images.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Features {
    pub tables: usize,
    pub images: usize,
    pub attachments: usize,
    pub colors: bool,
    pub highlights: bool,
    pub fonts: bool,
    /// Other visible styling or markup a Markdown round trip would drop.
    pub custom: bool,
}

/// Parse HTML (or tagless plain text) into a normalized document.
pub(crate) fn parse(html: &str) -> (Doc, Features) {
    if !has_markup(html) {
        return (plain_text_doc(html), Features::default());
    }
    let root = build_tree(html);
    let mut conv = Converter::default();
    let mut blocks = Vec::new();
    conv.block_children(&root.children, &Ctx::default(), &mut blocks);
    (Doc::from_blocks(blocks), conv.features)
}

/// `(id, name)` of every mention anchor, names without the leading `@`.
pub fn mention_names(html: &str) -> Vec<(String, String)> {
    fn walk(node: &Node, out: &mut Vec<(String, String)>) {
        if let Node::Element(e) = node {
            if e.tag == "a"
                && let Some(id) = e.attr("data-vss-mention").and_then(mention_id)
            {
                let mut text = String::new();
                text_content(node, &mut text);
                let name = text.trim().trim_start_matches('@').trim().to_owned();
                if !name.is_empty() && !out.iter().any(|(known, _)| *known == id) {
                    out.push((id, name));
                }
                return;
            }
            e.children.iter().for_each(|child| walk(child, out));
        }
    }
    let root = build_tree(html);
    let mut out = Vec::new();
    root.children.iter().for_each(|child| walk(child, &mut out));
    out
}

/// `version:2.0,{guid}` → the lowercased guid.
fn mention_id(value: &str) -> Option<String> {
    let id = value.rsplit(',').next()?.trim();
    is_guid(id).then(|| id.to_ascii_lowercase())
}

/// Anything that tokenizes as a tag or comment.
fn has_markup(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.iter().enumerate().any(|(i, b)| {
        *b == b'<'
            && bytes
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, b'/' | b'!' | b'?'))
    })
}

/// Text with no markup at all: blank lines separate paragraphs, single
/// newlines are line breaks (plain values set through the API).
fn plain_text_doc(text: &str) -> Doc {
    let text = decode_entities(text);
    let mut blocks = Vec::new();
    let mut para: Vec<Inline> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !para.is_empty() {
                blocks.push(Block::Paragraph(std::mem::take(&mut para)));
            }
            continue;
        }
        if !para.is_empty() {
            para.push(Inline::Break);
        }
        para.push(Inline::Text(Span::plain(line)));
    }
    if !para.is_empty() {
        blocks.push(Block::Paragraph(para));
    }
    Doc::from_blocks(blocks)
}

// ── tree ───────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Node {
    Element(Element),
    Text(String),
}

/// `(name, value)`, names lowercased, values entity-decoded.
type Attrs = Vec<(String, String)>;

#[derive(Debug, Default)]
struct Element {
    tag: String,
    attrs: Attrs,
    children: Vec<Node>,
}

impl Element {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn push_text(&mut self, text: String) {
        if let Some(Node::Text(last)) = self.children.last_mut() {
            last.push_str(&text);
        } else {
            self.children.push(Node::Text(text));
        }
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Content is raw text that never renders.
const RAW_HIDDEN: &[&str] = &[
    "script", "style", "title", "textarea", "xmp", "iframe", "noembed", "noframes", "noscript",
];

/// Starting one of these closes an open `<p>`.
const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "fieldset",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
];

struct TreeBuilder {
    stack: Vec<Element>,
}

impl TreeBuilder {
    fn top(&mut self) -> &mut Element {
        self.stack.last_mut().expect("root stays on the stack")
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            let done = self.stack.pop().expect("checked length");
            self.top().children.push(Node::Element(done));
        }
    }

    /// Index of the nearest open `tag`, searching down to (not past) any of
    /// `scope`.
    fn find_open(&self, tag: &str, scope: &[&str]) -> Option<usize> {
        for (i, e) in self.stack.iter().enumerate().skip(1).rev() {
            if e.tag == tag {
                return Some(i);
            }
            if scope.contains(&e.tag.as_str()) {
                return None;
            }
        }
        None
    }

    fn close_to(&mut self, index: usize) {
        while self.stack.len() > index {
            self.pop();
        }
    }

    fn start(&mut self, tag: String, attrs: Attrs, self_closing: bool) {
        // Implied end tags.
        let implied: Option<(&str, &[&str])> = match tag.as_str() {
            "li" => Some(("li", &["ul", "ol", "table"])),
            "tr" => Some(("tr", &["table"])),
            "td" | "th" => None,
            "dt" | "dd" => None,
            t if CLOSES_P.contains(&t) => {
                Some(("p", &["div", "li", "td", "th", "blockquote", "table"]))
            }
            _ => None,
        };
        if let Some((open, scope)) = implied
            && let Some(i) = self.find_open(open, scope)
        {
            self.close_to(i);
        }
        if matches!(tag.as_str(), "td" | "th") {
            for cell in ["td", "th"] {
                if let Some(i) = self.find_open(cell, &["tr", "table"]) {
                    self.close_to(i);
                }
            }
        }
        if matches!(tag.as_str(), "dt" | "dd") {
            for item in ["dt", "dd"] {
                if let Some(i) = self.find_open(item, &["dl"]) {
                    self.close_to(i);
                }
            }
        }
        let element = Element {
            tag,
            attrs,
            children: Vec::new(),
        };
        if self_closing || VOID.contains(&element.tag.as_str()) {
            self.top().children.push(Node::Element(element));
        } else {
            self.stack.push(element);
        }
    }

    fn end(&mut self, tag: &str) {
        if let Some(i) = self.find_open(tag, &[]) {
            self.close_to(i);
        }
    }
}

fn build_tree(html: &str) -> Element {
    let mut tree = TreeBuilder {
        stack: vec![Element {
            tag: "#root".into(),
            ..Element::default()
        }],
    };
    let mut rest = html;
    let mut text = String::new();
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            text.push_str(rest);
            break;
        };
        text.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let next = rest.as_bytes().get(1).copied();
        if rest.starts_with("<!--") {
            rest = rest[4..].find("-->").map_or("", |end| &rest[4 + end + 3..]);
            continue;
        }
        match next {
            Some(b'!') | Some(b'?') => {
                rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
            }
            Some(b'/') if rest.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic) => {
                flush_text(&mut tree, &mut text);
                let (name, after) = tag_name(&rest[2..]);
                tree.end(&name);
                rest = after.find('>').map_or("", |end| &after[end + 1..]);
            }
            Some(b) if b.is_ascii_alphabetic() => {
                let (name, after) = tag_name(&rest[1..]);
                let Some((attrs, self_closing, after)) = parse_attrs(after) else {
                    // Unterminated tag at the end: drop it.
                    rest = "";
                    continue;
                };
                flush_text(&mut tree, &mut text);
                rest = after;
                if RAW_HIDDEN.contains(&name.as_str()) && !self_closing {
                    rest = skip_past_close(rest, &name);
                    continue;
                }
                tree.start(name, attrs, self_closing);
            }
            _ => {
                text.push('<');
                rest = &rest[1..];
            }
        }
    }
    flush_text(&mut tree, &mut text);
    tree.close_to(1);
    tree.stack.pop().expect("root")
}

fn flush_text(tree: &mut TreeBuilder, text: &mut String) {
    if !text.is_empty() {
        let decoded = decode_entities(text);
        text.clear();
        tree.top().push_text(decoded);
    }
}

/// Lowercased tag name and the rest after it.
fn tag_name(s: &str) -> (String, &str) {
    let end = s
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(s.len());
    (s[..end].to_ascii_lowercase(), &s[end..])
}

/// Attributes up to and including the closing `>`: `(attrs, self_closing,
/// rest)`, or `None` when the tag never closes.
fn parse_attrs(mut s: &str) -> Option<(Attrs, bool, &str)> {
    let mut attrs = Vec::new();
    let mut self_closing = false;
    loop {
        s = s.trim_start();
        let c = s.chars().next()?;
        match c {
            '>' => return Some((attrs, self_closing, &s[1..])),
            '/' => {
                self_closing = true;
                s = &s[1..];
            }
            _ => {
                self_closing = false;
                let end = s
                    .find(|c: char| c.is_whitespace() || matches!(c, '=' | '>' | '/'))
                    .unwrap_or(s.len());
                let name = s[..end].to_ascii_lowercase();
                s = s[end..].trim_start();
                let mut value = String::new();
                if let Some(after) = s.strip_prefix('=') {
                    let after = after.trim_start();
                    match after.chars().next() {
                        Some(q @ ('"' | '\'')) => {
                            let close = after[1..].find(q)?;
                            value = decode_entities(&after[1..1 + close]);
                            s = &after[close + 2..];
                        }
                        Some(_) => {
                            let end = after
                                .find(|c: char| c.is_whitespace() || c == '>')
                                .unwrap_or(after.len());
                            value = decode_entities(&after[..end]);
                            s = &after[end..];
                        }
                        None => return None,
                    }
                }
                // A stray `=value` has no name: dropped.
                if !name.is_empty() {
                    attrs.push((name, value));
                }
            }
        }
    }
}

/// The text after `</tag …>` (empty when the element never closes).
fn skip_past_close<'a>(rest: &'a str, tag: &str) -> &'a str {
    let close = format!("</{tag}");
    let Some(at) = find_ci(rest, &close) else {
        return "";
    };
    let after = &rest[at + close.len()..];
    after.find('>').map_or("", |end| &after[end + 1..])
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Named (common set) and numeric character references.
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let entity = rest[1..]
            .find(';')
            .filter(|n| *n <= 32)
            .map(|n| &rest[1..1 + n]);
        match entity.and_then(entity_char) {
            Some(ch) => {
                out.push(ch);
                rest = &rest[entity.map_or(0, str::len) + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity_char(e: &str) -> Option<char> {
    if let Some(num) = e.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code).filter(|c| *c != '\0');
    }
    Some(match e {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "ensp" | "emsp" | "thinsp" => ' ',
        "ndash" => '–',
        "mdash" => '—',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "ldquo" => '“',
        "rdquo" => '”',
        "bdquo" => '„',
        "laquo" => '«',
        "raquo" => '»',
        "bull" => '•',
        "middot" => '·',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "plusmn" => '±',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "harr" => '↔',
        "rArr" => '⇒',
        "le" => '≤',
        "ge" => '≥',
        "ne" => '≠',
        "check" => '✓',
        "zwj" => '\u{200d}',
        "zwnj" => '\u{200c}',
        "shy" => '\u{ad}',
        _ => return None,
    })
}

fn text_content(node: &Node, out: &mut String) {
    match node {
        Node::Text(t) => out.push_str(t),
        Node::Element(e) if e.tag == "br" => out.push('\n'),
        Node::Element(e) => e.children.iter().for_each(|c| text_content(c, out)),
    }
}

// ── conversion ─────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct Ctx {
    marks: Marks,
    link: Option<Link>,
}

#[derive(Default)]
struct Converter {
    features: Features,
}

/// Elements that only group blocks and have no look of their own.
const PLAIN_BLOCKS: &[&str] = &[
    "#root", "html", "body", "div", "p", "section", "article", "main", "header", "footer", "nav",
    "aside", "form",
];
/// Block elements with a look Markdown can't keep (indent, centering…).
const STYLED_BLOCKS: &[&str] = &[
    "center",
    "address",
    "dl",
    "dt",
    "dd",
    "figure",
    "figcaption",
    "fieldset",
    "legend",
    "details",
    "summary",
    "caption",
];
/// Inline elements with no visible effect.
const PLAIN_INLINES: &[&str] = &["span", "o:p", "abbr", "bdi", "bdo", "data", "time", "wbr"];
/// Never rendered.
const HIDDEN: &[&str] = &[
    "head", "template", "meta", "link", "base", "object", "embed", "svg", "math", "canvas",
    "video", "audio", "source", "track", "map", "area", "select", "button", "colgroup", "col",
];

impl Converter {
    fn block_children(&mut self, children: &[Node], ctx: &Ctx, out: &mut Vec<Block>) {
        let mut inl = Vec::new();
        for child in children {
            self.node(child, ctx, out, &mut inl);
        }
        flush(out, &mut inl);
    }

    fn node(&mut self, node: &Node, ctx: &Ctx, out: &mut Vec<Block>, inl: &mut Vec<Inline>) {
        match node {
            Node::Text(text) => inl.push(Inline::Text(Span {
                text: text.clone(),
                marks: ctx.marks,
                link: ctx.link.clone(),
            })),
            Node::Element(e) => self.element(e, ctx, out, inl),
        }
    }

    fn inline_children(
        &mut self,
        e: &Element,
        ctx: &Ctx,
        out: &mut Vec<Block>,
        inl: &mut Vec<Inline>,
    ) {
        for child in &e.children {
            self.node(child, ctx, out, inl);
        }
    }

    fn element(&mut self, e: &Element, ctx: &Ctx, out: &mut Vec<Block>, inl: &mut Vec<Inline>) {
        let tag = e.tag.as_str();
        if HIDDEN.contains(&tag) {
            if !matches!(tag, "head" | "meta" | "link" | "base" | "colgroup" | "col") {
                self.features.custom = true;
            }
            return;
        }
        self.check_attrs(e);
        let mark = match tag {
            "b" | "strong" => Some(Marks::BOLD),
            "i" | "em" | "cite" | "dfn" | "var" => Some(Marks::ITALIC),
            "u" | "ins" => Some(Marks::UNDERLINE),
            "s" | "strike" | "del" => Some(Marks::STRIKE),
            "code" | "kbd" | "samp" | "tt" => Some(Marks::CODE),
            _ => None,
        };
        if let Some(mark) = mark {
            if matches!(tag, "cite" | "dfn" | "var" | "kbd" | "samp" | "tt") {
                self.features.custom = true;
            }
            let ctx = Ctx {
                marks: ctx.marks | mark,
                link: ctx.link.clone(),
            };
            return self.inline_children(e, &ctx, out, inl);
        }
        match tag {
            "a" => {
                if let Some(id) = e.attr("data-vss-mention").and_then(mention_id) {
                    inl.push(Inline::Mention(id));
                    return;
                }
                let href = e.attr("href").map(str::trim).unwrap_or("");
                if is_attachment(href) {
                    self.features.attachments += 1;
                }
                let link = (!href.is_empty()
                    && !href.starts_with('#')
                    && !href.to_ascii_lowercase().starts_with("javascript:"))
                .then(|| Link::Url(href.to_owned()));
                let ctx = Ctx {
                    marks: ctx.marks,
                    link: link.or_else(|| ctx.link.clone()),
                };
                self.inline_children(e, &ctx, out, inl);
            }
            "br" => inl.push(Inline::Break),
            "img" => {
                self.features.images += 1;
                inl.push(Inline::Image {
                    src: e.attr("src").unwrap_or("").trim().to_owned(),
                    alt: e.attr("alt").unwrap_or("").trim().to_owned(),
                });
            }
            // Task checkboxes are read by `item`; anywhere else an input
            // is a form control Markdown can't keep.
            "input" => {
                if !is_checkbox(e) {
                    self.features.custom = true;
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                flush(out, inl);
                let level = tag.as_bytes()[1] - b'0';
                let content = self.inline_content(&e.children, ctx);
                out.push(Block::Heading { level, content });
            }
            "pre" => {
                flush(out, inl);
                let mut text = String::new();
                e.children.iter().for_each(|c| text_content(c, &mut text));
                out.push(Block::CodeBlock(text));
            }
            "ul" | "ol" | "menu" => {
                flush(out, inl);
                let list = self.list(e, ctx);
                out.push(Block::List(list));
            }
            "li" => {
                flush(out, inl);
                let item = self.item(e, ctx);
                out.push(Block::List(List {
                    start: None,
                    items: vec![item],
                }));
            }
            "blockquote" => {
                flush(out, inl);
                let mut inner = Vec::new();
                self.block_children(&e.children, ctx, &mut inner);
                out.push(Block::Quote(inner));
            }
            "hr" => {
                flush(out, inl);
                out.push(Block::Rule);
            }
            "table" => {
                flush(out, inl);
                self.features.tables += 1;
                let table = self.table(e);
                out.push(Block::Table(table));
            }
            "font" | "mark" => {
                if tag == "mark" {
                    self.features.highlights = true;
                }
                self.inline_children(e, ctx, out, inl);
            }
            t if PLAIN_INLINES.contains(&t) => self.inline_children(e, ctx, out, inl),
            t if PLAIN_BLOCKS.contains(&t) || STYLED_BLOCKS.contains(&t) => {
                if STYLED_BLOCKS.contains(&t) {
                    self.features.custom = true;
                }
                flush(out, inl);
                for child in &e.children {
                    self.node(child, ctx, out, inl);
                }
                flush(out, inl);
            }
            // Table parts outside a table: their text.
            "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" => {
                self.features.custom = true;
                flush(out, inl);
                for child in &e.children {
                    self.node(child, ctx, out, inl);
                }
                flush(out, inl);
            }
            // Unknown tags render their text (FR-3).
            _ => {
                self.features.custom = true;
                self.inline_children(e, ctx, out, inl);
            }
        }
    }

    /// Children as one run of inlines; block boundaries become breaks.
    fn inline_content(&mut self, children: &[Node], ctx: &Ctx) -> Vec<Inline> {
        let mut blocks = Vec::new();
        self.block_children(children, ctx, &mut blocks);
        let mut out = Vec::new();
        flatten(&blocks, &mut out);
        out
    }

    fn list(&mut self, e: &Element, ctx: &Ctx) -> List {
        let start = (e.tag == "ol").then(|| {
            e.attr("start")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1)
        });
        let mut items: Vec<ListItem> = Vec::new();
        for child in &e.children {
            match child {
                Node::Text(t) if t.trim().is_empty() => {}
                Node::Element(li) if li.tag == "li" => {
                    self.check_attrs(li);
                    items.push(self.item(li, ctx));
                }
                // `<ul><li>a</li><ul>…</ul></ul>`: a nested list belongs to
                // the item before it; so does stray content.
                other => {
                    let mut blocks = Vec::new();
                    self.block_children(std::slice::from_ref(other), ctx, &mut blocks);
                    match items.last_mut() {
                        Some(last) => last.blocks.extend(blocks),
                        None => items.push(ListItem { task: None, blocks }),
                    }
                }
            }
        }
        List { start, items }
    }

    fn item(&mut self, li: &Element, ctx: &Ctx) -> ListItem {
        let task = leading_checkbox(li);
        let mut blocks = Vec::new();
        self.block_children(&li.children, ctx, &mut blocks);
        ListItem { task, blocks }
    }

    fn table(&mut self, table: &Element) -> Table {
        fn rows<'a>(e: &'a Element, in_head: bool, out: &mut Vec<(&'a Element, bool)>) {
            for child in &e.children {
                if let Node::Element(c) = child {
                    match c.tag.as_str() {
                        "tr" => out.push((c, in_head)),
                        "thead" => rows(c, true, out),
                        "tbody" | "tfoot" => rows(c, false, out),
                        _ => {}
                    }
                }
            }
        }
        let mut found = Vec::new();
        rows(table, false, &mut found);
        let mut header = false;
        let mut out_rows = Vec::with_capacity(found.len());
        for (index, (tr, in_head)) in found.into_iter().enumerate() {
            let mut cells = Vec::new();
            let mut all_th = true;
            for child in &tr.children {
                if let Node::Element(cell) = child
                    && matches!(cell.tag.as_str(), "td" | "th")
                {
                    all_th &= cell.tag == "th";
                    cells.push(self.inline_content(&cell.children, &Ctx::default()));
                }
            }
            if index == 0 {
                header = in_head || (all_th && !cells.is_empty());
            }
            out_rows.push(cells);
        }
        Table {
            header,
            rows: out_rows,
        }
    }

    /// FR-18: presentational attributes. `box-sizing` and `display: inline`
    /// (what Azure DevOps' own editor adds) are ignored.
    fn check_attrs(&mut self, e: &Element) {
        for (name, value) in &e.attrs {
            match name.as_str() {
                "style" => {
                    for decl in value.split(';') {
                        let Some((prop, val)) = decl.split_once(':') else {
                            continue;
                        };
                        let prop = prop.trim().to_ascii_lowercase();
                        let val = val.trim().to_ascii_lowercase();
                        match prop.as_str() {
                            "box-sizing" => {}
                            "display"
                                if val.strip_suffix("!important").unwrap_or(&val).trim()
                                    == "inline" => {}
                            "color" | "-webkit-text-fill-color" => self.features.colors = true,
                            "background" | "background-color" => self.features.highlights = true,
                            p if p.starts_with("font") => self.features.fonts = true,
                            _ => self.features.custom = true,
                        }
                    }
                }
                "color" => self.features.colors = true,
                "face" | "size" if e.tag == "font" => self.features.fonts = true,
                "bgcolor" => self.features.highlights = true,
                "align" | "valign" | "width" | "height" | "border" | "hidden" => {
                    self.features.custom = true
                }
                _ => {}
            }
        }
    }
}

fn flush(out: &mut Vec<Block>, inl: &mut Vec<Inline>) {
    if inl.is_empty() {
        return;
    }
    let content = std::mem::take(inl);
    // Whitespace-only text between blocks is not a paragraph.
    let visible = content.iter().any(|i| match i {
        Inline::Text(span) => !span.text.trim().is_empty(),
        Inline::Break => false,
        _ => true,
    });
    if visible {
        out.push(Block::Paragraph(content));
    }
}

/// Blocks as one inline run, a break between blocks.
pub(crate) fn flatten(blocks: &[Block], out: &mut Vec<Inline>) {
    for block in blocks {
        if !out.is_empty() && !matches!(block, Block::Rule) {
            out.push(Inline::Break);
        }
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => {
                out.extend(content.iter().cloned())
            }
            Block::CodeBlock(text) => {
                for (i, line) in text.lines().enumerate() {
                    if i > 0 {
                        out.push(Inline::Break);
                    }
                    out.push(Inline::Text(Span {
                        text: line.to_owned(),
                        marks: Marks::CODE,
                        link: None,
                    }));
                }
            }
            Block::List(list) => {
                for item in &list.items {
                    flatten(&item.blocks, out);
                }
            }
            Block::Quote(inner) => flatten(inner, out),
            Block::Rule => {}
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in row {
                        if !out.is_empty() {
                            out.push(Inline::Text(Span::plain(" ")));
                        }
                        out.extend(cell.iter().cloned());
                    }
                }
            }
        }
    }
}

fn is_checkbox(e: &Element) -> bool {
    e.tag == "input"
        && e.attr("type")
            .is_some_and(|t| t.eq_ignore_ascii_case("checkbox"))
}

/// A checkbox before any text in a list item: a task item.
fn leading_checkbox(li: &Element) -> Option<bool> {
    for child in &li.children {
        match child {
            Node::Text(t) if t.trim().is_empty() => {}
            Node::Element(e) if is_checkbox(e) => return Some(e.attr("checked").is_some()),
            Node::Element(e) if matches!(e.tag.as_str(), "p" | "div" | "span" | "label") => {
                return leading_checkbox(e);
            }
            _ => return None,
        }
    }
    None
}

/// Azure DevOps work item attachment URLs.
fn is_attachment(href: &str) -> bool {
    find_ci(href, "/_apis/wit/attachments/").is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Block {
        Block::Paragraph(vec![Inline::Text(Span::plain(text))])
    }

    /// Work item 14's live description (`crates/sync/src/wire.rs` tests).
    const LIVE: &str = "<div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">The total order amount shown during checkout does not match the actual sum of the item prices in the cart.</span><br style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">&#128310;</span> </div><div style=\"box-sizing:border-box;\"> </div><div style=\"box-sizing:border-box;\">workItemId<br> </div><br> </div><div> </div>";

    #[test]
    fn editor_div_soup_becomes_plain_paragraphs_with_no_features() {
        let (doc, features) = parse(LIVE);
        assert_eq!(
            doc.blocks,
            vec![
                para(
                    "The total order amount shown during checkout does not match the actual sum of the item prices in the cart."
                ),
                para("\u{1F536}"),
                para("workItemId"),
            ]
        );
        assert_eq!(features, Features::default());
    }

    #[test]
    fn hidden_markup_and_unclosed_tags_never_render() {
        let pasted = "<html><head><title>t</title><style>p{color:red}</style></head>\
            <!--[if gte mso 9]><xml><o:OfficeDocumentSettings/></xml><![endif]-->\
            <p>Kept<p>too</p><script type=\"x\">alert(1)</script><STYLE>.a{}</STYLE></html>";
        let (doc, _) = parse(pasted);
        assert_eq!(doc.blocks, vec![para("Kept"), para("too")]);
        assert_eq!(parse("a <!-- never closed").0.blocks, vec![para("a")]);
        assert_eq!(parse("<b>a < b</b>").0.blocks.len(), 1);
    }

    #[test]
    fn tagless_text_keeps_its_paragraphs_and_lines() {
        let (doc, _) = parse("Plain &amp; simple\nline\n\n  Paragraph ");
        assert_eq!(
            doc.blocks,
            vec![
                Block::Paragraph(vec![
                    Inline::Text(Span::plain("Plain & simple")),
                    Inline::Break,
                    Inline::Text(Span::plain("line")),
                ]),
                para("Paragraph"),
            ]
        );
    }

    #[test]
    fn styles_are_classified_and_editor_noise_is_ignored() {
        let features = |html: &str| parse(html).1;
        assert_eq!(
            features("<span style=\"display: inline !important; box-sizing: border-box\">x</span>"),
            Features::default()
        );
        assert!(features("<span style=\"color:red\">x</span>").colors);
        assert!(features("<span style=\"background-color: yellow\">x</span>").highlights);
        assert!(features("<font face=\"Arial\">x</font>").fonts);
        assert!(features("<div style=\"margin-left:40px\">x</div>").custom);
        assert!(features("<sup>2</sup>").custom);
        assert_eq!(features("<img src=a><img src=b>").images, 2);
        assert_eq!(
            features("<a href=\"https://dev.azure.com/o/p/_apis/wit/attachments/1?fileName=a.log\">a.log</a>")
                .attachments,
            1
        );
    }

    #[test]
    fn nested_lists_attach_to_the_item_before_them() {
        let (doc, _) =
            parse("<ul><li>a</li><ul><li>b</li></ul><li>c<ol start=\"3\"><li>d</ol></ul>");
        let Block::List(list) = &doc.blocks[0] else {
            panic!("{doc:?}")
        };
        assert_eq!(list.items.len(), 2);
        assert!(matches!(&list.items[0].blocks[1], Block::List(inner) if inner.start.is_none()));
        assert!(matches!(&list.items[1].blocks[1], Block::List(inner) if inner.start == Some(3)));
    }

    #[test]
    fn mention_names_come_from_anchor_text() {
        let html = "<a href=\"#\" data-vss-mention=\"version:2.0,D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607\">@Dana Example</a>";
        assert_eq!(
            mention_names(html),
            vec![(
                "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607".to_owned(),
                "Dana Example".to_owned()
            )]
        );
    }
}
