//! Keyboard-selectable list row: leading glyph column, title with fuzzy-match
//! highlights, optional subtitle, trailing meta. Anatomy and selection
//! styling from docs/design/command-palette.md §2 and §5.1.

use std::ops::Range;

use adoken_theme::{radius, size, space, typography};
use gpui::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, HighlightStyle, IntoElement, MouseButton,
    MouseDownEvent, RenderOnce, SharedString, StyledText, Window, div, prelude::*, px,
};

use crate::theme::{ActiveTheme, TypeStyleExt};

/// Byte ranges in `text` covering the chars at `positions` (char indices, as
/// the search index reports them), merged into runs of adjacent chars.
/// Out-of-range and duplicate positions are ignored; order doesn't matter.
pub fn match_ranges(text: &str, positions: &[u32]) -> Vec<Range<usize>> {
    let mut wanted: Vec<u32> = positions.to_vec();
    wanted.sort_unstable();
    wanted.dedup();
    let mut ranges: Vec<Range<usize>> = Vec::new();
    let mut wanted = wanted.into_iter().peekable();
    for (char_ix, (byte_ix, ch)) in text.char_indices().enumerate() {
        let Some(&next) = wanted.peek() else { break };
        if next as usize != char_ix {
            continue;
        }
        wanted.next();
        let end = byte_ix + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == byte_ix => last.end = end,
            _ => ranges.push(byte_ix..end),
        }
    }
    ranges
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type MouseHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct ListRow {
    id: ElementId,
    height: f32,
    leading: Option<AnyElement>,
    title: SharedString,
    highlights: Vec<Range<usize>>,
    subtitle: Option<SharedString>,
    badges: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    selected: bool,
    on_click: Option<ClickHandler>,
    on_secondary_click: Option<MouseHandler>,
    /// Extra layers inside the row (bounds trackers, anchored menus).
    overlays: Vec<AnyElement>,
}

impl ListRow {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            height: size::PALETTE_ROW_DEFAULT,
            leading: None,
            title: title.into(),
            highlights: Vec::new(),
            subtitle: None,
            badges: Vec::new(),
            trailing: Vec::new(),
            selected: false,
            on_click: None,
            on_secondary_click: None,
            overlays: Vec::new(),
        }
    }

    /// Row height, one of the `size::PALETTE_ROW_*` density steps.
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// 16 px glyph, centered in a 24 px column.
    pub fn leading(mut self, leading: impl IntoElement) -> Self {
        self.leading = Some(leading.into_any_element());
        self
    }

    /// Byte ranges of the title to paint as matches (see [`match_ranges`]).
    pub fn highlights(mut self, highlights: Vec<Range<usize>>) -> Self {
        self.highlights = highlights;
        self
    }

    /// Second line; omitted when empty.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        let subtitle = subtitle.into();
        self.subtitle = (!subtitle.is_empty()).then_some(subtitle);
        self
    }

    /// Inline chip after the title (design §5.1: at most two).
    pub fn badge(mut self, badge: impl IntoElement) -> Self {
        self.badges.push(badge.into_any_element());
        self
    }

    pub fn trailing(mut self, item: impl IntoElement) -> Self {
        self.trailing.push(item.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Right-click (the pointer path to the row's action menu).
    pub fn on_secondary_click(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary_click = Some(Box::new(handler));
        self
    }

    /// An absolutely positioned layer over the row.
    pub fn overlay(mut self, element: impl IntoElement) -> Self {
        self.overlays.push(element.into_any_element());
        self
    }

    pub fn track_selected_bounds(self, slot: crate::action_menu::BoundsSlot) -> Self {
        if self.selected {
            self.overlay(crate::action_menu::track_bounds(slot))
        } else {
            self
        }
    }
}

impl RenderOnce for ListRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let match_style = HighlightStyle {
            color: Some(colors.text_match),
            font_weight: Some(FontWeight(typography::BODY_STRONG.weight)),
            ..HighlightStyle::default()
        };
        let title = StyledText::new(self.title)
            .with_highlights(self.highlights.into_iter().map(|r| (r, match_style)));

        let title = div()
            .truncate()
            .text_color(colors.text_primary)
            .type_style(typography::BODY_STRONG, theme)
            .child(title);
        let title = if self.badges.is_empty() {
            title.into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(space::S6))
                .min_w_0()
                .child(title.min_w_0())
                .children(self.badges)
                .into_any_element()
        };

        let body = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(title)
            .when_some(self.subtitle, |body, subtitle| {
                body.child(
                    div()
                        .truncate()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(subtitle),
                )
            });

        let row = div()
            .relative()
            .size_full()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .pr(px(space::S12))
            .rounded(px(radius::SM))
            .overflow_hidden()
            .when(self.selected, |row| {
                row.bg(colors.bg_selected).child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(space::S2))
                        .bg(colors.accent_solid),
                )
            })
            .child(
                div()
                    .flex_none()
                    .w(px(size::CONTROL_SM))
                    .ml(px(space::S4))
                    .flex()
                    .justify_center()
                    .children(self.leading),
            )
            .child(body)
            .when(!self.trailing.is_empty(), |row| {
                row.child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, theme)
                        .children(self.trailing),
                )
            });

        div()
            .id(self.id)
            .relative()
            .h(px(self.height))
            .w_full()
            .px(px(space::S6))
            .when_some(self.on_click, |el, handler| el.on_click(handler))
            .when_some(self.on_secondary_click, |el, handler| {
                el.on_mouse_down(MouseButton::Right, handler)
            })
            .child(row)
            .children(self.overlays)
    }
}

#[cfg(test)]
mod tests {
    use super::match_ranges;

    #[test]
    fn adjacent_positions_merge_into_one_run() {
        assert_eq!(match_ranges("retry idem", &[0, 1, 2, 6, 7]), [0..3, 6..8]);
    }

    #[test]
    fn char_positions_map_to_utf8_byte_ranges() {
        // "é" is 2 bytes, "日" is 3: char 1 is bytes 1..3, char 3 is bytes 4..7.
        assert_eq!(match_ranges("aéb日c", &[1, 3]), [1..3, 4..7]);
        assert_eq!(match_ranges("aéb日c", &[1, 2, 3]), vec![1..7]);
    }

    #[test]
    fn unsorted_duplicate_and_out_of_range_positions_are_tolerated() {
        assert_eq!(match_ranges("abc", &[2, 0, 2, 99]), [0..1, 2..3]);
        assert!(match_ranges("", &[0]).is_empty());
    }
}
