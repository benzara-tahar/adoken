//! Notification center (spec 0005): one inbox of [`Notification`] rows fed by
//! sync events and nudges, with noise removal, priorities and user rules,
//! local triage (done / read / snooze / mute / undo), quiet hours, per-source
//! delivery windows, and the OS-banner decision.
//!
//! Everything here is pure and driven by an injected [`Clock`]; the app runs
//! the blocking IO in [`source`] (store → events) and [`persist`] (local
//! tables + `settings.toml [notify]`) on a background executor and hands [`Effects`]
//! to real banner/sound sinks.

pub mod classify;
pub mod clock;
pub mod delivery;
pub mod engine;
pub mod filter;
pub mod persist;
pub mod rules;
pub mod schedule;
pub mod settings;
pub mod snooze;
pub mod source;

pub use classify::{EventContext, Kind, Priority, PriorityTable, RuleKey, Source};
pub use clock::{Clock, FakeClock, SystemClock};
pub use delivery::{
    Banner, BannerAction, BannerSink, DeliveryEnv, Effects, Interrupt, OsPermission, Recorder,
    SoundCue, SoundSink, decide, release_time,
};
pub use engine::{
    COALESCE_WINDOW, Changes, Engine, EngineState, Held, Incoming, Notification, NotificationId,
    Outcome, RETENTION, STALE_AFTER,
};
pub use filter::{Filter, StateFilter};
pub use rules::{EventRef, Mute, RuleAction, RuleMatch, UserRule, evaluate};
pub use schedule::{Schedule, Span, TimeOfDay, Weekdays, clock_label};
pub use settings::{BadgeMode, DeliveryByPriority, DeliveryMode, NotifySettings, QuietHours};
pub use snooze::{DEFAULT_MORNING, Snooze, SnoozeChoice};
