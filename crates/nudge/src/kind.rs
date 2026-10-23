//! Nudge types N1–N7 (spec 0006 FR-1), their priority (FR-4), thresholds and
//! per-type settings (§3 "Settings → Nudges").

use adoken_model::Millis;

use crate::hours::WorkingHours;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NudgeKind {
    /// N1: I'm a reviewer with no vote on an active, non-draft PR.
    ReviewWaiting,
    /// N2: someone voted wait-for-author or reject on my PR; I haven't pushed or replied since.
    WaitingForAuthor,
    /// N3: the latest build on my PR failed and nobody has pushed since.
    BuildFailing,
    /// N4: my PR has no votes and no human comments from others.
    NoReviewerActivity,
    /// N5: my PR has merge conflicts.
    MergeConflicts,
    /// N6: my in-progress work item's linked PR merged a while ago.
    MergedStillActive,
    /// N7: my in-progress work item hasn't changed in a long time.
    QuietActiveItem,
}

/// Which sync tier (spec 0002 FR-4) a nudge's inputs come from. A nudge is
/// suppressed while any tier it depends on is stale (spec 0006 AC-8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Hot,
    Warm,
}

/// How long a condition must hold before the nudge fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Threshold {
    Immediate,
    WorkingHours(u32),
    WorkingDays(u32),
}

impl Threshold {
    /// The threshold as working time under `hours`.
    pub fn working_ms(self, hours: &WorkingHours) -> Millis {
        match self {
            Threshold::Immediate => 0,
            Threshold::WorkingHours(h) => Millis::from(h) * adoken_model::time::HOUR,
            Threshold::WorkingDays(d) => Millis::from(d) * hours.day_length(),
        }
    }
}

impl NudgeKind {
    /// Declaration order = N1…N7.
    pub const ALL: [NudgeKind; 7] = [
        NudgeKind::ReviewWaiting,
        NudgeKind::WaitingForAuthor,
        NudgeKind::BuildFailing,
        NudgeKind::NoReviewerActivity,
        NudgeKind::MergeConflicts,
        NudgeKind::MergedStillActive,
        NudgeKind::QuietActiveItem,
    ];

    /// FR-4 firing priority: N5 > N3 > N2 > N1 > N6 > N4 > N7.
    pub const PRIORITY: [NudgeKind; 7] = [
        NudgeKind::MergeConflicts,
        NudgeKind::BuildFailing,
        NudgeKind::WaitingForAuthor,
        NudgeKind::ReviewWaiting,
        NudgeKind::MergedStillActive,
        NudgeKind::NoReviewerActivity,
        NudgeKind::QuietActiveItem,
    ];

    fn index(self) -> usize {
        self as usize
    }

    /// 0 = fires first.
    pub fn rank(self) -> usize {
        Self::PRIORITY
            .iter()
            .position(|k| *k == self)
            .expect("every kind is ranked")
    }

    /// Spec id, `N1` … `N7`; also the persisted form.
    pub fn id(self) -> &'static str {
        ["N1", "N2", "N3", "N4", "N5", "N6", "N7"][self.index()]
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// Settings label.
    pub fn label(self) -> &'static str {
        match self {
            NudgeKind::ReviewWaiting => "Review waiting on me",
            NudgeKind::WaitingForAuthor => "My PR: waiting for author",
            NudgeKind::BuildFailing => "My PR: build failing",
            NudgeKind::NoReviewerActivity => "My PR: no reviewer activity",
            NudgeKind::MergeConflicts => "My PR: merge conflicts",
            NudgeKind::MergedStillActive => "Work item: merged but still active",
            NudgeKind::QuietActiveItem => "Work item: quiet active item",
        }
    }

    pub fn default_threshold(self) -> Threshold {
        match self {
            NudgeKind::ReviewWaiting => Threshold::WorkingHours(24),
            NudgeKind::WaitingForAuthor => Threshold::WorkingHours(4),
            NudgeKind::BuildFailing => Threshold::WorkingHours(2),
            NudgeKind::NoReviewerActivity => Threshold::WorkingHours(24),
            NudgeKind::MergeConflicts => Threshold::Immediate,
            NudgeKind::MergedStillActive => Threshold::WorkingHours(24),
            NudgeKind::QuietActiveItem => Threshold::WorkingDays(5),
        }
    }

    /// Tiers the rule reads (spec 0002 FR-4): PR lists, my work items and my
    /// builds are warm; reviewer-added times and comments come from hot-set
    /// threads.
    pub fn tiers(self) -> &'static [Tier] {
        match self {
            NudgeKind::ReviewWaiting
            | NudgeKind::WaitingForAuthor
            | NudgeKind::BuildFailing
            | NudgeKind::NoReviewerActivity => &[Tier::Warm, Tier::Hot],
            NudgeKind::MergeConflicts
            | NudgeKind::MergedStillActive
            | NudgeKind::QuietActiveItem => &[Tier::Warm],
        }
    }

    pub fn is_pr_kind(self) -> bool {
        !matches!(
            self,
            NudgeKind::MergedStillActive | NudgeKind::QuietActiveItem
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindSettings {
    pub enabled: bool,
    pub threshold: Threshold,
}

/// Settings → Nudges.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub hours: WorkingHours,
    daily_cap: u8,
    kinds: [KindSettings; 7],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hours: WorkingHours::default(),
            daily_cap: Self::DEFAULT_CAP,
            kinds: NudgeKind::ALL.map(|kind| KindSettings {
                enabled: true,
                threshold: kind.default_threshold(),
            }),
        }
    }
}

impl Settings {
    pub const DEFAULT_CAP: u8 = 3;
    pub const MAX_CAP: u8 = 10;

    pub fn with_hours(mut self, hours: WorkingHours) -> Self {
        self.hours = hours;
        self
    }

    pub fn daily_cap(&self) -> u8 {
        self.daily_cap
    }

    /// Clamped to 0–10 (FR-4).
    pub fn set_daily_cap(&mut self, cap: u8) {
        self.daily_cap = cap.min(Self::MAX_CAP);
    }

    pub fn kind(&self, kind: NudgeKind) -> KindSettings {
        self.kinds[kind.index()]
    }

    /// "Turn off this nudge type" (FR-8) and Settings toggles.
    pub fn set_enabled(&mut self, kind: NudgeKind, enabled: bool) {
        self.kinds[kind.index()].enabled = enabled;
    }

    pub fn set_threshold(&mut self, kind: NudgeKind, threshold: Threshold) {
        self.kinds[kind.index()].threshold = threshold;
    }

    pub fn enabled(&self, kind: NudgeKind) -> bool {
        self.kind(kind).enabled
    }

    pub fn threshold_ms(&self, kind: NudgeKind) -> Millis {
        self.kind(kind).threshold.working_ms(&self.hours)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_priority_is_a_permutation() {
        for kind in NudgeKind::ALL {
            assert_eq!(NudgeKind::parse(kind.id()), Some(kind));
        }
        let mut ranks: Vec<_> = NudgeKind::ALL.iter().map(|k| k.rank()).collect();
        ranks.sort_unstable();
        assert_eq!(ranks, (0..7).collect::<Vec<_>>());
        assert_eq!(NudgeKind::MergeConflicts.rank(), 0);
        assert_eq!(NudgeKind::QuietActiveItem.rank(), 6);
    }

    #[test]
    fn daily_cap_is_clamped_to_ten() {
        let mut settings = Settings::default();
        assert_eq!(settings.daily_cap(), 3);
        settings.set_daily_cap(25);
        assert_eq!(settings.daily_cap(), 10);
        settings.set_daily_cap(0);
        assert_eq!(settings.daily_cap(), 0);
    }

    #[test]
    fn working_day_thresholds_scale_with_the_schedule() {
        let mut settings = Settings::default();
        let day = settings.hours.day_length();
        assert_eq!(settings.threshold_ms(NudgeKind::QuietActiveItem), 5 * day);
        settings.hours.end_min = 13 * 60;
        assert_eq!(
            settings.threshold_ms(NudgeKind::QuietActiveItem),
            5 * 4 * adoken_model::time::HOUR
        );
    }
}
