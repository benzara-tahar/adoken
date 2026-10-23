//! adoken sounds (spec 0007 FR-7/FR-11, spec 0025 FR-13, docs/design/sound.md).
//!
//! - [`SoundEvent`]: the event → sound map (§2).
//! - [`SoundSettings`]: `settings.toml [sound]`, on/off and volume; off
//!   after a fresh install.
//! - [`decide`]: the gating matrix (Quiet mode, quiet hours, delivery
//!   windows, focus, backlog, priority, Arcade mode) and the gain formula.
//! - [`Scheduler`]: debounce and notify burst coalescing (AC-9/AC-10).
//! - [`Pack::builtin`]: the one original pack (Dojo, with Arcade chip tones
//!   for Ken's events) synthesized from code, validated by [`Pack::validate`].
//! - [`SoundEngine`]: the handle callers use; opens nothing (no thread, no
//!   audio device, no pack memory) until sounds are enabled, and the device
//!   only on the first play. Never touched before the first frame.
//!
//! No gpui dependency: the notification center (`adoken-notify`) and the app
//! call it with a [`PlayContext`] they fill from their own state and clock.

mod engine;
mod event;
mod gate;
mod mixer;
mod output;
mod pack;
mod schedule;
mod settings;
pub mod synth;

pub use engine::{MAX_COMBO_SEMITONES, SoundEngine};
pub use event::{Category, SoundEvent};
pub use gate::{NotifyPriority, PlayContext, Silenced, decide};
pub use mixer::{MAX_VOICES, Mixer, Voice};
pub use output::{DeviceOutput, IDLE_CLOSE, Output};
pub use pack::{Clip, MAX_PACK_BYTES, Pack, PackIssue, Problem, Severity};
pub use schedule::{DEBOUNCE, NOTIFY_COOLDOWN, Scheduler};
pub use settings::{IssueKind, SETTINGS_TABLE, SoundIssue, SoundSettings};
