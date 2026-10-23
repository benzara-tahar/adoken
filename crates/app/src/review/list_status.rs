//! Status actions and copy commands on the Pull requests list (spec 0019
//! groups D and E): the row menu's status items, `⇧S` / `⇧P` / `⇧A` on the
//! selected row, the same status menu and Abandon confirmation as the
//! review's, and the `⌘P` commands' gates (FR-19).
//!
//! Every status change runs the review's one implementation (FR-21):
//! [`writes::status_actions`] decides what is valid, [`writes::apply_status_action`]
//! shows it on the next frame, [`writes::Access::send`] sends the `PATCH`,
//! and a refusal reverts it with Azure DevOps' reason in a toast.

use std::collections::HashSet;

use adoken_sync::writes::{Applied, PrRef};
use adoken_ui::action_menu::{BoundsSlot, MenuPosition};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{
    AnyElement, App, Context, FocusHandle, Global, Pixels, Point, SharedString, WeakEntity, Window,
    anchored, deferred, div, prelude::*, px,
};

use super::web::{self, CopyTarget, ToastKind};
use super::writes::{self, StatusAction};
use super::{ReviewView, pr};

/// The palette's reason when no pull request is at hand (FR-19).
pub const SELECT_FIRST: &str = "Select a pull request first";
/// Principles §3.5: status changes need the network.
pub const OFFLINE_REASON: &str = "Offline — reconnect to send";

#[derive(Default)]
pub(super) struct ListStatus {
    menu: Option<StatusMenu>,
    confirm: Option<(i64, FocusHandle)>,
    /// PRs with a status change in flight (one per PR, FR-21).
    sending: HashSet<i64>,
}

struct StatusMenu {
    actions: Vec<StatusAction>,
    selected: usize,
    focus: FocusHandle,
    position: MenuPosition,
}

/// The window's Pull requests view, for `⌘P` gates that ask whether a
/// pull request is at hand.
struct Installed(WeakEntity<ReviewView>);

impl Global for Installed {}

pub(super) fn install(cx: &mut Context<ReviewView>) {
    let weak = cx.entity().downgrade();
    cx.set_global(Installed(weak));
    fn status(cx: &App) -> Option<SharedString> {
        if !writes::access(cx).can_write() {
            return Some(writes::READ_ONLY_HINT.into());
        }
        target_blocker(cx)
    }
    for action in [
        Box::new(pr::OpenStatusMenu) as Box<dyn gpui::Action>,
        Box::new(pr::ToggleDraft),
        Box::new(pr::AbandonPr),
        Box::new(pr::ReactivatePr),
    ] {
        crate::palette::gates::register(action.as_ref(), status, cx);
    }
    for action in [
        Box::new(super::CopyPrTitle) as Box<dyn gpui::Action>,
        Box::new(super::CopySourceBranch),
        Box::new(super::CopyTargetBranch),
    ] {
        crate::palette::gates::register(action.as_ref(), target_blocker, cx);
    }
}

/// FR-19: the open review, else the Pull requests list's selected row,
/// while that view is in front; else "Select a pull request first".
fn target_blocker(cx: &App) -> Option<SharedString> {
    let in_front =
        crate::background::front_view(cx) == Some(adoken_theme::background::ViewId::PullRequests);
    let has_target = cx
        .try_global::<Installed>()
        .and_then(|Installed(view)| view.upgrade())
        .is_some_and(|view| {
            let view = view.read(cx);
            view.open_review_id(cx).is_some() || view.selected_pr_id().is_some()
        });
    (!(in_front && has_target)).then(|| SELECT_FIRST.into())
}

impl ReviewView {
    /// The status changes valid for list PR `id` now (FR-18).
    fn list_status_actions(&self, id: i64, cx: &App) -> Vec<StatusAction> {
        let Some(list) = &self.list else {
            return Vec::new();
        };
        let Some(pr) = list.prs.iter().find(|p| p.id == id) else {
            return Vec::new();
        };
        let read_only = !writes::access(cx).can_write();
        writes::status_actions(read_only, pr, list.facts.completing.contains(&id))
    }

    /// The row menu's status items for the selected row (FR-18).
    pub(super) fn status_menu_items(&self, cx: &App) -> Vec<adoken_ui::action_menu::MenuItem> {
        let Some(id) = self.selected_pr_id() else {
            return Vec::new();
        };
        self.list_status_actions(id, cx)
            .into_iter()
            .map(|action| {
                let item = match action {
                    StatusAction::Publish | StatusAction::MarkDraft => {
                        crate::pin_actions::action(action.label(), pr::ToggleDraft)
                    }
                    StatusAction::Abandon => {
                        crate::pin_actions::action(action.label(), pr::AbandonPr)
                    }
                    StatusAction::Reactivate => {
                        crate::pin_actions::action(action.label(), pr::ReactivatePr)
                    }
                };
                match pr::status_keys(action) {
                    Some(keys) => item.keys(keys),
                    None => item,
                }
            })
            .collect()
    }

    /// The list's status overlay is open (its keys reach the view).
    pub(super) fn list_overlay_open(&self) -> bool {
        self.list_status.menu.is_some() || self.list_status.confirm.is_some()
    }

    /// A status change of list PR `id` is on its way.
    pub fn is_sending_status(&self, id: i64) -> bool {
        self.list_status.sending.contains(&id)
    }

    /// The open row menu's item labels, in order (tests).
    pub fn row_menu_labels(&self, cx: &App) -> Option<Vec<SharedString>> {
        let menu = self.row_menu.menu()?.read(cx);
        Some(menu.items().iter().map(|item| item.label.clone()).collect())
    }

    /// The list's status menu or Abandon confirmation is open (tests).
    pub fn list_status_overlay(&self) -> Option<&'static str> {
        if self.list_status.menu.is_some() {
            Some("menu")
        } else if self.list_status.confirm.is_some() {
            Some("confirm")
        } else {
            None
        }
    }

    /// List PR `id` as the row shows it (tests).
    pub fn listed_pr(&self, id: i64) -> Option<&adoken_model::PullRequest> {
        self.list.as_ref()?.prs.iter().find(|p| p.id == id)
    }

    /// The open review, once built (tests).
    pub fn open_review(&self) -> Option<gpui::Entity<pr::PrReview>> {
        match &self.detail {
            super::Detail::Open(review) => Some(review.clone()),
            _ => None,
        }
    }

    /// `⇧S` on the list: the status menu at the selected row (FR-18).
    pub(super) fn open_list_status_menu(
        &mut self,
        bounds: &BoundsSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_pr_id() else {
            return;
        };
        if let Some(why) = self.list_status_blocker(id, cx) {
            self.show_toast(why.into(), cx);
            return;
        }
        let actions = self.list_status_actions(id, cx);
        if actions.is_empty() {
            let why = match self
                .list
                .as_ref()
                .and_then(|l| l.prs.iter().find(|p| p.id == id))
            {
                Some(pr)
                    if self
                        .list
                        .as_ref()
                        .is_some_and(|l| l.facts.completing.contains(&id)) =>
                {
                    format!("Azure DevOps is completing !{}", pr.id)
                }
                _ => format!("!{id} is completed"),
            };
            self.show_toast(why.into(), cx);
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.list_status.menu = Some(StatusMenu {
            actions,
            selected: 0,
            focus,
            position: adoken_ui::action_menu::keyboard_position(bounds, window),
        });
        cx.notify();
    }

    /// Why nothing can be sent for `id` now: read-only or offline.
    fn list_status_blocker(&self, _id: i64, cx: &App) -> Option<&'static str> {
        if !writes::access(cx).can_write() {
            Some(writes::READ_ONLY_HINT)
        } else if self.offline {
            Some(OFFLINE_REASON)
        } else {
            None
        }
    }

    /// `⇧P` / `⇧A` / Reactivate / a menu item on the selected row: explained
    /// when not valid, confirmed first when it's Abandon (FR-21).
    pub(super) fn request_list_status(
        &mut self,
        action: Option<StatusAction>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_list_status(window, cx);
        let Some(id) = self.selected_pr_id() else {
            return;
        };
        let Some(pr) = self.selected_pr().cloned() else {
            return;
        };
        // `None`: ⇧P, publish or mark as draft by the row's state.
        let action = action.unwrap_or_else(|| writes::draft_toggle(&pr));
        if let Some(why) = self.list_status_blocker(id, cx) {
            self.show_toast(why.into(), cx);
            return;
        }
        let completing = self
            .list
            .as_ref()
            .is_some_and(|l| l.facts.completing.contains(&id));
        if let Some(why) = writes::status_refusal(false, &pr, completing, action) {
            self.show_toast(why.into(), cx);
            return;
        }
        if action == StatusAction::Abandon {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            self.list_status.confirm = Some((id, focus));
            cx.notify();
            return;
        }
        self.change_list_status(id, action, cx);
    }

    /// Send `action` for list PR `id`: the row shows it with "Sending…"
    /// on the next frame; Azure DevOps' answer replaces it, a refusal
    /// reverts it with the reason in a toast.
    fn change_list_status(&mut self, id: i64, action: StatusAction, cx: &mut Context<Self>) {
        if self.list_status.sending.contains(&id) {
            return;
        }
        let now = self.now();
        let Some(list) = self.list.as_mut() else {
            return;
        };
        let Some(pr) = list.prs.iter_mut().find(|p| p.id == id) else {
            return;
        };
        let before = writes::apply_status_action(pr, action, now);
        let write = action.write(PrRef::of(pr));
        let shown = pr.clone();
        self.list_status.sending.insert(id);
        self.update_listed_pr(shown, cx);
        let task = writes::access(cx).send(Some(self.store.clone()), write, now, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                this.list_status.sending.remove(&id);
                let Some(current) = this
                    .list
                    .as_ref()
                    .and_then(|l| l.prs.iter().find(|p| p.id == id))
                    .cloned()
                else {
                    return;
                };
                match result {
                    Ok(Applied::Pr(server)) => this.update_listed_pr(server, cx),
                    Ok(_) => cx.notify(),
                    Err(error) => {
                        let mut reverted = current;
                        writes::revert_status(&mut reverted, before);
                        this.update_listed_pr(reverted, cx);
                        this.show_toast_kind(
                            writes::status_refused(id, &error.to_string()).into(),
                            ToastKind::Failed,
                            cx,
                        );
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn close_list_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_overlay_open() {
            self.list_status.menu = None;
            self.list_status.confirm = None;
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    pub(super) fn step_list_status(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(menu) = self.list_status.menu.as_mut() {
            menu.selected =
                crate::palette::step_selection(menu.selected, menu.actions.len(), delta);
            cx.notify();
        }
    }

    pub(super) fn confirm_list_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(menu) = &self.list_status.menu {
            let action = menu.actions[menu.selected];
            self.request_list_status(Some(action), window, cx);
        } else if let Some((id, _)) = self.list_status.confirm.take() {
            window.focus(&self.focus, cx);
            self.change_list_status(id, StatusAction::Abandon, cx);
        }
    }

    /// Copy title / source branch / target branch of the selected row.
    pub(super) fn list_copy(&mut self, target: CopyTarget, cx: &mut Context<Self>) {
        let Some(pr) = self.selected_pr().cloned() else {
            return;
        };
        let toast = web::copy_pr_text(&pr, target, cx);
        self.show_toast(toast, cx);
    }

    /// The status menu at the row and the Abandon confirmation over the
    /// list (design §4).
    pub(super) fn render_list_status(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.theme().clone();
        let mut out = Vec::new();
        let entity = cx.entity().downgrade();
        let close: pr::Handler<()> = std::rc::Rc::new(move |_, window, cx| {
            entity
                .update(cx, |this, cx| this.close_list_status(window, cx))
                .ok();
        });
        if let Some(menu) = &self.list_status.menu {
            let entity = cx.entity().downgrade();
            let pick: pr::Handler<StatusAction> = std::rc::Rc::new(move |action, window, cx| {
                entity
                    .update(cx, |this, cx| {
                        this.request_list_status(Some(action), window, cx)
                    })
                    .ok();
            });
            let panel = pr::status_menu_panel(
                &menu.actions,
                menu.selected,
                &menu.focus,
                &theme,
                pick,
                close.clone(),
            );
            out.push(
                deferred(
                    anchored()
                        .position(menu.position.point)
                        .anchor(menu.position.corner)
                        .snap_to_window_with_margin(px(adoken_theme::space::S8))
                        .child(panel),
                )
                .with_priority(usize::from(adoken_theme::z::DROPDOWN))
                .into_any_element(),
            );
        }
        if let Some((id, focus)) = &self.list_status.confirm {
            let (title, body, yes) = pr::abandon_text(*id);
            let entity = cx.entity().downgrade();
            let accept: pr::Handler<()> = std::rc::Rc::new(move |_, window, cx| {
                entity
                    .update(cx, |this, cx| this.confirm_list_status(window, cx))
                    .ok();
            });
            out.push(
                pr::confirm_dialog(title, body, yes, focus, &theme, accept, close)
                    .into_any_element(),
            );
        }
        out
    }

    /// Stories: the list's status menu open at the selected row.
    pub fn story_list_status_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bounds = self.row_bounds.clone();
        self.open_list_status_menu(&bounds, window, cx);
    }

    /// Stories: Abandon's confirmation over the list for the selected row.
    pub fn story_list_abandon(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_pr_id() {
            self.list_status.confirm = Some((id, cx.focus_handle()));
            cx.notify();
        }
    }

    /// Stories: the row menu as a right-click at `at` opens it.
    pub fn story_row_menu(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_row_menu(MenuPosition::at_pointer(at), window, cx);
    }

    /// Stories: group reviewers shown or hidden whatever the setting.
    pub fn story_group_reviewers(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_groups = Some(show);
        cx.notify();
    }

    /// Stories: the selected row's status change on its way.
    pub fn story_list_sending(&mut self, action: StatusAction, cx: &mut Context<Self>) {
        let Some(id) = self.selected_pr_id() else {
            return;
        };
        let now = self.now();
        if let Some(pr) = self
            .list
            .as_mut()
            .and_then(|l| l.prs.iter_mut().find(|p| p.id == id))
        {
            writes::apply_status_action(pr, action, now);
            let shown = pr.clone();
            self.list_status.sending.insert(id);
            self.update_listed_pr(shown, cx);
        }
    }
}

/// The row chip's "Sending…" tail (design §4).
pub(super) fn sending_badge(theme: &adoken_ui::Theme) -> impl IntoElement + use<> {
    let colors = &theme.colors;
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(adoken_theme::space::S2))
        .text_color(colors.text_tertiary)
        .type_style(adoken_theme::typography::SMALL, theme)
        .child(
            adoken_ui::Icon::new(adoken_ui::IconName::Clock)
                .size(adoken_theme::size::ICON_XS)
                .color(colors.text_tertiary),
        )
        .child("Sending…")
}
