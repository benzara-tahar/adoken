//! Spec 0022 on the Ken layer: triggers → [`decide`] → one sound, and the
//! effect, static glyph, toast, announcement or K.O. it calls for. Lives
//! with the layer because the K.O. handoff (FR-6), the shared particle cap
//! (FR-12) and the toast stack are the layer's.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use adoken_ken::celebrate::{
    self, CelebrationSettings, Conditions as CelebrationConditions, Decision, Memory, Show, Sound,
    StateMove, Style, Tier, Trigger, decide,
};
use adoken_ken::engine::KO_MAX_AGE;
use adoken_ken::settings::Intensity;
use adoken_model::{Millis, PrStatus, PullRequest, short_branch};
use adoken_notify::{QuietHours, SystemClock};
use adoken_sound::SoundEvent;
use adoken_sync::engine::COMPLETING_STALE_MS;
use gpui::{AppContext as _, Context, SharedString, Task};

use super::{KenLayer, QueuedKo, ToastBody};
use crate::celebrate::{Anchor, CelebrationToast, Emission, Hooks, WorkItemMove};
use crate::sound;

/// Announcements stay in the live region this long.
const ANNOUNCE_FOR: Duration = Duration::from_secs(5);

/// The layer's celebration state. In memory, per session.
#[derive(Default)]
pub(super) struct CelebrationState {
    pub(super) settings: CelebrationSettings,
    /// Spec 0005 quiet hours, when enabled.
    quiet: Option<QuietHours>,
    memory: Memory,
    /// My accepted Completes still merging: PR → accepted at.
    pending_prs: HashMap<i64, Millis>,
    pr_watch: Option<Task<()>>,
    glyph_timer: Option<Task<()>>,
    /// PRs whose K.O. started on their confirming frame; Ken's later ingest
    /// of the same merge shows nothing again.
    pub(super) ko_started: HashSet<i64>,
    /// The latest polite announcement (FR-17).
    pub(super) announcement: Option<SharedString>,
    announce_timer: Option<Task<()>>,
}

/// What a store read of a PR means for my Complete accepted at `accepted`
/// (FR-1, AC-7): the completing mark lasts [`COMPLETING_STALE_MS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletingRead {
    /// Still merging inside the mark: keep watching.
    Merging,
    /// Confirmed `completed` inside the mark: celebrate.
    Merged,
    /// Failed, abandoned, or the mark ran out (even if merged now):
    /// stop watching, celebrate nothing.
    Lapsed,
}

fn completing_read(pr: &PullRequest, accepted: Millis, now: Millis) -> CompletingRead {
    let marked = now - accepted < COMPLETING_STALE_MS;
    match pr.status {
        PrStatus::Completed if marked => CompletingRead::Merged,
        PrStatus::Active if marked && !pr.merge_status.is_failure() => CompletingRead::Merging,
        _ => CompletingRead::Lapsed,
    }
}

impl KenLayer {
    /// The overlay's frame / timer / effect counters (spec 0022 AC-6, AC-16).
    pub fn celebration_hooks(&self) -> Hooks {
        self.fx.borrow().hooks
    }

    /// Heap bytes held by particles and seals (AC-16 allocation counter).
    pub fn celebration_bytes(&self) -> usize {
        self.fx.borrow().allocated_bytes()
    }

    /// Live particles window-wide (≤ 64).
    pub fn live_particles(&self) -> usize {
        self.fx.borrow().live_particles()
    }

    /// Whether the seal geometry was ever built.
    pub fn seal_built(&self) -> bool {
        self.fx.borrow().seal_built()
    }

    /// Titles of the celebration toasts on screen, oldest first.
    pub fn celebration_toasts(&self) -> Vec<SharedString> {
        self.toasts
            .iter()
            .filter_map(|t| match &t.body {
                ToastBody::Celebration(toast) => Some(toast.title.clone()),
                ToastBody::Ken(_) => None,
            })
            .collect()
    }

    /// The static glyph shows beside `anchor` now.
    pub fn glyph_showing(&self, anchor: Anchor) -> bool {
        let fx = self.fx.borrow();
        fx.glyph_live(anchor, fx.now())
    }

    /// Pin "now" for celebration memory, limits and glyph timers.
    pub fn set_celebration_clock(&mut self, now: Option<Millis>) {
        self.fx.borrow_mut().clock = now;
    }

    fn celebration_now(&self) -> Millis {
        self.fx.borrow().now()
    }

    pub(super) fn apply_celebrations(
        &mut self,
        settings: CelebrationSettings,
        quiet: Option<QuietHours>,
    ) {
        if !settings.enabled && self.celebrations.settings.enabled {
            self.clear_celebrations();
        }
        self.celebrations.settings = settings;
        self.celebrations.quiet = quiet;
    }

    /// Drop waiting effects and glyphs (Quiet mode, switched off).
    pub(super) fn clear_celebrations(&mut self) {
        self.fx.borrow_mut().clear_waiting();
        self.toasts
            .retain(|t| !matches!(t.body, ToastBody::Celebration(_)));
    }

    fn quiet_now(&self, now: Millis) -> bool {
        self.celebrations
            .quiet
            .as_ref()
            .is_some_and(|quiet| quiet.schedule.contains(now, &SystemClock))
    }

    fn celebration_conditions(&self, now: Millis, cx: &gpui::App) -> CelebrationConditions {
        let ken = &self.settings;
        CelebrationConditions {
            window_active: self.window_active,
            reduce_motion: cx.reduce_motion(),
            quiet_hours: self.quiet_now(now),
            quiet_mode: ken.quiet_active(now),
            ken_arcade: ken.enabled && ken.intensity == Intensity::Arcade && ken.particles,
        }
    }

    /// FR-1 work item trigger; `true` when it took care of the sound.
    pub fn celebrate_work_item(&mut self, change: WorkItemMove, cx: &mut Context<Self>) -> bool {
        let now = self.celebration_now();
        let conditions = self.celebration_conditions(now, cx);
        let decision = decide(
            Trigger::WorkItem(StateMove {
                id: change.id,
                from: &change.from,
                from_category: change.from_category,
                to: &change.to,
                to_category: change.to_category,
            }),
            &self.celebrations.settings,
            conditions,
            &mut self.celebrations.memory,
            now,
        );
        self.show_celebration(
            decision,
            Anchor::WorkItem(change.id),
            (
                format!("Moved to {}", change.to).into(),
                format!("#{} {}", change.id, change.title).into(),
            ),
            format!("Moved work item {} to {}", change.id, change.to),
            cx,
        );
        self.play_celebration(decision.sound(), conditions, cx)
    }

    /// Watch the store for my accepted Complete's merge.
    pub fn complete_accepted(&mut self, pr_id: i64, cx: &mut Context<Self>) {
        let now = self.celebration_now();
        self.celebrations.pending_prs.insert(pr_id, now);
        if self.celebrations.pr_watch.is_none() {
            let store = self.store.clone();
            self.celebrations.pr_watch = Some(crate::browse::reload_on_commit(
                &store,
                cx,
                |this: &mut Self, _, cx| this.recheck_completing(cx),
            ));
        }
    }

    /// One store read of every pending PR (background), then decide.
    fn recheck_completing(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<i64> = self.celebrations.pending_prs.keys().copied().collect();
        if ids.is_empty() {
            self.celebrations.pr_watch = None;
            return;
        }
        let store = self.store.clone();
        let job = cx.background_spawn(async move {
            let me = store.me().ok().flatten().map(|me| me.id);
            let prs: Vec<PullRequest> = ids
                .into_iter()
                .filter_map(|id| {
                    store
                        .with_conn(|c| adoken_store::read::pr(c, id))
                        .ok()
                        .flatten()
                })
                .collect();
            (me, prs)
        });
        self._tasks.retain(|task| !task.is_ready());
        self._tasks.push(cx.spawn(async move |this, cx| {
            let (me, prs) = job.await;
            let me = me.unwrap_or_default();
            this.update(cx, |this, cx| {
                for pr in prs {
                    this.pr_confirmed(pr, &me, false, cx);
                }
            })
            .ok();
        }));
    }

    /// FR-1 PR trigger: `pr` as just read or answered. `immediate`: my
    /// Complete's own answer says `completed`; otherwise only a pending
    /// Complete of this session within the completing mark's 10 min counts.
    pub fn pr_confirmed(
        &mut self,
        pr: PullRequest,
        me: &str,
        immediate: bool,
        cx: &mut Context<Self>,
    ) {
        let now = self.celebration_now();
        if !immediate {
            let Some(accepted) = self.celebrations.pending_prs.get(&pr.id).copied() else {
                return;
            };
            let read = completing_read(&pr, accepted, now);
            if read == CompletingRead::Merging {
                return;
            }
            self.celebrations.pending_prs.remove(&pr.id);
            if self.celebrations.pending_prs.is_empty() {
                self.celebrations.pr_watch = None;
            }
            if read == CompletingRead::Lapsed {
                return;
            }
        } else if pr.status != PrStatus::Completed {
            return;
        }
        let ken = &self.settings;
        let ko = ken.enabled
            && ken.ko
            && pr.author == me
            && pr.closed.is_none_or(|closed| now - closed <= KO_MAX_AGE);
        let conditions = self.celebration_conditions(now, cx);
        let decision = decide(
            Trigger::PullRequest { id: pr.id, ko },
            &self.celebrations.settings,
            conditions,
            &mut self.celebrations.memory,
            now,
        );
        let branch = short_branch(&pr.target_branch).to_owned();
        if decision == Decision::Ko {
            self.ko_now(&pr, &branch, cx);
        }
        self.show_celebration(
            decision,
            Anchor::PullRequest(pr.id),
            (
                format!("Merged into {branch}").into(),
                format!("!{} {}", pr.id, pr.title).into(),
            ),
            format!("Pull request {} merged into {branch}", pr.id),
            cx,
        );
        self.play_celebration(decision.sound(), conditions, cx);
    }

    /// FR-6: Ken's K.O. on the confirming frame instead of the next poll.
    fn ko_now(&mut self, pr: &PullRequest, branch: &str, cx: &mut Context<Self>) {
        self.celebrations.ko_started.insert(pr.id);
        let surfaces = self.surfaces(cx);
        let prs = vec![(pr.id, pr.title.clone())];
        if surfaces.ko_overlay {
            let reviewers = pr.reviewers.iter().filter(|r| r.vote.as_i8() != 0).count();
            let reviewers = match reviewers {
                0 => String::new(),
                1 => " · 1 reviewer".to_owned(),
                n => format!(" · {n} reviewers"),
            };
            self.ko_queue.push(QueuedKo {
                prs,
                double: false,
                subtitle: format!("merged into {branch}{reviewers}"),
                queued: std::time::Instant::now(),
            });
            cx.notify();
        } else {
            if surfaces.sounds {
                self.play(SoundEvent::KenKo, None, cx);
            }
            self.push_toast(super::ko::toast_text(&prs, false).into(), true, None, cx);
        }
    }

    /// "Preview celebration" (FR-15): a Milestone at the window's center in
    /// the current style; Reduce Motion shows the static toast.
    pub fn preview_celebration(&mut self, cx: &mut Context<Self>) {
        let now = self.celebration_now();
        let conditions = CelebrationConditions {
            // An explicit request from this window.
            window_active: true,
            ..self.celebration_conditions(now, cx)
        };
        let decision = decide(
            Trigger::Preview,
            &self.celebrations.settings,
            conditions,
            &mut self.celebrations.memory,
            now,
        );
        let toast = (
            "Preview celebration".into(),
            if self.celebrations.settings.enabled {
                "Celebrations are on".into()
            } else {
                "Celebrations are off · turn them on in Settings › Celebrations".into()
            },
        );
        self.show_celebration(decision, Anchor::Center, toast, String::new(), cx);
        self.play_celebration(decision.sound(), conditions, cx);
    }

    fn show_celebration(
        &mut self,
        decision: Decision,
        anchor: Anchor,
        toast: (SharedString, SharedString),
        announcement: String,
        cx: &mut Context<Self>,
    ) {
        let Decision::Celebrate { tier, show } = decision else {
            return;
        };
        let milestone = tier == Tier::Milestone;
        match show {
            Show::Full(style) => {
                self.fx.borrow_mut().queue(Emission {
                    anchor,
                    tier,
                    style,
                    fallback: (milestone && anchor != Anchor::Center).then(|| toast.clone()),
                });
            }
            Show::Static { .. } => {
                let until = self.celebration_now() + celebrate::GLYPH_MS;
                self.fx.borrow_mut().show_glyph(anchor, until);
                self.arm_glyph_timer(cx);
                if milestone {
                    self.push_celebration_toast(toast.clone(), false, cx);
                }
            }
        }
        if milestone && !announcement.is_empty() {
            self.announce(announcement.into(), cx);
        }
        cx.notify();
    }

    /// The Milestones whose anchor wasn't painted: their toast now, and the
    /// burst from it on this frame. Returns whether one was placed.
    pub(super) fn place_unanchored(&mut self, cx: &mut Context<Self>) -> bool {
        let unanchored = std::mem::take(&mut self.fx.borrow_mut().unanchored);
        if unanchored.is_empty() {
            return false;
        }
        for emission in unanchored {
            let Some((title, body)) = emission.fallback.clone() else {
                continue;
            };
            let toast = CelebrationToast {
                id: self.next_toast,
                title,
                body,
                stripe: emission.style == Style::Arcade,
            };
            let id = self.add_toast(ToastBody::Celebration(toast), cx);
            self.fx.borrow_mut().queue(Emission {
                anchor: Anchor::Toast(id),
                fallback: None,
                ..emission
            });
        }
        true
    }

    fn push_celebration_toast(
        &mut self,
        (title, body): (SharedString, SharedString),
        stripe: bool,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast;
        self.push_toast_body(
            ToastBody::Celebration(CelebrationToast {
                id,
                title,
                body,
                stripe,
            }),
            cx,
        );
    }

    /// One re-render when the earliest glyph ends (no timer otherwise).
    fn arm_glyph_timer(&mut self, cx: &mut Context<Self>) {
        let fx = self.fx.clone();
        self.fx.borrow_mut().hooks.timers = 1;
        self.celebrations.glyph_timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(celebrate::GLYPH_MS as u64))
                    .await;
                let more = this
                    .update(cx, |this, cx| {
                        let now = this.celebration_now();
                        let more = fx.borrow_mut().prune_glyphs(now);
                        cx.notify();
                        more
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
            this.update(cx, |this, _| {
                this.celebrations.glyph_timer = None;
                this.fx.borrow_mut().hooks.timers = 0;
            })
            .ok();
        }));
    }

    fn announce(&mut self, text: SharedString, cx: &mut Context<Self>) {
        self.celebrations.announcement = Some(text);
        self.celebrations.announce_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ANNOUNCE_FOR).await;
            this.update(cx, |this, cx| {
                this.celebrations.announcement = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// FR-6, FR-13: one sound. `false` when the caller's usual sound should
    /// play instead.
    fn play_celebration(
        &self,
        sound: Sound,
        conditions: CelebrationConditions,
        cx: &mut Context<Self>,
    ) -> bool {
        let event = match sound {
            Sound::Plain => return false,
            Sound::KoOnly => return true,
            Sound::CelebrateStep => SoundEvent::UiCelebrateStep,
            Sound::CelebrateMilestone => SoundEvent::UiCelebrateMilestone,
        };
        sound::play_celebration(event, conditions.quiet_hours, self.window_active, cx).ok();
        true
    }
}

#[cfg(test)]
mod tests {
    use adoken_model::MergeStatus;

    use super::*;
    use crate::review::fixtures::pull_request;

    const ACCEPTED: Millis = 1_000_000;

    fn read(status: PrStatus, merge: MergeStatus, after: Millis) -> CompletingRead {
        let mut pr = pull_request(4821, "Retry idempotency keys");
        pr.status = status;
        pr.merge_status = merge;
        completing_read(&pr, ACCEPTED, ACCEPTED + after)
    }

    /// AC-7: queued is no merge; the re-read that stores `completed`
    /// within the mark is.
    #[test]
    fn a_queued_complete_celebrates_only_its_confirmed_merge() {
        assert_eq!(
            read(PrStatus::Active, MergeStatus::Queued, 2_000),
            CompletingRead::Merging
        );
        assert_eq!(
            read(PrStatus::Completed, MergeStatus::Succeeded, 2_000),
            CompletingRead::Merged
        );
    }

    #[test]
    fn failures_and_abandons_end_the_watch_without_a_celebration() {
        for merge in [
            MergeStatus::RejectedByPolicy,
            MergeStatus::Conflicts,
            MergeStatus::Failure,
        ] {
            assert_eq!(
                read(PrStatus::Active, merge, 4_000),
                CompletingRead::Lapsed,
                "{merge:?}"
            );
        }
        assert_eq!(
            read(PrStatus::Abandoned, MergeStatus::Queued, 4_000),
            CompletingRead::Lapsed
        );
    }

    /// The mark lasts 10 min: a merge first seen at 10 min (AC-7's 11 min
    /// case included) celebrates nothing, and the watch ends.
    #[test]
    fn the_completing_mark_ends_at_ten_minutes() {
        let last = COMPLETING_STALE_MS - 1;
        assert_eq!(
            read(PrStatus::Completed, MergeStatus::Succeeded, last),
            CompletingRead::Merged
        );
        assert_eq!(
            read(PrStatus::Active, MergeStatus::Queued, last),
            CompletingRead::Merging
        );
        for after in [COMPLETING_STALE_MS, 11 * 60_000] {
            assert_eq!(
                read(PrStatus::Completed, MergeStatus::Succeeded, after),
                CompletingRead::Lapsed
            );
            assert_eq!(
                read(PrStatus::Active, MergeStatus::Queued, after),
                CompletingRead::Lapsed
            );
        }
    }
}
