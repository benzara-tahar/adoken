//! What the user controls in Settings › Sync (spec 0002 FR-4a, user decision
//! 2026-09-30) and how it maps onto the tiers.
//!
//! The tiers stay (they set governor priority, FR-7), but each tier's work
//! is split into [`Lane`]s with their own due times, so every user-facing
//! [`Kind`] keeps its own cadence:
//!
//! | Lane | Tier | Work | Cadence |
//! | --- | --- | --- | --- |
//! | `MyPrs` | hot | hot-set threads, file lists after a push | Pull requests — mine |
//! | `LiveRuns` | hot | my in-progress runs, their stages | Pipeline runs |
//! | `AllPrs` | warm | active PR lists per project | Pull requests — all active |
//! | `WorkItems` | warm | my work items delta, discussions | Work items |
//! | `DoneRuns` | warm | finished runs, their stages | Pipeline runs, at least 2 min |
//! | `Index` | cold | projects, repos, search title index | fixed 30 min (60 min in background) |
//!
//! Pure data and math; the [`crate::scheduler`] applies it and keeps the
//! governor's budget, pressure and backoff on top (the governor always
//! wins).

use adoken_model::Millis;
use adoken_model::time::{MINUTE, SECOND};

use crate::Tier;
use crate::governor::{MAX_COST_PER_WINDOW, WINDOW_MS};

/// What Settings › Sync shows one interval choice for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// PRs I wrote or review (the hot set): comments, votes, pushes.
    MyPrs,
    /// Every active PR in the selected projects (list level).
    AllPrs,
    WorkItems,
    Pipelines,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::MyPrs, Kind::AllPrs, Kind::WorkItems, Kind::Pipelines];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Shortest allowed interval. Lanes that query every selected project
    /// per pass (PR lists, work items) get a longer floor than the ones
    /// that only touch my own PRs and runs, so no choice can push sync past
    /// the FR-9 budget on its own.
    pub fn minimum(self) -> Millis {
        match self {
            Kind::MyPrs | Kind::Pipelines => 30 * SECOND,
            Kind::AllPrs | Kind::WorkItems => MINUTE,
        }
    }

    /// Spec 0002 FR-4 cadence before Settings existed.
    pub fn default_cadence(self) -> Cadence {
        match self {
            Kind::MyPrs | Kind::Pipelines => Cadence::Every(MINUTE),
            Kind::AllPrs | Kind::WorkItems => Cadence::Every(2 * MINUTE),
        }
    }
}

/// How often one kind syncs in the foreground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cadence {
    Every(Millis),
    /// Only on Sync now (⌘R).
    Manual,
}

/// The choices Settings offers, fastest first.
pub const CHOICES: [Cadence; 7] = [
    Cadence::Every(30 * SECOND),
    Cadence::Every(MINUTE),
    Cadence::Every(2 * MINUTE),
    Cadence::Every(5 * MINUTE),
    Cadence::Every(15 * MINUTE),
    Cadence::Every(30 * MINUTE),
    Cadence::Manual,
];

impl Cadence {
    pub fn every(self) -> Option<Millis> {
        match self {
            Cadence::Every(ms) => Some(ms),
            Cadence::Manual => None,
        }
    }

    /// Never faster than `kind`'s minimum.
    pub fn clamped(self, kind: Kind) -> Cadence {
        match self {
            Cadence::Every(ms) => Cadence::Every(ms.max(kind.minimum())),
            Cadence::Manual => Cadence::Manual,
        }
    }

    /// The choices allowed for `kind` (at or above its minimum).
    pub fn choices(kind: Kind) -> impl Iterator<Item = Cadence> {
        CHOICES
            .into_iter()
            .filter(move |c| c.every().is_none_or(|ms| ms >= kind.minimum()))
    }

    /// The allowed choice closest to `ms` (ties go slower).
    pub fn nearest(kind: Kind, ms: Millis) -> Cadence {
        Cadence::choices(kind)
            .filter_map(Cadence::every)
            .min_by_key(|c| ((c - ms).abs(), -c))
            .map_or(Cadence::Manual, Cadence::Every)
    }
}

/// When adoken is in the background (window not key and idle for 5 min).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Background {
    /// Same cadence as in the foreground.
    Same,
    /// Every interval × [`SLOWER_FACTOR`] (FR-4's background column).
    #[default]
    Slower,
    /// No scheduled passes until adoken is in the foreground again.
    Paused,
}

impl Background {
    pub const ALL: [Background; 3] = [Background::Same, Background::Slower, Background::Paused];
}

/// FR-4: background intervals were 5× the foreground ones (60 s → 5 min,
/// 2 min → 10 min); "Slower" keeps exactly that.
pub const SLOWER_FACTOR: i64 = 5;
/// Finished runs aren't checked more often than this (they matter for
/// `BuildCompleted` of PR runs, not for live progress).
pub const FINISHED_RUNS_MIN: Millis = 2 * MINUTE;
/// The cold lane is not user-adjustable.
pub const INDEX_FG: Millis = 30 * MINUTE;
pub const INDEX_BG: Millis = 60 * MINUTE;

/// Pause sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Pause {
    #[default]
    Off,
    /// Until this clock time (e.g. "for 1 hour").
    Until(Millis),
    UntilResumed,
}

impl Pause {
    pub fn active(self, now: Millis) -> bool {
        match self {
            Pause::Off => false,
            Pause::Until(at) => now < at,
            Pause::UntilResumed => true,
        }
    }

    /// When scheduled passes may run again; `None` while paused until
    /// resumed.
    pub fn resumes(self, now: Millis) -> Option<Millis> {
        match self {
            Pause::Off => Some(now),
            Pause::Until(at) => Some(at.max(now)),
            Pause::UntilResumed => None,
        }
    }
}

/// Settings › Sync, as the scheduler uses it. `Default` is FR-4's cadence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Prefs {
    pub my_prs: Cadence,
    pub all_prs: Cadence,
    pub work_items: Cadence,
    pub pipelines: Cadence,
    pub background: Background,
    pub pause: Pause,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            my_prs: Kind::MyPrs.default_cadence(),
            all_prs: Kind::AllPrs.default_cadence(),
            work_items: Kind::WorkItems.default_cadence(),
            pipelines: Kind::Pipelines.default_cadence(),
            background: Background::default(),
            pause: Pause::Off,
        }
    }
}

impl Prefs {
    pub fn cadence(&self, kind: Kind) -> Cadence {
        match kind {
            Kind::MyPrs => self.my_prs,
            Kind::AllPrs => self.all_prs,
            Kind::WorkItems => self.work_items,
            Kind::Pipelines => self.pipelines,
        }
    }

    /// Set `kind`'s cadence, clamped to its minimum.
    pub fn set_cadence(&mut self, kind: Kind, cadence: Cadence) {
        let cadence = cadence.clamped(kind);
        match kind {
            Kind::MyPrs => self.my_prs = cadence,
            Kind::AllPrs => self.all_prs = cadence,
            Kind::WorkItems => self.work_items = cadence,
            Kind::Pipelines => self.pipelines = cadence,
        }
    }

    /// Every cadence at or above its minimum (for values built by hand).
    pub fn clamped(mut self) -> Prefs {
        for kind in Kind::ALL {
            self.set_cadence(kind, self.cadence(kind));
        }
        self
    }

    /// Interval of `lane` before rate pressure; `None` = no scheduled
    /// passes (manual only, or paused in the background).
    pub fn interval(&self, lane: Lane, foreground: bool) -> Option<Millis> {
        let fg = match lane {
            Lane::MyPrs => self.my_prs.clamped(Kind::MyPrs).every(),
            Lane::LiveRuns => self.pipelines.clamped(Kind::Pipelines).every(),
            Lane::AllPrs => self.all_prs.clamped(Kind::AllPrs).every(),
            Lane::WorkItems => self.work_items.clamped(Kind::WorkItems).every(),
            Lane::DoneRuns => self
                .pipelines
                .clamped(Kind::Pipelines)
                .every()
                .map(|ms| ms.max(FINISHED_RUNS_MIN)),
            Lane::Index => Some(INDEX_FG),
        }?;
        if foreground {
            return Some(fg);
        }
        match (self.background, lane) {
            (Background::Same, _) => Some(fg),
            (Background::Slower, Lane::Index) => Some(INDEX_BG),
            (Background::Slower, _) => Some(fg * SLOWER_FACTOR),
            (Background::Paused, _) => None,
        }
    }
}

/// A slice of one tier with its own due time (see the module table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lane {
    MyPrs,
    LiveRuns,
    AllPrs,
    WorkItems,
    DoneRuns,
    Index,
}

impl Lane {
    pub const ALL: [Lane; 6] = [
        Lane::MyPrs,
        Lane::LiveRuns,
        Lane::AllPrs,
        Lane::WorkItems,
        Lane::DoneRuns,
        Lane::Index,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Governor priority of the lane's requests.
    pub fn tier(self) -> Tier {
        match self {
            Lane::MyPrs | Lane::LiveRuns => Tier::Hot,
            Lane::AllPrs | Lane::WorkItems | Lane::DoneRuns => Tier::Warm,
            Lane::Index => Tier::Cold,
        }
    }

    /// The Settings row that sets this lane's cadence (`None`: fixed).
    pub fn kind(self) -> Option<Kind> {
        match self {
            Lane::MyPrs => Some(Kind::MyPrs),
            Lane::AllPrs => Some(Kind::AllPrs),
            Lane::WorkItems => Some(Kind::WorkItems),
            Lane::LiveRuns | Lane::DoneRuns => Some(Kind::Pipelines),
            Lane::Index => None,
        }
    }

    /// TSTU of one pass before a pass has been measured: the 2026-09-25
    /// spike (spec 0002 §13, one small org): ~0.004 per list call, a warm
    /// pass 0.02 over its three lanes.
    pub fn prior_cost(self) -> f64 {
        match self {
            Lane::MyPrs | Lane::LiveRuns | Lane::AllPrs | Lane::Index => 0.004,
            Lane::WorkItems | Lane::DoneRuns => 0.008,
        }
    }
}

/// A set of lanes (one pass may run several lanes of the same tier).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lanes(u8);

impl Lanes {
    pub const NONE: Lanes = Lanes(0);

    pub fn of(tier: Tier) -> Lanes {
        Lane::ALL
            .into_iter()
            .filter(|lane| lane.tier() == tier || (tier == Tier::User && lane.tier() == Tier::Hot))
            .collect()
    }

    pub fn contains(self, lane: Lane) -> bool {
        self.0 & (1 << lane.index()) != 0
    }

    pub fn insert(&mut self, lane: Lane) {
        self.0 |= 1 << lane.index();
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Lane> {
        Lane::ALL
            .into_iter()
            .filter(move |lane| self.contains(*lane))
    }
}

impl FromIterator<Lane> for Lanes {
    fn from_iter<I: IntoIterator<Item = Lane>>(iter: I) -> Self {
        let mut lanes = Lanes::NONE;
        for lane in iter {
            lanes.insert(lane);
        }
        lanes
    }
}

/// Azure DevOps' per-user limit: 200 TSTU per sliding 5 minutes.
pub const RATE_LIMIT_TSTU: f64 = 200.0;
/// FR-9 steady-state target per 5 minutes.
pub const STEADY_TARGET_TSTU: f64 = 10.0;

/// TSTU per 5 minutes `prefs` would spend at the measured per-pass costs
/// (`pass_cost[lane]`, else the lane's prior). Excludes rate pressure,
/// which only ever slows sync down.
pub fn estimate_tstu(prefs: &Prefs, foreground: bool, pass_cost: &[Option<f64>; 6]) -> f64 {
    Lane::ALL
        .into_iter()
        .filter_map(|lane| {
            let interval = prefs.interval(lane, foreground)?;
            let cost = pass_cost[lane.index()].unwrap_or(lane.prior_cost());
            Some(cost * WINDOW_MS as f64 / interval as f64)
        })
        .sum::<f64>()
        // The governor never lets sync past its hard self-limit.
        .min(MAX_COST_PER_WINDOW)
}

/// Share of Azure DevOps' rate limit, in percent.
pub fn percent_of_limit(tstu: f64) -> f64 {
    tstu / RATE_LIMIT_TSTU * 100.0
}

/// What the scheduler reports to Settings › Sync.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Activity {
    /// Last successful pass per lane.
    pub last_ok: [Option<Millis>; 6],
    /// Smoothed TSTU per pass per lane, once measured.
    pub pass_cost: [Option<f64>; 6],
    /// Measured TSTU over the last 5 minutes (every request, including
    /// yours), when last reported.
    pub window_cost: f64,
    pub foreground: bool,
}

impl Activity {
    /// Latest successful pass of any lane `kind` sets.
    pub fn last_ok(&self, kind: Kind) -> Option<Millis> {
        Lane::ALL
            .into_iter()
            .filter(|lane| lane.kind() == Some(kind))
            .filter_map(|lane| self.last_ok[lane.index()])
            .max()
    }

    pub fn estimate_tstu(&self, prefs: &Prefs) -> f64 {
        estimate_tstu(prefs, self.foreground, &self.pass_cost)
    }
}

/// Weight of a new pass cost in the smoothed value.
const COST_WEIGHT: f64 = 0.3;

/// Fold one measured pass cost into the smoothed value.
pub fn smooth(previous: Option<f64>, sample: f64) -> f64 {
    match previous {
        None => sample,
        Some(p) => p + COST_WEIGHT * (sample - p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [Option<f64>; 6] = [None; 6];

    #[test]
    fn defaults_are_fr4_cadence() {
        let p = Prefs::default();
        let fg = |lane| p.interval(lane, true);
        let bg = |lane| p.interval(lane, false);
        assert_eq!(fg(Lane::MyPrs), Some(MINUTE));
        assert_eq!(fg(Lane::LiveRuns), Some(MINUTE));
        assert_eq!(fg(Lane::AllPrs), Some(2 * MINUTE));
        assert_eq!(fg(Lane::WorkItems), Some(2 * MINUTE));
        assert_eq!(fg(Lane::DoneRuns), Some(2 * MINUTE));
        assert_eq!(fg(Lane::Index), Some(30 * MINUTE));
        assert_eq!(bg(Lane::MyPrs), Some(5 * MINUTE));
        assert_eq!(bg(Lane::AllPrs), Some(10 * MINUTE));
        assert_eq!(bg(Lane::Index), Some(60 * MINUTE));
    }

    #[test]
    fn background_same_and_paused() {
        let mut p = Prefs {
            background: Background::Same,
            ..Prefs::default()
        };
        assert_eq!(p.interval(Lane::AllPrs, false), Some(2 * MINUTE));
        assert_eq!(p.interval(Lane::Index, false), Some(30 * MINUTE));
        p.background = Background::Paused;
        assert!(Lane::ALL.iter().all(|l| p.interval(*l, false).is_none()));
        assert_eq!(p.interval(Lane::MyPrs, true), Some(MINUTE));
    }

    #[test]
    fn minimums_hold_whatever_is_set() {
        let mut p = Prefs::default();
        p.set_cadence(Kind::AllPrs, Cadence::Every(5 * SECOND));
        assert_eq!(p.all_prs, Cadence::Every(MINUTE));
        // Built by hand past the setter: intervals still clamp.
        let raw = Prefs {
            work_items: Cadence::Every(SECOND),
            my_prs: Cadence::Every(SECOND),
            ..Prefs::default()
        };
        assert_eq!(raw.interval(Lane::WorkItems, true), Some(MINUTE));
        assert_eq!(raw.interval(Lane::MyPrs, true), Some(30 * SECOND));
        assert_eq!(raw.clamped().my_prs, Cadence::Every(30 * SECOND));
    }

    #[test]
    fn pipelines_split_live_and_finished_runs() {
        let mut p = Prefs {
            pipelines: Cadence::Every(30 * SECOND),
            ..Prefs::default()
        };
        assert_eq!(p.interval(Lane::LiveRuns, true), Some(30 * SECOND));
        assert_eq!(p.interval(Lane::DoneRuns, true), Some(2 * MINUTE));
        p.pipelines = Cadence::Every(15 * MINUTE);
        assert_eq!(p.interval(Lane::DoneRuns, true), Some(15 * MINUTE));
        p.pipelines = Cadence::Manual;
        assert_eq!(p.interval(Lane::LiveRuns, true), None);
        assert_eq!(p.interval(Lane::DoneRuns, true), None);
    }

    #[test]
    fn choices_respect_minimums_and_nearest_snaps() {
        assert_eq!(Cadence::choices(Kind::MyPrs).count(), 7);
        assert_eq!(
            Cadence::choices(Kind::AllPrs).next(),
            Some(Cadence::Every(MINUTE))
        );
        assert_eq!(
            Cadence::nearest(Kind::AllPrs, 10 * SECOND),
            Cadence::Every(MINUTE)
        );
        assert_eq!(
            Cadence::nearest(Kind::MyPrs, 3 * MINUTE),
            Cadence::Every(2 * MINUTE)
        );
        // 90 s is as far from 1 min as from 2 min: the slower one wins.
        assert_eq!(
            Cadence::nearest(Kind::MyPrs, 90 * SECOND),
            Cadence::Every(2 * MINUTE)
        );
        assert_eq!(
            Cadence::nearest(Kind::MyPrs, 4 * 60 * MINUTE),
            Cadence::Every(30 * MINUTE)
        );
    }

    #[test]
    fn pause_windows() {
        assert!(!Pause::Off.active(10));
        assert!(Pause::Until(20).active(10));
        assert!(!Pause::Until(20).active(20));
        assert!(Pause::UntilResumed.active(i64::MAX));
        assert_eq!(Pause::Until(20).resumes(10), Some(20));
        assert_eq!(Pause::UntilResumed.resumes(10), None);
    }

    #[test]
    fn estimate_scales_with_passes_per_window() {
        let p = Prefs::default();
        let mut cost = NONE;
        cost[Lane::MyPrs.index()] = Some(0.5);
        cost[Lane::LiveRuns.index()] = Some(0.1);
        cost[Lane::AllPrs.index()] = Some(0.2);
        cost[Lane::WorkItems.index()] = Some(0.2);
        cost[Lane::DoneRuns.index()] = Some(0.2);
        cost[Lane::Index.index()] = Some(3.0);
        // 5 min window: hot ×5, warm ×2.5, cold ×1/6.
        let expected = (0.5 + 0.1) * 5.0 + (0.2 * 3.0) * 2.5 + 3.0 / 6.0;
        let fg = estimate_tstu(&p, true, &cost);
        assert!((fg - expected).abs() < 1e-9, "{fg} vs {expected}");
        assert!((percent_of_limit(fg) - expected / 2.0).abs() < 1e-9);
        // Background slower: ×5 fewer passes, index ×2 fewer.
        let bg = estimate_tstu(&p, false, &cost);
        let expected_bg = (0.5 + 0.1) + (0.2 * 3.0) * 0.5 + 3.0 / 12.0;
        assert!((bg - expected_bg).abs() < 1e-9, "{bg} vs {expected_bg}");
        // Manual only drops the lane.
        let manual = Prefs {
            my_prs: Cadence::Manual,
            ..p
        };
        assert!((estimate_tstu(&manual, true, &cost) - (expected - 2.5)).abs() < 1e-9);
    }

    #[test]
    fn estimate_uses_priors_and_caps_at_the_governor_limit() {
        let p = Prefs::default();
        let prior = estimate_tstu(&p, true, &NONE);
        let expected = (0.004 + 0.004) * 5.0 + (0.004 + 0.008 + 0.008) * 2.5 + 0.004 / 6.0;
        assert!((prior - expected).abs() < 1e-9);
        let huge = [Some(100.0); 6];
        assert_eq!(estimate_tstu(&p, true, &huge), MAX_COST_PER_WINDOW);
    }

    #[test]
    fn smoothing_moves_toward_new_samples() {
        assert_eq!(smooth(None, 2.0), 2.0);
        assert!((smooth(Some(1.0), 2.0) - 1.3).abs() < 1e-9);
    }

    #[test]
    fn lanes_by_tier() {
        let hot: Vec<Lane> = Lanes::of(Tier::Hot).iter().collect();
        assert_eq!(hot, [Lane::MyPrs, Lane::LiveRuns]);
        assert_eq!(Lanes::of(Tier::User), Lanes::of(Tier::Hot));
        assert_eq!(
            Lanes::of(Tier::Cold).iter().collect::<Vec<_>>(),
            [Lane::Index]
        );
        let activity = Activity {
            last_ok: [None, Some(5), None, None, Some(9), None],
            ..Default::default()
        };
        assert_eq!(activity.last_ok(Kind::Pipelines), Some(9));
        assert_eq!(activity.last_ok(Kind::MyPrs), None);
    }
}
