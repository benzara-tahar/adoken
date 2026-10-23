//! Changed-files tree (spec 0004 §3 Files tab): directories first, single-
//! child directory chains compressed (`src/sync/`), files with change type,
//! `+/-` counts, comment count and a local "viewed" checkbox. Rows can
//! highlight the ranges a filter matched (spec 0019 FR-2, `text.match`).
//!
//! [`build`] is pure and fixes the tree order that `j`/`k` walk.

use std::collections::BTreeMap;
use std::ops::Range;

use adoken_theme::{radius, size, space, typography};
use gpui::{
    App, ElementId, FontWeight, HighlightStyle, IntoElement, RenderOnce, SharedString, StyledText,
    Window, div, prelude::*, px,
};

use crate::chip::Checkbox;
use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    Dir {
        /// One or more path segments joined by `/` (compressed chain).
        name: String,
        depth: usize,
    },
    File {
        /// Index into the `paths` given to [`build`].
        index: usize,
        name: String,
        depth: usize,
    },
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<(String, usize)>,
}

/// Tree rows for `paths` (leading `/` ignored, as ADO paths have one).
pub fn build<S: AsRef<str>>(paths: &[S]) -> Vec<TreeRow> {
    let mut root = Node::default();
    for (index, path) in paths.iter().enumerate() {
        let path = path.as_ref().trim_start_matches('/');
        let mut segments: Vec<&str> = path.split('/').collect();
        let file = segments.pop().unwrap_or_default().to_owned();
        let mut node = &mut root;
        for segment in segments {
            node = node.dirs.entry(segment.to_owned()).or_default();
        }
        node.files.push((file, index));
    }
    let mut rows = Vec::with_capacity(paths.len() * 2);
    flatten(&root, 0, &mut rows);
    rows
}

fn flatten(node: &Node, depth: usize, rows: &mut Vec<TreeRow>) {
    for (name, child) in &node.dirs {
        let mut name = name.clone();
        let mut child = child;
        // Compress `a/` → `b/` chains that hold nothing else.
        while child.files.is_empty() && child.dirs.len() == 1 {
            let (next_name, next) = child.dirs.iter().next().expect("one dir");
            name.push('/');
            name.push_str(next_name);
            child = next;
        }
        rows.push(TreeRow::Dir { name, depth });
        flatten(child, depth + 1, rows);
    }
    let mut files: Vec<_> = node.files.iter().collect();
    files.sort();
    rows.extend(files.into_iter().map(|(name, index)| TreeRow::File {
        index: *index,
        name: name.clone(),
        depth,
    }));
}

/// File indices in tree order (the `j`/`k` order).
pub fn file_order(rows: &[TreeRow]) -> Vec<usize> {
    rows.iter()
        .filter_map(|row| match row {
            TreeRow::File { index, .. } => Some(*index),
            TreeRow::Dir { .. } => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Add,
    Edit,
    Delete,
    Rename,
}

impl ChangeKind {
    pub fn letter(self) -> &'static str {
        match self {
            Self::Add => "A",
            Self::Edit => "M",
            Self::Delete => "D",
            Self::Rename => "R",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "Added",
            Self::Edit => "Modified",
            Self::Delete => "Deleted",
            Self::Rename => "Renamed",
        }
    }
}

fn indent(depth: usize) -> f32 {
    space::S12 + depth as f32 * space::S12
}

type Handler = Box<dyn Fn(&mut Window, &mut App)>;

/// `name` with `highlights` in `text.match` at the palette's match weight.
fn name_text(name: SharedString, highlights: Vec<Range<usize>>, cx: &App) -> StyledText {
    let theme = cx.theme();
    let style = HighlightStyle {
        color: Some(theme.colors.text_match),
        font_weight: Some(FontWeight(typography::BODY_STRONG.weight)),
        ..HighlightStyle::default()
    };
    StyledText::new(name).with_highlights(highlights.into_iter().map(|r| (r, style)))
}

/// A directory row.
#[derive(IntoElement)]
pub struct FileTreeDir {
    name: SharedString,
    depth: usize,
    highlights: Vec<Range<usize>>,
    /// Listed only for its descendants while filtering: `text.tertiary`.
    muted: bool,
}

impl FileTreeDir {
    pub fn new(name: impl Into<SharedString>, depth: usize) -> Self {
        Self {
            name: name.into(),
            depth,
            highlights: Vec::new(),
            muted: false,
        }
    }

    /// Byte ranges of the name a filter matched.
    pub fn highlights(mut self, ranges: Vec<Range<usize>>) -> Self {
        self.highlights = ranges;
        self
    }

    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }
}

impl RenderOnce for FileTreeDir {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = name_text(self.name, self.highlights, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        div()
            .flex()
            .items_center()
            .gap(px(space::S6))
            .h(px(size::CONTROL_MD))
            .pl(px(indent(self.depth)))
            .pr(px(space::S12))
            .text_color(if self.muted {
                colors.text_tertiary
            } else {
                colors.text_secondary
            })
            .type_style(typography::SMALL, theme)
            .child(
                Icon::new(IconName::Folder)
                    .size(size::ICON_SM)
                    .color(colors.text_tertiary),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(name),
            )
    }
}

/// A file row: `[viewed] A name   +12 −3  💬2`.
#[derive(IntoElement)]
pub struct FileTreeFile {
    id: ElementId,
    name: SharedString,
    depth: usize,
    change: ChangeKind,
    additions: Option<u32>,
    deletions: Option<u32>,
    comments: usize,
    viewed: bool,
    selected: bool,
    highlights: Vec<Range<usize>>,
    on_select: Option<Handler>,
    on_toggle_viewed: Option<Handler>,
}

impl FileTreeFile {
    pub fn new(
        id: impl Into<ElementId>,
        name: impl Into<SharedString>,
        depth: usize,
        change: ChangeKind,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            depth,
            change,
            additions: None,
            deletions: None,
            comments: 0,
            viewed: false,
            selected: false,
            highlights: Vec::new(),
            on_select: None,
            on_toggle_viewed: None,
        }
    }

    /// Byte ranges of the name a filter matched.
    pub fn highlights(mut self, ranges: Vec<Range<usize>>) -> Self {
        self.highlights = ranges;
        self
    }

    /// `+/-` counts; omitted until the diff is computed.
    pub fn counts(mut self, additions: u32, deletions: u32) -> Self {
        self.additions = Some(additions);
        self.deletions = Some(deletions);
        self
    }

    pub fn comments(mut self, comments: usize) -> Self {
        self.comments = comments;
        self
    }

    pub fn viewed(mut self, viewed: bool) -> Self {
        self.viewed = viewed;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Box::new(handler));
        self
    }

    pub fn on_toggle_viewed(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_toggle_viewed = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for FileTreeFile {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = name_text(self.name, self.highlights, cx);
        let theme = cx.theme();
        let colors = &theme.colors;
        let change_color = match self.change {
            ChangeKind::Add => colors.diff_add_gutter,
            ChangeKind::Delete => colors.diff_del_gutter,
            ChangeKind::Edit => colors.status_info_fg,
            ChangeKind::Rename => colors.status_warning_fg,
        };
        let name_color = if self.viewed {
            colors.text_tertiary
        } else {
            colors.text_primary
        };
        let checkbox_id =
            ElementId::NamedChild(std::sync::Arc::new(self.id.clone()), "viewed".into());
        let counts = self.additions.zip(self.deletions).map(|(add, del)| {
            div()
                .flex()
                .flex_none()
                .gap(px(space::S4))
                .type_style(typography::CODE_INLINE, theme)
                .child(
                    div()
                        .text_color(colors.diff_add_gutter)
                        .child(format!("+{add}")),
                )
                .child(
                    div()
                        .text_color(colors.diff_del_gutter)
                        .child(format!("−{del}")),
                )
        });
        let comments = (self.comments > 0).then(|| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(space::S2))
                .text_color(colors.text_secondary)
                .child(
                    Icon::new(IconName::MessageSquare)
                        .size(size::ICON_XS)
                        .color(colors.diff_comment),
                )
                .child(self.comments.to_string())
        });
        let hover = colors.bg_hover;
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(space::S6))
            .h(px(size::CONTROL_MD))
            .mx(px(space::S4))
            .pl(px(indent(self.depth) - space::S4))
            .pr(px(space::S8))
            .rounded(px(radius::SM))
            .when(self.selected, |this| this.bg(colors.bg_selected))
            .when(!self.selected, |this| this.hover(move |s| s.bg(hover)))
            .cursor_pointer()
            .type_style(typography::SMALL, theme)
            .when_some(self.on_select, |this, handler| {
                this.on_click(move |_, window, cx| handler(window, cx))
            })
            .child(
                div()
                    .id(checkbox_id)
                    .flex_none()
                    .when_some(self.on_toggle_viewed, |this, handler| {
                        this.on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            handler(window, cx)
                        })
                    })
                    .child(Checkbox::new(self.viewed)),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(size::ICON_SM))
                    .text_color(change_color)
                    .type_style(typography::LABEL, theme)
                    .child(self.change.letter()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_color(name_color)
                    .child(name),
            )
            .children(comments)
            .children(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn describe(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                TreeRow::Dir { name, depth } => format!("{}{name}/", "  ".repeat(*depth)),
                TreeRow::File { name, depth, index } => {
                    format!("{}{name}#{index}", "  ".repeat(*depth))
                }
            })
            .collect()
    }

    #[test]
    fn directories_first_sorted_and_chains_compressed() {
        let paths = [
            "/README.md",
            "/src/sync/governor.rs",
            "/src/sync/mod.rs",
            "/src/lib.rs",
            "/web/app/components/Retry.tsx",
            "/Cargo.toml",
        ];
        assert_eq!(
            describe(&build(&paths)),
            [
                "src/",
                "  sync/",
                "    governor.rs#1",
                "    mod.rs#2",
                "  lib.rs#3",
                "web/app/components/",
                "  Retry.tsx#4",
                "Cargo.toml#5",
                "README.md#0",
            ]
        );
        assert_eq!(file_order(&build(&paths)), [1, 2, 3, 4, 5, 0]);
    }

    #[test]
    fn empty_and_root_only() {
        assert!(build::<&str>(&[]).is_empty());
        assert_eq!(describe(&build(&["a.rs"])), ["a.rs#0"]);
    }
}
