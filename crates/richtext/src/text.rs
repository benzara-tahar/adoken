//! The plain-text projection (search index, previews, FR-34's
//! `description` column): one line per block, list items as `- `,
//! mentions kept as `@<id>` tokens for the caller to resolve.

use crate::model::{Block, Doc, Inline};

impl Doc {
    pub fn plain_text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        blocks(&self.blocks, "", &mut lines);
        lines.join("\n")
    }

    /// Rendered line count at roughly `chars_per_line` characters a line,
    /// for "Show all · N lines" (design §5.1). An estimate, not a layout.
    pub fn line_estimate(&self, chars_per_line: usize) -> usize {
        let per = chars_per_line.max(1);
        let mut lines: Vec<String> = Vec::new();
        blocks(&self.blocks, "", &mut lines);
        lines
            .iter()
            .map(|line| line.chars().count().div_ceil(per).max(1))
            .sum()
    }
}

fn blocks(blocks: &[Block], indent: &str, out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => {
                for line in inline_text(content).split('\n') {
                    out.push(format!("{indent}{line}"));
                }
            }
            Block::CodeBlock(text) => {
                out.extend(text.lines().map(|line| format!("{indent}{line}")));
            }
            Block::List(list) => {
                for (i, item) in list.items.iter().enumerate() {
                    let mut marker = match list.start {
                        None => "- ".to_owned(),
                        Some(start) => format!("{}. ", start + i as u64),
                    };
                    match item.task {
                        Some(true) => marker.push_str("[x] "),
                        Some(false) => marker.push_str("[ ] "),
                        None => {}
                    }
                    let mut inner = Vec::new();
                    self::blocks(&item.blocks, "", &mut inner);
                    let nested = format!("{indent}  ");
                    let mut first = true;
                    for line in inner {
                        if first {
                            out.push(format!("{indent}{marker}{line}"));
                            first = false;
                        } else {
                            out.push(format!("{nested}{line}"));
                        }
                    }
                    if first {
                        out.push(format!("{indent}{}", marker.trim_end()));
                    }
                }
            }
            Block::Quote(inner) => self::blocks(inner, indent, out),
            Block::Rule => {}
            Block::Table(table) => {
                for row in &table.rows {
                    let cells: Vec<String> = row
                        .iter()
                        .map(|c| inline_text(c).replace('\n', " "))
                        .collect();
                    out.push(format!("{indent}{}", cells.join(" | ")));
                }
            }
        }
    }
}

fn inline_text(content: &[Inline]) -> String {
    let mut out = String::new();
    for inline in content {
        match inline {
            Inline::Text(span) => out.push_str(&span.text),
            Inline::Break => out.push('\n'),
            Inline::Mention(id) => {
                out.push_str("@<");
                out.push_str(id);
                out.push('>');
            }
            Inline::Image { alt, .. } => out.push_str(alt),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::{Format, parse};

    /// The strings `crates/sync/src/wire.rs` pins for `html_to_text`.
    #[test]
    fn projection_matches_the_sync_plain_text() {
        let text = |html: &str| parse(html, Format::Html).plain_text();
        assert_eq!(
            text(
                "<div><div style=\"box-sizing:border-box;\"><span style=\"display:inline !important;\">Total is wrong.</span><br style=\"box-sizing:border-box;\"> </div><div>&#128310; &amp; more&nbsp;text</div><div> </div><ul><li>one</li><li>two</li></ul></div>"
            ),
            "Total is wrong.\n\u{1F536} & more text\n- one\n- two"
        );
        assert_eq!(text("a < b"), "a < b");
        assert_eq!(
            text(
                "<div><a href=\"#\" data-vss-mention=\"version:2.0,D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607\">@Dana Example</a> can you look? cc <a href=\"https://x\">docs</a></div>"
            ),
            "@<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> can you look? cc docs"
        );
        assert_eq!(
            text(
                "<ol start=\"2\"><li>a<ul><li>b</li></ul></li></ol><table><tr><td>x</td><td>y</td></tr></table>"
            ),
            "2. a\n  - b\nx | y"
        );
    }
}
