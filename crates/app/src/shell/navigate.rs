//! The shell's back / forward (spec 0018): one [`History`] per main
//! window, entries of a location plus the shown view's restorable state.
//!
//! Every place that changes what the window shows goes through
//! [`Shell::navigate`] (target routing, section switches, pins) or reports
//! the change with the state it left ([`Shell::view_navigated`], for opens
//! inside a view). Both capture the current view first, so its entry keeps
//! exactly where the user was (FR-5); an open that lands on the place
//! already shown pushes nothing (FR-4). Back and forward capture, move the
//! cursor and restore; nothing here does IO (FR-13).

use std::rc::Rc;

use adoken_theme::{radius, size};
use adoken_ui::{ActiveTheme, Icon, IconName, Kbd, Tooltip};
use gpui::{
    App, Context, KeyBinding, NavigationDirection, SharedString, Window, div, prelude::*, px,
};

use super::history::{History, Subject};
use super::{GoBack, GoForward, Nav, Shell};
use crate::inbox::nav::InboxNavState;
use crate::palette::PaletteCommand;
use crate::pipelines::nav::PipelinesNavState;
use crate::review::nav::{PrListNavState, PullRequestsNavState, ReviewNavState};
use crate::today::TodayNavState;
use crate::workitems::nav::WorkItemsNavState;

/// History back / forward (command-palette.md §7.2); a text field with the
/// keyboard keeps them (FR-10).
pub const BACK_KEYS: &str = "secondary-[";
pub const FORWARD_KEYS: &str = "secondary-]";
const CONTEXT: &str = "Workspace && !TextInput";

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(BACK_KEYS, GoBack, Some(CONTEXT)),
        KeyBinding::new(FORWARD_KEYS, GoForward, Some(CONTEXT)),
    ]
}

pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Go back",
            keys: Some(BACK_KEYS),
            action: Box::new(GoBack),
        },
        PaletteCommand {
            title: "Go forward",
            keys: Some(FORWARD_KEYS),
            action: Box::new(GoForward),
        },
    ]
}

/// Where an entry is (FR-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    Section(Nav),
    /// A PR review, which replaces the PR list (0004 §3.1).
    Review(i64),
}

impl Location {
    pub fn nav(self) -> Nav {
        match self {
            Location::Section(nav) => nav,
            Location::Review(_) => Nav::PullRequests,
        }
    }
}

/// A view's restorable state (FR-5), one variant per view.
#[derive(Debug, Clone)]
pub enum ViewState {
    /// No view to ask (Search, or the store is still opening).
    Blank,
    Today(TodayNavState),
    Inbox(InboxNavState),
    Pipelines(PipelinesNavState),
    WorkItems(WorkItemsNavState),
    PullRequests(PrListNavState),
    Review(Box<ReviewNavState>),
}

/// One history entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub location: Location,
    pub state: ViewState,
    /// The destination label for tooltips (§3.3), refreshed on capture.
    pub label: SharedString,
}

impl Entry {
    fn blank(nav: Nav) -> Self {
        Self {
            location: Location::Section(nav),
            state: ViewState::Blank,
            label: nav.label().into(),
        }
    }

    fn subject(&self) -> Option<Subject> {
        match &self.state {
            ViewState::Pipelines(state) => state.subject(),
            ViewState::WorkItems(state) => state.subject(),
            // A review's subject is its location's PR.
            ViewState::Blank
            | ViewState::Today(_)
            | ViewState::Inbox(_)
            | ViewState::PullRequests(_)
            | ViewState::Review(_) => None,
        }
    }

    /// Same location and subject: an open of it pushes nothing (FR-4).
    fn same_place(&self, other: &Entry) -> bool {
        self.location == other.location && self.subject() == other.subject()
    }
}

/// The shell's history (first section at launch).
pub(super) fn launch_history(nav: Nav) -> History<Entry> {
    History::new(Entry::blank(nav))
}

impl Shell {
    /// The Inbox view, once the store is open (smoke runs).
    pub fn inbox_view(&self) -> Option<&gpui::Entity<crate::inbox::InboxView>> {
        self.views.as_ref().map(|views| &views.inbox)
    }

    /// The Pipelines view, once the store is open (smoke runs).
    pub fn pipelines_view(&self) -> Option<&gpui::Entity<crate::pipelines::PipelinesView>> {
        self.views.as_ref().map(|views| &views.pipelines)
    }

    /// The Pull requests view, once the store is open (smoke runs).
    pub fn review_view(&self) -> Option<&gpui::Entity<crate::review::ReviewView>> {
        self.views.as_ref().map(|views| &views.review)
    }

    /// The history (smoke runs, tests).
    pub fn history(&self) -> &History<Entry> {
        &self.history
    }

    /// What a back to the place shown now would be labeled (smoke runs).
    pub fn current_label(&self, window: &Window, cx: &App) -> SharedString {
        self.capture(window, cx).label
    }

    /// What the window shows now, captured from the shown view.
    fn capture(&self, window: &Window, cx: &App) -> Entry {
        let Some(views) = &self.views else {
            return Entry::blank(self.nav);
        };
        let section = Location::Section(self.nav);
        match self.nav {
            Nav::Today => Entry {
                location: section,
                label: Nav::Today.label().into(),
                state: ViewState::Today(views.today.read(cx).nav_state()),
            },
            Nav::Inbox => {
                let state = views.inbox.read(cx).nav_state();
                Entry {
                    location: section,
                    label: state.label().into(),
                    state: ViewState::Inbox(state),
                }
            }
            Nav::Pipelines => {
                let state = views.pipelines.read(cx).nav_state(window, cx);
                Entry {
                    location: section,
                    label: state.label().into(),
                    state: ViewState::Pipelines(state),
                }
            }
            Nav::WorkItems => {
                let state = views.workitems.read(cx).nav_state(window, cx);
                Entry {
                    location: section,
                    label: state.label().into(),
                    state: ViewState::WorkItems(state),
                }
            }
            Nav::PullRequests => match views.review.read(cx).nav_state(cx) {
                PullRequestsNavState::List(state) => Entry {
                    location: section,
                    label: Nav::PullRequests.label().into(),
                    state: ViewState::PullRequests(state),
                },
                PullRequestsNavState::Review(state) => Entry {
                    location: Location::Review(state.pr()),
                    label: state.label().into(),
                    state: ViewState::Review(state),
                },
            },
            nav => Entry::blank(nav),
        }
    }

    /// Runs `perform` (an open or a section switch) and records it: the
    /// entry left keeps `before` (captured now when `None`), and the place
    /// shown after is pushed unless it's the same one (FR-3, FR-4).
    pub(super) fn navigate(
        &mut self,
        before: Option<ViewState>,
        window: &mut Window,
        cx: &mut Context<Self>,
        perform: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>),
    ) {
        let before = match before {
            Some(state) => self.entry_for(state, window, cx),
            None => self.capture(window, cx),
        };
        perform(self, window, cx);
        self.record(before, window, cx);
    }

    /// A view changed its subject itself (a link, a list row's open) and
    /// reports the state it left. Opens the shell routed are already
    /// recorded; `force` records one that stays on the place (a Threads
    /// card's open, FR-3.4).
    pub(super) fn view_navigated(
        &mut self,
        before: ViewState,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.views.is_none() {
            return;
        }
        let before = self.entry_for(before, window, cx);
        let after = self.capture(window, cx);
        if !force && (after.same_place(self.history.current()) || after.same_place(&before)) {
            return;
        }
        *self.history.current_mut() = before;
        self.history.push(after);
        cx.notify();
    }

    /// `Esc` or the back button left a review for the list (FR-12): a back
    /// when the list is the entry before, else an open of the list.
    pub(super) fn review_left(
        &mut self,
        before: Box<ReviewNavState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let from_list = self.history.current().location == Location::Review(before.pr())
            && self.history.peek_back().map(|e| e.location)
                == Some(Location::Section(Nav::PullRequests));
        if from_list {
            // The view already shows the list as it was: the cursor moves.
            *self.history.current_mut() = self.entry_for(ViewState::Review(before), window, cx);
            self.history.back();
            cx.notify();
        } else {
            self.view_navigated(ViewState::Review(before), false, window, cx);
        }
    }

    /// `state` as an entry of the place it belongs to.
    fn entry_for(&self, state: ViewState, window: &Window, cx: &App) -> Entry {
        let (location, label) = match &state {
            ViewState::Today(_) => (Location::Section(Nav::Today), Nav::Today.label().to_owned()),
            ViewState::Inbox(s) => (Location::Section(Nav::Inbox), s.label()),
            ViewState::Pipelines(s) => (Location::Section(Nav::Pipelines), s.label()),
            ViewState::WorkItems(s) => (Location::Section(Nav::WorkItems), s.label()),
            ViewState::PullRequests(_) => (
                Location::Section(Nav::PullRequests),
                Nav::PullRequests.label().to_owned(),
            ),
            ViewState::Review(s) => (Location::Review(s.pr()), s.label()),
            ViewState::Blank => return self.capture(window, cx),
        };
        Entry {
            location,
            state,
            label: label.into(),
        }
    }

    fn record(&mut self, before: Entry, window: &mut Window, cx: &mut Context<Self>) {
        let after = self.capture(window, cx);
        if self.views.is_none() {
            // Still launching: one entry, the first section shown (FR-1).
            *self.history.current_mut() = after;
        } else if !after.same_place(&before) {
            *self.history.current_mut() = before;
            self.history.push(after);
        }
        cx.notify();
    }

    /// `⌘[`, the mouse back button, the titlebar `‹`, "Go back".
    pub fn go_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.peek_back().is_none() {
            return;
        }
        *self.history.current_mut() = self.capture(window, cx);
        if let Some(entry) = self.history.back().cloned() {
            self.restore(entry, window, cx);
        }
    }

    /// `⌘]`, the mouse forward button, the titlebar `›`, "Go forward".
    pub fn go_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.peek_forward().is_none() {
            return;
        }
        *self.history.current_mut() = self.capture(window, cx);
        if let Some(entry) = self.history.forward().cloned() {
            self.restore(entry, window, cx);
        }
    }

    /// Shows `entry`'s place with its state; the keyboard goes where it
    /// was (FR-16). A missing subject shows the view's own unavailable
    /// state (FR-7).
    fn restore(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        self.set_section(entry.location.nav(), window, cx);
        let Some(views) = &self.views else {
            return;
        };
        match entry.state {
            ViewState::Today(state) => views
                .today
                .update(cx, |view, cx| view.restore_nav_state(state, window, cx)),
            ViewState::Inbox(state) => views
                .inbox
                .update(cx, |view, cx| view.restore_nav_state(state, window, cx)),
            ViewState::WorkItems(state) => views
                .workitems
                .update(cx, |view, cx| view.restore_nav_state(state, window, cx)),
            ViewState::Pipelines(state) => views
                .pipelines
                .update(cx, |view, cx| view.restore_nav_state(state, window, cx)),
            ViewState::PullRequests(state) => views
                .review
                .update(cx, |view, cx| view.restore_list(state, window, cx)),
            ViewState::Review(state) => views
                .review
                .update(cx, |view, cx| view.restore_review(*state, window, cx)),
            ViewState::Blank => {}
        }
        cx.notify();
    }

    /// Input before a deferred restore applied: the user wins (FR-8).
    pub(super) fn cancel_pending_restores(&mut self, cx: &mut Context<Self>) {
        let Some(views) = &self.views else {
            return;
        };
        views
            .today
            .update(cx, |view, _| view.cancel_pending_restore());
        views
            .inbox
            .update(cx, |view, _| view.cancel_pending_restore());
        views
            .pipelines
            .update(cx, |view, _| view.cancel_pending_restore());
        views
            .workitems
            .update(cx, |view, _| view.cancel_pending_restore());
        views
            .review
            .update(cx, |view, cx| view.cancel_pending_restore(cx));
    }

    /// The titlebar's `‹ ›` with their destinations (FR-14).
    pub(super) fn render_history_buttons(&self, cx: &mut Context<Self>) -> NavButtons {
        let back = self.history.peek_back().map(|e| e.label.clone());
        let forward = self.history.peek_forward().map(|e| e.label.clone());
        NavButtons::new(back, forward)
            .on_back(cx.listener(|this, _, window, cx| this.go_back(window, cx)))
            .on_forward(cx.listener(|this, _, window, cx| this.go_forward(window, cx)))
    }

    /// The mouse's back / forward buttons, which the GPUI fork also emits
    /// for mouse and three-finger swipes (FR-11).
    pub(super) fn on_navigate_button(
        &mut self,
        direction: NavigationDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match direction {
            NavigationDirection::Back => self.go_back(window, cx),
            NavigationDirection::Forward => self.go_forward(window, cx),
        }
        // The restored view keeps the keyboard (FR-16): no focusable
        // element under the pointer takes it on this press.
        window.prevent_default();
    }
}

type Press = Rc<dyn Fn(&(), &mut Window, &mut App)>;

/// "Back to {destination} · ⌘[", or why it's disabled (§3.3).
pub fn back_tooltip(destination: Option<&str>) -> String {
    tooltip("Back to", destination, BACK_KEYS, "Nothing to go back to")
}

/// "Forward to {destination} · ⌘]", or why it's disabled (§3.3).
pub fn forward_tooltip(destination: Option<&str>) -> String {
    tooltip(
        "Forward to",
        destination,
        FORWARD_KEYS,
        "Nothing to go forward to",
    )
}

fn tooltip(verb: &str, destination: Option<&str>, keys: &str, none: &str) -> String {
    match destination {
        Some(destination) => {
            let keys = Kbd::parse(keys).expect("static binding");
            format!("{verb} {destination} · {}", keys.label())
        }
        None => none.to_owned(),
    }
}

/// The titlebar's back and forward buttons (principles §2, spec 0018
/// §3.3): ghost icon buttons, disabled with nothing in that direction.
#[derive(IntoElement)]
pub struct NavButtons {
    back: Option<SharedString>,
    forward: Option<SharedString>,
    on_back: Option<Press>,
    on_forward: Option<Press>,
}

impl NavButtons {
    /// Destination labels; `None` disables that direction.
    pub fn new(back: Option<SharedString>, forward: Option<SharedString>) -> Self {
        Self {
            back,
            forward,
            on_back: None,
            on_forward: None,
        }
    }

    pub fn on_back(mut self, f: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_back = Some(Rc::new(f));
        self
    }

    pub fn on_forward(mut self, f: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_forward = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for NavButtons {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let button = |id: &'static str,
                      icon: IconName,
                      verb: &'static str,
                      destination: Option<SharedString>,
                      tooltip: String,
                      press: Option<Press>,
                      cx: &App| {
            let colors = &cx.theme().colors;
            let enabled = destination.is_some();
            let label = match &destination {
                Some(destination) => format!("{verb} {destination}"),
                None => verb.split(' ').next().unwrap_or(verb).to_owned(),
            };
            div()
                .id(id)
                .flex_none()
                .size(px(size::CONTROL_SM))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::SM))
                .border_1()
                // The titlebar's own color at rest: the focus ring swaps it.
                .border_color(colors.bg_sidebar)
                .role(gpui::Role::Button)
                .aria_label(label)
                .tooltip(Tooltip::text(tooltip))
                .child(Icon::new(icon).size(size::ICON_SM).color(if enabled {
                    colors.text_secondary
                } else {
                    colors.text_disabled
                }))
                .when(enabled, |button| {
                    let focus = colors.border_focus;
                    let hover = colors.bg_hover;
                    button
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .focus(move |s| s.border_color(focus))
                        .when_some(press, |button, press| {
                            let on_key = press.clone();
                            button
                                .on_click(move |_, window, cx| press(&(), window, cx))
                                .on_key_down(move |event, window, cx| {
                                    let keys = &event.keystroke;
                                    if !keys.modifiers.modified()
                                        && matches!(keys.key.as_str(), "enter" | "space")
                                    {
                                        cx.stop_propagation();
                                        on_key(&(), window, cx);
                                    }
                                })
                        })
                })
        };
        let back_tip = back_tooltip(self.back.as_deref());
        let forward_tip = forward_tooltip(self.forward.as_deref());
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(adoken_theme::space::S2))
            .child(button(
                "history-back",
                IconName::ChevronLeft,
                "Back to",
                self.back,
                back_tip,
                self.on_back,
                cx,
            ))
            .child(button(
                "history-forward",
                IconName::ChevronRight,
                "Forward to",
                self.forward,
                forward_tip,
                self.on_forward,
                cx,
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltips_name_the_destination_or_why_not() {
        assert_eq!(back_tooltip(None), "Nothing to go back to");
        assert_eq!(forward_tooltip(None), "Nothing to go forward to");
        assert_eq!(
            back_tooltip(Some("Inbox · Unread · Pull requests")),
            format!(
                "Back to Inbox · Unread · Pull requests · {}",
                Kbd::parse(BACK_KEYS).unwrap().label()
            )
        );
    }

    /// AC-9: below the workspace, no view context shadows `⌘[` / `⌘]`;
    /// a text field keeps them; onboarding keeps its own Back.
    #[test]
    fn history_keys_reach_the_workspace_from_every_view() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let resolve = |keys: &str, stack: &[&str]| {
            let input = [Keystroke::parse(keys).unwrap()];
            let stack: Vec<KeyContext> = stack
                .iter()
                .map(|c| KeyContext::parse(c).unwrap())
                .collect();
            let (bindings, _) = keymap.bindings_for_input(&input, &stack);
            bindings.first().map(|b| b.action().name())
        };
        let views = [
            "PrList",
            "PrReview",
            "PullRequest",
            "Inbox",
            "PipelineList",
            "PipelineDetail",
            "Sidebar",
        ];
        for view in views {
            assert_eq!(
                resolve(BACK_KEYS, &["Workspace", view]),
                Some(GoBack.name()),
                "{view}"
            );
            assert_eq!(
                resolve(FORWARD_KEYS, &["Workspace", view]),
                Some(GoForward.name()),
                "{view}"
            );
        }
        assert_ne!(
            resolve(BACK_KEYS, &["Workspace", "PrReview", "TextInput"]),
            Some(GoBack.name()),
            "a focused text field keeps ⌘["
        );
        assert_ne!(
            resolve(BACK_KEYS, &["Onboarding"]),
            Some(GoBack.name()),
            "onboarding keeps its Back"
        );
    }
}
