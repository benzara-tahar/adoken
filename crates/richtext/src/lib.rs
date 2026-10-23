//! Rich text for work item descriptions and comments (spec 0015).
//!
//! - [`parse`]: Azure DevOps HTML or Markdown → one [`Doc`] model (FR-3).
//!   Pure and `Send`: run it on a background executor and cache the result
//!   per `(id, rev)` ([`DocCache`], FR-2). Never fetches anything.
//! - [`Doc::plain_text`]: the projection for search and previews.
//! - [`editable_html`]: FR-18's gate; [`to_markdown`] / [`markdown_to_html`]
//!   for the editor and the save path (FR-17).
//!
//! No IO and no gpui; the renderer is `adoken_ui::rich_text`.

mod cache;
mod gate;
mod html;
mod links;
mod markdown;
mod model;
mod text;
mod write_html;
mod write_md;

pub use cache::DocCache;
pub use gate::{BlockReason, Blocked, editable_html};
pub use html::mention_names;
pub use links::{LinkTarget, url_target};
pub use markdown::{mention_ranges, mention_token};
pub use model::{Block, Doc, Inline, Link, List, ListItem, Marks, Span, Table, is_guid};
pub use write_html::to_html;
pub use write_md::to_markdown;

/// A long-text field's or comment's source format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Format {
    #[default]
    Html,
    Markdown,
}

impl Format {
    /// Azure DevOps' `multilineFieldsFormat` value or a comment's `format`:
    /// `"markdown"` (any case) is Markdown, anything else (absent, `"html"`)
    /// is HTML.
    pub fn from_ado(value: Option<&str>) -> Self {
        match value {
            Some(v) if v.trim().eq_ignore_ascii_case("markdown") => Self::Markdown,
            _ => Self::Html,
        }
    }
}

/// The document to render: parsed, normalized, with bare URLs and `#123`
/// references linked.
pub fn parse(source: &str, format: Format) -> Doc {
    let mut doc = match format {
        Format::Html => html::parse(source).0,
        Format::Markdown => markdown::parse(source),
    };
    links::linkify(&mut doc);
    doc
}

/// HTML → document as written (no display linking).
pub fn parse_html(html: &str) -> Doc {
    html::parse(html).0
}

/// Markdown → document as written (no display linking).
pub fn parse_markdown(markdown: &str) -> Doc {
    markdown::parse(markdown)
}

/// Lossy HTML → Markdown, without FR-18's gate (use [`editable_html`] to
/// open the editor).
pub fn html_to_markdown(html: &str) -> String {
    to_markdown(&parse_html(html))
}

/// The save path for an HTML field edited as Markdown (FR-17).
pub fn markdown_to_html(markdown: &str, names: &dyn Fn(&str) -> Option<String>) -> String {
    to_html(&parse_markdown(markdown), names)
}
