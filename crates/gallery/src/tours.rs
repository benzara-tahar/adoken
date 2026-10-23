//! Guided tour stories (spec 0020 AC-12, docs/design/tours.md §8): a coach
//! mark over the real view's story, on one step, every gate ignored.

use adoken_app::tours::{TourId, Tours};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

/// `view` with `tour` showing its step `step` (0-based in the definition)
/// once the view's anchors paint.
fn over(
    key: &'static str,
    view: fn(&mut Window, &mut App) -> AnyElement,
    tour: TourId,
    step: usize,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let tours = window.use_keyed_state(key, cx, |window, cx| {
        let mut tours = Tours::new((0.0, 0.0), window, cx);
        tours.show_step(tour, step, cx);
        tours
    });
    div()
        .size_full()
        .child(view(window, cx))
        .child(tours)
        .into_any_element()
}

/// Step 2 (Files) over `review-open`.
pub fn review_files(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-review-files",
        crate::review::open_story,
        TourId::Review,
        1,
        window,
        cx,
    )
}

/// The last step: Next reads Done.
pub fn review_done(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-review-done",
        crate::review::open_story,
        TourId::Review,
        4,
        window,
        cx,
    )
}

/// Step 2 (Narrow it down) inside the palette.
pub fn palette(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-palette",
        crate::shell::palette_my_work_story,
        TourId::Palette,
        1,
        window,
        cx,
    )
}

/// Step 2 (Filter with one key) of the Inbox.
pub fn inbox_filters(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-inbox-filters",
        crate::inbox::inbox_story,
        TourId::Inbox,
        1,
        window,
        cx,
    )
}

/// Step 1 (State in one key) of the work item.
pub fn work_item_state(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-work-item-state",
        crate::workitems::detail_story,
        TourId::WorkItem,
        0,
        window,
        cx,
    )
}

/// Step 1 (Your day at a glance) of Today.
pub fn today_summary(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-today-summary",
        crate::today::today_busy_day_story,
        TourId::Today,
        0,
        window,
        cx,
    )
}

/// Step 2 of the review at the 900 × 600 minimum (flipped placement).
pub fn narrow(window: &mut Window, cx: &mut App) -> AnyElement {
    over(
        "tour-narrow",
        crate::review::open_story,
        TourId::Review,
        1,
        window,
        cx,
    )
}

/// Step 3 (The diff) at `font_scale` 1.3: inside-anchor placement.
pub fn font_scale(window: &mut Window, cx: &mut App) -> AnyElement {
    window.set_rem_size(px(16.0 * 1.3));
    over(
        "tour-font-scale",
        crate::review::open_story,
        TourId::Review,
        2,
        window,
        cx,
    )
}
