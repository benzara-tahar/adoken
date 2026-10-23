//! "Banner permission" row for Settings → Notifications: the macOS
//! authorization status plus the one action that can change it from here.
//! Spec 0005 §3 state "OS notifications not authorized": explain once,
//! inline; the inbox works regardless.

use adoken_theme::{radius, size, space, typography};
use adoken_ui::{ActiveTheme, Button, ButtonSize, ButtonVariant, Chip, ChipTone, TypeStyleExt};
use gpui::{
    App, AppContext as _, Context, Entity, Render, Subscription, Window, div, prelude::*, px,
};

use super::{
    Permission, PermissionState, open_system_settings, permission, refresh_permission,
    request_permission,
};

/// What the row's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowAction {
    /// Show the macOS prompt.
    Allow,
    /// Deep-link to System Settings → Notifications.
    OpenSystemSettings,
}

/// The row's copy for one status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionCopy {
    pub status: &'static str,
    pub tone: ChipTone,
    pub hint: &'static str,
    pub action: Option<(&'static str, RowAction)>,
}

pub const TITLE: &str = "Banner permission";

pub fn permission_copy(permission: Permission) -> PermissionCopy {
    match permission {
        Permission::Unavailable => PermissionCopy {
            status: "Unavailable",
            tone: ChipTone::Neutral,
            hint: "Banners need the adoken app bundle. This build shows in-app toasts only.",
            action: None,
        },
        Permission::Checking => PermissionCopy {
            status: "Checking…",
            tone: ChipTone::Neutral,
            hint: "Asking macOS whether adoken may show banners.",
            action: None,
        },
        Permission::NotDetermined => PermissionCopy {
            status: "Not asked",
            tone: ChipTone::Warning,
            hint: "macOS asks once. Until you allow banners, everything still lands in the inbox.",
            action: Some(("Allow banners…", RowAction::Allow)),
        },
        Permission::Denied => PermissionCopy {
            status: "Off",
            tone: ChipTone::Danger,
            hint: "Turned off in System Settings → Notifications → adoken. The inbox still gets everything.",
            action: Some(("Open System Settings", RowAction::OpenSystemSettings)),
        },
        Permission::Granted => PermissionCopy {
            status: "Allowed",
            tone: ChipTone::Success,
            hint: "Banners follow your delivery rules, quiet hours and macOS Focus.",
            action: None,
        },
    }
}

/// Embeddable status row; follows the live permission unless pinned.
pub struct PermissionRow {
    pinned: Option<Permission>,
    _observe: Option<Subscription>,
}

impl PermissionRow {
    /// Live row: re-reads the status (the user may have changed it in System
    /// Settings) and re-renders on every change.
    pub fn new(cx: &mut App) -> Entity<Self> {
        refresh_permission(cx);
        cx.new(|cx| Self {
            pinned: None,
            _observe: Some(cx.observe_global::<PermissionState>(|_, cx| cx.notify())),
        })
    }

    /// Gallery: a fixed status, no OS access (`cx.new(|_| pinned(..))`).
    pub fn pinned(permission: Permission) -> Self {
        Self {
            pinned: Some(permission),
            _observe: None,
        }
    }
}

impl Render for PermissionRow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.pinned.unwrap_or_else(|| permission(cx));
        let copy = permission_copy(status);
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let action = copy.action.map(|(label, action)| {
            Button::new("os-notify-permission-action", label)
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Md)
                .on_press(move |_, cx| match action {
                    RowAction::Allow => request_permission(cx),
                    RowAction::OpenSystemSettings => open_system_settings(cx),
                })
        });
        div()
            .id("os-notify-permission")
            .min_h(px(size::PALETTE_ROW_DEFAULT))
            .px(px(space::S8))
            .py(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S12))
            .rounded(px(radius::SM))
            .role(gpui::Role::Group)
            .aria_label(format!("{TITLE}: {}", copy.status))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::S8))
                            .child(
                                div()
                                    .text_color(colors.text_primary)
                                    .type_style(typography::BODY, &theme)
                                    .child(TITLE),
                            )
                            .child(Chip::new(copy.status).tone(copy.tone)),
                    )
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(copy.hint),
                    ),
            )
            .children(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_undecided_and_denied_offer_an_action() {
        let action = |p| permission_copy(p).action.map(|(_, a)| a);
        assert_eq!(action(Permission::NotDetermined), Some(RowAction::Allow));
        assert_eq!(
            action(Permission::Denied),
            Some(RowAction::OpenSystemSettings)
        );
        for p in [
            Permission::Unavailable,
            Permission::Checking,
            Permission::Granted,
        ] {
            assert_eq!(action(p), None, "{p:?}");
        }
    }

    #[test]
    fn denied_and_unavailable_keep_the_inbox_promise() {
        use adoken_notify::OsPermission;
        // Spec 0005 AC-13: no banners, inbox unchanged.
        assert_eq!(Permission::Denied.to_os(), OsPermission::Denied);
        assert_eq!(Permission::Unavailable.to_os(), OsPermission::Denied);
        assert_eq!(Permission::NotDetermined.to_os(), OsPermission::Unknown);
        assert_eq!(Permission::Granted.to_os(), OsPermission::Granted);
        assert!(permission_copy(Permission::Denied).hint.contains("inbox"));
    }
}
