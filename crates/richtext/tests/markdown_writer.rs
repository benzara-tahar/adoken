//! The Markdown writer is lossless for every FR-18-allowed document:
//! `parse_markdown(to_markdown(doc)) == doc` and the saved HTML parses
//! back to the same document, over generated documents full of
//! Markdown-looking text, nested emphasis, links, mentions and breaks.

use adoken_richtext::{
    Block, Doc, Inline, Link, List, ListItem, Marks, Span, parse_html, parse_markdown, to_html,
    to_markdown,
};

/// Deterministic xorshift.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const WORDS: &[&str] = &[
    "total",
    "cart",
    "1.",
    "2)",
    "#",
    "#12",
    "-",
    "+",
    "=",
    "*",
    "**",
    "_x_",
    "snake_case",
    "`tick`",
    "[a]",
    "(b)",
    "<b>",
    "&amp;",
    "&",
    "AT&T",
    "a|b",
    "~",
    "~~",
    "\\",
    "!",
    "![x]",
    "\"q\"",
    "é",
    "🔶",
    "x*y",
    "end.",
    "---",
    ">",
    "@<x>",
    "http://x.io/a_b",
    "a_",
    "_a",
];

fn text(rng: &mut Rng) -> String {
    (0..1 + rng.below(4))
        .map(|_| *rng.pick(WORDS))
        .collect::<Vec<_>>()
        .join(" ")
}

fn inlines(rng: &mut Rng) -> Vec<Inline> {
    let marks = [
        Marks::NONE,
        Marks::BOLD,
        Marks::ITALIC,
        Marks::BOLD | Marks::ITALIC,
        Marks::UNDERLINE,
        Marks::STRIKE,
        Marks::CODE,
        Marks::BOLD | Marks::STRIKE,
    ];
    let mut out = Vec::new();
    for _ in 0..1 + rng.below(6) {
        match rng.below(10) {
            0 => out.push(Inline::Break),
            1 => out.push(Inline::Mention(
                "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607".into(),
            )),
            _ => {
                let link = (rng.below(5) == 0).then(|| Link::Url("https://x.io/p?a=1".into()));
                out.push(Inline::Text(Span {
                    text: text(rng),
                    marks: *rng.pick(&marks),
                    link,
                }));
                if rng.below(2) == 0 {
                    out.push(Inline::Text(Span::plain(" ")));
                }
            }
        }
    }
    out
}

fn blocks(rng: &mut Rng, depth: usize) -> Vec<Block> {
    (0..1 + rng.below(4))
        .map(|_| match rng.below(if depth > 1 { 3 } else { 7 }) {
            0 | 1 => Block::Paragraph(inlines(rng)),
            2 => Block::Heading {
                level: 1 + rng.below(6) as u8,
                content: inlines(rng),
            },
            3 => Block::CodeBlock(format!("{}\n  {}", text(rng), text(rng))),
            4 => Block::List(List {
                start: [None, Some(1), Some(3)][rng.below(3)],
                items: (0..1 + rng.below(3))
                    .map(|_| ListItem {
                        task: None,
                        blocks: blocks(rng, depth + 1),
                    })
                    .collect(),
            }),
            5 => Block::Quote(blocks(rng, depth + 1)),
            _ => Block::Rule,
        })
        .collect()
}

#[test]
fn generated_documents_round_trip_through_markdown_and_html() {
    let names = |_: &str| Some("Dana Kim".to_owned());
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut failures = Vec::new();
    const CASES: usize = 1000;
    for case in 0..CASES {
        let doc = Doc::from_blocks(blocks(&mut rng, 0));
        let md = to_markdown(&doc);
        let back = parse_markdown(&md);
        if back != doc {
            // Report the first top-level block that differs, on its own.
            let at = (0..doc.blocks.len())
                .find(|&i| back.blocks.get(i) != Some(&doc.blocks[i]))
                .unwrap_or(0);
            let alone = Doc {
                blocks: vec![doc.blocks[at].clone()],
            };
            failures.push(format!(
                "case {case} block {at} markdown:\n{}\n--- expected\n{:?}\n--- got\n{:?}",
                to_markdown(&alone),
                doc.blocks[at],
                back.blocks.get(at)
            ));
            continue;
        }
        let html = to_html(&doc, &names);
        if parse_html(&html) != doc {
            failures.push(format!("case {case} html:\n{html}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {CASES} differ; first:\n{}",
        failures.len(),
        failures[0]
    );
}
