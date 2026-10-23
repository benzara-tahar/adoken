//! Spec 0015 AC-1 (one model from HTML and Markdown), AC-14 (HTML
//! description round trip over a fixture corpus) and AC-15 (blocked
//! descriptions).

use std::path::PathBuf;

use adoken_richtext::{
    Block, BlockReason, Doc, Format, Inline, Link, LinkTarget, Marks, Span, editable_html,
    markdown_to_html, mention_names, parse, parse_html, parse_markdown, to_markdown,
};

const DANA: &str = "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607";

fn fixtures(dir: &str) -> Vec<(String, String)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(dir);
    let mut out: Vec<(String, String)> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .map(|entry| {
            let path = entry.expect("fixture entry").path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&path).expect("fixture text"))
        })
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no fixtures in {}", root.display());
    out
}

// ── AC-1 ───────────────────────────────────────────────────────────────

const AC1_HTML: &str = "<div style=\"box-sizing:border-box;\"><h2>Checkout totals</h2>\
<ul><li>Cart<ul><li>Coupons</li></ul></li><li>Tax</li></ul>\
<p>See <a href=\"https://example.com/spec\">the spec</a> and ask \
<a href=\"#\" data-vss-mention=\"version:2.0,D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607\">@Dana Kim</a>.</p>\
<table><thead><tr><th>Region</th><th>Offset</th><th>Cutoff</th></tr></thead>\
<tbody><tr><td>AU</td><td>+10</td><td>00:05</td></tr><tr><td>NZ</td><td>+12</td><td>00:05</td></tr></tbody></table>\
<p><img src=\"https://dev.azure.com/contoso/_apis/wit/attachments/9f1c?fileName=shot.png\" alt=\"shot\"></p></div>";

const AC1_MARKDOWN: &str = "## Checkout totals

- Cart
  - Coupons
- Tax

See [the spec](https://example.com/spec) and ask @<D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607>.

| Region | Offset | Cutoff |
| --- | --- | --- |
| AU | +10 | 00:05 |
| NZ | +12 | 00:05 |

![shot](https://dev.azure.com/contoso/_apis/wit/attachments/9f1c?fileName=shot.png)
";

#[test]
fn ac1_html_and_markdown_render_to_the_same_model() {
    let from_html = parse(AC1_HTML, Format::Html);
    let from_md = parse(AC1_MARKDOWN, Format::from_ado(Some("Markdown")));
    assert_eq!(from_html, from_md);

    let [heading, list, para, table, image] = from_html.blocks.as_slice() else {
        panic!("five blocks: {from_html:#?}");
    };
    assert!(matches!(heading, Block::Heading { level: 2, .. }));

    let Block::List(list) = list else {
        panic!("{list:?}")
    };
    assert_eq!(list.items.len(), 2);
    let Some(Block::List(nested)) = list.items[0].blocks.get(1) else {
        panic!("second level under the first item: {list:?}");
    };
    assert_eq!(nested.items.len(), 1);

    let Block::Paragraph(content) = para else {
        panic!("{para:?}")
    };
    let link = content.iter().find_map(|i| match i {
        Inline::Text(Span {
            link: Some(link),
            text,
            ..
        }) => Some((text.as_str(), link.target())),
        _ => None,
    });
    assert_eq!(
        link,
        Some((
            "the spec",
            LinkTarget::Web("https://example.com/spec".into())
        ))
    );
    assert!(content.contains(&Inline::Mention(DANA.into())));
    assert_eq!(from_html.mentions(), vec![DANA]);

    let Block::Table(table) = table else {
        panic!("{table:?}")
    };
    assert!(table.header);
    assert_eq!(table.rows.len(), 3);
    assert!(table.rows.iter().all(|row| row.len() == 3));

    assert!(matches!(
        image,
        Block::Paragraph(content) if matches!(content.as_slice(), [Inline::Image { alt, .. }] if alt == "shot")
    ));
}

#[test]
fn ac1_every_fr3_construct_parses_the_same_from_both_sources() {
    let html = "<h1>Title</h1>\
<p><b>bold</b> <i>italic</i> <u>under</u> <s>strike</s> <code>code</code><br>next line</p>\
<pre><code>fn main() {\n    println!(\"hi\");\n}</code></pre>\
<ol start=\"2\"><li>two</li><li>three</li></ol>\
<ul><li><input type=\"checkbox\" disabled checked> done</li><li><input type=\"checkbox\" disabled> todo</li></ul>\
<blockquote><p>quoted</p></blockquote><hr>\
<p>Fixes #4821, see https://dev.azure.com/contoso/Storefront/_git/web/pullrequest/4295.</p>";
    let md = "# Title

**bold** *italic* <u>under</u> ~~strike~~ `code`\\
next line

```rust
fn main() {
    println!(\"hi\");
}
```

2. two
3. three

- [x] done
- [ ] todo

> quoted

---

Fixes #4821, see https://dev.azure.com/contoso/Storefront/_git/web/pullrequest/4295.
";
    let from_html = parse(html, Format::Html);
    assert_eq!(from_html, parse(md, Format::Markdown));

    let mut targets = Vec::new();
    from_html.visit_inlines(&mut |i| {
        if let Inline::Text(Span {
            link: Some(link), ..
        }) = i
        {
            targets.push(link.target());
        }
    });
    assert_eq!(
        targets,
        vec![LinkTarget::WorkItem(4821), LinkTarget::PullRequest(4295)]
    );
    let marks: Vec<Marks> = match &from_html.blocks[1] {
        Block::Paragraph(content) => content
            .iter()
            .filter_map(|i| match i {
                Inline::Text(s) if !s.marks.is_empty() => Some(s.marks),
                _ => None,
            })
            .collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        marks,
        vec![
            Marks::BOLD,
            Marks::ITALIC,
            Marks::UNDERLINE,
            Marks::STRIKE,
            Marks::CODE
        ]
    );
}

// ── AC-14 ──────────────────────────────────────────────────────────────

#[test]
fn ac14_editable_fixtures_round_trip_and_accept_an_added_line() {
    for (name, html) in fixtures("editable") {
        let original = parse_html(&html);
        let md = editable_html(&html).unwrap_or_else(|b| panic!("{name}: blocked {b:?}"));
        assert_eq!(parse_markdown(&md), original, "{name}: markdown\n{md}");

        // The user adds a line and saves: the HTML sent renders to the same
        // document plus that line.
        let edited = format!("{md}\n\nAdded from adoken");
        let names = mention_names(&html);
        let saved = markdown_to_html(&edited, &|id| {
            names.iter().find(|(k, _)| k == id).map(|(_, n)| n.clone())
        });
        let mut expected = original.clone();
        expected
            .blocks
            .push(Block::Paragraph(vec![Inline::Text(Span::plain(
                "Added from adoken",
            ))]));
        assert_eq!(parse_html(&saved), expected, "{name}: saved\n{saved}");
    }
}

#[test]
fn ac14_live_sample_opens_as_markdown_text() {
    let html = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/editable/live-item-14.html"),
    )
    .unwrap();
    assert_eq!(
        editable_html(&html).unwrap(),
        "The total order amount shown during checkout does not match the actual sum of the item prices in the cart.\n\n\u{1F536}\n\nworkItemId"
    );
}

#[test]
fn ac14_saved_mentions_keep_their_names_and_ids() {
    let html = "<div><a href=\"#\" data-vss-mention=\"version:2.0,D2C4C61B-5A1A-6CF0-8A11-F7DD5385B607\">@Dana Kim</a> ok</div>";
    let md = editable_html(html).unwrap();
    assert_eq!(md, format!("@<{DANA}> ok"));
    let names = mention_names(html);
    let saved = markdown_to_html(&md, &|id| {
        names.iter().find(|(k, _)| k == id).map(|(_, n)| n.clone())
    });
    assert_eq!(
        saved,
        format!("<p><a href=\"#\" data-vss-mention=\"version:2.0,{DANA}\">@Dana Kim</a> ok</p>")
    );
}

#[test]
fn ac14_escaped_text_never_turns_into_markdown_structure() {
    let html = fixtures("editable")
        .into_iter()
        .find(|(name, _)| name == "markdown-lookalikes.html")
        .expect("fixture")
        .1;
    let doc = parse_html(&html);
    let md = to_markdown(&doc);
    // Only paragraphs of plain text, before and after.
    let reparsed = parse_markdown(&md);
    assert!(
        reparsed.blocks.iter().all(|b| matches!(
            b,
            Block::Paragraph(content) if content.iter().all(|i| matches!(i, Inline::Text(s) if s.marks.is_empty() && s.link.is_none()))
        )),
        "{md}\n{reparsed:#?}"
    );
    assert_eq!(reparsed, doc);
}

// ── AC-15 ──────────────────────────────────────────────────────────────

#[test]
fn ac15_images_tables_and_colors_block_the_editor() {
    let blocked = |html: &str| editable_html(html).expect_err(html).reasons;
    assert_eq!(
        blocked("<div>Before<img src=\"https://x/a.png\"></div>"),
        vec![BlockReason::Images(1)]
    );
    assert_eq!(
        blocked("<table><tr><td>a</td></tr></table>"),
        vec![BlockReason::Tables(1)]
    );
    assert_eq!(
        blocked("<div><span style=\"color:red\">alert</span></div>"),
        vec![BlockReason::TextColors]
    );
}

#[test]
fn ac15_blocked_fixtures_name_what_was_found() {
    let expected = [
        ("attachment-link.html", "an attachment"),
        (
            "highlight-and-font.html",
            "highlights and custom formatting",
        ),
        ("indent-and-align.html", "custom formatting"),
        ("table-and-images.html", "a table and 2 images"),
        ("word-paste.html", "text colors and custom formatting"),
    ];
    let found = fixtures("blocked");
    assert_eq!(found.len(), expected.len());
    for ((name, html), (want_name, want)) in found.iter().zip(expected) {
        assert_eq!(name, want_name);
        let blocked = editable_html(html).expect_err(name);
        assert_eq!(blocked.describe(), want, "{name}: {blocked:?}");
        // Blocked descriptions still render (FR-3).
        assert!(!parse(html, Format::Html).is_empty(), "{name}");
    }
}

#[test]
fn markdown_sources_are_never_converted() {
    // FR-17: Markdown fields are edited as written; the gate is HTML-only.
    // The renderer still reads them, including HTML blocks inside.
    let md = "Intro\n\n<div>html <b>block</b></div>\n\n| a |\n| - |\n| 1 |";
    let doc: Doc = parse(md, Format::Markdown);
    assert!(matches!(doc.blocks.last(), Some(Block::Table(_))));
    assert!(doc.blocks.iter().any(|b| matches!(
        b,
        Block::Paragraph(c) if c.iter().any(|i| matches!(i, Inline::Text(s) if s.marks == Marks::BOLD && s.text == "block"))
    )));
    assert!(matches!(
        &doc.blocks[0],
        Block::Paragraph(c) if c == &vec![Inline::Text(Span { text: "Intro".into(), marks: Marks::NONE, link: None })]
    ));
    let _: Option<Link> = None;
}

#[test]
fn spaces_between_runs_keep_only_the_emphasis_both_sides_share() {
    let html = parse_html("<p><b>bold <i>both</i> bold</b> <u>a</u> <u>b</u></p>");
    assert_eq!(
        html,
        parse_markdown("**bold *both* bold** <u>a</u> <u>b</u>")
    );
    let Block::Paragraph(content) = &html.blocks[0] else {
        panic!()
    };
    let runs: Vec<(&str, Marks)> = content
        .iter()
        .map(|i| match i {
            Inline::Text(s) => (s.text.as_str(), s.marks),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        runs,
        vec![
            ("bold ", Marks::BOLD),
            ("both", Marks::BOLD | Marks::ITALIC),
            (" bold", Marks::BOLD),
            (" ", Marks::NONE),
            ("a", Marks::UNDERLINE),
            (" ", Marks::NONE),
            ("b", Marks::UNDERLINE),
        ]
    );
}
