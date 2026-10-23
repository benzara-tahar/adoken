//! A pin's glyphs and tints (docs/design/pins.md §3.2–§3.5): the kind glyph
//! shape, the headline tint shared by the kind glyph and the trailing mark,
//! and the trailing slot. Pure over [`PinView`]; [`Tint`] names the theme
//! token so the mapping is testable without a theme. Shared by the sidebar,
//! the rail, the peek and the `⌘J` rows.

use adoken_model::{Millis, StateCategory, TypeFamily, WorkItemType};
use adoken_ui::{IconName, PinTrailing, Theme};
use gpui::{Hsla, SharedString};

use super::snapshot::PinView;
use super::status::{self, DefinitionHeadline, Headline, Kind, PrHeadline};

/// The theme token a glyph is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    PrActive,
    PrDraft,
    PrCompleted,
    PrAbandoned,
    Danger,
    Success,
    VoteRejected,
    VoteWaiting,
    VoteApproved,
    PipelineRunning,
    PipelineFailed,
    PipelinePartial,
    PipelineCanceled,
    PipelineSucceeded,
    Accent,
    Secondary,
    Tertiary,
}

impl Tint {
    pub fn color(self, theme: &Theme) -> Hsla {
        let c = &theme.colors;
        match self {
            Tint::PrActive => c.pr_active,
            Tint::PrDraft => c.pr_draft,
            Tint::PrCompleted => c.pr_completed,
            Tint::PrAbandoned => c.pr_abandoned,
            Tint::Danger => c.status_danger_fg,
            Tint::Success => c.status_success_fg,
            Tint::VoteRejected => c.vote_rejected,
            Tint::VoteWaiting => c.vote_waiting,
            Tint::VoteApproved => c.vote_approved,
            Tint::PipelineRunning => c.pipeline_running,
            Tint::PipelineFailed => c.pipeline_failed,
            Tint::PipelinePartial => c.pipeline_partial,
            Tint::PipelineCanceled => c.pipeline_canceled,
            Tint::PipelineSucceeded => c.pipeline_succeeded,
            Tint::Accent => c.accent_fg,
            Tint::Secondary => c.text_secondary,
            Tint::Tertiary => c.text_tertiary,
        }
    }
}

/// The trailing slot before colors are resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trailing {
    None,
    Glyph(IconName, Tint),
    Word(String),
    Running { tint: Tint, elapsed: String },
    NotAvailable,
}

impl Trailing {
    /// The `adoken_ui` slot; `phase` is the running glyph's spin phase.
    pub fn resolve(self, theme: &Theme, phase: f32) -> PinTrailing {
        match self {
            Trailing::None => PinTrailing::None,
            Trailing::Glyph(icon, tint) => PinTrailing::Glyph(icon, tint.color(theme)),
            Trailing::Word(word) => PinTrailing::Word(SharedString::from(word)),
            Trailing::Running { tint, elapsed } => PinTrailing::Running {
                tint: tint.color(theme),
                elapsed: elapsed.into(),
                phase,
            },
            Trailing::NotAvailable => PinTrailing::NotAvailable,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Trailing::Running { .. })
    }
}

/// Everything a row needs besides text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub glyph: IconName,
    pub tint: Tint,
    pub trailing: Trailing,
    /// Fetching row: the id before the title arrives (§3.5).
    pub fetching: Option<String>,
}

/// §3.2: the kind glyph's shape.
pub fn kind_glyph(kind: &Kind, headline: &Headline) -> IconName {
    match kind {
        Kind::Pr { draft } => match headline {
            Headline::Pr(PrHeadline::Completed) => IconName::GitMerge,
            Headline::Pr(PrHeadline::Abandoned) => IconName::GitPullRequestClosed,
            _ if *draft => IconName::GitPullRequestDraft,
            _ => IconName::GitPullRequest,
        },
        Kind::WorkItem(kind) => match kind.as_ref().map(WorkItemType::family) {
            Some(TypeFamily::Bug) => IconName::Bug,
            Some(TypeFamily::Task) => IconName::SquareCheck,
            Some(TypeFamily::Requirement) => IconName::BookOpen,
            Some(TypeFamily::Feature) => IconName::Trophy,
            Some(TypeFamily::Epic) => IconName::Crown,
            Some(TypeFamily::Issue) => IconName::CircleAlert,
            Some(TypeFamily::Other) | None => IconName::Square,
        },
        Kind::Definition => IconName::Workflow,
    }
}

/// §3.3: the headline's tint and trailing mark.
fn headline_look(headline: &Headline, now: Millis) -> (Tint, Trailing) {
    use Trailing::Glyph;
    match headline {
        Headline::Pr(h) => match h {
            PrHeadline::Completed => (Tint::PrCompleted, Trailing::None),
            PrHeadline::Abandoned => (Tint::PrAbandoned, Trailing::None),
            PrHeadline::MergeConflict => (Tint::Danger, Glyph(IconName::GitMerge, Tint::Danger)),
            PrHeadline::CiFailing => (
                Tint::PipelineFailed,
                Glyph(IconName::CircleX, Tint::PipelineFailed),
            ),
            PrHeadline::ChangesRequested { rejected: true } => (
                Tint::VoteRejected,
                Glyph(IconName::OctagonX, Tint::VoteRejected),
            ),
            PrHeadline::ChangesRequested { rejected: false } => {
                (Tint::VoteWaiting, Glyph(IconName::Clock, Tint::VoteWaiting))
            }
            PrHeadline::CiRunning => (
                Tint::PipelineRunning,
                Trailing::Running {
                    tint: Tint::PipelineRunning,
                    elapsed: String::new(),
                },
            ),
            PrHeadline::Approved => (
                Tint::VoteApproved,
                Glyph(IconName::BadgeCheck, Tint::VoteApproved),
            ),
            PrHeadline::Draft => (Tint::PrDraft, Trailing::None),
            PrHeadline::Active => (Tint::PrActive, Trailing::None),
        },
        Headline::WorkItem { state, category } => (
            match category {
                StateCategory::InProgress => Tint::Accent,
                StateCategory::Resolved => Tint::Success,
                StateCategory::Proposed => Tint::Secondary,
                StateCategory::Completed | StateCategory::Removed => Tint::Tertiary,
            },
            Trailing::Word(state.clone()),
        ),
        Headline::Definition(h) => match h {
            DefinitionHeadline::Running { since } => (
                Tint::PipelineRunning,
                Trailing::Running {
                    tint: Tint::PipelineRunning,
                    elapsed: status::elapsed(now - since),
                },
            ),
            DefinitionHeadline::Failed => (
                Tint::PipelineFailed,
                Glyph(IconName::CircleX, Tint::PipelineFailed),
            ),
            DefinitionHeadline::Partial => (
                Tint::PipelinePartial,
                Glyph(IconName::CircleAlert, Tint::PipelinePartial),
            ),
            DefinitionHeadline::Canceled => (
                Tint::PipelineCanceled,
                Glyph(IconName::CircleSlash, Tint::PipelineCanceled),
            ),
            DefinitionHeadline::Succeeded => (
                Tint::PipelineSucceeded,
                Glyph(IconName::CircleCheck, Tint::PipelineSucceeded),
            ),
            DefinitionHeadline::NoRuns => (
                Tint::Tertiary,
                Glyph(IconName::CircleDashed, Tint::Tertiary),
            ),
        },
        Headline::Fetching => (Tint::Tertiary, Trailing::None),
        Headline::NotAvailable(_) => (Tint::Tertiary, Trailing::NotAvailable),
    }
}

/// The row's look (§3.2–§3.5), stale rows recolored to `text.tertiary`
/// with their shapes kept (the work-item word is unchanged).
pub fn look(view: &PinView, now: Millis) -> Look {
    let glyph = kind_glyph(&view.kind, &view.headline);
    let (mut tint, mut trailing) = headline_look(&view.headline, now);
    if view.stale_since.is_some() {
        tint = Tint::Tertiary;
        trailing = match trailing {
            Trailing::Glyph(icon, _) => Trailing::Glyph(icon, Tint::Tertiary),
            Trailing::Running { elapsed, .. } => Trailing::Running {
                tint: Tint::Tertiary,
                elapsed,
            },
            other => other,
        };
    }
    let fetching = matches!(view.headline, Headline::Fetching)
        .then(|| view.subject().reference())
        .flatten();
    Look {
        glyph,
        tint,
        trailing,
        fetching,
    }
}

#[cfg(test)]
mod tests {
    use adoken_model::{Pin, PinSubject, PinUnavailable, time::MINUTE};

    use super::*;

    fn view(subject: PinSubject, kind: Kind, headline: Headline) -> PinView {
        let mut v = PinView::provisional(Pin {
            subject,
            position: 0,
            label: None,
            title: "t".into(),
            pinned_at: 0,
            opened_at: None,
            seen_result: None,
            unavailable: None,
        });
        v.kind = kind;
        v.headline = headline;
        v
    }

    fn pr(h: PrHeadline, draft: bool) -> Look {
        look(
            &view(PinSubject::Pr(1), Kind::Pr { draft }, Headline::Pr(h)),
            0,
        )
    }

    #[test]
    fn pr_rows_follow_the_design_table() {
        let completed = pr(PrHeadline::Completed, false);
        assert_eq!(
            (completed.glyph, completed.tint, completed.trailing),
            (IconName::GitMerge, Tint::PrCompleted, Trailing::None)
        );
        assert_eq!(
            pr(PrHeadline::Abandoned, false).glyph,
            IconName::GitPullRequestClosed
        );
        let conflict = pr(PrHeadline::MergeConflict, false);
        assert_eq!(
            (conflict.glyph, conflict.trailing),
            (
                IconName::GitPullRequest,
                Trailing::Glyph(IconName::GitMerge, Tint::Danger)
            )
        );
        // A draft keeps the draft shape whatever the headline.
        let failing_draft = pr(PrHeadline::CiFailing, true);
        assert_eq!(
            (failing_draft.glyph, failing_draft.tint),
            (IconName::GitPullRequestDraft, Tint::PipelineFailed)
        );
        assert_eq!(
            pr(PrHeadline::ChangesRequested { rejected: true }, false).trailing,
            Trailing::Glyph(IconName::OctagonX, Tint::VoteRejected)
        );
        assert_eq!(
            pr(PrHeadline::ChangesRequested { rejected: false }, false).trailing,
            Trailing::Glyph(IconName::Clock, Tint::VoteWaiting)
        );
        assert!(pr(PrHeadline::CiRunning, false).trailing.is_running());
        assert_eq!(
            pr(PrHeadline::Approved, false).trailing,
            Trailing::Glyph(IconName::BadgeCheck, Tint::VoteApproved)
        );
        assert_eq!(pr(PrHeadline::Active, false).trailing, Trailing::None);
        assert_eq!(pr(PrHeadline::Draft, true).tint, Tint::PrDraft);
    }

    #[test]
    fn work_items_take_the_type_shape_and_the_category_tint() {
        let wi = |kind, category| {
            look(
                &view(
                    PinSubject::WorkItem(7),
                    Kind::WorkItem(kind),
                    Headline::WorkItem {
                        state: "Doing".into(),
                        category,
                    },
                ),
                0,
            )
        };
        let doing = wi(Some(WorkItemType::Bug), StateCategory::InProgress);
        assert_eq!(
            (doing.glyph, doing.tint, doing.trailing),
            (IconName::Bug, Tint::Accent, Trailing::Word("Doing".into()))
        );
        assert_eq!(
            wi(Some(WorkItemType::UserStory), StateCategory::Resolved).tint,
            Tint::Success
        );
        assert_eq!(wi(None, StateCategory::Completed).glyph, IconName::Square);
        assert_eq!(wi(None, StateCategory::Removed).tint, Tint::Tertiary);
    }

    #[test]
    fn definitions_show_their_latest_run() {
        let def = |h| {
            look(
                &view(
                    PinSubject::Definition {
                        project_id: "p".into(),
                        id: 1,
                    },
                    Kind::Definition,
                    Headline::Definition(h),
                ),
                12 * MINUTE,
            )
        };
        assert_eq!(
            def(DefinitionHeadline::Running { since: 0 }).trailing,
            Trailing::Running {
                tint: Tint::PipelineRunning,
                elapsed: "12m".into()
            }
        );
        assert_eq!(def(DefinitionHeadline::Failed).glyph, IconName::Workflow);
        assert_eq!(
            def(DefinitionHeadline::NoRuns).trailing,
            Trailing::Glyph(IconName::CircleDashed, Tint::Tertiary)
        );
    }

    #[test]
    fn stale_rows_recolor_glyphs_but_keep_shapes_and_words() {
        let mut v = view(
            PinSubject::Pr(1),
            Kind::Pr { draft: false },
            Headline::Pr(PrHeadline::CiFailing),
        );
        v.stale_since = Some(0);
        let stale = look(&v, 0);
        assert_eq!(
            (stale.glyph, stale.tint, stale.trailing),
            (
                IconName::GitPullRequest,
                Tint::Tertiary,
                Trailing::Glyph(IconName::CircleX, Tint::Tertiary)
            )
        );
    }

    #[test]
    fn fetching_and_unavailable_rows() {
        let fetching = look(
            &view(
                PinSubject::Pr(4821),
                Kind::Pr { draft: false },
                Headline::Fetching,
            ),
            0,
        );
        assert_eq!(fetching.fetching.as_deref(), Some("!4821"));
        assert_eq!(fetching.tint, Tint::Tertiary);
        let gone = look(
            &view(
                PinSubject::WorkItem(9),
                Kind::WorkItem(None),
                Headline::NotAvailable(PinUnavailable::Gone),
            ),
            0,
        );
        assert_eq!(
            (gone.tint, gone.trailing, gone.fetching),
            (Tint::Tertiary, Trailing::NotAvailable, None)
        );
    }
}
