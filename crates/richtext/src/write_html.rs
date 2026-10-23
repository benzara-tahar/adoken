//! [`Doc`] → HTML, for saving an HTML description (FR-17). Plain elements
//! only, no styles or classes: Azure DevOps renders them with its own
//! typography, and the saved value parses back to the same document.

use crate::model::{Block, Doc, Inline, Link, Marks};

/// `names(id)` gives a mention's display name for the anchor text Azure
/// DevOps shows; unknown ids fall back to the id.
pub fn to_html(doc: &Doc, names: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    blocks(&doc.blocks, names, &mut out);
    out
}

fn blocks(blocks: &[Block], names: &dyn Fn(&str) -> Option<String>, out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph(content) => {
                out.push_str("<p>");
                inlines(content, names, out);
                out.push_str("</p>");
            }
            Block::Heading { level, content } => {
                out.push_str(&format!("<h{level}>"));
                inlines(content, names, out);
                out.push_str(&format!("</h{level}>"));
            }
            Block::CodeBlock(text) => {
                out.push_str("<pre><code>");
                escape_into(text, out);
                out.push_str("</code></pre>");
            }
            Block::List(list) => {
                match list.start {
                    None => out.push_str("<ul>"),
                    Some(1) => out.push_str("<ol>"),
                    Some(n) => out.push_str(&format!("<ol start=\"{n}\">")),
                }
                for item in &list.items {
                    out.push_str("<li>");
                    match item.task {
                        Some(true) => out.push_str("<input type=\"checkbox\" disabled checked> "),
                        Some(false) => out.push_str("<input type=\"checkbox\" disabled> "),
                        None => {}
                    }
                    // A first paragraph stays bare: `<li>text</li>`, the
                    // way Azure DevOps writes tight lists.
                    let rest = match item.blocks.first() {
                        Some(Block::Paragraph(content)) => {
                            inlines(content, names, out);
                            &item.blocks[1..]
                        }
                        _ => &item.blocks[..],
                    };
                    self::blocks(rest, names, out);
                    out.push_str("</li>");
                }
                out.push_str(if list.start.is_some() {
                    "</ol>"
                } else {
                    "</ul>"
                });
            }
            Block::Quote(inner) => {
                out.push_str("<blockquote>");
                self::blocks(inner, names, out);
                out.push_str("</blockquote>");
            }
            Block::Rule => out.push_str("<hr>"),
            Block::Table(table) => {
                out.push_str("<table>");
                for (r, row) in table.rows.iter().enumerate() {
                    let head = table.header && r == 0;
                    if head {
                        out.push_str("<thead>");
                    } else if r == usize::from(table.header) {
                        out.push_str("<tbody>");
                    }
                    out.push_str("<tr>");
                    let cell = if head { "th" } else { "td" };
                    for content in row {
                        out.push_str(&format!("<{cell}>"));
                        inlines(content, names, out);
                        out.push_str(&format!("</{cell}>"));
                    }
                    out.push_str("</tr>");
                    if head {
                        out.push_str("</thead>");
                    }
                }
                if table.rows.len() > usize::from(table.header) {
                    out.push_str("</tbody>");
                }
                out.push_str("</table>");
            }
        }
    }
}

const TAGS: [(Marks, &str); 5] = [
    (Marks::BOLD, "strong"),
    (Marks::ITALIC, "em"),
    (Marks::UNDERLINE, "u"),
    (Marks::STRIKE, "s"),
    (Marks::CODE, "code"),
];

fn inlines(content: &[Inline], names: &dyn Fn(&str) -> Option<String>, out: &mut String) {
    let mut open_link: Option<&Link> = None;
    for inline in content {
        let link = match inline {
            Inline::Text(span) => span.link.as_ref(),
            _ => None,
        };
        if open_link != link {
            if open_link.is_some() {
                out.push_str("</a>");
            }
            if let Some(link) = link {
                let href = match link {
                    Link::Url(url) => url.clone(),
                    Link::WorkItem(id) => format!("#{id}"),
                };
                out.push_str("<a href=\"");
                escape_attr_into(&href, out);
                out.push_str("\">");
            }
            open_link = link;
        }
        match inline {
            Inline::Text(span) => {
                for (mark, tag) in TAGS {
                    if span.marks.contains(mark) {
                        out.push_str(&format!("<{tag}>"));
                    }
                }
                escape_into(&span.text, out);
                for (mark, tag) in TAGS.iter().rev() {
                    if span.marks.contains(*mark) {
                        out.push_str(&format!("</{tag}>"));
                    }
                }
            }
            Inline::Break => out.push_str("<br>"),
            Inline::Mention(id) => {
                let name = names(id).unwrap_or_else(|| id.clone());
                out.push_str("<a href=\"#\" data-vss-mention=\"version:2.0,");
                escape_attr_into(id, out);
                out.push_str("\">@");
                escape_into(&name, out);
                out.push_str("</a>");
            }
            Inline::Image { src, alt } => {
                out.push_str("<img src=\"");
                escape_attr_into(src, out);
                out.push_str("\" alt=\"");
                escape_attr_into(alt, out);
                out.push_str("\">");
            }
        }
    }
    if open_link.is_some() {
        out.push_str("</a>");
    }
}

fn escape_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn escape_attr_into(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            c => escape_into(c.encode_utf8(&mut [0; 4]), out),
        }
    }
}
