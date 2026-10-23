//! Frame recorder over gpui's built-in frame trace.
//!
//! gpui times every `Window::draw` itself once `set_frame_trace_enabled(true)`
//! is on (`gpui::profiler`); this turns those timings into [`Samples`] the app
//! can summarize or check against [`crate::Budget::FRAME`].

use gpui::{FrameTimingCollector, set_frame_trace_enabled};

use crate::stats::Samples;

/// Collects draw timings for every gpui window from the moment it is started.
///
/// While any recorder is alive gpui pays one `Instant::now()` pair per frame;
/// dropping the recorder that turned tracing on turns it back off.
pub struct FrameRecorder {
    collector: FrameTimingCollector,
    owns_trace: bool,
    draw: Samples,
    dirty_to_draw: Samples,
}

impl FrameRecorder {
    pub fn start() -> Self {
        let owns_trace = set_frame_trace_enabled(true);
        Self {
            collector: FrameTimingCollector::new(),
            owns_trace,
            draw: Samples::new("frame draw"),
            dirty_to_draw: Samples::new("frame dirty->draw"),
        }
    }

    /// Moves frames drawn since the last call into the sample series. Call it
    /// at least every few seconds of continuous drawing: gpui's ring buffer
    /// holds 16 MiB of timings and drops the oldest past that.
    pub fn collect(&mut self) {
        for timing in self.collector.collect_unseen() {
            self.draw.record(timing.draw_duration());
            if let Some(latency) = timing.dirty_to_draw_duration() {
                self.dirty_to_draw.record(latency);
            }
        }
    }

    /// Time inside `Window::draw` (layout + prepaint + paint) per frame.
    pub fn draw(&self) -> &Samples {
        &self.draw
    }

    /// First invalidation → end of draw per frame: how long a change waited to
    /// be drawn. Only frames whose invalidation happened while recording.
    pub fn dirty_to_draw(&self) -> &Samples {
        &self.dirty_to_draw
    }
}

impl Drop for FrameRecorder {
    fn drop(&mut self) {
        if self.owns_trace {
            set_frame_trace_enabled(false);
        }
    }
}
