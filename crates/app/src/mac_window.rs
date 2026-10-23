//! AppKit state gpui doesn't expose: a window's `NSWindow`, whether it is
//! visible (not minimized, hidden or fully covered), and an observer for
//! occlusion changes. Used by the resource meter (spec 0023 FR-5) and the
//! global summon shortcut (spec 0021 FR-6). Main thread only.

use futures::channel::mpsc::UnboundedSender;
use gpui::Window;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// `NSWindowOcclusionStateVisible`.
const OCCLUSION_VISIBLE: usize = 1 << 1;

/// The `NSWindow` behind `window` (borrowed: AppKit keeps it alive while
/// gpui's window is open). `None` for windows with no AppKit window behind
/// them (headless tests: gpui's test windows panic when asked for a handle,
/// and AppKit then has no windows at all).
pub fn ns_window(window: &Window) -> Option<*mut AnyObject> {
    // SAFETY: NSApplication's shared instance and `windows` exist on every
    // supported macOS; main thread.
    let app_windows: usize = unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        msg_send![windows, count]
    };
    if app_windows == 0 {
        return None;
    }
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: `ns_view` is the live NSView gpui renders into.
    let ns_window: *mut AnyObject = unsafe { msg_send![view, window] };
    (!ns_window.is_null()).then_some(ns_window)
}

/// The window shows some of its pixels on screen.
pub fn is_visible(ns_window: *mut AnyObject) -> bool {
    // SAFETY: `ns_window` came from [`ns_window`] on the main thread.
    let state: usize = unsafe { msg_send![ns_window, occlusionState] };
    state & OCCLUSION_VISIBLE != 0
}

/// The window is in the Dock.
pub fn is_minimized(ns_window: *mut AnyObject) -> bool {
    // SAFETY: as above.
    unsafe { msg_send![ns_window, isMiniaturized] }
}

/// The app is hidden (`⌘H`).
pub fn app_hidden() -> bool {
    // SAFETY: NSApplication's shared instance and `isHidden` exist on every
    // supported macOS; main thread.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        msg_send![app, isHidden]
    }
}

/// adoken is the active app.
pub fn app_active() -> bool {
    // SAFETY: as above.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        msg_send![app, isActive]
    }
}

/// Unhide the app (`⌘H` undone); no-op when it isn't hidden.
pub fn unhide_app() {
    // SAFETY: as above.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![app, unhide: std::ptr::null::<AnyObject>()];
    }
}

/// Removes its `NSNotificationCenter` observer when dropped.
pub struct OcclusionObserver {
    token: Retained<AnyObject>,
}

impl Drop for OcclusionObserver {
    fn drop(&mut self) {
        // SAFETY: `token` is the observer `addObserverForName:` returned.
        unsafe {
            let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: &*self.token];
        }
    }
}

/// Send the window's visibility on every occlusion change (minimize,
/// `⌘H`, another Space, fully covered).
pub fn observe_occlusion(
    ns_window: *mut AnyObject,
    send: UnboundedSender<bool>,
) -> Option<OcclusionObserver> {
    let window_addr = ns_window as usize;
    let block = block2::RcBlock::new(move |_notification: *mut AnyObject| {
        let visible = is_visible(window_addr as *mut AnyObject);
        send.unbounded_send(visible).ok();
    });
    let name = objc2_foundation::NSString::from_str("NSWindowDidChangeOcclusionStateNotification");
    // SAFETY: a nil queue delivers on the posting (main) thread; the block
    // and name outlive the call (the center copies the block).
    let token: Option<Retained<AnyObject>> = unsafe {
        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        msg_send![
            center,
            addObserverForName: &*name,
            object: ns_window,
            queue: std::ptr::null::<AnyObject>(),
            usingBlock: &*block
        ]
    };
    token.map(|token| OcclusionObserver { token })
}
