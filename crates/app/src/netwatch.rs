//! Network reachability and sleep/wake for sync (spec 0002 FR-5, §3
//! "Offline" / "Asleep"): the scheduler pauses without a network path or
//! while the Mac sleeps, the statusbar says Offline at once, and waking or
//! reconnecting runs a hot pass right away.
//!
//! macOS: `NWPathMonitor` (Network.framework's C API plus one block) for
//! the path, `NSWorkspace` will-sleep / did-wake notifications for sleep.
//! The monitor bridge follows zeron's `crates/sync/src/net_path.rs` (see
//! THIRD_PARTY_NOTICES.md). Other platforms: nothing is reported, sync
//! assumes it's online and failed requests still say Offline.

use adoken_sync::Control;

/// `nw_path_status_t`.
const NW_PATH_STATUS_UNSATISFIED: i32 = 2;

/// Only a definitive "no path" is offline: `invalid` and `satisfiable`
/// (e.g. VPN on demand) stay online, so a confused monitor can only make
/// sync try, never go silent.
pub fn path_online(status: i32) -> bool {
    status != NW_PATH_STATUS_UNSATISFIED
}

/// Turns raw path reports (the monitor repeats `satisfied` on every
/// interface change) into [`Control::Online`] transitions.
#[derive(Debug, Default)]
pub struct PathState {
    online: Option<bool>,
}

impl PathState {
    /// The control to send for a report, if the state changed. The first
    /// report always counts: it says how the app started.
    pub fn observe(&mut self, online: bool) -> Option<Control> {
        (self.online.replace(online) != Some(online)).then_some(Control::Online(online))
    }
}

/// Keeps the observers alive; dropping it stops them.
pub struct NetWatch {
    #[cfg(target_os = "macos")]
    _imp: imp::Watch,
}

/// Start reporting to `send`. Call on the main thread (the sleep observers
/// register with `NSWorkspace`). `send` runs on the monitor's dispatch
/// queue and on the main thread; it must only forward.
pub fn watch(send: impl Fn(Control) + Send + Sync + 'static) -> NetWatch {
    #[cfg(target_os = "macos")]
    {
        NetWatch {
            _imp: imp::Watch::start(std::sync::Arc::new(send)),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = send;
        NetWatch {}
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_void};
    use std::sync::{Arc, Mutex};

    use block2::{Block, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    use super::{Control, PathState, path_online};

    // nw_path_monitor_t / nw_path_t / dispatch_queue_t are ObjC objects;
    // opaque pointers are all this bridge needs.
    #[link(name = "Network", kind = "framework")]
    unsafe extern "C" {
        fn nw_path_monitor_create() -> *mut c_void;
        fn nw_path_monitor_set_update_handler(
            monitor: *mut c_void,
            handler: &Block<dyn Fn(*mut c_void)>,
        );
        fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *mut c_void);
        fn nw_path_monitor_start(monitor: *mut c_void);
        fn nw_path_monitor_cancel(monitor: *mut c_void);
        fn nw_path_get_status(path: *mut c_void) -> i32;
        fn nw_release(obj: *mut c_void);
    }

    unsafe extern "C" {
        fn dispatch_queue_create(label: *const c_char, attr: *mut c_void) -> *mut c_void;
        fn dispatch_release(obj: *mut c_void);
    }

    type Sink = Arc<dyn Fn(Control) + Send + Sync>;

    pub struct Watch {
        monitor: *mut c_void,
        center: Option<Retained<AnyObject>>,
        observers: Vec<Retained<AnyObject>>,
    }

    impl Watch {
        pub fn start(send: Sink) -> Watch {
            Watch {
                monitor: start_path_monitor(send.clone()),
                center: workspace_center(),
                observers: Vec::new(),
            }
            .observe_sleep(send)
        }

        fn observe_sleep(mut self, send: Sink) -> Watch {
            let Some(center) = self.center.clone() else {
                return self;
            };
            for (name, control) in [
                ("NSWorkspaceWillSleepNotification", Control::Sleep),
                ("NSWorkspaceDidWakeNotification", Control::Wake),
            ] {
                let send = send.clone();
                let block = RcBlock::new(move |_note: *mut AnyObject| send(control));
                let name = NSString::from_str(name);
                // SAFETY: `center` is NSWorkspace's notification center;
                // `addObserverForName:object:queue:usingBlock:` copies the
                // block and returns the observer token, retained here and
                // removed on drop. nil object/queue: any sender, posting
                // thread.
                let observer: Option<Retained<AnyObject>> = unsafe {
                    msg_send![
                        &*center,
                        addObserverForName: &*name,
                        object: std::ptr::null::<AnyObject>(),
                        queue: std::ptr::null::<AnyObject>(),
                        usingBlock: &*block
                    ]
                };
                self.observers.extend(observer);
            }
            self
        }
    }

    impl Drop for Watch {
        fn drop(&mut self) {
            if let Some(center) = &self.center {
                for observer in self.observers.drain(..) {
                    // SAFETY: tokens returned by this center.
                    let _: () = unsafe { msg_send![&**center, removeObserver: &*observer] };
                }
            }
            if !self.monitor.is_null() {
                // SAFETY: created by `nw_path_monitor_create`, started once;
                // cancel stops the handler, release drops our reference.
                unsafe {
                    nw_path_monitor_cancel(self.monitor);
                    nw_release(self.monitor);
                }
            }
        }
    }

    fn workspace_center() -> Option<Retained<AnyObject>> {
        // SAFETY: `NSWorkspace.sharedWorkspace` and its `notificationCenter`
        // are process singletons.
        unsafe {
            let workspace: Option<Retained<AnyObject>> =
                msg_send![class!(NSWorkspace), sharedWorkspace];
            msg_send![&*workspace?, notificationCenter]
        }
    }

    fn start_path_monitor(send: Sink) -> *mut c_void {
        let state = Mutex::new(PathState::default());
        let handler = RcBlock::new(move |path: *mut c_void| {
            // SAFETY: `path` is the live `nw_path_t` the monitor passes in.
            let online = path_online(unsafe { nw_path_get_status(path) });
            let change = state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .observe(online);
            if let Some(control) = change {
                send(control);
            }
        });
        // SAFETY: Network.framework C API; `set_update_handler` copies the
        // block, which runs on our serial queue; the monitor retains the
        // queue (our reference is released).
        unsafe {
            let monitor = nw_path_monitor_create();
            if monitor.is_null() {
                log::warn!("netwatch: NWPathMonitor unavailable");
                return monitor;
            }
            let queue =
                dispatch_queue_create(c"dev.adoken.netwatch".as_ptr(), std::ptr::null_mut());
            nw_path_monitor_set_queue(monitor, queue);
            dispatch_release(queue);
            nw_path_monitor_set_update_handler(monitor, &handler);
            nw_path_monitor_start(monitor);
            monitor
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_reports_become_online_transitions() {
        let mut s = PathState::default();
        assert_eq!(
            s.observe(true),
            Some(Control::Online(true)),
            "first report counts"
        );
        assert_eq!(
            s.observe(true),
            None,
            "interface handovers repeat `satisfied`"
        );
        assert_eq!(s.observe(false), Some(Control::Online(false)));
        assert_eq!(s.observe(false), None);
        assert_eq!(s.observe(true), Some(Control::Online(true)));
        let mut offline_start = PathState::default();
        assert_eq!(offline_start.observe(false), Some(Control::Online(false)));
    }

    #[test]
    fn only_an_unsatisfied_path_is_offline() {
        // invalid, satisfied, unsatisfied, satisfiable
        assert_eq!([0, 1, 2, 3].map(path_online), [true, true, false, true]);
    }
}
