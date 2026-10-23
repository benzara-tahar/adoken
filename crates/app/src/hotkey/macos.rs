//! Carbon hot keys and app activation for the global shortcut (spec 0021
//! FR-1, FR-4, FR-7). Ported from zeron `crates/ui/src/appshots/macos.rs`
//! (`start_global_shortcut`, `refresh_global_shortcut`, `layout_keycode`,
//! `appshot_hot_key_handler`, `foreground_after_capture`) at
//! `d1010657d69c9730f8020edc36cc2649b136b6fa` (MIT; THIRD_PARTY_NOTICES.md).
//! Adapted: one hot key (spec 0025 FR-12), the layout lookup behind
//! [`Layout`], objc2 instead of objc 0.2. Main thread only.

use std::ffi::c_void;
use std::ptr;

use futures::channel::mpsc::UnboundedSender;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

use super::model::{Layout, Registrar};

type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Default)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

const EVENT_CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb");
const EVENT_HOT_KEY_PRESSED: u32 = 5;
/// adoken's hot key signature.
const SIGNATURE: u32 = u32::from_be_bytes(*b"ADKN");
/// The summon hot key's `EventHotKeyID.id`.
const HOT_KEY_ID: u32 = 1;
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = u32::from_be_bytes(*b"hkid");

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> CFTypeRef;
    fn TISGetInputSourceProperty(source: CFTypeRef, property: CFStringRef) -> *const c_void;
    static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    fn UCKeyTranslate(
        layout: *const c_void,
        key_code: u16,
        action: u16,
        modifiers: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: *mut u32,
        max_length: usize,
        actual_length: *mut usize,
        output: *mut u16,
    ) -> i32;
    fn LMGetKbdType() -> u8;
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: Option<unsafe extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> i32>,
        event_type_count: u32,
        event_types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_handler: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        hot_key_id: EventHotKeyId,
        target: EventTargetRef,
        options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> i32;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> i32;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
}

/// The application event target with adoken's handler installed; one per
/// process.
pub struct CarbonRegistrar {
    target: EventTargetRef,
    hot_key: EventHotKeyRef,
}

impl CarbonRegistrar {
    /// Install the hot key handler; presses arrive on `presses` (on the main
    /// thread, from the Carbon event loop). `None` if Carbon refused.
    pub fn install(presses: UnboundedSender<()>) -> Option<Self> {
        let sender = Box::into_raw(Box::new(presses)).cast::<c_void>();
        let event_type = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_PRESSED,
        };
        // SAFETY: Carbon calls on the main thread; `sender` lives for the
        // process (the handler is never removed).
        let target = unsafe { GetApplicationEventTarget() };
        let status = unsafe {
            InstallEventHandler(
                target,
                Some(hot_key_handler),
                1,
                &event_type,
                sender,
                ptr::null_mut(),
            )
        };
        if status != 0 {
            // SAFETY: the handler wasn't installed; reclaim the box.
            unsafe { drop(Box::from_raw(sender.cast::<UnboundedSender<()>>())) };
            log::warn!("hotkey: Carbon handler unavailable ({status})");
            return None;
        }
        Some(Self {
            target,
            hot_key: ptr::null_mut(),
        })
    }
}

impl Registrar for CarbonRegistrar {
    fn register(&mut self, key_code: u32, modifiers: u32) -> Result<(), i32> {
        let out = &mut self.hot_key;
        // SAFETY: `target` is the application target; `out` receives the
        // hot key ref.
        let status = unsafe {
            RegisterEventHotKey(
                key_code,
                modifiers,
                EventHotKeyId {
                    signature: SIGNATURE,
                    id: HOT_KEY_ID,
                },
                self.target,
                0,
                out,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            *out = ptr::null_mut();
            log::warn!("hotkey: couldn't register the global shortcut ({status})");
            Err(status)
        }
    }

    fn unregister(&mut self) {
        let hot_key = std::mem::replace(&mut self.hot_key, ptr::null_mut());
        if !hot_key.is_null() {
            // SAFETY: a ref RegisterEventHotKey returned, unregistered once.
            unsafe { UnregisterEventHotKey(hot_key) };
        }
    }
}

unsafe extern "C" fn hot_key_handler(
    _call: EventHandlerCallRef,
    event: EventRef,
    user_data: *mut c_void,
) -> i32 {
    if user_data.is_null() {
        return 0;
    }
    let mut id = EventHotKeyId::default();
    // SAFETY: a hot key event carries its `EventHotKeyID` as the direct
    // object; `id` is exactly that size.
    let status = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            ptr::null_mut(),
            std::mem::size_of::<EventHotKeyId>(),
            ptr::null_mut(),
            (&mut id as *mut EventHotKeyId).cast(),
        )
    };
    if status == 0 && id.signature == SIGNATURE && id.id == HOT_KEY_ID {
        // SAFETY: `user_data` is the boxed sender `install` leaked.
        let sender = unsafe { &*user_data.cast::<UnboundedSender<()>>() };
        sender.unbounded_send(()).ok();
    }
    0
}

/// The current ASCII-capable keyboard layout (TIS + `UCKeyTranslate`),
/// read fresh on every lookup so an input source change is picked up.
pub struct CarbonLayout;

impl Layout for CarbonLayout {
    fn translate(&self, code: u16, shift: bool) -> Option<String> {
        // SAFETY: TIS returns a +1 input source (released below); its
        // layout data is a CFData owned by the source.
        unsafe {
            let source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
            if source.is_null() {
                return None;
            }
            let data = TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData);
            let result = if data.is_null() {
                None
            } else {
                let layout = CFDataGetBytePtr(data).cast::<c_void>();
                let mut output = [0_u16; 4];
                let mut length = 0;
                let mut dead = 0;
                // Modifier state is `(EventModifiers >> 8) & 0xFF`: shift = 2.
                let modifiers = if shift { 2 } else { 0 };
                let status = UCKeyTranslate(
                    layout,
                    code,
                    0,
                    modifiers,
                    u32::from(LMGetKbdType()),
                    1,
                    &mut dead,
                    output.len(),
                    &mut length,
                    output.as_mut_ptr(),
                );
                (status == 0 && length > 0 && length <= output.len())
                    .then(|| String::from_utf16_lossy(&output[..length]))
            };
            CFRelease(source);
            result
        }
    }
}

/// FR-7 fallback: ask LaunchServices to open adoken's own bundle, which
/// activates it even when macOS declined the cooperative activation. A
/// bare `cargo run` has no bundle: standard activation only.
pub fn activate_via_launch_services() {
    // SAFETY: NSWorkspace / NSBundle / NSWorkspaceOpenConfiguration exist
    // on every supported macOS (10.15+); main thread.
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
        let url: *mut AnyObject = msg_send![bundle, bundleURL];
        if url.is_null() {
            return;
        }
        let extension: *mut objc2_foundation::NSString = msg_send![url, pathExtension];
        if extension.is_null() || (*extension).to_string() != "app" {
            return;
        }
        let configuration: *mut AnyObject =
            msg_send![class!(NSWorkspaceOpenConfiguration), configuration];
        let _: () = msg_send![configuration, setActivates: true];
        let _: () = msg_send![configuration, setCreatesNewApplicationInstance: false];
        let completion = block2::RcBlock::new(|_app: *mut AnyObject, error: *mut AnyObject| {
            if !error.is_null() {
                log::warn!("hotkey: LaunchServices couldn't bring adoken forward");
            }
        });
        let _: () = msg_send![
            workspace,
            openApplicationAtURL: url,
            configuration: configuration,
            completionHandler: &*completion
        ];
    }
}
