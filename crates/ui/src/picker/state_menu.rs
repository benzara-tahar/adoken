//! State menu (spec 0015 FR-11; design §3.3): the states reachable from the
//! current one in category order, each with its category glyph and tint; the
//! current state checked and not choosable; typing filters.

use std::ops::Range;

use adoken_theme::{size, space, typography};
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    Role, SharedString, Subscription, Task, Window, div, prelude::*, px,
};

use super::{
    Choose, Close, LoadingGate, Placement, ReturnFocus, SelectNext, SelectPrevious, card, find_ci,
    group_label, highlighted, message_row, now_ms, overlay, row_frame, schedule,
};
use crate::icon::{Icon, IconName};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};

/// Azure DevOps state categories, in menu order (FR-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateCategory {
    Proposed,
    InProgress,
    Resolved,
    Completed,
    Removed,
}

impl StateCategory {
    /// From the `category` of a work item type state.
    pub fn from_ado(category: &str) -> Option<Self> {
        Some(match category {
            "Proposed" => Self::Proposed,
            "InProgress" => Self::InProgress,
            "Resolved" => Self::Resolved,
            "Completed" => Self::Completed,
            "Removed" => Self::Removed,
            _ => return None,
        })
    }

    /// Design §3.3: the glyph is the non-color channel.
    pub fn icon(self) -> IconName {
        match self {
            Self::Proposed => IconName::CircleDashed,
            Self::InProgress => IconName::CircleDot,
            Self::Resolved => IconName::CircleCheck,
            Self::Completed => IconName::CircleCheckBig,
            Self::Removed => IconName::CircleSlash,
        }
    }

    /// pins.md §3.3 work-item tints.
    pub fn tint(self, theme: &Theme) -> Hsla {
        let colors = &theme.colors;
        match self {
            Self::Proposed => colors.text_secondary,
            Self::InProgress => colors.accent_fg,
            Self::Resolved => colors.status_success_fg,
            Self::Completed | Self::Removed => colors.text_tertiary,
        }
    }

    /// For labels: "Code review, in progress".
    pub fn spoken(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::InProgress => "in progress",
            Self::Resolved => "resolved",
            Self::Completed => "completed",
            Self::Removed => "removed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateOption {
    pub name: SharedString,
    pub category: StateCategory,
}

impl StateOption {
    pub fn new(name: impl Into<SharedString>, category: StateCategory) -> Self {
        Self {
            name: name.into(),
            category,
        }
    }
}

/// Filtering and selection of the state menu.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateMenuModel {
    /// In category order, stable within a category.
    states: Vec<StateOption>,
    current: SharedString,
    query: String,
    /// `(index into states, matched byte range)` in display order.
    rows: Vec<(usize, Option<Range<usize>>)>,
    /// Index into `rows`; `None` when nothing is choosable.
    selected: Option<usize>,
}

impl StateMenuModel {
    /// `states` are FR-11's reachable states, the current one included.
    pub fn new(mut states: Vec<StateOption>, current: impl Into<SharedString>) -> Self {
        states.sort_by_key(|s| s.category);
        let mut model = Self {
            states,
            current: current.into(),
            ..Self::default()
        };
        model.set_query("");
        model
    }

    pub fn states(&self) -> &[StateOption] {
        &self.states
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn is_current(&self, state: usize) -> bool {
        self.states[state].name == self.current
    }

    /// Re-filters (substring, case-insensitive). Unfiltered, the selection
    /// lands on the first state after the current one (the likely next
    /// step); filtered, on the first choosable match.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.trim().to_owned();
        self.rows = if self.query.is_empty() {
            (0..self.states.len()).map(|ix| (ix, None)).collect()
        } else {
            self.states
                .iter()
                .enumerate()
                .filter_map(|(ix, s)| find_ci(&s.name, &self.query).map(|r| (ix, Some(r))))
                .collect()
        };
        let choosable = |row: &usize| !self.is_current(self.rows[*row].0);
        let first = (0..self.rows.len()).find(choosable);
        self.selected = if self.query.is_empty() {
            match self.rows.iter().position(|&(ix, _)| self.is_current(ix)) {
                Some(cur) => (cur + 1..self.rows.len()).find(choosable).or(first),
                None => first,
            }
        } else {
            first
        };
    }

    /// `(state index, matched range)` per row, in display order.
    pub fn rows(&self) -> &[(usize, Option<Range<usize>>)] {
        &self.rows
    }

    /// Selected row (index into [`Self::rows`]).
    pub fn selected_row(&self) -> Option<usize> {
        self.selected
    }

    /// `↑`/`↓`: one choosable row, wrapping; the current state is skipped.
    pub fn step(&mut self, delta: isize) {
        let len = self.rows.len() as isize;
        let Some(mut row) = self.selected else { return };
        for _ in 0..len {
            row = (row as isize + delta).rem_euclid(len) as usize;
            if !self.is_current(self.rows[row].0) {
                self.selected = Some(row);
                return;
            }
        }
    }

    /// Pointer hover on row `row`.
    pub fn highlight(&mut self, row: usize) {
        if row < self.rows.len() && !self.is_current(self.rows[row].0) {
            self.selected = Some(row);
        }
    }

    /// The state `↩` sends, if any.
    pub fn chosen(&self) -> Option<&StateOption> {
        self.selected.map(|row| &self.states[self.rows[row].0])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateMenuEvent {
    Chosen(SharedString),
}

pub struct StateMenu {
    title: SharedString,
    current: SharedString,
    /// `None` while the type metadata loads.
    model: Option<StateMenuModel>,
    /// Data that arrived while "Loading…" must stay up a little longer.
    pending: Option<Vec<StateOption>>,
    loading: LoadingGate,
    input: Entity<TextInput>,
    placement: Placement,
    return_focus: ReturnFocus,
    _timer: Option<Task<()>>,
    _input_events: Subscription,
}

impl EventEmitter<StateMenuEvent> for StateMenu {}
impl EventEmitter<DismissEvent> for StateMenu {}

impl Focusable for StateMenu {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl StateMenu {
    /// Opens with the keyboard in its (hidden until typed) filter. `title`
    /// names the menu ("Change state"); `states: None` shows the loading
    /// row (after its delay) until [`Self::set_states`].
    pub fn new(
        title: impl Into<SharedString>,
        current: impl Into<SharedString>,
        states: Option<Vec<StateOption>>,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new("", cx));
        let input_events = cx.subscribe(&input, |this, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let query = input.read(cx).text().to_owned();
                if let Some(model) = &mut this.model
                    && model.query() != query.trim()
                {
                    model.set_query(&query);
                }
                cx.notify();
            }
        });
        let return_focus = ReturnFocus::capture(window, cx);
        window.focus(&input.focus_handle(cx), cx);
        let current = current.into();
        let mut menu = Self {
            title: title.into(),
            model: None,
            pending: None,
            loading: LoadingGate::default(),
            current,
            input,
            placement,
            return_focus,
            _timer: None,
            _input_events: input_events,
        };
        menu.set_states(states, cx);
        menu
    }

    /// The type metadata (FR-33) arrived, or (`None`) is loading again.
    pub fn set_states(&mut self, states: Option<Vec<StateOption>>, cx: &mut Context<Self>) {
        match states {
            None => {
                self.model = None;
                self.loading = LoadingGate::start(now_ms());
                if let Some(at) = self.loading.show_at() {
                    self._timer = Some(schedule(at, cx, |this: &mut Self, _| {
                        this.loading.tick(now_ms());
                    }));
                }
            }
            Some(states) => {
                let now = now_ms();
                let at = self.loading.ready_at(now);
                if at > now {
                    self.pending = Some(states);
                    self._timer = Some(schedule(at, cx, |this: &mut Self, cx| {
                        if let Some(states) = this.pending.take() {
                            this.apply(states, cx);
                        }
                    }));
                } else {
                    self.apply(states, cx);
                }
            }
        }
        cx.notify();
    }

    fn apply(&mut self, states: Vec<StateOption>, cx: &mut Context<Self>) {
        self._timer = None;
        self.loading = LoadingGate::default();
        let mut model = StateMenuModel::new(states, self.current.clone());
        model.set_query(self.input.read(cx).text());
        self.model = Some(model);
    }

    /// Shows "Loading…" without its delay (the host already waited).
    pub fn show_loading_now(&mut self, cx: &mut Context<Self>) {
        if self.model.is_none() {
            self.loading = LoadingGate::start(0);
            self.loading.tick(u64::from(super::LOADING_DELAY));
            cx.notify();
        }
    }

    pub fn model(&self) -> Option<&StateMenuModel> {
        self.model.as_ref()
    }

    /// Replaces the filter text, as typing would.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(query, cx));
        if let Some(model) = &mut self.model {
            model.set_query(query);
        }
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            model.step(delta);
            cx.notify();
        }
    }

    /// Closes and hands the keyboard back.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus.restore(window, cx);
        cx.emit(DismissEvent);
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self
            .model
            .as_ref()
            .and_then(|m| m.chosen())
            .map(|s| s.name.clone())
        else {
            return;
        };
        self.return_focus.restore(window, cx);
        cx.emit(StateMenuEvent::Chosen(state));
        cx.emit(DismissEvent);
    }

    /// `Esc`: clears the filter, then closes.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).text().is_empty() {
            self.dismiss(window, cx);
        } else {
            self.set_query("", cx);
        }
    }

    fn render_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let model = self.model.as_ref().expect("rows render with a model");
        let (state_ix, range) = model.rows()[row].clone();
        let state = &model.states()[state_ix];
        let current = model.is_current(state_ix);
        let selected = model.selected_row() == Some(row);
        let label = if current {
            format!("{}, current", state.name)
        } else {
            format!("{}, {}", state.name, state.category.spoken())
        };
        let frame = row_frame(("state-menu-row", row), selected, theme);
        let frame = if current {
            frame.cursor_default()
        } else {
            frame
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if let Some(model) = &mut this.model
                        && model.selected_row() != Some(row)
                    {
                        model.highlight(row);
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(model) = &mut this.model {
                        model.highlight(row);
                    }
                    this.choose(window, cx);
                }))
        };
        frame
            .role(Role::MenuItem)
            .aria_label(label)
            .aria_selected(selected)
            .child(
                Icon::new(state.category.icon())
                    .size(size::ICON_SM)
                    .color(state.category.tint(theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_primary)
                    .child(highlighted(
                        state.name.clone(),
                        range.into_iter().collect(),
                        theme,
                    )),
            )
            .when(current, |d| {
                d.child(
                    Icon::new(IconName::Check)
                        .size(size::ICON_SM)
                        .color(colors.text_secondary),
                )
            })
            .into_any_element()
    }
}

impl Render for StateMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.input.read(cx).text().to_owned();
        let rows: Vec<AnyElement> = match &self.model {
            Some(model) => (0..model.rows().len())
                .map(|row| self.render_row(row, cx))
                .collect(),
            None => Vec::new(),
        };
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let filtering = !query.is_empty();
        // The filter line replaces the label on the first keystroke; the
        // input stays mounted (transparent) so it keeps the keyboard.
        let header = div()
            .relative()
            .h(px(size::ROW_COMPACT))
            .flex_none()
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .when(!filtering, |d| {
                d.child(
                    group_label(self.title.clone(), &theme)
                        .absolute()
                        .left_0()
                        .top_0()
                        .items_center()
                        .pb_0(),
                )
            })
            .when(filtering, |d| {
                d.child(
                    Icon::new(IconName::Search)
                        .size(size::ICON_XS)
                        .color(colors.text_tertiary),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .opacity(if filtering { 1.0 } else { 0.0 })
                    .child(self.input.clone()),
            );
        let message = match &self.model {
            None if self.loading.is_shown() => Some(message_row("Loading…", &theme)),
            None => None,
            Some(_) if rows.is_empty() => Some(message_row(
                format!("No state matches \u{201c}{query}\u{201d}"),
                &theme,
            )),
            Some(_) => None,
        };
        let card = card("state-menu", self.placement.width, &theme)
            .role(Role::Menu)
            .aria_label(self.title.clone())
            .type_style(typography::BODY, &theme)
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Choose, window, cx| this.choose(window, cx)))
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close(window, cx)))
            .child(header)
            .children(rows)
            .children(message);
        overlay("state-menu-in", card, self.placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use StateCategory::*;

    fn agile_bug() -> Vec<StateOption> {
        // Deliberately out of category order.
        vec![
            StateOption::new("Closed", Completed),
            StateOption::new("Doing", InProgress),
            StateOption::new("New", Proposed),
            StateOption::new("Resolved", Resolved),
            StateOption::new("Code review", InProgress),
            StateOption::new("Removed", Removed),
        ]
    }

    fn names(model: &StateMenuModel) -> Vec<&str> {
        model
            .rows()
            .iter()
            .map(|&(ix, _)| model.states()[ix].name.as_ref())
            .collect()
    }

    #[test]
    fn states_are_in_category_order_and_stable_within_a_category() {
        let model = StateMenuModel::new(agile_bug(), "Doing");
        assert_eq!(
            names(&model),
            [
                "New",
                "Doing",
                "Code review",
                "Resolved",
                "Closed",
                "Removed"
            ]
        );
    }

    #[test]
    fn selection_starts_after_the_current_state_and_skips_it_when_moving() {
        let mut model = StateMenuModel::new(agile_bug(), "Doing");
        assert_eq!(model.chosen().unwrap().name, "Code review");
        model.step(-1);
        assert_eq!(model.chosen().unwrap().name, "New", "current is skipped");
        model.step(-1);
        assert_eq!(model.chosen().unwrap().name, "Removed", "wraps");
        model.step(1);
        model.step(1);
        assert_eq!(model.chosen().unwrap().name, "Code review");
    }

    #[test]
    fn current_last_wraps_to_the_first_choosable_state() {
        let model = StateMenuModel::new(agile_bug(), "Removed");
        assert_eq!(model.chosen().unwrap().name, "New");
    }

    #[test]
    fn typing_filters_and_selects_the_first_choosable_match() {
        let mut model = StateMenuModel::new(agile_bug(), "Doing");
        model.set_query("res");
        assert_eq!(names(&model), ["Resolved"]);
        assert_eq!(model.chosen().unwrap().name, "Resolved");
        assert_eq!(model.rows()[0].1, Some(0..3));
        // "o" matches Doing (current) first: the selection skips it.
        model.set_query("O");
        assert_eq!(model.chosen().unwrap().name, "Code review");
        model.set_query("zz");
        assert!(model.rows().is_empty());
        assert_eq!(model.chosen(), None);
        model.set_query("");
        assert_eq!(model.chosen().unwrap().name, "Code review");
    }

    #[test]
    fn the_current_state_alone_offers_nothing_to_choose() {
        let mut model = StateMenuModel::new(vec![StateOption::new("Done", Completed)], "Done");
        assert_eq!(model.chosen(), None);
        model.step(1);
        model.highlight(0);
        assert_eq!(model.chosen(), None);
    }

    #[test]
    fn categories_map_from_azure_devops_names() {
        assert_eq!(StateCategory::from_ado("InProgress"), Some(InProgress));
        assert_eq!(StateCategory::from_ado("Custom"), None);
    }
}
