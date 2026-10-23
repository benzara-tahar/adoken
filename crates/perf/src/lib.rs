//! Perf harness for the budgets in AGENTS.md: cold start < 500 ms, frames
//! < 16 ms, Cmd+P < 50 ms on 100k items.
//!
//! - [`stats`]: sample series and nearest-rank percentiles.
//! - [`budget`]: the budgets and pass/fail checks `scripts/perf.sh` prints.
//! - [`probe`]: the `ADOKEN_PERF` stdout protocol between app and runner.
//! - `cold_start` / [`FrameRecorder`] (feature `gpui`): the in-app half.
//!
//! Runners: `perf-cold-start` (spawn → first presented frame, N launches) and
//! `perf-search` (100k-item index build + query latency). `scripts/perf.sh`
//! drives both.

pub mod budget;
pub mod probe;
pub mod stats;

#[cfg(feature = "gpui")]
pub mod cold_start;
#[cfg(feature = "gpui")]
mod frames;

pub use budget::{Budget, Check, Stat};
#[cfg(feature = "gpui")]
pub use frames::FrameRecorder;
pub use stats::{Ms, Samples, Summary};
