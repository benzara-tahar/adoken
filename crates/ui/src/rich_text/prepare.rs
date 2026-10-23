//! [`Doc`] → render-ready blocks: one flat string per text block with
//! style runs and clickable ranges, mentions resolved to names. Built once
//! per document (and again only when a mention's name changes), not per
//! frame.

use std::ops::Range;

use adoken_richtext::{Block, Doc, Inline, LinkTarget, Marks};
use gpui::SharedString;

/// A mentioned identity as the caller knows it (design §7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionName {
    pub name: SharedString,
    /// Sign-in name for the tooltip ("dana@contoso.com").
    pub unique_name: Option<SharedString>,
    /// Mentions of me get the `accent.wash` shape.
    pub is_me: bool,
}

pub(crate) const UNKNOWN_USER: &str = "@Unknown user";
pub(crate) const UNKNOWN_USER_TOOLTIP: &str = "Not in adoken's people list yet";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MentionKind {
    Other,
    Me,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunStyle {
    pub marks: Marks,
    pub link: bool,
    pub mention: Option<MentionKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub range: Range<usize>,
    /// `None`: a mention (tooltip only, not a link).
    pub target: Option<LinkTarget>,
    pub tooltip: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PText {
    /// Unique within the document: element ids and hover state.
    pub ix: usize,
    pub text: SharedString,
    /// Styled ranges only, sorted, non-overlapping.
    pub runs: Vec<(Range<usize>, RunStyle)>,
    pub hits: Vec<Hit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Seg {
    Text(PText),
    Image {
        ix: usize,
        src: SharedString,
        alt: SharedString,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PBlock {
    Paragraph(Vec<Seg>),
    Heading(u8, Vec<Seg>),
    Code {
        ix: usize,
        text: SharedString,
    },
    List {
        start: Option<u64>,
        items: Vec<PItem>,
    },
    Quote(Vec<PBlock>),
    Rule,
    Table {
        ix: usize,
        header: bool,
        columns: usize,
        rows: Vec<Vec<Vec<Seg>>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PItem {
    pub task: Option<bool>,
    pub blocks: Vec<PBlock>,
}

pub(crate) fn prepare(doc: &Doc, names: &dyn Fn(&str) -> Option<MentionName>) -> Vec<PBlock> {
    let mut next = 0;
    blocks(&doc.blocks, names, &mut next)
}

fn blocks(
    blocks: &[Block],
    names: &dyn Fn(&str) -> Option<MentionName>,
    next: &mut usize,
) -> Vec<PBlock> {
    blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph(content) => PBlock::Paragraph(segments(content, names, next)),
            Block::Heading { level, content } => {
                PBlock::Heading(*level, segments(content, names, next))
            }
            Block::CodeBlock(text) => PBlock::Code {
                ix: take(next),
                text: text.clone().into(),
            },
            Block::List(list) => PBlock::List {
                start: list.start,
                items: list
                    .items
                    .iter()
                    .map(|item| PItem {
                        task: item.task,
                        blocks: self::blocks(&item.blocks, names, next),
                    })
                    .collect(),
            },
            Block::Quote(inner) => PBlock::Quote(self::blocks(inner, names, next)),
            Block::Rule => PBlock::Rule,
            Block::Table(table) => PBlock::Table {
                ix: take(next),
                header: table.header,
                columns: table.rows.first().map_or(0, Vec::len),
                rows: table
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|cell| segments(cell, names, next)).collect())
                    .collect(),
            },
        })
        .collect()
}

fn take(next: &mut usize) -> usize {
    *next += 1;
    *next - 1
}

/// Inline content split at images (chips are elements, not glyphs).
fn segments(
    content: &[Inline],
    names: &dyn Fn(&str) -> Option<MentionName>,
    next: &mut usize,
) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut run: Vec<&Inline> = Vec::new();
    for inline in content {
        if let Inline::Image { src, alt } = inline {
            if !run.is_empty() {
                out.push(Seg::Text(text(
                    &std::mem::take(&mut run),
                    names,
                    take(next),
                )));
            }
            out.push(Seg::Image {
                ix: take(next),
                src: src.clone().into(),
                alt: alt.clone().into(),
            });
        } else {
            run.push(inline);
        }
    }
    if !run.is_empty() {
        out.push(Seg::Text(text(&run, names, take(next))));
    }
    out
}

fn text(content: &[&Inline], names: &dyn Fn(&str) -> Option<MentionName>, ix: usize) -> PText {
    let mut text = String::new();
    let mut runs: Vec<(Range<usize>, RunStyle)> = Vec::new();
    let mut hits: Vec<Hit> = Vec::new();
    for inline in content {
        let start = text.len();
        match inline {
            Inline::Text(span) => {
                text.push_str(&span.text);
                let style = RunStyle {
                    marks: span.marks,
                    link: span.link.is_some(),
                    mention: None,
                };
                if style.marks != Marks::NONE || style.link {
                    runs.push((start..text.len(), style));
                }
                if let Some(link) = &span.link {
                    let range = start..text.len();
                    match hits.last_mut() {
                        // One hit per link even when its text has several runs.
                        Some(last)
                            if last.range.end == start
                                && last.target.as_ref() == Some(&link.target()) =>
                        {
                            last.range.end = range.end
                        }
                        _ => hits.push(Hit {
                            range,
                            target: Some(link.target()),
                            tooltip: link.url().map(|u| SharedString::from(u.to_owned())),
                        }),
                    }
                }
            }
            Inline::Break => text.push('\n'),
            Inline::Mention(id) => {
                let (kind, tooltip) = match names(id) {
                    Some(person) => {
                        text.push('@');
                        text.push_str(&person.name);
                        let kind = if person.is_me {
                            MentionKind::Me
                        } else {
                            MentionKind::Other
                        };
                        (kind, person.unique_name)
                    }
                    None => {
                        text.push_str(UNKNOWN_USER);
                        (MentionKind::Unknown, Some(UNKNOWN_USER_TOOLTIP.into()))
                    }
                };
                runs.push((
                    start..text.len(),
                    RunStyle {
                        marks: Marks::NONE,
                        link: false,
                        mention: Some(kind),
                    },
                ));
                hits.push(Hit {
                    range: start..text.len(),
                    target: None,
                    tooltip,
                });
            }
            Inline::Image { .. } => {}
        }
    }
    PText {
        ix,
        text: text.into(),
        runs,
        hits,
    }
}

/// Every mention's resolved name, to notice when people load later.
pub(crate) fn resolved_names(
    doc: &Doc,
    names: &dyn Fn(&str) -> Option<MentionName>,
) -> Vec<Option<MentionName>> {
    doc.mentions().into_iter().map(names).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_richtext::{Format, parse};

    const DANA: &str = "d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607";

    fn names(id: &str) -> Option<MentionName> {
        (id == DANA).then(|| MentionName {
            name: "Dana Kim".into(),
            unique_name: Some("dana@contoso.com".into()),
            is_me: true,
        })
    }

    #[test]
    fn mentions_show_names_and_unknown_people_never_show_ids() {
        let doc = parse(
            &format!("@<{DANA}> and @<6b3f0c2e-1d4a-4f5b-9c8d-7e6f5a4b3c2d> ok"),
            Format::Markdown,
        );
        let prepared = prepare(&doc, &names);
        let [PBlock::Paragraph(segs)] = prepared.as_slice() else {
            panic!()
        };
        let [Seg::Text(t)] = segs.as_slice() else {
            panic!()
        };
        assert_eq!(t.text.as_ref(), "@Dana Kim and @Unknown user ok");
        let kinds: Vec<_> = t
            .runs
            .iter()
            .map(|(r, s)| (&t.text[r.clone()], s.mention))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("@Dana Kim", Some(MentionKind::Me)),
                ("@Unknown user", Some(MentionKind::Unknown))
            ]
        );
        assert!(
            t.hits.iter().all(|h| h.target.is_none()),
            "mentions aren't links"
        );
        assert_eq!(t.hits[0].tooltip.as_deref(), Some("dana@contoso.com"));
    }

    #[test]
    fn a_link_split_by_emphasis_is_one_click_target_and_images_are_chips() {
        let doc = parse(
            "see [**the** spec](https://x.io/s) ![shot](https://x.io/a.png) and #12",
            Format::Markdown,
        );
        let prepared = prepare(&doc, &names);
        let [PBlock::Paragraph(segs)] = prepared.as_slice() else {
            panic!()
        };
        let [Seg::Text(before), Seg::Image { alt, .. }, Seg::Text(after)] = segs.as_slice() else {
            panic!("{segs:?}")
        };
        assert_eq!(alt.as_ref(), "shot");
        assert_eq!(before.hits.len(), 1);
        assert_eq!(&before.text[before.hits[0].range.clone()], "the spec");
        assert_eq!(before.hits[0].tooltip.as_deref(), Some("https://x.io/s"));
        assert_eq!(after.hits[0].target, Some(LinkTarget::WorkItem(12)));
        assert_eq!(after.hits[0].tooltip, None);
        // Indices are unique across segments.
        let Seg::Image { ix, .. } = &segs[1] else {
            panic!()
        };
        assert!(before.ix != *ix && after.ix != *ix && before.ix != after.ix);
    }
}
