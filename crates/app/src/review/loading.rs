//! Loading indicators of an open review (spec 0004 States, principles
//! §3.2): the file list's fetch on open, and one file's blobs + diff.
//! Nothing shows for the first [`GRACE`] (a fast load never flashes);
//! after [`SLOW`] a second line says what the wait is on. Pure: time comes
//! in as arguments, so the rules are testable without a window.

use std::time::{Duration, Instant};

use adoken_sync::Hold;

/// Indicators appear after this long (principles §3.2).
pub const GRACE: Duration = crate::browse::LOADING_GRACE;
/// After this long the diff pane says what it's waiting for.
pub const SLOW: Duration = Duration::from_secs(3);

/// The file list's refresh at open (spec 0004 FR-1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ListLoad {
    #[default]
    Idle,
    Loading {
        since: Instant,
    },
    /// Why it failed; the tree offers Retry.
    Failed(String),
}

impl ListLoad {
    pub fn start(&mut self, now: Instant) {
        *self = ListLoad::Loading { since: now };
    }

    pub fn finish(&mut self, result: Result<(), String>) {
        *self = match result {
            Ok(()) => ListLoad::Idle,
            Err(error) => ListLoad::Failed(error),
        };
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, ListLoad::Loading { .. })
    }

    /// Skeleton rows / "Loading files…": only once the fetch outlasts the
    /// grace.
    pub fn shows_indicator(&self, now: Instant) -> bool {
        matches!(self, ListLoad::Loading { since } if now.saturating_duration_since(*since) >= GRACE)
    }

    pub fn failure(&self) -> Option<&str> {
        match self {
            ListLoad::Failed(error) => Some(error),
            _ => None,
        }
    }
}

/// What a file's diff is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Its missing blobs are being downloaded.
    Fetching { blobs: usize },
    /// Blobs are local; the diff is being computed.
    Computing,
}

/// One file's load in progress (selection, prefetch, retry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLoad {
    pub since: Instant,
    pub phase: Phase,
}

/// What the diff pane shows for a loading file, if anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indicator {
    /// The second line, once the load is slow.
    pub detail: Option<String>,
}

/// The diff pane's loading state at `now`: nothing within [`GRACE`], a
/// spinner and the file name until [`SLOW`], then also why. `hold` is the
/// governor's reason a request would wait; `offline` is sync's view of
/// reachability.
pub fn indicator(
    load: &FileLoad,
    now: Instant,
    offline: bool,
    hold: Option<Hold>,
) -> Option<Indicator> {
    let elapsed = now.saturating_duration_since(load.since);
    if elapsed < GRACE {
        return None;
    }
    let detail = (elapsed >= SLOW).then(|| slow_detail(load.phase, offline, hold));
    Some(Indicator { detail })
}

fn slow_detail(phase: Phase, offline: bool, hold: Option<Hold>) -> String {
    let Phase::Fetching { blobs } = phase else {
        return "Still computing the diff…".to_owned();
    };
    if offline {
        return "Offline. The file loads when Azure DevOps is reachable again.".to_owned();
    }
    match hold {
        Some(Hold::Paused { .. }) => "Azure DevOps asked adoken to slow down. Waiting…".to_owned(),
        Some(Hold::Budget { .. } | Hold::Busy) => {
            "Waiting for Azure DevOps (sync in progress)…".to_owned()
        }
        None if blobs == 1 => "Downloading 1 file from Azure DevOps…".to_owned(),
        None => format!("Downloading {blobs} files from Azure DevOps…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn the_file_list_indicator_waits_out_the_grace() {
        let t0 = Instant::now();
        let mut list = ListLoad::default();
        assert!(!list.shows_indicator(t0 + at(1_000)), "idle shows nothing");
        list.start(t0);
        assert!(list.is_loading());
        assert!(
            !list.shows_indicator(t0 + at(149)),
            "no flash on fast loads"
        );
        assert!(list.shows_indicator(t0 + GRACE));
        list.finish(Ok(()));
        assert_eq!(list, ListLoad::Idle);
        assert!(!list.shows_indicator(t0 + at(1_000)));
    }

    #[test]
    fn a_failed_file_list_can_be_retried() {
        let t0 = Instant::now();
        let mut list = ListLoad::default();
        list.start(t0);
        list.finish(Err("offline".into()));
        assert_eq!(list.failure(), Some("offline"));
        assert!(
            !list.shows_indicator(t0 + at(1_000)),
            "failure isn't loading"
        );
        let retry = t0 + at(5_000);
        list.start(retry);
        assert_eq!(list.failure(), None);
        assert!(!list.shows_indicator(retry + at(10)), "the grace restarts");
        assert!(list.shows_indicator(retry + GRACE));
        list.finish(Ok(()));
        assert_eq!(list, ListLoad::Idle);
    }

    #[test]
    fn the_diff_indicator_shows_after_the_grace_and_explains_slow_loads() {
        let t0 = Instant::now();
        let load = FileLoad {
            since: t0,
            phase: Phase::Fetching { blobs: 2 },
        };
        let show = |ms, offline, hold| indicator(&load, t0 + at(ms), offline, hold);
        assert_eq!(show(100, false, None), None);
        assert_eq!(show(150, false, None), Some(Indicator { detail: None }));
        assert_eq!(show(2_999, false, None), Some(Indicator { detail: None }));
        let detail = |ms, offline, hold| show(ms, offline, hold).and_then(|i| i.detail);
        assert_eq!(
            detail(3_000, false, None).as_deref(),
            Some("Downloading 2 files from Azure DevOps…")
        );
        assert_eq!(
            detail(3_000, false, Some(Hold::Busy)).as_deref(),
            Some("Waiting for Azure DevOps (sync in progress)…")
        );
        assert_eq!(
            detail(3_000, false, Some(Hold::Budget { until: 0 })).as_deref(),
            Some("Waiting for Azure DevOps (sync in progress)…")
        );
        assert_eq!(
            detail(3_000, false, Some(Hold::Paused { until: 0 })).as_deref(),
            Some("Azure DevOps asked adoken to slow down. Waiting…")
        );
        assert_eq!(
            detail(3_000, true, Some(Hold::Busy)).as_deref(),
            Some("Offline. The file loads when Azure DevOps is reachable again."),
            "offline outranks the governor"
        );
        let one = FileLoad {
            since: t0,
            phase: Phase::Fetching { blobs: 1 },
        };
        assert_eq!(
            indicator(&one, t0 + SLOW, false, None)
                .and_then(|i| i.detail)
                .as_deref(),
            Some("Downloading 1 file from Azure DevOps…")
        );
        let computing = FileLoad {
            since: t0,
            phase: Phase::Computing,
        };
        assert_eq!(
            indicator(&computing, t0 + SLOW, true, Some(Hold::Busy))
                .and_then(|i| i.detail)
                .as_deref(),
            Some("Still computing the diff…"),
            "a local diff doesn't wait on the network"
        );
    }
}
