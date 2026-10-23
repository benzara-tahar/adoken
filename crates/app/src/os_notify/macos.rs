//! The AppKit / UserNotifications bridge: `UNUserNotificationCenter` for
//! banners, `NSDockTile` for the badge. Main thread only; the framework's
//! completion handlers and delegate callbacks may run on any thread, so they
//! only send a [`Message`] back through a channel.
//!
//! Adapted from zui's `crates/gpui_macos/src/system_notifications.rs` (see
//! THIRD_PARTY_NOTICES.md): the bundle guard, the response delegate and the
//! always-present `willPresent` answer. adoken adds thread identifiers,
//! fixed action categories and authorization status reporting.

use block2::RcBlock;
use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, class, define_class, msg_send};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationDefaultActionIdentifier,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSettings, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use super::Message;
use super::Permission;
use super::mapping::{
    ACTION_OPEN, ACTION_SNOOZE, CATEGORY_OPEN, CATEGORY_OPEN_SNOOZE, Request, click,
};

/// `NSBundle.mainBundle.bundleIdentifier`: present only when the process was
/// launched from a `.app` (or its `Contents/MacOS` binary). Touching
/// `UNUserNotificationCenter` without one raises
/// `NSInternalInconsistencyException` and aborts the process.
pub fn bundled() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

pub struct Center {
    center: Retained<UNUserNotificationCenter>,
    sender: UnboundedSender<Message>,
    /// The center's `delegate` is weak; keep ours alive for the app's life.
    _delegate: Retained<Delegate>,
}

impl Center {
    /// `None` outside an app bundle (plain `cargo run`): banners no-op.
    pub fn new(sender: UnboundedSender<Message>) -> Option<Self> {
        if !bundled() {
            log::info!("adoken: OS banners off (not running from an app bundle)");
            return None;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate = Delegate::new(sender.clone());
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        center.setNotificationCategories(&categories());
        Some(Self {
            center,
            sender,
            _delegate: delegate,
        })
    }

    /// Asynchronous; answers with [`Message::Permission`].
    pub fn query_permission(&self) {
        let sender = self.sender.clone();
        let completion =
            RcBlock::new(move |settings: std::ptr::NonNull<UNNotificationSettings>| {
                // SAFETY: the framework passes a valid settings object that lives
                // for the duration of the callback.
                let status = unsafe { settings.as_ref() }.authorizationStatus();
                sender
                    .unbounded_send(Message::Permission(permission(status)))
                    .ok();
            });
        self.center
            .getNotificationSettingsWithCompletionHandler(&completion);
    }

    /// Shows the system prompt if the user hasn't decided yet; answers with
    /// [`Message::Permission`] either way.
    pub fn request_permission(&self) {
        let sender = self.sender.clone();
        let completion = RcBlock::new(move |granted: Bool, error: *mut NSError| {
            // SAFETY: when non-null, `error` is valid for the callback.
            if let Some(error) = unsafe { error.as_ref() } {
                log::warn!(
                    "adoken: notification authorization failed: {}",
                    error.localizedDescription()
                );
            }
            let permission = if granted.as_bool() {
                Permission::Granted
            } else {
                Permission::Denied
            };
            sender.unbounded_send(Message::Permission(permission)).ok();
        });
        // Alerts only: adoken plays its own sounds (spec 0007) and badges the
        // dock tile directly.
        self.center
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert,
                &completion,
            );
    }

    pub fn post(&self, request: &Request) {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&request.title));
        content.setBody(&NSString::from_str(&request.body));
        content.setThreadIdentifier(&NSString::from_str(&request.thread_id));
        if let Some(category) = request.category {
            content.setCategoryIdentifier(&NSString::from_str(category));
        }
        // A nil trigger delivers now; the same identifier replaces.
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&request.id),
            &content,
            None,
        );
        let completion = RcBlock::new(|error: *mut NSError| {
            // SAFETY: when non-null, `error` is valid for the callback.
            if let Some(error) = unsafe { error.as_ref() } {
                log::warn!(
                    "adoken: couldn't deliver banner: {}",
                    error.localizedDescription()
                );
            }
        });
        self.center
            .addNotificationRequest_withCompletionHandler(&request, Some(&completion));
    }
}

fn permission(status: UNAuthorizationStatus) -> Permission {
    match status {
        UNAuthorizationStatus::NotDetermined => Permission::NotDetermined,
        UNAuthorizationStatus::Denied => Permission::Denied,
        // Authorized, Provisional (quiet delivery), Ephemeral.
        _ => Permission::Granted,
    }
}

/// Both action sets notify produces (`mapping::category`), registered once:
/// macOS replaces the whole set on every `setNotificationCategories`.
fn categories() -> Retained<NSSet<UNNotificationCategory>> {
    // "Open" brings adoken forward; snoozing happens in the background.
    let open = UNNotificationAction::actionWithIdentifier_title_options(
        &NSString::from_str(ACTION_OPEN),
        &NSString::from_str("Open"),
        UNNotificationActionOptions::Foreground,
    );
    let snooze = UNNotificationAction::actionWithIdentifier_title_options(
        &NSString::from_str(ACTION_SNOOZE),
        &NSString::from_str("Snooze 1h"),
        UNNotificationActionOptions::empty(),
    );
    let category = |id: &str, actions: &[Retained<UNNotificationAction>]| {
        UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
            &NSString::from_str(id),
            &NSArray::from_retained_slice(actions),
            &NSArray::new(),
            UNNotificationCategoryOptions::empty(),
        )
    };
    NSSet::from_retained_slice(&[
        category(CATEGORY_OPEN, std::slice::from_ref(&open)),
        category(CATEGORY_OPEN_SNOOZE, &[open, snooze]),
    ])
}

/// `NSApp.dockTile.badgeLabel`; `None` clears it. Needs no notification
/// permission and works in unbundled dev runs too.
pub fn set_badge(label: Option<&str>) {
    let label = label.map(NSString::from_str);
    // SAFETY: `NSApplication.sharedApplication` exists once gpui has started
    // the app; `dockTile` is non-null for regular (dock) apps and messaging
    // nil is a no-op otherwise. `setBadgeLabel:` takes a nullable NSString.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![app, dockTile];
        if tile.is_null() {
            return;
        }
        let _: () = msg_send![tile, setBadgeLabel: label.as_deref()];
    }
}

struct Ivars {
    sender: UnboundedSender<Message>,
}

define_class!(
    // SAFETY: `NSObject` has no subclassing requirements and `Delegate`
    // doesn't implement `Drop`.
    #[unsafe(super(NSObject))]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // A banner (or one of its buttons) was clicked; possibly off the main
        // thread, so hop through the channel.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive_response(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            let action = response.actionIdentifier();
            // SAFETY: a constant `NSString` exported by UserNotifications.
            let default = &*action == unsafe { UNNotificationDefaultActionIdentifier };
            let action = (!default).then(|| action.to_string());
            if let Some(click) = click(&id, action.as_deref()) {
                self.ivars()
                    .sender
                    .unbounded_send(Message::Click(click))
                    .ok();
            }
            completion_handler.call(());
        }

        // Without this, macOS drops banners while adoken is frontmost. When
        // it is, the inbox already shows an in-app toast, so the OS copy only
        // goes to Notification Center's list (grouped by thread).
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions::List,));
        }
    }
);

impl Delegate {
    fn new(sender: UnboundedSender<Message>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { sender });
        // SAFETY: `NSObject`'s `init` is its designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}
