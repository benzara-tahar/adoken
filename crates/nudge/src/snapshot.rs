//! The local data one evaluation reads (spec 0006 FR-7: store only, zero ADO
//! requests). Built by [`crate::store`] from the store, or by hand in tests.

use std::collections::{HashMap, HashSet};

use adoken_model::time::MINUTE;
use adoken_model::{
    Millis, Person, PersonId, PipelineRun, PrIteration, PrStatus, PrThread, PullRequest,
    StateCategory, WorkItem,
};

use crate::kind::Tier;

/// Why Today can't see the latest (spec 0017 FR-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleReason {
    /// The app is offline (principles §3.5).
    Offline,
    /// The token was rejected, or a lane hasn't succeeded for too long.
    Delayed,
}

/// Data Today must not claim absence on (spec 0017 FR-8 row 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stale {
    pub reason: StaleReason,
    /// The older of the My pull requests and Work items lanes' last
    /// successes ("counts are from 14:02"); `None` when neither is known.
    pub since: Option<Millis>,
}

/// Sync freshness as reported by `crates/sync` (spec 0002 FR-4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataHealth {
    /// Hot and warm tiers have each completed once (0006 §3 "First sync
    /// incomplete": no nudges on partial data).
    pub initial_sync_done: bool,
    /// The hot tier is failing; nudges reading hot-set threads hold off.
    pub hot_stale: bool,
    /// The warm tier is failing; every nudge holds off (AC-8).
    pub warm_stale: bool,
    /// Spec 0017 FR-10: set while Today's data is stale.
    pub stale: Option<Stale>,
    /// The token can read work items (`vso.work`); without it Today hides
    /// the done count and Loose ends (spec 0017 §5).
    pub work_scope: bool,
}

impl DataHealth {
    pub const FRESH: DataHealth = DataHealth {
        initial_sync_done: true,
        hot_stale: false,
        warm_stale: false,
        stale: None,
        work_scope: true,
    };

    pub fn is_stale(&self, tier: Tier) -> bool {
        match tier {
            Tier::Hot => self.hot_stale,
            Tier::Warm => self.warm_stale,
        }
    }
}

/// One scheduler lane as Today's freshness reads it (spec 0016 FR-10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaneHealth {
    /// The lane's last successful pass.
    pub last_ok: Option<Millis>,
    /// Its interval in Settings › Sync; `None` while Manual only or paused
    /// by the user (never stale by age then).
    pub interval: Option<Millis>,
}

impl LaneHealth {
    /// No success for max(15 min, 2 × the lane's interval).
    pub fn is_late(&self, now: Millis) -> bool {
        match (self.last_ok, self.interval) {
            (Some(last), Some(interval)) => now - last > (15 * MINUTE).max(2 * interval),
            _ => false,
        }
    }
}

/// What the running sync says about Today's two lanes (spec 0017 FR-10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncHealth {
    pub online: bool,
    pub token_rejected: bool,
    pub my_prs: LaneHealth,
    pub work_items: LaneHealth,
    pub work_scope: bool,
}

impl Default for SyncHealth {
    /// Nothing known (demo, no sync running): fresh.
    fn default() -> Self {
        Self {
            online: true,
            token_rejected: false,
            my_prs: LaneHealth::default(),
            work_items: LaneHealth::default(),
            work_scope: true,
        }
    }
}

impl SyncHealth {
    /// FR-10: offline, a rejected token, or a lane late past its budget.
    pub fn stale(&self, now: Millis) -> Option<Stale> {
        let since = match (self.my_prs.last_ok, self.work_items.last_ok) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let reason = if !self.online {
            StaleReason::Offline
        } else if self.token_rejected || self.my_prs.is_late(now) || self.work_items.is_late(now) {
            StaleReason::Delayed
        } else {
            return None;
        };
        Some(Stale { reason, since })
    }

    /// The health a pass reads: the hot tier holds off while My pull
    /// requests is late, the warm tier while Work items is; both while
    /// offline or the token is rejected.
    pub fn data_health(&self, initial_sync_done: bool, now: Millis) -> DataHealth {
        let down = !self.online || self.token_rejected;
        DataHealth {
            initial_sync_done,
            hot_stale: down || self.my_prs.is_late(now),
            warm_stale: down || self.work_items.is_late(now),
            stale: self.stale(now),
            work_scope: self.work_scope,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub me: PersonId,
    /// Display names by identity id.
    pub names: HashMap<PersonId, String>,
    /// Identities that are groups or teams (`Person.is_group`).
    pub groups: HashSet<PersonId>,
    /// PRs I author or review, plus PRs linked from my work items (any status).
    pub prs: Vec<PullRequest>,
    /// Hot-set threads by PR id, plus my PRs merged today.
    pub threads: HashMap<i64, Vec<PrThread>>,
    /// Runs for the PRs above (`pr_id` set).
    pub runs: Vec<PipelineRun>,
    /// Work items assigned to me.
    pub work_items: Vec<WorkItem>,
    pub health: DataHealth,
    /// Today's context for the Today view (not read by the rules): PRs
    /// involving me that completed since the local day began (merges, and
    /// reviews I finished on PRs that merged since).
    pub merged_today: Vec<PullRequest>,
    /// Changed-file count per PR whose files have synced.
    pub file_counts: HashMap<i64, u32>,
    /// Pushes to my PRs (active or merged today) since the day began,
    /// oldest first (spec 0017 FR-4).
    pub iterations_today: HashMap<i64, Vec<PrIteration>>,
    /// My PRs whose stored iteration list predates their head: a push the
    /// commit counts haven't caught up with yet.
    pub iterations_behind: HashSet<i64>,
    /// My work items that reached Completed since the day began.
    pub done_today: Vec<i64>,
    /// When each of my in-progress work items entered its state (else its
    /// last change).
    pub state_since: HashMap<i64, Millis>,
}

/// One PR's stored detail for [`Snapshot::with_day_records`].
#[derive(Clone, Copy, Debug)]
pub struct PrRecords<'a> {
    pub iterations: &'a [PrIteration],
    pub threads: &'a [PrThread],
    pub files: u32,
}

impl Snapshot {
    pub fn empty(me: impl Into<PersonId>) -> Self {
        Self {
            me: me.into(),
            names: HashMap::new(),
            groups: HashSet::new(),
            prs: Vec::new(),
            threads: HashMap::new(),
            runs: Vec::new(),
            work_items: Vec::new(),
            health: DataHealth::FRESH,
            merged_today: Vec::new(),
            file_counts: HashMap::new(),
            iterations_today: HashMap::new(),
            iterations_behind: HashSet::new(),
            done_today: Vec::new(),
            state_since: HashMap::new(),
        }
    }

    /// The same selection [`crate::store::load_snapshot`] makes, over
    /// in-memory records (demo datasets, gallery stories, benches): active
    /// PRs involving me, PRs linked from my in-progress work items, hot
    /// threads for the former, and the latest run of each PR I authored.
    pub fn from_records<'a>(
        me: &str,
        people: impl IntoIterator<Item = &'a Person>,
        prs: &[PullRequest],
        threads: impl IntoIterator<Item = (i64, Vec<PrThread>)>,
        runs: &[PipelineRun],
        work_items: &[WorkItem],
    ) -> Self {
        let mut snap = Self::empty(me);
        for p in people {
            snap.names.insert(p.id.clone(), p.display_name.clone());
            if p.is_group {
                snap.groups.insert(p.id.clone());
            }
        }
        snap.work_items = work_items
            .iter()
            .filter(|wi| {
                wi.state_category == StateCategory::InProgress
                    && wi.assigned_to.as_deref() == Some(me)
            })
            .cloned()
            .collect();
        snap.state_since = snap
            .work_items
            .iter()
            .map(|wi| (wi.id, wi.changed))
            .collect();
        let involved = |pr: &PullRequest| {
            pr.status == PrStatus::Active && (pr.author == me || pr.is_reviewer(me))
        };
        let linked: HashSet<i64> = snap
            .work_items
            .iter()
            .flat_map(|wi| wi.linked_pr_ids.iter().copied())
            .collect();
        snap.prs = prs
            .iter()
            .filter(|pr| involved(pr) || linked.contains(&pr.id))
            .cloned()
            .collect();
        let hot: HashSet<i64> = snap
            .prs
            .iter()
            .filter(|pr| involved(pr))
            .map(|pr| pr.id)
            .collect();
        snap.threads = threads
            .into_iter()
            .filter(|(id, _)| hot.contains(id))
            .collect();
        let mine: HashSet<i64> = snap
            .prs
            .iter()
            .filter(|pr| pr.status == PrStatus::Active && pr.author == me)
            .map(|pr| pr.id)
            .collect();
        let mut latest: HashMap<i64, &PipelineRun> = HashMap::new();
        for run in runs {
            let Some(pr) = run.pr_id.filter(|id| mine.contains(id)) else {
                continue;
            };
            let slot = latest.entry(pr).or_insert(run);
            if (run.queued, run.id) > (slot.queued, slot.id) {
                *slot = run;
            }
        }
        snap.runs = latest.into_values().cloned().collect();
        snap.runs.sort_by_key(|r| r.id);
        snap
    }

    /// Today's context ([`Self::merged_today`], today's pushes, my done
    /// items …) over in-memory records, the same selection
    /// [`crate::store::load_snapshot`] makes for the local day that began
    /// at `day_start`. Call after [`Self::from_records`]. A work item's
    /// closure is its `changed` time (what the store records without a wire
    /// date); a PR is behind when its head isn't its latest iteration.
    pub fn with_day_records(
        mut self,
        day_start: Millis,
        prs: &[PullRequest],
        extras: &HashMap<i64, PrRecords<'_>>,
        work_items: &[WorkItem],
    ) -> Self {
        let me = self.me.clone();
        let me = me.as_str();
        self.merged_today = prs
            .iter()
            .filter(|pr| {
                pr.status == PrStatus::Completed
                    && pr.closed.is_some_and(|t| t >= day_start)
                    && (pr.author == me || pr.is_reviewer(me))
            })
            .cloned()
            .collect();
        let mine: Vec<&PullRequest> = self
            .prs
            .iter()
            .filter(|pr| pr.status == PrStatus::Active)
            .chain(&self.merged_today)
            .filter(|pr| pr.author == me)
            .collect();
        for pr in &mine {
            let Some(rec) = extras.get(&pr.id) else {
                continue;
            };
            if pr.status == PrStatus::Completed {
                self.threads.insert(pr.id, rec.threads.to_vec());
            }
            let today: Vec<PrIteration> = rec
                .iterations
                .iter()
                .filter(|it| it.created >= day_start)
                .cloned()
                .collect();
            if !today.is_empty() {
                self.iterations_today.insert(pr.id, today);
            }
            let head = rec.iterations.iter().max_by_key(|it| it.id);
            let behind = match (head, pr.last_source_commit.as_deref()) {
                (Some(it), Some(commit)) => !it.source_commit.eq_ignore_ascii_case(commit),
                (None, Some(_)) => true,
                (_, None) => false,
            };
            if behind {
                self.iterations_behind.insert(pr.id);
            }
        }
        self.done_today = work_items
            .iter()
            .filter(|wi| {
                wi.state_category == StateCategory::Completed
                    && wi.assigned_to.as_deref() == Some(me)
                    && wi.changed >= day_start
            })
            .map(|wi| wi.id)
            .collect();
        self.done_today.sort_unstable();
        let involved: HashSet<i64> = self.prs.iter().map(|pr| pr.id).collect();
        self.file_counts = extras
            .iter()
            .filter(|(id, rec)| rec.files > 0 && involved.contains(id))
            .map(|(id, rec)| (*id, rec.files))
            .collect();
        self
    }

    pub fn name<'a>(&'a self, id: &'a str) -> &'a str {
        self.names.get(id).map_or(id, String::as_str)
    }

    pub fn is_group(&self, id: &str) -> bool {
        self.groups.contains(id)
    }
}
