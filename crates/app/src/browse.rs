//! Building blocks shared by the read-only list + detail views (work items,
//! pipelines; spec 0008): pane sizes, the full-width detail toggle,
//! state-group headers, detail sections and fields, the first-load skeleton
//! timing (principles §3.2), staleness copy (§3.3), and store-commit reloads.

use std::time::{Duration, Instant};

use adoken_model::Millis;
use adoken_model::time::{HOUR, MINUTE};
use adoken_store::Store;
use adoken_theme::{radius, space, typography};
use adoken_ui::{Skeleton, Theme, TypeStyleExt};
use gpui::{
    App, Context, IntoElement, KeyBinding, SharedString, Window, actions, div, prelude::*, px,
};

use crate::palette::PaletteCommand;

actions!(
    browse,
    [
        /// Hide or show the list next to the detail (work items, pipelines).
        ToggleFullWidth,
    ]
);

/// principles §2: `Cmd+Shift+Enter` toggles the detail pane full-width.
pub const FULL_WIDTH_KEYS: &str = "secondary-shift-enter";

pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new(FULL_WIDTH_KEYS, ToggleFullWidth, None)]
}

pub fn palette_commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: "Toggle full-width detail",
        keys: Some(FULL_WIDTH_KEYS),
        action: Box::new(ToggleFullWidth),
    }]
}

/// `size.list_pane` (principles §2).
pub const LIST_WIDTH: f32 = 380.0;
/// `size.row2.compact` (tokens.md): title + meta line.
pub const ROW_HEIGHT: f32 = 48.0;
/// Label column in detail field grids.
const FIELD_LABEL_WIDTH: f32 = 120.0;
/// Show loading indicators only after this long …
pub const LOADING_GRACE: Duration = Duration::from_millis(150);
/// … and keep them at least this long once shown (no flash).
pub const LOADING_MIN: Duration = Duration::from_millis(300);
/// Commits arrive in bursts during sync; reload once they settle.
pub const COMMIT_DEBOUNCE: Duration = Duration::from_millis(400);
/// Data older than this shows a staleness label (principles §3.3).
const STALE_AFTER: Millis = 15 * MINUTE;

pub fn now_ms() -> Millis {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as Millis)
}

/// How long to hold a skeleton shown at `shown_at` before swapping in data
/// (`None` = swap now).
pub fn hold(shown_at: Option<Instant>, now: Instant) -> Option<Duration> {
    let shown = shown_at?;
    LOADING_MIN
        .checked_sub(now.saturating_duration_since(shown))
        .filter(|d| !d.is_zero())
}

/// "Updated 18m ago" once the last sync is older than 15 minutes; `None`
/// when fresh or never synced (a seeded demo store has cursors).
pub fn staleness(last_sync: Option<Millis>, now: Millis) -> Option<String> {
    let age = now - last_sync?;
    if age <= STALE_AFTER {
        return None;
    }
    Some(if age < HOUR {
        format!("Updated {}m ago", age / MINUTE)
    } else if age < 48 * HOUR {
        format!("Updated {}h ago", age / HOUR)
    } else {
        format!("Updated {}d ago", age / (24 * HOUR))
    })
}

/// A state-group header row, same height as item rows (uniform lists).
pub fn group_header(label: impl Into<SharedString>, count: usize, theme: &Theme) -> gpui::Div {
    let colors = &theme.colors;
    div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .px(px(space::S12))
        .pb(px(space::S6))
        .flex()
        .items_end()
        .gap(px(space::S8))
        .child(
            div()
                .text_color(colors.text_secondary)
                .type_style(typography::LABEL, theme)
                .child(label.into()),
        )
        .child(
            div()
                .text_color(colors.text_tertiary)
                .type_style(typography::LABEL, theme)
                .child(count.to_string()),
        )
        .child(
            div()
                .flex_1()
                .mb(px(space::S6))
                .h(px(1.0))
                .bg(colors.border_subtle),
        )
}

/// A titled detail section.
pub fn section(title: impl Into<SharedString>, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .pt(px(space::S16))
        .child(
            div()
                .text_color(theme.colors.text_secondary)
                .type_style(typography::LABEL, theme)
                .child(title.into()),
        )
}

/// One `label  value` line of a detail field grid.
pub fn field(label: &'static str, value: impl IntoElement, theme: &Theme) -> gpui::Div {
    div()
        .flex()
        .gap(px(space::S12))
        .child(
            div()
                .w(px(FIELD_LABEL_WIDTH))
                .flex_none()
                .text_color(theme.colors.text_tertiary)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(theme.colors.text_primary)
                .child(value),
        )
}

/// A person's field value: their avatar, then their name (spec 0012).
pub fn person(id: &str, name: String) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap(px(space::S6))
        .child(
            crate::avatar::Avatar::new(id.to_owned(), name.clone())
                .size(crate::avatar::AvatarSize::Sm),
        )
        .child(div().min_w_0().truncate().child(name))
}

/// Muted block for long plain text (descriptions, comments).
pub fn text_block(theme: &Theme) -> gpui::Div {
    div()
        .p(px(space::S12))
        .rounded(px(radius::MD))
        .bg(theme.colors.bg_raised)
        .text_color(theme.colors.text_secondary)
        .flex()
        .flex_col()
        .gap(px(space::S8))
}

/// First-load placeholder rows at the list's row height.
pub fn skeleton_rows(prefix: &'static str, rows: usize) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .px(px(space::S12))
        .children((0..rows).map(move |ix| {
            div()
                .h(px(ROW_HEIGHT))
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(space::S4))
                .child(Skeleton::new((prefix, ix * 2)).width(220.0 - (ix % 3) as f32 * 40.0))
                .child(
                    Skeleton::new((prefix, ix * 2 + 1))
                        .width(140.0)
                        .height(typography::SMALL.line_height),
                )
        }))
}

/// A `Button::on_press` handler that runs on the view.
pub fn press<V: 'static>(
    cx: &Context<V>,
    f: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let view = cx.entity().downgrade();
    move |window, cx| {
        view.update(cx, |this, cx| f(this, window, cx)).ok();
    }
}

/// Calls `reload` on the view after store commits settle
/// ([`COMMIT_DEBOUNCE`]) with the latest store generation, so a view can
/// skip its own writes. Ends when the view is dropped.
pub fn reload_on_commit<V: 'static>(
    store: &Store,
    cx: &mut Context<V>,
    reload: impl Fn(&mut V, u64, &mut Context<V>) + 'static,
) -> gpui::Task<()> {
    use futures::StreamExt as _;
    let commits = store.subscribe();
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<u64>();
    // `subscribe` hands out a blocking receiver; bridge it off the
    // executors. The thread ends at the first commit after the view drops.
    std::thread::Builder::new()
        .name("browse-commits".into())
        .spawn(move || {
            while let Ok(generation) = commits.recv() {
                if tx.unbounded_send(generation).is_err() {
                    break;
                }
            }
        })
        .ok();
    cx.spawn(async move |this, cx| {
        while let Some(mut latest) = rx.next().await {
            cx.background_executor().timer(COMMIT_DEBOUNCE).await;
            while let Ok(more) = rx.try_recv() {
                latest = more;
            }
            if this
                .update(cx, |this, cx| reload(this, latest, cx))
                .is_err()
            {
                return;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeletons_hold_for_the_minimum_once_shown() {
        let shown = Instant::now();
        assert_eq!(hold(None, shown), None, "never shown: swap at once");
        assert_eq!(
            hold(Some(shown), shown + Duration::from_millis(100)),
            Some(Duration::from_millis(200))
        );
        assert_eq!(hold(Some(shown), shown + LOADING_MIN), None);
        assert_eq!(hold(Some(shown), shown + Duration::from_secs(2)), None);
    }

    #[test]
    fn staleness_appears_after_fifteen_minutes() {
        let now = 100 * HOUR;
        assert_eq!(staleness(None, now), None);
        assert_eq!(staleness(Some(now - 15 * MINUTE), now), None);
        assert_eq!(
            staleness(Some(now - 18 * MINUTE), now).as_deref(),
            Some("Updated 18m ago")
        );
        assert_eq!(
            staleness(Some(now - 5 * HOUR), now).as_deref(),
            Some("Updated 5h ago")
        );
        assert_eq!(
            staleness(Some(now - 72 * HOUR), now).as_deref(),
            Some("Updated 3d ago")
        );
    }
}
