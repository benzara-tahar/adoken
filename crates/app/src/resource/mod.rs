//! Resource meter (spec 0023, activity.md §2.7): adoken's own memory and
//! CPU in the statusbar, off by default (`[activity] resource_meter`).
//!
//! - [`meter`]: pure formatting, hysteresis, rate and cadence.
//! - [`source`]: the process counters (macOS `proc_pid_rusage`).
//! - [`ResourceMeter`]: one per main window. While the setting is on it
//!   owns a sampler task on the background executor; the task sends the
//!   UI a message only when the shown text changes (FR-5, FR-7), and keeps
//!   the latest details in a shared slot the tooltip and "Copy resource
//!   usage" read.

pub mod meter;
pub mod source;

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use futures::{FutureExt as _, StreamExt as _};
use gpui::{
    App, BackgroundExecutor, ClipboardItem, Context, FocusHandle, SharedString, Task, Window,
    actions,
};
use parking_lot::Mutex;

use crate::palette::PaletteCommand;
use meter::{Details, Lifecycle, MeterText, Sampler, Wake};
use source::ResourceSource;

actions!(
    resource,
    [
        /// Show or hide adoken's memory and CPU in the statusbar.
        ToggleResourceMeter,
        /// Copy one line with adoken's memory and CPU (for bug reports).
        CopyResourceUsage,
    ]
);

/// Settings › General switch (spec 0009 amendment, FR-11).
pub const SETTING: &str = "Show memory and CPU in the statusbar";
pub const SETTING_KEYWORDS: &str =
    "resource meter memory ram cpu usage footprint performance statusbar activity monitor";
pub const TOGGLE_TITLE: &str = "Statusbar: Show or hide memory and CPU";
pub const COPY_TITLE: &str = "Copy resource usage";
const COPY_FAILED: &str = "Couldn't read memory usage";

static LAUNCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Note the launch instant (the copied line's "up 3h 12m").
pub fn mark_launch() {
    LazyLock::force(&LAUNCH);
}

fn uptime_ms() -> u64 {
    LAUNCH.elapsed().as_millis() as u64
}

/// Palette commands (FR-10); none where there is no source (FR-4).
pub fn commands() -> Vec<PaletteCommand> {
    if !source::SUPPORTED {
        return Vec::new();
    }
    vec![
        PaletteCommand {
            title: TOGGLE_TITLE,
            keys: None,
            action: Box::new(ToggleResourceMeter),
        },
        PaletteCommand {
            title: COPY_TITLE,
            keys: None,
            action: Box::new(CopyResourceUsage),
        },
    ]
}

/// The setting as the main window last applied it (the palette toggle).
#[derive(Default)]
struct Shown(bool);

impl gpui::Global for Shown {}

/// Memory and CPU show in the statusbar.
pub fn shown(cx: &App) -> bool {
    cx.try_global::<Shown>().is_some_and(|s| s.0)
}

/// "Copy resource usage" works while the meter is hidden.
pub fn init(cx: &mut App) {
    mark_launch();
    cx.set_global(Shown(false));
    cx.on_action(|_: &CopyResourceUsage, cx| copy_usage(cx));
}

/// A fresh sample off the UI thread, copied as one line (FR-10). The 10 s
/// CPU figure comes from the running meter; without one the line gives
/// total CPU only.
pub fn copy_usage(cx: &mut App) {
    let Some(mut source) = source::platform() else {
        return;
    };
    let read = cx.background_executor().spawn(async move { source.read() });
    let recent = cx
        .try_global::<RecentCpu>()
        .and_then(|r| (*r.0.lock()).and_then(|d| d.cpu_recent));
    cx.spawn(async move |cx| {
        let reading = read.await;
        cx.update(|cx| {
            use adoken_ui::{IconName, Toast, ToastStack, ToastTone};
            match reading {
                Some(reading) => {
                    let details = Details {
                        reading,
                        cpu_recent: recent,
                        uptime_ms: uptime_ms(),
                    };
                    let line = meter::copy_line(env!("CARGO_PKG_VERSION"), &details);
                    cx.write_to_clipboard(ClipboardItem::new_string(line));
                    ToastStack::show(
                        Toast::new("resource-copy", IconName::Copy, "Copied resource usage"),
                        cx,
                    );
                }
                None => ToastStack::show(
                    Toast::new("resource-copy", IconName::CircleAlert, COPY_FAILED)
                        .tone(ToastTone::Warning),
                    cx,
                ),
            }
        });
    })
    .detach();
}

/// The running meter's latest details, so a copy while it samples keeps
/// its 10 s CPU figure.
struct RecentCpu(Shared);

impl gpui::Global for RecentCpu {}

type Shared = Arc<Mutex<Option<Details>>>;

/// Sampler messages to the UI: only text changes (or hiding).
type Updates = UnboundedSender<Wake>;

/// The sampler loop (FR-5): waits for its next due time or a lifecycle
/// change, reads the source only when a sample is due, publishes details
/// to `details`, and sends `ui` only what changes the statusbar. Runs on
/// the background executor; never touches the UI thread.
pub async fn run(
    mut source: Box<dyn ResourceSource>,
    mut control: UnboundedReceiver<Lifecycle>,
    ui: Updates,
    details: Shared,
    executor: BackgroundExecutor,
) {
    let start = executor.now();
    let now =
        |executor: &BackgroundExecutor| executor.now().duration_since(start).as_millis() as u64;
    let mut sampler = Sampler::default();
    loop {
        let timer = match sampler.next_wake() {
            Some(at) => {
                let wait = at.saturating_sub(now(&executor));
                futures::future::Either::Left(executor.timer(Duration::from_millis(wait)))
            }
            None => futures::future::Either::Right(futures::future::pending::<()>()),
        };
        futures::select_biased! {
            lifecycle = control.next() => match lifecycle {
                Some(lifecycle) => sampler.set_lifecycle(lifecycle, now(&executor)),
                None => return,
            },
            _ = timer.fuse() => {}
        }
        let t = now(&executor);
        if sampler.next_wake().is_none_or(|at| at > t) {
            continue;
        }
        let sampled = sampler.sample_due(t);
        let wake = sampler.wake(t, || source.read());
        if sampled && let Some(latest) = sampler.meter.latest() {
            let next = Details {
                reading: latest.reading,
                cpu_recent: sampler.meter.cpu_recent(),
                uptime_ms: uptime_ms(),
            };
            *details.lock() = Some(next);
        }
        if wake != Wake::Quiet && ui.unbounded_send(wake).is_err() {
            return;
        }
    }
}

/// The statusbar's resource meter for one window.
pub struct ResourceMeter {
    lifecycle: Lifecycle,
    text: Option<MeterText>,
    details: Shared,
    control: Option<UnboundedSender<Lifecycle>>,
    pub(crate) focus: FocusHandle,
    pub(crate) menu: adoken_ui::action_menu::MenuSlot,
    pub(crate) bounds: adoken_ui::action_menu::BoundsSlot,
    /// Stories: a fixed text, no sampler.
    fixture: bool,
    /// Tests: read this instead of the platform's source when sampling
    /// starts.
    injected: Option<Box<dyn ResourceSource>>,
    _sampler: Option<(Task<()>, Task<()>)>,
    #[cfg(target_os = "macos")]
    _occlusion: Option<(crate::mac_window::OcclusionObserver, Task<()>)>,
}

impl ResourceMeter {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let details: Shared = Arc::default();
        cx.set_global(RecentCpu(details.clone()));
        Self {
            lifecycle: Lifecycle {
                enabled: false,
                visible: true,
                active: window.is_window_active(),
                collapsed: false,
            },
            text: None,
            details,
            control: None,
            focus: cx.focus_handle().tab_index(100).tab_stop(true),
            menu: Default::default(),
            bounds: Default::default(),
            fixture: false,
            injected: None,
            _sampler: None,
            #[cfg(target_os = "macos")]
            _occlusion: None,
        }
    }

    /// Stories: show `text` with `details` in the tooltip; no sampling.
    pub fn set_fixture(&mut self, text: MeterText, details: Details, cx: &mut Context<Self>) {
        self.fixture = true;
        self._sampler = None;
        self.control = None;
        self.lifecycle.enabled = true;
        self.text = Some(text);
        *self.details.lock() = Some(details);
        cx.notify();
    }

    /// What the statusbar paints; `None` while off, hidden after failures,
    /// or before the first sample.
    pub fn text(&self) -> Option<&MeterText> {
        self.text.as_ref().filter(|_| self.lifecycle.enabled)
    }

    pub fn enabled(&self) -> bool {
        self.lifecycle.enabled
    }

    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// The tooltip, from the latest sample.
    pub fn tooltip(&self) -> Option<String> {
        let details = (*self.details.lock())?;
        Some(meter::tooltip(&details))
    }

    /// The setting changed. The first time it turns on, the meter starts
    /// following the window's visibility (never before: the native window
    /// is only looked up once there is something to sample).
    pub fn set_enabled(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        cx.set_global(Shown(enabled && source::SUPPORTED));
        if self.fixture {
            return;
        }
        let enabled = enabled && source::SUPPORTED;
        let mut lifecycle = Lifecycle {
            enabled,
            ..self.lifecycle
        };
        #[cfg(target_os = "macos")]
        if enabled
            && self._occlusion.is_none()
            && let Some(ns_window) = crate::mac_window::ns_window(window)
        {
            let (tx, mut rx) = mpsc::unbounded();
            let observer = crate::mac_window::observe_occlusion(ns_window, tx);
            let task = cx.spawn(async move |this, cx| {
                while let Some(visible) = rx.next().await {
                    if this
                        .update(cx, |this, cx| this.set_visible(visible, cx))
                        .is_err()
                    {
                        return;
                    }
                }
            });
            lifecycle.visible = crate::mac_window::is_visible(ns_window);
            self._occlusion = observer.map(|observer| (observer, task));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = window;
        self.update_lifecycle(lifecycle, cx);
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        self.update_lifecycle(
            Lifecycle {
                active,
                ..self.lifecycle
            },
            cx,
        );
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.update_lifecycle(
            Lifecycle {
                visible,
                ..self.lifecycle
            },
            cx,
        );
    }

    /// The statusbar collapsed the meter away (§2.5 step 4), or back.
    pub fn set_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.update_lifecycle(
            Lifecycle {
                collapsed,
                ..self.lifecycle
            },
            cx,
        );
    }

    fn update_lifecycle(&mut self, lifecycle: Lifecycle, cx: &mut Context<Self>) {
        if lifecycle == self.lifecycle || self.fixture {
            return;
        }
        self.lifecycle = lifecycle;
        if !lifecycle.enabled {
            // Off: no sampler task exists (§7 "zero cost").
            self._sampler = None;
            self.control = None;
            if self.text.take().is_some() {
                cx.notify();
            }
            return;
        }
        if self.control.is_none() {
            self.start(cx);
        }
        if let Some(control) = &self.control {
            control.unbounded_send(lifecycle).ok();
        }
    }

    /// Sample from `source` instead of the platform's once sampling next
    /// starts (headless tests: a scripted source on the test clock).
    pub fn set_source(&mut self, source: Box<dyn ResourceSource>) {
        self.injected = Some(source);
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.injected.take().or_else(source::platform) else {
            return;
        };
        let (control, control_rx) = mpsc::unbounded();
        let (ui, mut updates) = mpsc::unbounded();
        let executor = cx.background_executor().clone();
        let sampler = executor.spawn(run(
            source,
            control_rx,
            ui,
            self.details.clone(),
            executor.clone(),
        ));
        let receiver = cx.spawn(async move |this, cx| {
            while let Some(wake) = updates.next().await {
                let alive = this
                    .update(cx, |this, cx| {
                        match wake {
                            Wake::Text(text) => this.text = Some(text),
                            Wake::Hidden => this.text = None,
                            Wake::Quiet => return,
                        }
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    return;
                }
            }
        });
        self.control = Some(control);
        self._sampler = Some((sampler, receiver));
    }

    /// Click, `↩`, `Space` or right-click: the two-row menu, anchored above
    /// the meter, right-aligned (activity.md §2.7).
    pub fn open_menu(
        &mut self,
        on_hide: impl Fn(&mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use adoken_ui::action_menu::{MenuItem, MenuPosition};
        let position = match self.bounds.get() {
            Some(bounds) => MenuPosition {
                point: gpui::point(bounds.right(), bounds.top()),
                corner: gpui::Anchor::BottomRight,
            },
            None => adoken_ui::action_menu::keyboard_position(&self.bounds, window),
        };
        let items = vec![
            MenuItem::new(COPY_TITLE, |_, cx| copy_usage(cx)),
            MenuItem::new("Hide from statusbar", on_hide),
        ];
        self.menu.open(
            "Resource meter",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this: &mut Self| &mut this.menu,
            window,
            cx,
        );
    }

    pub fn menu_open(&self) -> bool {
        self.menu.is_open()
    }

    /// The statusbar painted the meter (its menu has an anchor).
    pub fn laid_out(&self) -> bool {
        self.bounds.get().is_some()
    }

    pub fn spoken(&self) -> Option<SharedString> {
        self.text().map(|t| t.spoken().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meter::Reading;

    struct Fake {
        main: std::thread::ThreadId,
        calls: std::sync::mpsc::Sender<std::thread::ThreadId>,
    }

    impl ResourceSource for Fake {
        fn read(&mut self) -> Option<Reading> {
            let here = std::thread::current().id();
            assert_ne!(
                here, self.main,
                "the source is never called on the main thread"
            );
            self.calls.send(here).ok();
            Some(Reading {
                footprint: 260 << 20,
                resident: 325 << 20,
                peak: Some(373 << 20),
                cpu_ns: 0,
            })
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_sampler_reads_off_the_calling_thread_and_only_sends_changes() {
        // AC-6 on the real macOS background executor (GCD threads).
        let platform = gpui_platform::current_platform(true);
        let executor = platform.background_executor();
        let (calls, seen) = std::sync::mpsc::channel();
        let fake = Fake {
            main: std::thread::current().id(),
            calls,
        };
        let (control, control_rx) = mpsc::unbounded();
        let (ui, mut updates) = mpsc::unbounded();
        let details: Shared = Arc::default();
        let task = executor.spawn(run(
            Box::new(fake),
            control_rx,
            ui,
            details.clone(),
            executor.clone(),
        ));
        // Turning the setting on samples at once (AC-5's last case).
        control
            .unbounded_send(Lifecycle {
                enabled: true,
                visible: true,
                active: true,
                collapsed: false,
            })
            .unwrap();
        let first = futures::executor::block_on(updates.next()).unwrap();
        assert!(
            matches!(&first, Wake::Text(t) if t.full() == "260 MB"),
            "{first:?}"
        );
        assert!(seen.recv_timeout(Duration::from_secs(5)).is_ok());
        assert!(details.lock().is_some(), "details published");
        drop(control);
        futures::executor::block_on(task);
    }
}
