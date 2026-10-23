//! Celebrations for finished work (spec 0022, docs/design/ken.md §12) in
//! the app. Rules live in `adoken_ken::celebrate` (pure, clock-injected);
//! the Ken layer (`crate::ken`, present with Ken off) decides, plays the
//! sound and paints the effect in its pointer-transparent overlay.
//!
//! Contract for views and write paths:
//! - Triggers (FR-1): [`work_item_confirmed`] from `WiEdits::answered` for
//!   a confirmed state write; [`complete_accepted`] when Azure DevOps
//!   accepts my Complete, then [`pr_completed`] / [`pr_seen`] when the PR
//!   is confirmed `completed`.
//! - Anchors (FR-8): the element showing the new state adds
//!   [`track`] as a child (it exists only while a celebration waits for that
//!   anchor, so idle frames cost nothing) and [`glyph`] after its label
//!   (the static `sparkles` glyph of FR-9).
//! - [`commands`] in the palette; [`settings_issues`] for Settings' report.
//!
//! Timing (FR-5): a trigger arms its anchor and asks for a frame; in that
//! frame the anchor records its bounds during prepaint, and the overlay
//! (prepainted last) starts the effect there, so the burst starts on the
//! frame that applies the confirmation.

mod fx;
mod toast;

use std::cell::RefCell;
use std::rc::Rc;

use adoken_ken::celebrate::CelebrationSettings;
use adoken_model::StateCategory;
use adoken_theme::size;
use adoken_ui::{ActiveTheme, Icon, IconName};
use gpui::{AnyElement, App, Global, IntoElement, actions, canvas, div, prelude::*};

use crate::palette::PaletteCommand;

pub use fx::{Emission, Fx, FxColors, Hooks, glow_opacity, paint, seal_frame};
pub use toast::CelebrationToast;

/// The element a celebration comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// The work item's state chip (detail header), else its list row.
    WorkItem(i64),
    /// The PR's status chip in the review header, else its list row.
    PullRequest(i64),
    /// A Milestone toast (no visible anchor; ken.md §12.5).
    Toast(u64),
    /// The window's center ("Preview celebration").
    Center,
}

/// The shared effect state, reachable from any view while rendering.
pub(crate) struct GlobalFx(pub(crate) Rc<RefCell<Fx>>);

impl Global for GlobalFx {}

pub(crate) fn fx(cx: &App) -> Option<Rc<RefCell<Fx>>> {
    cx.try_global::<GlobalFx>().map(|g| g.0.clone())
}

/// An invisible child filling its (relative) parent that records the
/// parent's bounds for a waiting celebration. `None` when nothing waits for
/// `anchor` (no element, no work).
pub fn track(anchor: Anchor, cx: &App) -> Option<AnyElement> {
    let fx = fx(cx)?;
    if !fx.borrow().armed(anchor) {
        return None;
    }
    Some(
        canvas(
            move |bounds, _, _| fx.borrow_mut().record(anchor, bounds),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// The static `sparkles` glyph (FR-9, ken.md §12.4) beside the new state:
/// 14 px, `status.success.fg`, for 3 s, no transition. Anchors are flex
/// rows with a `space.4` gap, which places it after the label. `None`
/// otherwise.
pub fn glyph(anchor: Anchor, cx: &App) -> Option<AnyElement> {
    let fx = fx(cx)?;
    let fx = fx.borrow();
    if !fx.glyph_live(anchor, fx.now()) {
        return None;
    }
    Some(sparkles(cx).into_any_element())
}

/// The glyph itself (also used by stories).
pub fn sparkles(cx: &App) -> impl IntoElement {
    div().flex_none().flex().items_center().child(
        Icon::new(IconName::Sparkles)
            .size(size::ICON_SM)
            .color(cx.theme().colors.status_success_fg),
    )
}

/// A confirmed state write of mine (`WiEdits::answered` with
/// `Ok(Applied)` for `WiChange::State`): base state and the answer's state
/// with their categories (type metadata, else the stock names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkItemMove {
    pub id: i64,
    pub title: String,
    pub from: String,
    pub from_category: StateCategory,
    pub to: String,
    pub to_category: StateCategory,
}

/// FR-1 work item trigger. Plays the celebration sound when it celebrates
/// and returns `true`; `false` means the caller plays its usual
/// `ui.action_done` (FR-6: never both).
pub fn work_item_confirmed(change: WorkItemMove, cx: &mut App) -> bool {
    crate::ken::layer_entity(cx)
        .map(|layer| layer.update(cx, |layer, cx| layer.celebrate_work_item(change, cx)))
        .unwrap_or(false)
}

/// Azure DevOps accepted my Complete with the merge queued: watch for the
/// confirmed merge while the completing mark lasts (≤ 10 min).
pub fn complete_accepted(pr_id: i64, cx: &mut App) {
    if let Some(layer) = crate::ken::layer_entity(cx) {
        layer.update(cx, |layer, cx| layer.complete_accepted(pr_id, cx));
    }
}

/// My Complete's answer already says `completed` (a fast merge, the demo
/// org): that answer is the confirmation.
pub fn pr_completed(pr: &adoken_model::PullRequest, me: &str, cx: &mut App) {
    if let Some(layer) = crate::ken::layer_entity(cx) {
        layer.update(cx, |layer, cx| layer.pr_confirmed(pr.clone(), me, true, cx));
    }
}

/// A store read of `pr` (the review's completing re-read): celebrates if
/// it's a merge of my pending Complete.
pub fn pr_seen(pr: &adoken_model::PullRequest, me: &str, cx: &mut App) {
    if let Some(layer) = crate::ken::layer_entity(cx) {
        layer.update(cx, |layer, cx| {
            layer.pr_confirmed(pr.clone(), me, false, cx)
        });
    }
}

/// `[celebrations]` problems for Settings' report (0009 FR-5).
pub fn settings_issues(document: &toml::Table) -> Vec<String> {
    CelebrationSettings::parse(document).1
}

actions!(
    celebrate,
    [
        /// Play a Milestone at the window's center (ignores the master
        /// switch and the rate limits).
        PreviewCelebration,
    ]
);

pub const PREVIEW: &str = "Preview celebration";

/// Palette commands (FR-15). No default shortcuts. Turning celebrations
/// off is Settings › Celebrations or Quiet mode (spec 0025 FR-8).
pub fn commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: PREVIEW,
        keys: None,
        action: Box::new(PreviewCelebration),
    }]
}

/// The action handler; once per app.
pub(crate) fn register(cx: &mut App) {
    cx.on_action(|_: &PreviewCelebration, cx| {
        if let Some(layer) = crate::ken::layer_entity(cx) {
            layer.update(cx, |layer, cx| layer.preview_celebration(cx));
        }
    });
}
