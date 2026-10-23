//! Background sync from Azure DevOps into `adoken-store` (spec 0002).
//!
//! Runs on a Tokio runtime (bridged into GPUI via `gpui_tokio`); store writes
//! run on the blocking pool. The UI never awaits network IO.
//!
//! - [`transport`]: the only network path; GET, POST for WIQL, and JSON
//!   writes for user actions only.
//! - [`governor`]: per-host concurrency, `Retry-After`, `X-RateLimit-*`,
//!   429 backoff, self-imposed budget, request ledger.
//! - [`engine`]: fetchers per cursor scope; each page is one transaction of
//!   upserts + events + cursor advance.
//! - [`scheduler`]: per-lane due times inside the hot / warm / cold tiers
//!   with jitter, foreground and background cadence, pause, offline and
//!   refresh controls.
//! - [`cadence`]: Settings › Sync (per-kind intervals, background, pause)
//!   mapped onto lanes, and the rate-budget estimate.
//! - [`ledger`]: the session's network log (spec 0016 group B): every
//!   request with its route, purpose, origin, timing, status and rate
//!   headers; totals and the JSON Lines export.
//! - [`routes`]: the allow-list of every Azure DevOps route adoken calls,
//!   with purpose labels and redaction (spec 0016 FR-19).
//! - [`attribution`]: who caused a request, inherited from where the work
//!   started (spec 0016 FR-18).
//! - [`activity`]: sync activity (spec 0016 group A): lane state, pass
//!   records with exact change counts, FR-8 row states.
//! - [`writes`]: user-initiated PR review writes (spec 0004) through the
//!   same governor and ledger, applied to the store server-wins.
//! - [`avatars`]: profile pictures fetched lazily through the governor into
//!   an on-disk cache (spec 0012).
//! - [`pr_files`]: review files of real PRs (spec 0004 FR-1..FR-3): file
//!   lists per push and blobs into the store's content-addressed cache.

pub mod activity;
pub mod attribution;
pub mod avatars;
pub mod cadence;
pub mod clock;
pub mod engine;
pub mod governor;
pub mod ledger;
pub mod pr_files;
pub mod routes;
pub mod scheduler;
pub mod transport;
pub mod wi;
pub mod wire;
pub mod writes;

pub use clock::{Clock, ManualClock, SystemClock};
pub use engine::{Engine, PassReport, Phase, SyncConfig, SyncError, SyncStatus};
pub use governor::{GovError, GovSnapshot, Governor, Hold, Pressure};
pub use ledger::{NetLog, Recording};
pub use pr_files::{PrFiles, RepoRef};
pub use scheduler::{Control, Scheduler};
pub use transport::{AdoTransport, ApiRequest, LoopbackTransport, Method, Transport};
pub use wi::{WiError, WiWriter};
pub use writes::{Applied, ReviewWrite, WriteError, Writer};

/// Scheduling tier (FR-4); `User` is user-initiated, highest priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    User,
    Hot,
    Warm,
    Cold,
}
