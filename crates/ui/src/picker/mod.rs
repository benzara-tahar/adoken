//! Anchored pickers for editing a work item (spec 0015; design
//! `docs/design/work-item-detail.md` §3.3, §4.5, §6.2, §7.3):
//!
//! - [`StateMenu`]: `Role::Menu` of reachable states, grouped by category
//!   with its tint, type-to-filter (FR-11).
//! - [`PeoplePicker`]: `Role::ListBox` of people with fixed rows, Recent,
//!   store matches and an async directory group the host fills (FR-13), or
//!   the composer's @mention list (FR-26).
//! - [`TreePicker`]: `Role::Tree` over area or iteration paths with filter by
//!   any path segment keeping ancestors visible, and the iteration
//!   Current / Next / Recent shortcuts (FR-14).
//! - [`LinkTypeMenu`]: the relation menu of "Add link" (FR-21).
//!
//! Each picker is an entity that remembers the focus it was opened from,
//! paints itself on a deferred, anchored layer at `z.dropdown`, and hands the
//! keyboard back when it closes (`Esc`, a click outside, or a choice; 0004
//! FR-18). It emits its `…Event::Chosen` and then gpui's `DismissEvent`.
//! The pure models (`*Model`) hold the filtering and selection rules and are
//! unit tested without a window; hosts keep pickers in a [`PickerSlot`].

mod link_menu;
mod people;
mod state_menu;
mod tree;

use std::ops::Range;

use adoken_theme::{elevation, motion, radius, size, space, typography, z};
use gpui::{
    Anchor, AnimationExt, AnyElement, App, Bounds, Context, DismissEvent, Div, Entity,
    EventEmitter, FocusHandle, FontWeight, HighlightStyle, KeyBinding, Pixels, Point, Render,
    SharedString, StyledText, Subscription, Window, actions, anchored, deferred, div, point,
    prelude::*, px,
};

use crate::kbd::Kbd;
use crate::motion::{EASE_OUT, animation, lerp};
use crate::theme::{Theme, TypeStyleExt};

pub use link_menu::{LinkType, LinkTypeEvent, LinkTypeMenu, LinkTypeModel};
pub use people::{
    AvatarFn, Directory, PeopleChoice, PeopleEvent, PeopleMode, PeopleModel, PeoplePicker,
    PeopleRow, Person, Section, wants_directory,
};
pub use state_menu::{StateCategory, StateMenu, StateMenuEvent, StateMenuModel, StateOption};
pub use tree::{
    DayRange, Shortcut, TreeEvent, TreeKind, TreeModel, TreeNode, TreePicker, TreeRow,
    format_day_range, nodes_from_paths,
};

actions!(
    picker,
    [
        /// Picker: next row (wraps).
        SelectNext,
        /// Picker: previous row (wraps).
        SelectPrevious,
        /// Picker: choose the highlighted row.
        Choose,
        /// Picker: clear the filter, or close.
        Close,
    ]
);

pub const KEY_CONTEXT: &str = "Picker";

/// Register once at startup, after `text_input::bindings()`.
pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("down", SelectNext, cx),
        KeyBinding::new("up", SelectPrevious, cx),
        KeyBinding::new("enter", Choose, cx),
        KeyBinding::new("escape", Close, cx),
    ]
}

/// Visible rows before the list scrolls: 8.5, the half row hints at more
/// (design §4.5, palette convention).
pub const VISIBLE_ROWS: f32 = 8.5;
/// The @mention popover shows at most this many rows (design §7.3).
pub const MENTION_ROWS: f32 = 6.0;
/// "Loading…" waits this long before it shows, then stays at least
/// [`LOADING_MIN_SHOWN`] (principles.md §3.2; design §3.3, §4.5).
pub const LOADING_DELAY: u32 = motion::HOVER_INTENT;
pub const LOADING_MIN_SHOWN: u32 = motion::HOVER_GRACE;

/// Where a picker paints, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub point: Point<Pixels>,
    /// The picker corner placed at `point`.
    pub corner: Anchor,
    pub width: f32,
    /// `menu-in` on open; keyboard-opened pickers appear at once (design §11).
    pub animate: bool,
    /// Flip to the other side of the anchor when there's no room (anchored
    /// pickers); otherwise shift inside the window (pointer menus).
    pub flip: bool,
}

impl Placement {
    /// Under `anchor` (a property cell or chip), left edges aligned, as wide
    /// as the anchor and at least `min_width`; above it when there's no room.
    pub fn below(anchor: Bounds<Pixels>, min_width: f32) -> Self {
        Self {
            point: anchor.bottom_left(),
            corner: Anchor::TopLeft,
            width: f32::from(anchor.size.width).max(min_width),
            animate: true,
            flip: true,
        }
    }

    /// Above `anchor` (the composer's caret line), below it when there's no
    /// room (design §7.3).
    pub fn above(anchor: Bounds<Pixels>, width: f32) -> Self {
        Self {
            point: anchor.origin,
            corner: Anchor::BottomLeft,
            width,
            animate: true,
            flip: true,
        }
    }

    /// `corner` at `point`, shifted inside the window.
    pub fn at(point: Point<Pixels>, corner: Anchor, width: f32) -> Self {
        Self {
            point,
            corner,
            width,
            animate: true,
            flip: false,
        }
    }

    /// Opened by a key: no `menu-in` (design §11).
    pub fn keyboard(mut self) -> Self {
        self.animate = false;
        self
    }
}

impl Default for Placement {
    fn default() -> Self {
        Self::at(
            point(px(0.0), px(0.0)),
            Anchor::TopLeft,
            size::ACTION_MENU_WIDTH,
        )
    }
}

/// A view's open picker, if any. The host renders [`PickerSlot::element`]
/// anywhere in its tree; the slot empties itself when the picker closes.
pub struct PickerSlot<T> {
    picker: Option<(Entity<T>, Subscription)>,
}

impl<T> Default for PickerSlot<T> {
    fn default() -> Self {
        Self { picker: None }
    }
}

impl<T: Render + EventEmitter<DismissEvent>> PickerSlot<T> {
    /// Holds `picker` (replacing any open one) until it dismisses itself.
    /// `slot` finds this slot inside the host view.
    pub fn open<V: 'static>(
        &mut self,
        picker: Entity<T>,
        slot: fn(&mut V) -> &mut PickerSlot<T>,
        cx: &mut Context<V>,
    ) {
        let closed = cx.subscribe(&picker, move |view, _, _: &DismissEvent, cx| {
            slot(view).picker = None;
            cx.notify();
        });
        self.picker = Some((picker, closed));
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.picker.is_some()
    }

    pub fn picker(&self) -> Option<&Entity<T>> {
        self.picker.as_ref().map(|(picker, _)| picker)
    }

    pub fn element(&self) -> Option<AnyElement> {
        self.picker()
            .map(|picker| picker.clone().into_any_element())
    }

    /// Drops the picker without handing focus back (the host moves focus
    /// itself, e.g. when the view closes).
    pub fn close(&mut self) {
        self.picker = None;
    }
}

/// The focus a picker was opened from; handed back on close.
pub(crate) struct ReturnFocus(Option<FocusHandle>);

impl ReturnFocus {
    pub(crate) fn capture(window: &Window, cx: &App) -> Self {
        Self(window.focused(cx))
    }

    pub(crate) fn restore(&mut self, window: &mut Window, cx: &mut App) {
        if let Some(focus) = self.0.take() {
            window.focus(&focus, cx);
        }
    }
}

/// Case-insensitive substring search by chars; the byte range of the first
/// match in `hay`. Empty `needle` → `None`.
pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<Range<usize>> {
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return None;
    }
    let hay_chars: Vec<(usize, char)> = hay.char_indices().collect();
    'start: for start in 0..hay_chars.len() {
        let mut matched = 0;
        let mut ix = start;
        while matched < needle.len() {
            let Some(&(_, c)) = hay_chars.get(ix) else {
                continue 'start;
            };
            for lower in c.to_lowercase() {
                if needle.get(matched) != Some(&lower) {
                    continue 'start;
                }
                matched += 1;
            }
            ix += 1;
        }
        let begin = hay_chars[start].0;
        let end = hay_chars.get(ix).map_or(hay.len(), |&(b, _)| b);
        return Some(begin..end);
    }
    None
}

/// `text` with `ranges` in `text.match` 560 (design §3.3, §4.5).
pub(crate) fn highlighted(
    text: SharedString,
    ranges: Vec<Range<usize>>,
    theme: &Theme,
) -> StyledText {
    let style = HighlightStyle {
        color: Some(theme.colors.text_match),
        font_weight: Some(FontWeight(typography::BODY_STRONG.weight)),
        ..HighlightStyle::default()
    };
    StyledText::new(text).with_highlights(ranges.into_iter().map(|r| (r, style)))
}

/// The popover card: `bg.overlay`, 1 px `border.default`, `radius.md`,
/// `elevation.2` (design §3.3, §4.5).
pub(crate) fn card(id: &'static str, width: f32, theme: &Theme) -> gpui::Stateful<Div> {
    let colors = &theme.colors;
    div()
        .id(id)
        .key_context(KEY_CONTEXT)
        .w(px(width))
        .p(px(space::S4))
        .flex()
        .flex_col()
        .rounded(px(radius::MD))
        .border_1()
        .border_color(colors.border_default)
        .bg(colors.bg_overlay)
        .shadow(theme.shadows(elevation::LEVEL_2))
        .text_color(colors.text_primary)
        .type_style(typography::BODY, theme)
        .occlude()
}

/// Paints `card` anchored at `placement` above the host, with `menu-in`
/// (opacity 0 → 1, Y −4 → 0) unless opened by keyboard; gpui snaps it under
/// reduce motion.
pub(crate) fn overlay(
    animation_id: &'static str,
    card: gpui::Stateful<Div>,
    placement: Placement,
) -> impl IntoElement {
    let card = if placement.animate {
        card.with_animation(
            animation_id,
            animation(motion::QUICK, EASE_OUT),
            |card, t| card.opacity(t).mt(px(lerp(-space::S4, 0.0, t))),
        )
        .into_any_element()
    } else {
        card.into_any_element()
    };
    let layer = anchored()
        .position(placement.point)
        .anchor(placement.corner);
    let layer = if placement.flip {
        layer
    } else {
        layer.snap_to_window_with_margin(px(space::S8))
    };
    deferred(layer.child(card)).with_priority(usize::from(z::DROPDOWN))
}

/// One selectable row's frame: 32 high, `bg.selected` when highlighted,
/// `bg.hover` under the pointer otherwise (hover never moves the selection).
pub(crate) fn row_frame(
    id: impl Into<gpui::ElementId>,
    highlighted: bool,
    theme: &Theme,
) -> gpui::Stateful<Div> {
    let colors = &theme.colors;
    div()
        .id(id)
        .w_full()
        .h(px(size::ROW_COMPACT))
        .flex_none()
        .px(px(space::S8))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .rounded(px(radius::XS))
        .when(highlighted, |d| {
            d.bg(colors.bg_selected).aria_active_descendant()
        })
        .when(!highlighted, |d| {
            d.hover(|s| s.bg(colors.bg_hover)).cursor_pointer()
        })
}

/// A group label row ("Recent", "From your directory"): `type.label`
/// `text.tertiary`, sitting on the bottom of a row-high slot.
pub(crate) fn group_label(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .w_full()
        .h(px(size::ROW_COMPACT))
        .flex_none()
        .px(px(space::S8))
        .pb(px(space::S4))
        .flex()
        .items_end()
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::LABEL, theme)
        .child(text.into())
}

/// A one-line message row ("Loading…", "No people match …").
pub(crate) fn message_row(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .w_full()
        .h(px(size::ROW_COMPACT))
        .flex_none()
        .px(px(space::S8))
        .flex()
        .items_center()
        .gap(px(space::S8))
        .text_color(theme.colors.text_tertiary)
        .child(text.into())
}

/// "↑↓ Move · ↩ Choose · Esc Close" under people and tree pickers.
pub(crate) fn footer_hint(theme: &Theme) -> Div {
    let hint = |keys: &[&str], text: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(space::S4))
            .children(keys.iter().filter_map(|k| Kbd::parse(k).ok()))
            .child(text)
    };
    div()
        .mt(px(space::S4))
        .pt(px(space::S4))
        .px(px(space::S8))
        .border_t_1()
        .border_color(theme.colors.border_subtle)
        .flex()
        .items_center()
        .gap(px(space::S12))
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::SMALL, theme)
        .child(hint(&["up", "down"], "Move"))
        .child(hint(&["enter"], "Choose"))
        .child(hint(&["escape"], "Close"))
}

/// The search field of people and tree pickers: 32 high on `bg.input`.
pub(crate) fn search_field(input: AnyElement, theme: &Theme) -> Div {
    let colors = &theme.colors;
    div()
        .h(px(size::ROW_COMPACT))
        .flex_none()
        .mb(px(space::S4))
        .flex()
        .items_center()
        .gap(px(space::S6))
        .px(px(space::S8))
        .rounded(px(radius::SM))
        .bg(colors.bg_input)
        .child(
            crate::icon::Icon::new(crate::icon::IconName::Search)
                .size(size::ICON_SM)
                .color(colors.text_tertiary),
        )
        .child(div().flex_1().min_w_0().child(input))
}

/// Height of a list showing `rows` rows, capped at `max` rows.
pub(crate) fn list_height(rows: usize, max: f32) -> Pixels {
    px((rows as f32).clamp(1.0, max) * size::ROW_COMPACT)
}

/// The loading row's timing (design §3.3, §4.5): nothing for
/// [`LOADING_DELAY`], then "Loading…", kept ≥ [`LOADING_MIN_SHOWN`] once
/// shown. Times are milliseconds on any monotonic clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadingGate {
    started: Option<u64>,
    shown: Option<u64>,
}

impl LoadingGate {
    /// Loading began at `now`.
    pub fn start(now: u64) -> Self {
        Self {
            started: Some(now),
            shown: None,
        }
    }

    /// When to look again (`None`: nothing pending).
    pub fn show_at(&self) -> Option<u64> {
        match (self.started, self.shown) {
            (Some(start), None) => Some(start + u64::from(LOADING_DELAY)),
            _ => None,
        }
    }

    /// Advances to `now`; true when the row is visible.
    pub fn tick(&mut self, now: u64) -> bool {
        if let (Some(at), None) = (self.show_at(), self.shown)
            && now >= at
        {
            self.shown = Some(now);
        }
        self.shown.is_some()
    }

    /// Data arrived at `now`: the moment it may replace the loading row
    /// (`now` unless the row is showing and hasn't been up long enough).
    pub fn ready_at(&self, now: u64) -> u64 {
        match self.shown {
            Some(shown) => now.max(shown + u64::from(LOADING_MIN_SHOWN)),
            None => now,
        }
    }

    pub fn is_shown(&self) -> bool {
        self.shown.is_some()
    }
}

/// Milliseconds since an arbitrary process epoch, for [`LoadingGate`].
pub(crate) fn now_ms() -> u64 {
    use std::sync::LazyLock;
    use std::time::Instant;
    static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);
    EPOCH.elapsed().as_millis() as u64
}

/// Runs `apply` on `entity` once `at` (from [`now_ms`]) has passed.
pub(crate) fn schedule<T: 'static>(
    at: u64,
    cx: &mut Context<T>,
    apply: impl FnOnce(&mut T, &mut Context<T>) + 'static,
) -> gpui::Task<()> {
    let wait = std::time::Duration::from_millis(at.saturating_sub(now_ms()));
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(wait).await;
        this.update(cx, |this, cx| {
            apply(this, cx);
            cx.notify();
        })
        .ok();
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_ci_matches_case_insensitively_and_returns_byte_ranges() {
        assert_eq!(find_ci("Code Review", "rev"), Some(5..8));
        assert_eq!(find_ci("Ärger", "är"), Some(0..3));
        assert_eq!(find_ci("Done", "x"), None);
        assert_eq!(find_ci("Done", ""), None);
        assert_eq!(find_ci("ab", "abc"), None);
    }

    #[test]
    fn loading_row_waits_then_stays_at_least_the_minimum() {
        let mut gate = LoadingGate::start(1_000);
        assert!(!gate.tick(1_100), "nothing for the first 150 ms");
        // Data before the row ever showed replaces nothing: apply now.
        assert_eq!(gate.ready_at(1_120), 1_120);
        assert!(gate.tick(1_150));
        // Shown at 1150: data at 1200 waits until 1450.
        assert_eq!(gate.ready_at(1_200), 1_450);
        assert_eq!(gate.ready_at(1_600), 1_600);
    }

    #[test]
    fn placement_below_takes_the_wider_of_cell_and_minimum() {
        let cell = Bounds::new(point(px(10.0), px(20.0)), gpui::size(px(200.0), px(52.0)));
        let p = Placement::below(cell, size::PICKER_POPOVER);
        assert_eq!(p.width, size::PICKER_POPOVER);
        assert_eq!(p.point, point(px(10.0), px(72.0)));
        let wide = Bounds::new(point(px(0.0), px(0.0)), gpui::size(px(400.0), px(52.0)));
        assert_eq!(Placement::below(wide, size::PICKER_POPOVER).width, 400.0);
        assert!(!p.keyboard().animate);
    }
}
