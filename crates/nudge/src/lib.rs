//! Smart nudges (spec 0006): rules N1–N7 over local store data, a daily cap,
//! snooze and opt-outs, and the Today "my day" model (spec 0017).
//!
//! - Computation is local only (FR-7): nothing in this crate performs IO
//!   except [`store`], which reads and writes the local SQLite file.
//! - adoken never messages anyone (FR-9, §11): nudges only *name* reviewers;
//!   every action opens something.
//! - Time is injected ([`Clock`]); working-time math is [`WorkingHours`].

pub mod clock;
pub mod copy;
pub mod engine;
pub mod hours;
pub mod kind;
pub mod rules;
pub mod snapshot;
pub mod store;
pub mod summary;
pub mod today;

pub use clock::{Clock, FixedClock, SystemClock, local_utc_offset_min};
pub use engine::{Ledger, NudgeSink, Pass, SUBJECT_COOLDOWN, SubjectState, evaluate, run_pass};
pub use hours::WorkingHours;
pub use kind::{KindSettings, NudgeKind, Settings, Threshold, Tier};
pub use rules::{Nudge, NudgeAction};
pub use snapshot::{DataHealth, LaneHealth, PrRecords, Snapshot, Stale, StaleReason, SyncHealth};
pub use summary::{Commits, CountSlot, Counts, DayMood, Mood, Slot, Summary};
pub use today::{
    Age, Blocker, Day, Focus, FocusState, Heat, LooseEnd, PrActivity, PrState, Quiet, Stage, Track,
    TrackReviewer, WaitCard, today,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod today_tests;
