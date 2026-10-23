//! In-app cold-start probe. Inert unless [`crate::probe::ENV`] is set.

use gpui::{App, Window};

use crate::frames::FrameRecorder;
use crate::probe::{Event, Mode, unix_now_ns};

/// Arms the probe on the app's first window. Call it from the
/// `open_window` build closure. Without `ADOKEN_PERF` this only reads one
/// environment variable.
///
/// "First frame presented" is taken at the start of the *second* platform
/// frame: `open_window` draws frame 1 synchronously, the first platform frame
/// callback presents it, and the one after that fires only once the display
/// has refreshed with frame 1 on it. The reading is therefore conservative by
/// at most one refresh interval (8 ms at 120 Hz), never optimistic.
pub fn install(window: &Window) {
    let Some(mode) = Mode::from_env() else {
        return;
    };
    // Frame 1 is presented right after this callback returns.
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| {
            Event::FirstFrame {
                unix_ns: unix_now_ns(),
            }
            .emit();
            if mode.frames == 0 {
                finish(cx);
            } else {
                redraw(window, mode.frames, FrameRecorder::start());
            }
        });
    });
}

/// Forces `remaining` full redraws, one per platform frame, then reports them.
/// `refresh` bypasses view caching, so each is a worst-case full frame.
fn redraw(window: &mut Window, remaining: u32, mut recorder: FrameRecorder) {
    window.refresh();
    window.on_next_frame(move |window, cx| {
        if remaining > 1 {
            redraw(window, remaining - 1, recorder);
            return;
        }
        recorder.collect();
        for &draw in recorder.draw().as_slice() {
            Event::Frame { draw }.emit();
        }
        finish(cx);
    });
}

fn finish(cx: &mut App) {
    Event::Done.emit();
    cx.quit();
}
