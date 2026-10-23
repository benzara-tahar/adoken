//! Settings → Notifications "Banner permission" row (spec 0005 §3), one per
//! macOS authorization state. Pinned states: stories never touch the OS.

use adoken_app::os_notify::{Permission, PermissionRow};
use adoken_theme::space;
use gpui::{AnyElement, App, Window, div, prelude::*, px};

const STATES: [(&str, Permission); 5] = [
    ("os-notify-not-asked", Permission::NotDetermined),
    ("os-notify-checking", Permission::Checking),
    ("os-notify-granted", Permission::Granted),
    ("os-notify-denied", Permission::Denied),
    ("os-notify-unavailable", Permission::Unavailable),
];

pub fn permission_row_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let rows = STATES.map(|(key, permission)| {
        window.use_keyed_state(key, cx, move |_, _| PermissionRow::pinned(permission))
    });
    div()
        .size_full()
        .p(px(space::S16))
        .flex()
        .flex_col()
        .gap(px(space::S4))
        .children(rows)
        .into_any_element()
}
