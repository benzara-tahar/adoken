//! OS delivery for the notification center (spec 0005 FR-7, flow step 6):
//! macOS banners through `UNUserNotificationCenter`, grouped per subject by
//! thread id, click → activate adoken and route to the subject; the dock
//! badge from notify's `badge()` count; and the authorization status row for
//! Settings → Notifications.
//!
//! Everything is lazy: nothing touches the notification center before the
//! first banner, badge, permission query or click registration, so cold
//! start pays nothing. Outside an app bundle (plain `cargo run`) banners and
//! permission are a silent no-op ([`Permission::Unavailable`]); the dock
//! badge still works.
//!
//! Permission is requested only when the user turns banners on in Settings
//! ([`request_permission`]) or when the first banner is posted. Posts made
//! while the answer is pending are queued and flushed on "Allow".
//!
//! Don't mix with gpui's `App::show_system_notification`: it installs its
//! own center delegate, which would replace this one.

pub mod mapping;
mod row;

#[cfg(target_os = "macos")]
mod macos;

use adoken_notify::{Banner, BannerSink, OsPermission};
use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui::{App, Global, Task, actions};

use crate::palette::PaletteCommand;

pub use mapping::{Click, Route};
pub use row::{PermissionCopy, PermissionRow, RowAction, permission_copy};

actions!(
    os_notify,
    [
        /// Show the macOS banner prompt (if the user hasn't decided yet).
        AllowBanners,
        /// System Settings → Notifications.
        OpenNotificationSystemSettings,
    ]
);

/// App-wide handlers for [`commands`]; registers nothing with the OS.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &AllowBanners, cx| request_permission(cx));
    cx.on_action(|_: &OpenNotificationSystemSettings, cx| open_system_settings(cx));
}

/// Keyboard-first: both permission actions in the palette.
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Notifications: Allow banners",
            keys: None,
            action: Box::new(AllowBanners),
        },
        PaletteCommand {
            title: "Notifications: Open System Settings",
            keys: None,
            action: Box::new(OpenNotificationSystemSettings),
        },
    ]
}

/// Banners queued while authorization is pending; older ones are dropped
/// (they'd be stale by the time the user answers).
const MAX_PENDING: usize = 8;

/// macOS notification authorization as the app sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Permission {
    /// Not running from an app bundle (dev builds): no OS banners at all.
    Unavailable,
    /// Asked the system; answer not back yet.
    #[default]
    Checking,
    /// The user hasn't been asked yet.
    NotDetermined,
    Denied,
    /// Authorized, provisional or ephemeral.
    Granted,
}

impl Permission {
    /// notify's view (spec 0005 AC-13: denied → inbox only, no banners).
    pub fn to_os(self) -> OsPermission {
        match self {
            Permission::Granted => OsPermission::Granted,
            Permission::Denied | Permission::Unavailable => OsPermission::Denied,
            Permission::Checking | Permission::NotDetermined => OsPermission::Unknown,
        }
    }
}

/// From the framework's callbacks to the main thread.
enum Message {
    Permission(Permission),
    Click(Click),
}

type ClickHandler = Box<dyn FnMut(Click, &mut App)>;

/// Current permission, its own global so [`PermissionRow`]s observe only
/// real changes (not every banner or badge update).
#[derive(Clone, Copy, Default)]
struct PermissionState(Permission);

impl Global for PermissionState {}

#[derive(Default)]
struct OsNotify {
    started: bool,
    #[cfg(target_os = "macos")]
    center: Option<macos::Center>,
    /// A request for the prompt is in flight (or answered): ask once.
    asked: bool,
    pending: Vec<mapping::Request>,
    badge: Option<Option<String>>,
    on_click: Option<ClickHandler>,
    _messages: Option<Task<()>>,
}

impl Global for OsNotify {}

/// Posts a banner notify's delivery rules let through. Requests permission
/// on the first banner if the user hasn't been asked yet.
pub fn post(banner: &Banner, cx: &mut App) {
    start(cx);
    let request = mapping::request(banner);
    match permission(cx) {
        Permission::Unavailable | Permission::Denied => {}
        Permission::Granted => deliver(&request, cx),
        Permission::Checking | Permission::NotDetermined => {
            let state = cx.global_mut::<OsNotify>();
            if state.pending.len() == MAX_PENDING {
                state.pending.remove(0);
            }
            state.pending.push(request);
            if permission(cx) == Permission::NotDetermined {
                ask(cx);
            }
        }
    }
}

/// notify's `BannerSink` over the OS notification center.
pub struct OsBanners<'a>(pub &'a mut App);

impl BannerSink for OsBanners<'_> {
    fn post(&mut self, banner: &Banner) {
        post(banner, self.0);
    }
}

/// Dock badge = notify's `Engine::badge()` (unread P0+P1 by default). Zero
/// clears it.
pub fn set_badge(count: usize, cx: &mut App) {
    let label = mapping::badge_label(count);
    let state = cx.default_global::<OsNotify>();
    if state.badge.as_ref() == Some(&label) {
        return;
    }
    state.badge = Some(label.clone());
    #[cfg(target_os = "macos")]
    macos::set_badge(label.as_deref());
}

/// Routes banner clicks. adoken is activated first, then `handler` runs on
/// the main thread; it replaces any earlier handler. Registering also
/// installs the center delegate, which must happen early for clicks that
/// launched the app to arrive.
pub fn on_click(cx: &mut App, handler: impl FnMut(Click, &mut App) + 'static) {
    cx.default_global::<OsNotify>().on_click = Some(Box::new(handler));
    start(cx);
}

/// The cached authorization status; [`Permission::Checking`] until the first
/// query answers.
pub fn permission(cx: &App) -> Permission {
    cx.try_global::<PermissionState>()
        .map(|state| state.0)
        .unwrap_or_default()
}

/// Settings turned banners on: show the system prompt if the user hasn't
/// decided yet. No-op once decided or outside a bundle.
pub fn request_permission(cx: &mut App) {
    start(cx);
    if matches!(
        permission(cx),
        Permission::NotDetermined | Permission::Checking
    ) {
        ask(cx);
    }
}

/// Re-reads the status (the user may have changed it in System Settings).
pub fn refresh_permission(cx: &mut App) {
    if !cx.default_global::<OsNotify>().started {
        start(cx);
        return;
    }
    #[cfg(target_os = "macos")]
    if let Some(center) = &cx.global::<OsNotify>().center {
        center.query_permission();
    }
}

/// Opens System Settings → Notifications (for a denied permission).
pub fn open_system_settings(cx: &mut App) {
    cx.open_url("x-apple.systempreferences:com.apple.Notifications-Settings.extension");
}

/// First touch: connect the center (bundle only) and query the status.
fn start(cx: &mut App) {
    if cx.default_global::<OsNotify>().started {
        return;
    }
    let (sender, mut receiver) = mpsc::unbounded::<Message>();
    let task = cx.spawn(async move |cx| {
        while let Some(message) = receiver.next().await {
            cx.update(|cx| handle(message, cx));
        }
    });
    #[cfg(target_os = "macos")]
    let center = macos::Center::new(sender);
    #[cfg(not(target_os = "macos"))]
    drop(sender);
    let state = cx.global_mut::<OsNotify>();
    state.started = true;
    state._messages = Some(task);
    #[cfg(target_os = "macos")]
    {
        if let Some(center) = &center {
            center.query_permission();
        }
        let available = center.is_some();
        cx.global_mut::<OsNotify>().center = center;
        if !available {
            set_permission(Permission::Unavailable, cx);
        }
    }
    #[cfg(not(target_os = "macos"))]
    set_permission(Permission::Unavailable, cx);
}

fn ask(cx: &mut App) {
    let state = cx.global_mut::<OsNotify>();
    if std::mem::replace(&mut state.asked, true) {
        return;
    }
    #[cfg(target_os = "macos")]
    if let Some(center) = &state.center {
        center.request_permission();
    }
}

fn deliver(request: &mapping::Request, cx: &mut App) {
    #[cfg(target_os = "macos")]
    if let Some(center) = &cx.global::<OsNotify>().center {
        center.post(request);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (request, cx);
}

fn handle(message: Message, cx: &mut App) {
    match message {
        Message::Permission(permission) => {
            set_permission(permission, cx);
            match permission {
                Permission::Granted => {
                    let pending = std::mem::take(&mut cx.global_mut::<OsNotify>().pending);
                    for request in &pending {
                        deliver(request, cx);
                    }
                }
                // A banner arrived before the first status answer.
                Permission::NotDetermined if !cx.global::<OsNotify>().pending.is_empty() => ask(cx),
                Permission::NotDetermined | Permission::Checking => {}
                Permission::Denied | Permission::Unavailable => {
                    cx.global_mut::<OsNotify>().pending.clear();
                }
            }
        }
        Message::Click(click) => {
            cx.activate(true);
            let handler = cx.global_mut::<OsNotify>().on_click.take();
            if let Some(mut handler) = handler {
                handler(click, cx);
                // The handler may have registered a replacement.
                cx.global_mut::<OsNotify>().on_click.get_or_insert(handler);
            }
        }
    }
}

fn set_permission(permission: Permission, cx: &mut App) {
    if cx.try_global::<PermissionState>().map(|s| s.0) != Some(permission) {
        cx.set_global(PermissionState(permission));
    }
}
