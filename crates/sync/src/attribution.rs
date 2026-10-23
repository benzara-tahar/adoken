//! Who caused a request (spec 0016 FR-18). The origin is set where the work
//! starts (a scheduler pass, a lane step, a user action, the avatar
//! fetcher) and inherited by every request sent inside it: the governor
//! reads the task-local [`Attribution`] and stamps each attempt
//! ([`crate::transport::RequestMeta`]), so a new call site can't forget it.
//! A request the recorder sees without an origin is a bug: debug builds
//! assert, release builds log it as "Unattributed".

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use adoken_model::Millis;

use crate::cadence::Lane;

/// A user action that sends requests (0004 writes and reads, 0015 work item
/// edits and per-item sync).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UserAction {
    OpenPr,
    Vote,
    Comment,
    Reply,
    Resolve,
    StatusChange,
    Complete,
    AutoComplete,
    LinkWorkItem,
    UnlinkWorkItem,
    LoadOlderRuns,
    ChangeState,
    EditTitle,
    Assign,
    ChangeArea,
    ChangeIteration,
    EditDescription,
    AddLink,
    RemoveLink,
    ChangeParent,
    /// "Sync this item" (spec 0015 FR-30/31).
    SyncItem(i64),
    /// The work item list's area / iteration dropdown reading a project's
    /// classification tree (spec 0008 FR-8).
    FilterByArea,
    FilterByIteration,
}

impl UserAction {
    /// Lowercase verb phrase after "You: ".
    pub fn label(self) -> String {
        let s = match self {
            UserAction::OpenPr => "open PR",
            UserAction::Vote => "vote",
            UserAction::Comment => "comment",
            UserAction::Reply => "reply",
            UserAction::Resolve => "resolve",
            UserAction::StatusChange => "status change",
            UserAction::Complete => "complete",
            UserAction::AutoComplete => "auto-complete",
            UserAction::LinkWorkItem => "link work item",
            UserAction::UnlinkWorkItem => "unlink work item",
            UserAction::LoadOlderRuns => "load older runs",
            UserAction::ChangeState => "change state",
            UserAction::EditTitle => "edit title",
            UserAction::Assign => "assign",
            UserAction::ChangeArea => "change area",
            UserAction::ChangeIteration => "change iteration",
            UserAction::EditDescription => "edit description",
            UserAction::AddLink => "add link",
            UserAction::RemoveLink => "remove link",
            UserAction::ChangeParent => "change parent",
            UserAction::SyncItem(id) => return format!("Sync #{id}"),
            UserAction::FilterByArea => "filter by area",
            UserAction::FilterByIteration => "filter by iteration",
        };
        s.to_owned()
    }
}

/// Who caused a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// A sync pass's lane; `pinned` for a pinned-refresh step (0014 FR-11),
    /// `sync_now` when the pass came from ⌘R.
    Sync {
        lane: Lane,
        pinned: bool,
        sync_now: bool,
    },
    You(UserAction),
    /// The re-read after a write (0002 FR-15, 0004 FR-10).
    YouReread(UserAction),
    /// Profile pictures (0012).
    Avatars,
    /// Connection data, identity and projects before the first pass.
    Startup,
    /// A bug (FR-18): nothing set an origin.
    Unattributed,
}

/// Coarse origin for filters and totals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OriginGroup {
    Sync,
    You,
    Avatars,
    Startup,
    Unattributed,
}

impl OriginGroup {
    pub fn label(self) -> &'static str {
        match self {
            OriginGroup::Sync => "Sync",
            OriginGroup::You => "You",
            OriginGroup::Avatars => "Avatars",
            OriginGroup::Startup => "Startup",
            OriginGroup::Unattributed => "Unattributed",
        }
    }
}

/// The Activity row name of a lane (spec 0016 FR-7).
pub fn lane_label(lane: Lane) -> &'static str {
    match lane {
        Lane::MyPrs => "My pull requests",
        Lane::AllPrs => "All active pull requests",
        Lane::WorkItems => "Work items",
        Lane::LiveRuns | Lane::DoneRuns => "Pipeline runs",
        Lane::Index => "Search index and projects",
    }
}

impl Origin {
    /// "My pull requests", "Work items · pinned", "All active pull requests
    /// · Sync now", "You: vote", "You: vote · re-read", "Avatars",
    /// "Startup". Never contains a name or an id other than the user's own
    /// per-item sync ("You: Sync #20101").
    pub fn label(&self) -> String {
        match self {
            Origin::Sync {
                lane,
                pinned,
                sync_now,
            } => {
                let mut s = lane_label(*lane).to_owned();
                if *pinned {
                    s.push_str(" · pinned");
                }
                if *sync_now {
                    s.push_str(" · Sync now");
                }
                s
            }
            Origin::You(action) => format!("You: {}", action.label()),
            Origin::YouReread(action) => format!("You: {} · re-read", action.label()),
            Origin::Avatars => "Avatars".into(),
            Origin::Startup => "Startup".into(),
            Origin::Unattributed => "Unattributed".into(),
        }
    }

    pub fn group(&self) -> OriginGroup {
        match self {
            Origin::Sync { .. } => OriginGroup::Sync,
            Origin::You(_) | Origin::YouReread(_) => OriginGroup::You,
            Origin::Avatars => OriginGroup::Avatars,
            Origin::Startup => OriginGroup::Startup,
            Origin::Unattributed => OriginGroup::Unattributed,
        }
    }

    /// The user caused it (a user action, its re-read, or ⌘R): failures
    /// light the statusbar even on optional transports (FR-21, Q-10).
    pub fn user_caused(&self) -> bool {
        match self {
            Origin::You(_) | Origin::YouReread(_) => true,
            Origin::Sync { sync_now, .. } => *sync_now,
            _ => false,
        }
    }
}

/// The pass a request belongs to (spec 0016 FR-11), as entries carry it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PassRef {
    pub id: u64,
    pub started: Millis,
}

/// Requests and TSTU booked by a pass so far; the governor counts into it.
#[derive(Debug, Default)]
pub struct PassCounters {
    requests: AtomicU32,
    /// TSTU × 1e6.
    micro_tstu: AtomicU64,
}

impl PassCounters {
    pub fn add(&self, tstu: f64) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        self.micro_tstu
            .fetch_add((tstu.max(0.0) * 1e6).round() as u64, Ordering::Relaxed);
    }

    pub fn requests(&self) -> u32 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn tstu(&self) -> f64 {
        self.micro_tstu.load(Ordering::Relaxed) as f64 / 1e6
    }
}

/// A pass and its live counters.
#[derive(Clone, Debug)]
pub struct PassLink {
    pub pass: PassRef,
    pub counters: Arc<PassCounters>,
}

/// What a running piece of work stamps on its requests.
#[derive(Clone, Debug)]
pub struct Attribution {
    pub origin: Origin,
    pub pass: Option<PassLink>,
}

impl Attribution {
    pub fn new(origin: Origin) -> Self {
        Attribution { origin, pass: None }
    }
}

tokio::task_local! {
    static CURRENT: Attribution;
}

/// The attribution of the running task, if any scope set one.
pub fn current() -> Option<Attribution> {
    CURRENT.try_with(Clone::clone).ok()
}

/// Run `fut` with `attr`: every request it sends carries it. `fut` is
/// boxed right away: scopes nest around large pass futures, which would
/// otherwise grow every enclosing future (and the stack) by their size.
pub fn scope<'a, F: Future + 'a>(
    attr: Attribution,
    fut: F,
) -> impl Future<Output = F::Output> + 'a {
    CURRENT.scope(attr, Box::pin(fut))
}

/// Run `fut` with `origin`, keeping the current pass (a lane step inside
/// a pass, a write's metadata fetch inside its action).
pub fn with_origin<'a, F: Future + 'a>(
    origin: Origin,
    fut: F,
) -> impl Future<Output = F::Output> + 'a {
    let pass = current().and_then(|a| a.pass);
    scope(Attribution { origin, pass }, fut)
}

/// Run `fut` with `origin` unless an enclosing scope already set one (the
/// engine's hot-set file lists vs. the review opening a PR).
pub fn or_origin<'a, F: Future + 'a>(
    origin: Origin,
    fut: F,
) -> impl Future<Output = F::Output> + 'a {
    scope(current().unwrap_or_else(|| Attribution::new(origin)), fut)
}
