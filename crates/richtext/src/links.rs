//! Where a link goes (FR-3: work items and pull requests open in adoken,
//! everything else in the browser) and display-time linking of bare URLs
//! and `#123` references.

use crate::model::{Block, Doc, Inline, Link, Marks, Span};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LinkTarget {
    WorkItem(u32),
    PullRequest(u32),
    Web(String),
}

impl Link {
    pub fn target(&self) -> LinkTarget {
        match self {
            Link::WorkItem(id) => LinkTarget::WorkItem(*id),
            Link::Url(url) => url_target(url),
        }
    }

    /// The URL to show in a tooltip (`#123` has none of its own).
    pub fn url(&self) -> Option<&str> {
        match self {
            Link::Url(url) => Some(url),
            Link::WorkItem(_) => None,
        }
    }
}

/// Azure DevOps work item and pull request URLs, any host and casing:
/// `…/_workitems/edit/123`, `…/_apis/wit/workItems/123`,
/// `…/_git/{repo}/pullrequest/45`.
pub fn url_target(url: &str) -> LinkTarget {
    let lower = url.to_ascii_lowercase();
    let path = lower.split(['?', '#']).next().unwrap_or("");
    let id_after = |marker: &str| -> Option<u32> {
        let at = path.find(marker)?;
        let digits: String = path[at + marker.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let rest = &path[at + marker.len() + digits.len()..];
        (rest.is_empty() || rest.starts_with('/'))
            .then(|| digits.parse().ok())
            .flatten()
    };
    if lower.starts_with("http://") || lower.starts_with("https://") {
        if let Some(id) =
            id_after("/_workitems/edit/").or_else(|| id_after("/_apis/wit/workitems/"))
        {
            return LinkTarget::WorkItem(id);
        }
        if path.contains("/_git/")
            && let Some(id) = id_after("/pullrequest/")
        {
            return LinkTarget::PullRequest(id);
        }
    }
    LinkTarget::Web(url.to_owned())
}

/// Bare `http(s)://` URLs and `#123` references in unlinked, non-code text
/// become links. Display only: the editing round trip keeps text as text.
pub(crate) fn linkify(doc: &mut Doc) {
    fn walk(blocks: &mut [Block]) {
        for block in blocks {
            match block {
                Block::Paragraph(content) | Block::Heading { content, .. } => inlines(content),
                Block::List(list) => list.items.iter_mut().for_each(|i| walk(&mut i.blocks)),
                Block::Quote(inner) => walk(inner),
                Block::Table(table) => table.rows.iter_mut().flatten().for_each(inlines),
                Block::CodeBlock(_) | Block::Rule => {}
            }
        }
    }
    walk(&mut doc.blocks);
}

fn inlines(content: &mut Vec<Inline>) {
    let needs = content.iter().any(|i| {
        matches!(i, Inline::Text(s) if s.link.is_none()
            && !s.marks.contains(Marks::CODE)
            && (s.text.contains("://") || s.text.contains('#')))
    });
    if !needs {
        return;
    }
    let mut out = Vec::with_capacity(content.len() + 2);
    for inline in content.drain(..) {
        match inline {
            Inline::Text(span) if span.link.is_none() && !span.marks.contains(Marks::CODE) => {
                split(span, &mut out)
            }
            other => out.push(other),
        }
    }
    *content = out;
}

fn split(span: Span, out: &mut Vec<Inline>) {
    let text = span.text.as_str();
    let mut last = 0;
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        let boundary = i == 0 || !is_word(text[..i].chars().next_back().unwrap_or(' '));
        let found = if boundary
            && (text[i..].starts_with("http://") || text[i..].starts_with("https://"))
        {
            let len = url_len(&text[i..]);
            (len > "https://".len()).then(|| (len, Link::Url(text[i..i + len].to_owned())))
        } else if boundary && bytes[i] == b'#' {
            let digits = text[i + 1..].bytes().take_while(u8::is_ascii_digit).count();
            let after = text[i + 1 + digits..].chars().next();
            (digits > 0 && digits <= 9 && !after.is_some_and(is_word))
                .then(|| text[i + 1..i + 1 + digits].parse().ok())
                .flatten()
                .map(|id| (1 + digits, Link::WorkItem(id)))
        } else {
            None
        };
        match found {
            Some((len, link)) => {
                if last < i {
                    out.push(Inline::Text(Span {
                        text: text[last..i].to_owned(),
                        marks: span.marks,
                        link: None,
                    }));
                }
                out.push(Inline::Text(Span {
                    text: text[i..i + len].to_owned(),
                    marks: span.marks,
                    link: Some(link),
                }));
                i += len;
                last = i;
            }
            None => i += text[i..].chars().next().map_or(1, char::len_utf8),
        }
    }
    if last < text.len() {
        out.push(Inline::Text(Span {
            text: text[last..].to_owned(),
            marks: span.marks,
            link: None,
        }));
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '&' || c == '/'
}

/// A bare URL's length: up to whitespace or `<`, minus trailing
/// punctuation, keeping a `)` that closes a `(` inside the URL.
fn url_len(s: &str) -> usize {
    let mut end = s
        .find(|c: char| c.is_whitespace() || c == '<' || c == '"')
        .unwrap_or(s.len());
    loop {
        let Some(last) = s[..end].chars().next_back() else {
            return 0;
        };
        let trailing = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' | '~' => true,
            ')' => s[..end].matches('(').count() < s[..end].matches(')').count(),
            _ => false,
        };
        if !trailing {
            return end;
        }
        end -= last.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azure_devops_urls_open_in_adoken() {
        assert_eq!(
            url_target("https://dev.azure.com/org/Proj/_workitems/edit/4821/"),
            LinkTarget::WorkItem(4821)
        );
        assert_eq!(
            url_target("https://org.visualstudio.com/_apis/wit/workItems/12?x=1"),
            LinkTarget::WorkItem(12)
        );
        assert_eq!(
            url_target("https://dev.azure.com/org/Proj/_git/web/pullrequest/4295?_a=files"),
            LinkTarget::PullRequest(4295)
        );
        for web in [
            "https://example.com/_workitems/edit/12abc",
            "https://example.com/pullrequest/3",
            "mailto:a@b.c",
        ] {
            assert_eq!(url_target(web), LinkTarget::Web(web.into()), "{web}");
        }
    }

    #[test]
    fn bare_urls_and_work_item_refs_link_at_word_boundaries() {
        let mut out = Vec::new();
        split(
            Span::plain("see https://x.io/a_(b). and #12, not a#3 or #4x or &#5"),
            &mut out,
        );
        let links: Vec<_> = out
            .iter()
            .filter_map(|i| match i {
                Inline::Text(Span {
                    text,
                    link: Some(link),
                    ..
                }) => Some((text.as_str(), link.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            vec![
                ("https://x.io/a_(b)", Link::Url("https://x.io/a_(b)".into())),
                ("#12", Link::WorkItem(12)),
            ]
        );
    }
}
