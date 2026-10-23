//! FR-18 "no silent loss": an HTML description opens in the Markdown
//! editor only when nothing outside FR-3-minus-tables-and-images is present
//! and HTML → Markdown → HTML yields the same document.

use crate::model::Doc;
use crate::{html, markdown, write_html, write_md};

/// Why an HTML description can't be edited in adoken.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockReason {
    Tables(usize),
    Images(usize),
    Attachments(usize),
    TextColors,
    Highlights,
    Fonts,
    /// Other visible styling or markup (indents, alignment, sub/superscript…).
    CustomFormatting,
    /// Allowed constructs that still don't survive the Markdown round trip.
    NotLossless,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocked {
    pub reasons: Vec<BlockReason>,
}

impl Blocked {
    /// Design §5.3's `{list}`: "a table and 2 images", "text colors and
    /// custom formatting".
    pub fn describe(&self) -> String {
        let count = |n: usize, one: &str, many: &str| {
            if n == 1 {
                one.to_owned()
            } else {
                format!("{n} {many}")
            }
        };
        let mut parts: Vec<String> = Vec::new();
        let mut custom = false;
        for reason in &self.reasons {
            match *reason {
                BlockReason::Tables(n) => parts.push(count(n, "a table", "tables")),
                BlockReason::Images(n) => parts.push(count(n, "an image", "images")),
                BlockReason::Attachments(n) => parts.push(count(n, "an attachment", "attachments")),
                BlockReason::TextColors => parts.push("text colors".into()),
                BlockReason::Highlights => parts.push("highlights".into()),
                BlockReason::Fonts | BlockReason::CustomFormatting | BlockReason::NotLossless => {
                    custom = true
                }
            }
        }
        if custom {
            parts.push("custom formatting".into());
        }
        match parts.as_slice() {
            [] => String::new(),
            [one] => one.clone(),
            [init @ .., last] => format!("{} and {last}", init.join(", ")),
        }
    }
}

/// The FR-18 gate. `Ok(markdown)` to edit, or why not.
pub fn editable_html(source: &str) -> Result<String, Blocked> {
    let (doc, features) = html::parse(source);
    let mut reasons = Vec::new();
    if features.tables > 0 {
        reasons.push(BlockReason::Tables(features.tables));
    }
    if features.images > 0 {
        reasons.push(BlockReason::Images(features.images));
    }
    if features.attachments > 0 {
        reasons.push(BlockReason::Attachments(features.attachments));
    }
    if features.colors {
        reasons.push(BlockReason::TextColors);
    }
    if features.highlights {
        reasons.push(BlockReason::Highlights);
    }
    if features.fonts {
        reasons.push(BlockReason::Fonts);
    }
    if features.custom {
        reasons.push(BlockReason::CustomFormatting);
    }
    if !reasons.is_empty() {
        return Err(Blocked { reasons });
    }
    let md = write_md::to_markdown(&doc);
    if !round_trips(&doc, &md, source) {
        return Err(Blocked {
            reasons: vec![BlockReason::NotLossless],
        });
    }
    Ok(md)
}

fn round_trips(doc: &Doc, md: &str, source: &str) -> bool {
    let from_md = markdown::parse(md);
    if from_md != *doc {
        return false;
    }
    let names = html::mention_names(source);
    let lookup = |id: &str| {
        names
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, name)| name.clone())
    };
    let saved = write_html::to_html(&from_md, &lookup);
    html::parse(&saved).0 == *doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_names_what_was_found_in_plain_words() {
        let blocked = |reasons: Vec<BlockReason>| Blocked { reasons }.describe();
        assert_eq!(
            blocked(vec![BlockReason::Tables(1), BlockReason::Images(2)]),
            "a table and 2 images"
        );
        assert_eq!(
            blocked(vec![
                BlockReason::TextColors,
                BlockReason::Fonts,
                BlockReason::CustomFormatting
            ]),
            "text colors and custom formatting"
        );
        assert_eq!(
            blocked(vec![
                BlockReason::Images(1),
                BlockReason::Attachments(3),
                BlockReason::Highlights
            ]),
            "an image, 3 attachments and highlights"
        );
    }
}
