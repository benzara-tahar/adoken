//! Pipelines' restorable state for back / forward (spec 0018 FR-5): tab,
//! definition and branch filters, the selected definition or run by id,
//! the run shown in the detail, list and detail scroll, and which pane
//! has the keyboard.

use std::time::Instant;

use gpui::{App, Context, ScrollStrategy, Window, point, px};

use super::model::{Filters, Tab};
use super::{Detail, Phase, PipelinesView, ROW_HEIGHT, RowKey};
use crate::shell::history::{Pending, ScrollAnchor, Subject, truncate_title};

#[derive(Debug, Clone, PartialEq)]
pub struct PipelinesNavState {
    tab: Tab,
    filters: Filters,
    definition_project: Option<String>,
    selected: Option<RowKey>,
    /// The selection's row, for when it's gone (FR-7).
    position: Option<usize>,
    /// The run the detail shows (it can differ from the selected
    /// definition row).
    run: Option<i64>,
    list_scroll: Option<ScrollAnchor<RowKey>>,
    detail_scroll: f32,
    link: usize,
    detail_focused: bool,
    /// "payments-ci #20261001.4" when a run is shown.
    title: Option<String>,
}

impl PipelinesNavState {
    /// "Pipelines", or the run shown (§3.3).
    pub fn label(&self) -> String {
        match &self.title {
            Some(title) => truncate_title(title),
            None => "Pipelines".into(),
        }
    }

    /// The run shown, else the selected definition.
    pub fn subject(&self) -> Option<Subject> {
        match (&self.run, &self.selected) {
            (Some(run), _) => Some(Subject::Run(*run)),
            (None, Some(RowKey::Pipeline(_, id))) => Some(Subject::Definition(*id)),
            (None, Some(RowKey::Run(id))) => Some(Subject::Run(*id)),
            (None, None) => None,
        }
    }
}

impl PipelinesView {
    /// The state to keep in the history entry (no IO, no list-sized work).
    pub fn nav_state(&self, window: &Window, cx: &App) -> PipelinesNavState {
        let selected = self.selected_key();
        let title = match &self.detail {
            Detail::Run(detail) => Some(format!(
                "{} #{}",
                detail.run.pipeline_name, detail.run.run_number
            )),
            _ => None,
        };
        PipelinesNavState {
            tab: self.tab,
            filters: self.filters.clone(),
            definition_project: self.definition_project.clone(),
            selected,
            position: self.selected,
            run: self.detail_run_id(),
            list_scroll: ScrollAnchor::of_list(&self.list_scroll, ROW_HEIGHT, |ix| self.key_at(ix)),
            detail_scroll: -(self.detail_scroll.offset().y / px(1.0)),
            link: self.link,
            detail_focused: self.detail_focus.contains_focused(window, cx),
            title,
        }
    }

    /// Back / forward landed here. Waits for the list if it's still
    /// loading (FR-8).
    pub fn restore_nav_state(
        &mut self,
        state: PipelinesNavState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.phase, Phase::Ready) {
            window.focus(&self.list_focus, cx);
            self.pending_restore = Some(Pending::new(state, Instant::now()));
            return;
        }
        self.apply_nav_state(state, Some(window), cx);
    }

    /// Input reached the view first: the user wins (FR-8).
    pub fn cancel_pending_restore(&mut self) {
        self.pending_restore = None;
    }

    /// The list landed: apply a waiting restore unless it's stale.
    pub(super) fn apply_pending_restore(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self
            .pending_restore
            .take()
            .and_then(|pending| pending.take(Instant::now()))
        {
            self.apply_nav_state(state, None, cx);
        }
    }

    fn apply_nav_state(
        &mut self,
        state: PipelinesNavState,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        self.tab = state.tab;
        self.filters = state.filters;
        self.definition_project = state.definition_project;
        self.refilter();
        let count = self.row_count();
        self.selected = state
            .selected
            .and_then(|key| self.row_of(&key))
            .or_else(|| {
                let position = state.position?;
                (count > 0).then(|| position.min(count - 1))
            });
        match state
            .list_scroll
            .and_then(|a| Some((self.row_of(&a.row)?, a.offset)))
        {
            Some((ix, offset)) => {
                ScrollAnchor::<RowKey>::apply(&self.list_scroll, ROW_HEIGHT, ix, offset)
            }
            None => {
                if let Some(ix) = self.selected {
                    self.list_scroll.scroll_to_item(ix, ScrollStrategy::Top);
                }
            }
        }
        match state.run {
            Some(id) if self.detail_run_id() == Some(id) => {
                self.set_detail_scroll(state.detail_scroll);
            }
            Some(id) => {
                self.load_run(id, cx);
                self.restore_detail_scroll = Some(state.detail_scroll);
            }
            None => {
                self.show_selected(cx);
                self.set_detail_scroll(state.detail_scroll);
            }
        }
        self.link = state.link;
        if let Some(window) = window {
            let focus = if state.detail_focused {
                &self.detail_focus
            } else {
                &self.list_focus
            };
            window.focus(focus, cx);
        }
        cx.notify();
    }

    pub(super) fn set_detail_scroll(&self, top: f32) {
        self.detail_scroll.set_offset(point(px(0.0), px(-top)));
    }

    fn key_at(&self, ix: usize) -> Option<RowKey> {
        if self.tab.lists_runs() {
            let run = self.list.runs.get(*self.visible.get(ix)?)?;
            Some(RowKey::Run(run.id))
        } else {
            let def = self.defs.get(ix)?;
            let p = &self.list.pipelines[def.pipeline];
            Some(RowKey::Pipeline(p.project_id.clone(), p.id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(run: Option<i64>, selected: Option<RowKey>) -> PipelinesNavState {
        PipelinesNavState {
            tab: Tab::All,
            filters: Filters::default(),
            definition_project: None,
            selected,
            position: None,
            run,
            list_scroll: None,
            detail_scroll: 0.0,
            link: 0,
            detail_focused: false,
            title: run.map(|_| "payments-ci #20261001.4".to_owned()),
        }
    }

    #[test]
    fn the_shown_run_is_the_subject_and_names_the_entry() {
        let shown = state(Some(512), Some(RowKey::Pipeline("p".into(), 7)));
        assert_eq!(shown.subject(), Some(Subject::Run(512)));
        assert_eq!(shown.label(), "payments-ci #20261001.4");
        let definition = state(None, Some(RowKey::Pipeline("p".into(), 7)));
        assert_eq!(definition.subject(), Some(Subject::Definition(7)));
        assert_eq!(definition.label(), "Pipelines");
    }
}
