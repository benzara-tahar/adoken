//! Pipelines view (spec 0008, read-only): pipeline definitions with a
//! last-runs sparkline, run lists (Mine / Failed / All runs) filtered by
//! pipeline and branch, and run detail with stages, result, duration,
//! requester and linked PR.
//!
//! Store reads run on the background executor; tabs and filters work on
//! the loaded runs in memory (`model`). Nothing here talks to Azure DevOps
//! or writes: stages come from sync (build timelines) or the demo seed.

pub mod model;
pub mod nav;

use std::collections::HashMap;
use std::time::Instant;

use adoken_model::{
    Millis, PersonId, Pipeline, PipelineRun, PrStatus, PullRequest, RunStage, StageOutcome,
};
use adoken_store::{RunFilter, Store, read};
use adoken_theme::{ColorTokens, radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Chip, ChipTone, EmptyState, Icon, IconName, Kbd, ListRow,
    Notice, Segmented, Theme, TypeStyleExt,
};
use gpui::{
    App, Context, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding, ScrollHandle,
    ScrollStrategy, Task, UniformListScrollHandle, Window, actions, div, prelude::*, px,
    uniform_list,
};

use crate::browse::{self, LIST_WIDTH, ROW_HEIGHT, ToggleFullWidth, press};
use crate::palette::PaletteCommand;
use crate::review::data::age;
use model::{Bar, Definition, Filters, Outcome, Tab};

actions!(
    pipelines,
    [
        /// Move the pipelines list selection down.
        SelectNextPipelineRow,
        /// Move the pipelines list selection up.
        SelectPreviousPipelineRow,
        /// Pipeline: show its runs. Run: move focus into its links.
        ActivatePipelineRow,
        /// Return focus to the pipelines list.
        FocusPipelineList,
        /// Next link in the run detail.
        NextPipelineLink,
        /// Previous link in the run detail.
        PreviousPipelineLink,
        /// Open the focused link.
        OpenPipelineLink,
        /// Next pipelines tab.
        NextPipelinesTab,
        /// Previous pipelines tab.
        PreviousPipelinesTab,
        /// Filter runs by the next branch.
        NextRunBranch,
        /// Filter runs by the previous branch.
        PreviousRunBranch,
        /// Clear the pipeline and branch filters.
        ClearRunFilters,
        /// Show pipeline definitions.
        ShowPipelineDefinitions,
        /// Show runs requested for me.
        ShowMyRuns,
        /// Show failed runs.
        ShowFailedRuns,
        /// Show every run.
        ShowAllRuns,
        /// Open the selected run in Azure DevOps.
        OpenRunInAdo,
    ]
);

const LIST_CONTEXT: &str = "PipelineList";
const DETAIL_CONTEXT: &str = "PipelineDetail";
const NEXT_BRANCH_KEYS: &str = "]";
const PREVIOUS_BRANCH_KEYS: &str = "[";
const CLEAR_KEYS: &str = "secondary-backspace";
/// Runs loaded; sync keeps 14 days (spec 0002 FR-3).
const RUN_LIMIT: usize = 5_000;
/// Recent runs listed in a definition's detail.
const DEFINITION_RUNS: usize = 10;
const SKELETON_ROWS: usize = 8;
/// Sparkline geometry: list rows / definition detail.
const SPARK_ROW: (f32, f32) = (4.0, 16.0);
const SPARK_DETAIL: (f32, f32) = (12.0, 48.0);

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(LIST_CONTEXT);
    let detail = Some(DETAIL_CONTEXT);
    vec![
        KeyBinding::new("down", SelectNextPipelineRow, list),
        KeyBinding::new("j", SelectNextPipelineRow, list),
        KeyBinding::new("up", SelectPreviousPipelineRow, list),
        KeyBinding::new("k", SelectPreviousPipelineRow, list),
        KeyBinding::new("enter", ActivatePipelineRow, list),
        KeyBinding::new("right", NextPipelinesTab, list),
        KeyBinding::new("left", PreviousPipelinesTab, list),
        KeyBinding::new(NEXT_BRANCH_KEYS, NextRunBranch, list),
        KeyBinding::new(PREVIOUS_BRANCH_KEYS, PreviousRunBranch, list),
        KeyBinding::new(CLEAR_KEYS, ClearRunFilters, list),
        KeyBinding::new("down", NextPipelineLink, detail),
        KeyBinding::new("j", NextPipelineLink, detail),
        KeyBinding::new("up", PreviousPipelineLink, detail),
        KeyBinding::new("k", PreviousPipelineLink, detail),
        KeyBinding::new("enter", OpenPipelineLink, detail),
        KeyBinding::new("escape", FocusPipelineList, detail),
    ]
}

/// Every pipelines action, for the Cmd+P command list.
pub fn palette_commands() -> Vec<PaletteCommand> {
    let command = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        command(
            "Pipelines: definitions",
            None,
            Box::new(ShowPipelineDefinitions),
        ),
        command("Pipelines: my runs", None, Box::new(ShowMyRuns)),
        command("Pipelines: failed runs", None, Box::new(ShowFailedRuns)),
        command("Pipelines: all runs", None, Box::new(ShowAllRuns)),
        command(
            "Pipelines: next branch",
            Some(NEXT_BRANCH_KEYS),
            Box::new(NextRunBranch),
        ),
        command(
            "Pipelines: previous branch",
            Some(PREVIOUS_BRANCH_KEYS),
            Box::new(PreviousRunBranch),
        ),
        command(
            "Pipelines: clear run filters",
            Some(CLEAR_KEYS),
            Box::new(ClearRunFilters),
        ),
        command("Open run in Azure DevOps", None, Box::new(OpenRunInAdo)),
    ]
}

pub fn new(store: Store, window: &mut Window, cx: &mut App) -> gpui::Entity<PipelinesView> {
    cx.new(|cx| PipelinesView::new(store, window, cx))
}

/// Requests for the shell (canonical views live elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelinesEvent {
    OpenPr(i64),
    OpenWorkItem(i64),
}

/// What the view needs from the store, read in one background job.
#[derive(Debug, Clone, Default)]
struct ListData {
    me: PersonId,
    names: HashMap<PersonId, String>,
    projects: HashMap<String, String>,
    pipelines: Vec<Pipeline>,
    /// Newest queued first.
    runs: Vec<PipelineRun>,
    org_url: Option<String>,
    demo: bool,
    last_sync: Option<Millis>,
}

impl ListData {
    fn read(store: &Store) -> adoken_store::Result<Self> {
        Ok(Self {
            me: store.me()?.map(|p| p.id).unwrap_or_default(),
            names: store
                .people()?
                .into_iter()
                .map(|p| (p.id, p.display_name))
                .collect(),
            projects: store
                .projects()?
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect(),
            pipelines: store.pipelines()?,
            runs: store.runs(&RunFilter {
                limit: Some(RUN_LIMIT),
                ..RunFilter::default()
            })?,
            org_url: store.org()?.map(|org| org.url),
            demo: store.meta("demo")?.is_some(),
            last_sync: store.last_sync()?,
        })
    }

    fn name(&self, id: &str) -> String {
        self.names.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }
}

/// Run detail, read in the background.
#[derive(Debug, Clone)]
struct RunDetail {
    run: PipelineRun,
    pr: Option<PullRequest>,
    stages: Vec<RunStage>,
}

impl RunDetail {
    fn read(store: &Store, id: i64) -> adoken_store::Result<Option<Self>> {
        let Some(run) = store.run(id)? else {
            return Ok(None);
        };
        let pr = match run.pr_id {
            Some(pr_id) => store.with_conn(|c| read::pr(c, pr_id))?,
            None => None,
        };
        let stages = store.run_stages(id)?;
        Ok(Some(Self { run, pr, stages }))
    }

    fn links(&self) -> Vec<RunLink> {
        self.run
            .pr_id
            .map(RunLink::Pr)
            .into_iter()
            .chain([RunLink::Pipeline(self.run.pipeline_id)])
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunLink {
    Pr(i64),
    /// Show every run of this definition in the list.
    Pipeline(i64),
}

enum Phase {
    Loading { skeleton_since: Option<Instant> },
    Ready,
    Failed(String),
}

enum Detail {
    Nothing,
    /// Index into `defs`.
    Definition(usize),
    Loading(i64),
    Missing(i64),
    Failed(String),
    Run(Box<RunDetail>),
}

pub struct PipelinesView {
    store: Store,
    phase: Phase,
    list: ListData,
    defs: Vec<Definition>,
    tab: Tab,
    filters: Filters,
    definition_project: Option<String>,
    /// Run tabs: indices into `list.runs`.
    visible: Vec<usize>,
    /// Row index (into `defs` on the Pipelines tab, else `visible`).
    selected: Option<usize>,
    detail: Detail,
    link: usize,
    full_width: bool,
    clock: Option<Millis>,
    list_focus: FocusHandle,
    detail_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    detail_scroll: ScrollHandle,
    _load: Option<Task<()>>,
    _detail: Option<Task<()>>,
    _grace: Option<Task<()>>,
    _watch: Option<Task<()>>,
    row_menu: adoken_ui::action_menu::MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    _pins: Option<gpui::Subscription>,
    opened_run: Option<i64>,
    /// Back / forward before the list landed (spec 0018 FR-8).
    pending_restore: Option<crate::shell::history::Pending<nav::PipelinesNavState>>,
    /// The detail's scroll to restore once the run it shows has loaded.
    restore_detail_scroll: Option<f32>,
}

impl EventEmitter<PipelinesEvent> for PipelinesView {}

impl PipelinesView {
    pub fn new(store: Store, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let watch = browse::reload_on_commit(&store, cx, |this: &mut Self, _, cx| this.load(cx));
        let grace = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(browse::LOADING_GRACE).await;
            this.update(cx, |this, cx| {
                if let Phase::Loading { skeleton_since } = &mut this.phase {
                    *skeleton_since = Some(Instant::now());
                    cx.notify();
                }
            })
            .ok();
        });
        let mut view = Self {
            store,
            phase: Phase::Loading {
                skeleton_since: None,
            },
            list: ListData::default(),
            defs: Vec::new(),
            tab: Tab::Pipelines,
            filters: Filters::default(),
            definition_project: None,
            visible: Vec::new(),
            selected: None,
            detail: Detail::Nothing,
            link: 0,
            full_width: false,
            clock: None,
            list_focus: cx.focus_handle(),
            detail_focus: cx.focus_handle(),
            list_scroll: UniformListScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            _load: None,
            _detail: None,
            _grace: Some(grace),
            _watch: Some(watch),
            row_menu: Default::default(),
            row_bounds: Default::default(),
            _pins: crate::pin_actions::observe(cx),
            opened_run: None,
            pending_restore: None,
            restore_detail_scroll: None,
        };
        view.load(cx);
        view
    }

    fn now(&self) -> Millis {
        self.clock.unwrap_or_else(browse::now_ms)
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        self._load = Some(cx.spawn(async move |this, cx| {
            let data = cx
                .background_spawn(async move { ListData::read(&store) })
                .await;
            let held = this
                .update(cx, |this, _| match this.phase {
                    Phase::Loading { skeleton_since } => {
                        browse::hold(skeleton_since, Instant::now())
                    }
                    _ => None,
                })
                .ok()
                .flatten();
            if let Some(wait) = held {
                cx.background_executor().timer(wait).await;
            }
            this.update(cx, |this, cx| {
                match data {
                    Ok(data) => this.set_list(data, cx),
                    Err(error) => {
                        log::warn!("pipelines: couldn't read the store: {error}");
                        this.phase = Phase::Failed(error.to_string());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Loading {
            skeleton_since: None,
        };
        self.load(cx);
        cx.notify();
    }

    fn set_list(&mut self, data: ListData, cx: &mut Context<Self>) {
        let first = !matches!(self.phase, Phase::Ready);
        let selected = self.selected_key();
        self.defs = model::definitions(&data.pipelines, &data.runs, self.now());
        self.list = data;
        self.phase = Phase::Ready;
        self.refilter();
        self.selected = selected
            .and_then(|key| self.row_of(&key))
            .or_else(|| (self.row_count() > 0).then_some(0));
        match self.detail_run_id() {
            // Opened before the list arrived (palette, Today): select it.
            Some(id) if first => self.selected = self.row_for_run(id),
            Some(id) => self.load_run(id, cx),
            None => self.show_selected(cx),
        }
        if first {
            self.apply_pending_restore(cx);
        }
    }

    /// The row showing run `id`: the run on run tabs, its definition on the
    /// Pipelines tab.
    fn row_for_run(&self, id: i64) -> Option<usize> {
        if self.tab.lists_runs() {
            return self.row_of(&RowKey::Run(id));
        }
        let run = self.list.runs.iter().find(|r| r.id == id)?;
        self.row_of(&RowKey::Pipeline(run.project_id.clone(), run.pipeline_id))
    }

    fn refilter(&mut self) {
        self.visible = model::runs(&self.list.runs, self.tab, &self.list.me, &self.filters);
        if let Some(project) = &self.definition_project {
            self.visible
                .retain(|ix| self.list.runs[*ix].project_id == *project);
        }
    }

    fn row_count(&self) -> usize {
        if self.tab.lists_runs() {
            self.visible.len()
        } else {
            self.defs.len()
        }
    }

    fn selected_key(&self) -> Option<RowKey> {
        let ix = self.selected?;
        if self.tab.lists_runs() {
            let run = self.list.runs.get(*self.visible.get(ix)?)?;
            Some(RowKey::Run(run.id))
        } else {
            let def = self.defs.get(ix)?;
            let p = &self.list.pipelines[def.pipeline];
            Some(RowKey::Pipeline(p.project_id.clone(), p.id))
        }
    }

    pub fn pin_subject(&self) -> Option<adoken_model::PinSubject> {
        match self.selected_key() {
            Some(RowKey::Pipeline(project_id, id)) => {
                Some(adoken_model::PinSubject::Definition { project_id, id })
            }
            Some(RowKey::Run(id)) => self.list.runs.iter().find(|run| run.id == id).map(|run| {
                adoken_model::PinSubject::Definition {
                    project_id: run.project_id.clone(),
                    id: run.pipeline_id,
                }
            }),
            None => match &self.detail {
                Detail::Run(detail) => Some(adoken_model::PinSubject::Definition {
                    project_id: detail.run.project_id.clone(),
                    id: detail.run.pipeline_id,
                }),
                _ => None,
            },
        }
    }

    pub fn open_definition(
        &mut self,
        project_id: &str,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.definition_project = Some(project_id.to_owned());
        self.tab = Tab::All;
        self.filters = Filters {
            pipeline: Some(id),
            branch: None,
        };
        self.refilter();
        self.selected = (!self.visible.is_empty()).then_some(0);
        self.show_selected(cx);
        crate::pin_actions::mark_opened(
            adoken_model::PinSubject::Definition {
                project_id: project_id.to_owned(),
                id,
            },
            cx,
        );
        window.focus(&self.list_focus, cx);
        cx.notify();
    }

    pub fn pin_title(&self) -> gpui::SharedString {
        match self.pin_subject() {
            Some(adoken_model::PinSubject::Definition { project_id, id }) => self
                .list
                .pipelines
                .iter()
                .find(|pipeline| pipeline.project_id == project_id && pipeline.id == id)
                .map(|pipeline| pipeline.name.clone())
                .or_else(|| {
                    self.list
                        .runs
                        .iter()
                        .find(|run| run.project_id == project_id && run.pipeline_id == id)
                        .map(|run| run.pipeline_name.clone())
                })
                .unwrap_or_default()
                .into(),
            _ => "".into(),
        }
    }

    fn open_row_menu(
        &mut self,
        position: adoken_ui::action_menu::MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let items = vec![
            adoken_ui::action_menu::MenuItem::new("Open", move |window, cx| {
                weak.update(cx, |this, cx| this.activate(window, cx)).ok();
            })
            .keys("enter"),
            crate::pin_actions::action("Open in browser", OpenRunInAdo).disabled(
                (self.list.demo || self.list.org_url.is_none())
                    .then_some("No browser target available"),
            ),
            crate::pin_actions::menu_item(self.pin_subject(), self.pin_title(), cx),
        ];
        self.row_menu.open(
            "Pipeline",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this| &mut this.row_menu,
            window,
            cx,
        );
    }

    fn row_of(&self, key: &RowKey) -> Option<usize> {
        match key {
            RowKey::Run(id) if self.tab.lists_runs() => self
                .visible
                .iter()
                .position(|&ix| self.list.runs[ix].id == *id),
            RowKey::Pipeline(project, id) if !self.tab.lists_runs() => {
                self.defs.iter().position(|d| {
                    let p = &self.list.pipelines[d.pipeline];
                    p.project_id == *project && p.id == *id
                })
            }
            _ => None,
        }
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.after_filter_change(cx);
    }

    fn after_filter_change(&mut self, cx: &mut Context<Self>) {
        let open = match &self.detail {
            Detail::Run(detail) => Some(RowKey::Run(detail.run.id)),
            _ => None,
        };
        self.refilter();
        self.selected = open
            .and_then(|key| self.row_of(&key))
            .or_else(|| (self.row_count() > 0).then_some(0));
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
        if let Some(ix) = self.selected {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        if let Some(subject) = self.pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        self.show_selected(cx);
        cx.notify();
    }

    fn cycle_branch(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.tab.lists_runs() {
            self.tab = Tab::All;
        }
        let branches = model::branches(
            &self.list.runs,
            self.tab,
            &self.list.me,
            self.filters.pipeline,
        );
        self.filters.branch = model::cycle_branch(&branches, self.filters.branch.as_deref(), delta);
        self.after_filter_change(cx);
    }

    fn clear_filters(&mut self, cx: &mut Context<Self>) {
        if self.filters.is_empty() {
            return;
        }
        self.filters = Filters::default();
        self.definition_project = None;
        self.after_filter_change(cx);
    }

    /// Runs of pipeline `id`, keeping the run tab (Pipelines → All runs).
    fn show_pipeline_runs(&mut self, id: i64, cx: &mut Context<Self>) {
        self.definition_project = None;
        if !self.tab.lists_runs() {
            self.tab = Tab::All;
        }
        self.filters = Filters {
            pipeline: Some(id),
            branch: None,
        };
        self.after_filter_change(cx);
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.row_count();
        if n == 0 {
            return;
        }
        let current = self.selected.unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, n as isize - 1) as usize;
        if Some(next) == self.selected {
            return;
        }
        self.selected = Some(next);
        if let Some(subject) = self.pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        self.show_selected(cx);
        cx.notify();
    }

    fn show_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected_key() {
            Some(RowKey::Run(id)) => self.load_run(id, cx),
            Some(RowKey::Pipeline(..)) => {
                self.detail = Detail::Definition(self.selected.unwrap_or(0));
                self._detail = None;
                self.link = 0;
            }
            None => {
                self.detail = Detail::Nothing;
                self._detail = None;
            }
        }
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_key() {
            Some(RowKey::Pipeline(_, id)) => self.show_pipeline_runs(id, cx),
            Some(RowKey::Run(_)) if matches!(self.detail, Detail::Run(_)) => {
                self.link = 0;
                window.focus(&self.detail_focus, cx);
                cx.notify();
            }
            _ => {}
        }
    }

    /// Opens run `id` (palette, Today, inbox): selects it when the current
    /// tab lists it (on the Pipelines tab, its definition) and shows it.
    pub fn open_run(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.opened_run = Some(id);
        self.selected = self.row_for_run(id);
        if let Some(run) = self.list.runs.iter().find(|run| run.id == id) {
            crate::pin_actions::mark_opened(
                adoken_model::PinSubject::Definition {
                    project_id: run.project_id.clone(),
                    id: run.pipeline_id,
                },
                cx,
            );
        }
        if let Some(ix) = self.selected {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        self.load_run(id, cx);
        window.focus(&self.list_focus, cx);
        cx.notify();
    }

    fn detail_run_id(&self) -> Option<i64> {
        match &self.detail {
            Detail::Loading(id) | Detail::Missing(id) => Some(*id),
            Detail::Run(detail) => Some(detail.run.id),
            _ => None,
        }
    }

    fn load_run(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.detail_run_id() != Some(id) {
            self.detail = Detail::Loading(id);
            self.link = 0;
            self.restore_detail_scroll = None;
            self.detail_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        }
        let store = self.store.clone();
        self._detail = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move { RunDetail::read(&store, id) })
                .await;
            this.update(cx, |this, cx| {
                if this.detail_run_id() != Some(id) {
                    return;
                }
                this.detail = match loaded {
                    Ok(Some(detail)) => {
                        if this.opened_run.take() == Some(id) {
                            crate::pin_actions::mark_opened(
                                adoken_model::PinSubject::Definition {
                                    project_id: detail.run.project_id.clone(),
                                    id: detail.run.pipeline_id,
                                },
                                cx,
                            );
                        }
                        this.link = this.link.min(detail.links().len().saturating_sub(1));
                        if let Some(top) = this.restore_detail_scroll.take() {
                            this.set_detail_scroll(top);
                        }
                        Detail::Run(Box::new(detail))
                    }
                    Ok(None) => Detail::Missing(id),
                    Err(error) => Detail::Failed(error.to_string()),
                };
                cx.notify();
            })
            .ok();
        }));
    }

    fn links(&self) -> Vec<RunLink> {
        match &self.detail {
            Detail::Run(detail) => detail.links(),
            _ => Vec::new(),
        }
    }

    fn step_link(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.links().len();
        if n == 0 {
            return;
        }
        self.link = (self.link as isize + delta).clamp(0, n as isize - 1) as usize;
        cx.notify();
    }

    fn open_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.links().get(self.link).copied() {
            Some(RunLink::Pr(id)) => cx.emit(PipelinesEvent::OpenPr(id)),
            Some(RunLink::Pipeline(id)) => {
                window.focus(&self.list_focus, cx);
                self.show_pipeline_runs(id, cx);
            }
            None => {}
        }
    }

    fn open_in_ado(&mut self, cx: &mut Context<Self>) {
        if self.list.demo {
            return;
        }
        let Some(org) = self.list.org_url.as_deref() else {
            return;
        };
        let target = match self.selected_key() {
            Some(RowKey::Pipeline(project, id)) => Some((project, "definitionId", id)),
            Some(RowKey::Run(id)) => self
                .list
                .runs
                .iter()
                .find(|run| run.id == id)
                .map(|run| (run.project_id.clone(), "buildId", id)),
            None => match &self.detail {
                Detail::Run(detail) => {
                    Some((detail.run.project_id.clone(), "buildId", detail.run.id))
                }
                _ => None,
            },
        };
        let Some((project_id, parameter, id)) = target else {
            return;
        };
        let project = self
            .list
            .projects
            .get(&project_id)
            .map_or(project_id.as_str(), String::as_str);
        let Ok(mut url) = url::Url::parse(org) else {
            return;
        };
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.pop_if_empty().extend([project, "_build"]);
            if parameter == "buildId" {
                segments.push("results");
            }
        } else {
            return;
        }
        url.query_pairs_mut()
            .append_pair(parameter, &id.to_string());
        if let Some(subject) = self.pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        cx.open_url(url.as_str());
    }

    // ── rendering ────────────────────────────────────────────────────────

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let selected_tab = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let labels = Tab::ALL.map(|tab| match tab {
            Tab::All => tab.label().to_owned(),
            _ if !matches!(self.phase, Phase::Ready) => tab.label().to_owned(),
            Tab::Pipelines => format!("{} {}", tab.label(), self.defs.len()),
            _ => format!(
                "{} {}",
                tab.label(),
                model::runs(&self.list.runs, tab, &self.list.me, &Filters::default()).len()
            ),
        });
        let tabs = Segmented::new("pipe-tabs", labels, selected_tab).on_select({
            let view = cx.entity().downgrade();
            move |ix, _, cx| {
                view.update(cx, |this, cx| this.set_tab(Tab::ALL[ix], cx))
                    .ok();
            }
        });
        let stale = match self.phase {
            Phase::Ready => browse::staleness(self.list.last_sync, self.now()),
            _ => None,
        };
        let header = div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .px(px(space::S12))
            .py(px(space::S12))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::HEADING, &theme)
                            .child("Pipelines"),
                    )
                    .children(stale.map(|label| {
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(label)
                    })),
            )
            .child(tabs)
            .when(self.tab.lists_runs(), |header| {
                header.child(self.render_filters(&theme, cx))
            });

        let body = match &self.phase {
            Phase::Loading { skeleton_since } => match skeleton_since {
                Some(_) => browse::skeleton_rows("pipe-skeleton", SKELETON_ROWS).into_any_element(),
                None => div().flex_1().into_any_element(),
            },
            Phase::Failed(error) => div()
                .p(px(space::S12))
                .child(
                    Notice::new(
                        ChipTone::Danger,
                        "Couldn't read pipelines from the local store.",
                    )
                    .icon(IconName::CircleX)
                    .detail(format!("Your synced data is untouched. {error}"))
                    .child(
                        Button::new("pipe-retry", "Retry")
                            .on_press(press(cx, |this: &mut Self, _, cx| this.retry(cx))),
                    ),
                )
                .into_any_element(),
            Phase::Ready if self.row_count() == 0 => self.render_empty(cx).into_any_element(),
            Phase::Ready => uniform_list(
                "pipe-list",
                self.row_count(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme().clone();
                    range
                        .map(|ix| {
                            if this.tab.lists_runs() {
                                this.render_run_row(ix, &theme, cx)
                            } else {
                                this.render_definition_row(ix, &theme, cx)
                            }
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.list_scroll)
            .flex_1()
            .into_any_element(),
        };

        div()
            .id("pipe-list-pane")
            .key_context(LIST_CONTEXT)
            .track_focus(&self.list_focus)
            .flex()
            .flex_col()
            .flex_none()
            .w(px(LIST_WIDTH))
            .h_full()
            .bg(colors.bg_surface)
            .border_r_1()
            .border_color(colors.border_subtle)
            .on_action(cx.listener(|this, _: &SelectNextPipelineRow, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPreviousPipelineRow, _, cx| this.step(-1, cx)))
            .on_action(
                cx.listener(|this, _: &ActivatePipelineRow, window, cx| this.activate(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &NextPipelinesTab, _, cx| this.set_tab(this.tab.step(1), cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousPipelinesTab, _, cx| {
                this.set_tab(this.tab.step(-1), cx)
            }))
            .child(header)
            .child(body)
    }

    fn render_filters(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &theme.colors;
        let pipeline = self.filters.pipeline.and_then(|id| {
            self.list
                .pipelines
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.name.clone())
        });
        let branch = self
            .filters
            .branch
            .as_deref()
            .map_or_else(|| "All branches".to_owned(), model::branch_label);
        let chevron = |id: &'static str, icon: IconName, delta: isize| {
            div()
                .id(id)
                .flex_none()
                .size(px(size::HIT_MIN))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::SM))
                .cursor_pointer()
                .hover(|s| s.bg(colors.bg_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.cycle_branch(delta, cx)))
                .child(
                    Icon::new(icon)
                        .size(size::ICON_SM)
                        .color(colors.text_secondary),
                )
        };
        div()
            .flex()
            .flex_col()
            .gap(px(space::S6))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .child(
                        Icon::new(IconName::GitBranch)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(chevron("pipe-branch-prev", IconName::ChevronLeft, -1))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY_STRONG, theme)
                            .child(branch),
                    )
                    .child(chevron("pipe-branch-next", IconName::ChevronRight, 1))
                    .child(Kbd::parse(PREVIOUS_BRANCH_KEYS).expect("static binding"))
                    .child(Kbd::parse(NEXT_BRANCH_KEYS).expect("static binding")),
            )
            .when_some(pipeline, |col, name| {
                col.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .child(Chip::new(format!("Pipeline: {name}")).tone(ChipTone::Accent))
                        .child(div().flex_1())
                        .child(
                            Button::new("pipe-clear", "Clear filters")
                                .variant(ButtonVariant::Ghost)
                                .shortcut(Kbd::parse(CLEAR_KEYS).expect("static binding"))
                                .on_press(press(cx, |this: &mut Self, _, cx| {
                                    this.clear_filters(cx)
                                })),
                        ),
                )
            })
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = if self.tab.lists_runs() && !self.filters.is_empty() {
            EmptyState::new(IconName::Search, "No runs match these filters").action(
                Button::new("pipe-empty-clear", "Clear filters")
                    .shortcut(Kbd::parse(CLEAR_KEYS).expect("static binding"))
                    .on_press(press(cx, |this: &mut Self, _, cx| this.clear_filters(cx))),
            )
        } else {
            let (icon, headline, body) = match self.tab {
                Tab::Pipelines => (
                    IconName::Workflow,
                    "No pipelines synced yet",
                    "Pipelines appear here after the first sync.",
                ),
                Tab::Mine => (
                    IconName::Workflow,
                    "No runs of yours in the last 14 days",
                    "Runs requested for you show up here.",
                ),
                Tab::Failed => (
                    IconName::CircleCheck,
                    "No failed runs",
                    "Nothing broke in the last 14 days.",
                ),
                Tab::All => (
                    IconName::Workflow,
                    "No runs synced yet",
                    "Runs from the last 14 days appear here after the first sync.",
                ),
            };
            let empty = EmptyState::new(icon, headline).body(body);
            // Real orgs sync a 7-day window; older runs are one request away.
            if self.list.demo || self.tab == Tab::Failed {
                empty
            } else {
                empty.action(Button::new("pipe-empty-older", "Load older runs").on_press(
                    |window, cx| window.dispatch_action(Box::new(crate::sync::LoadOlderRuns), cx),
                ))
            }
        };
        div().flex_1().child(empty)
    }

    fn render_definition_row(
        &self,
        ix: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let colors = &theme.colors;
        let def = &self.defs[ix];
        let pipeline = &self.list.pipelines[def.pipeline];
        let latest = def.latest().map(|r| &self.list.runs[r]);
        let (icon, tint) = latest.map_or((IconName::CircleDashed, colors.text_tertiary), |run| {
            outcome_style(Outcome::of(run), colors)
        });
        let folder = pipeline
            .folder
            .trim_start_matches('\\')
            .replace('\\', " / ");
        let subtitle = match latest {
            Some(run) => format!(
                "{} · last run {}",
                if folder.is_empty() {
                    "\\"
                } else {
                    folder.as_str()
                },
                Outcome::of(run).label().to_lowercase()
            ),
            None => format!("{folder} · no runs in 14 days"),
        };
        let mut row = ListRow::new(("pipe-def", ix), pipeline.name.clone())
            .height(ROW_HEIGHT)
            .leading(Icon::new(icon).size(size::ICON_MD).color(tint))
            .subtitle(subtitle)
            .selected(self.selected == Some(ix))
            .track_selected_bounds(self.row_bounds.clone())
            .on_secondary_click(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.selected = Some(ix);
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                },
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.selected = Some(ix);
                if let Some(subject) = this.pin_subject() {
                    crate::pin_actions::mark_opened(subject, cx);
                }
                this.show_selected(cx);
                cx.notify();
            }))
            .trailing(sparkline(&def.spark, SPARK_ROW, colors));
        if let Some(run) = latest {
            row = row.trailing(div().child(age(self.now(), run.queued)));
        }
        row.into_any_element()
    }

    fn render_run_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = &theme.colors;
        let run = &self.list.runs[self.visible[ix]];
        let (icon, tint) = outcome_style(Outcome::of(run), colors);
        let who = run
            .requested_for
            .as_deref()
            .map_or_else(String::new, |id| self.list.name(id));
        let subtitle = format!(
            "#{} · {} · {who} · {}",
            run.run_number,
            model::branch_label(&run.branch),
            model::reason_label(run.reason)
        );
        let now = self.now();
        let id = run.id;
        let mut row = ListRow::new(("pipe-run", ix), run.pipeline_name.clone())
            .height(ROW_HEIGHT)
            .leading(Icon::new(icon).size(size::ICON_MD).color(tint))
            .subtitle(subtitle)
            .selected(self.selected == Some(ix))
            .track_selected_bounds(self.row_bounds.clone())
            .on_secondary_click(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.selected = Some(ix);
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                },
            ))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.selected = Some(ix);
                    if event.modifiers().platform {
                        this.open_in_ado(cx);
                    } else {
                        this.open_run(id, window, cx);
                    }
                }),
            );
        if let Some(d) = model::duration(run, now) {
            row = row.trailing(div().child(model::format_duration(d)));
        }
        row.trailing(div().child(age(now, run.queued)))
            .into_any_element()
    }

    fn render_detail(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let container = div()
            .id("pipe-detail")
            .key_context(DETAIL_CONTEXT)
            .track_focus(&self.detail_focus)
            .flex_1()
            .min_w_0()
            .h_full()
            .text_color(theme.colors.text_primary)
            .on_action(cx.listener(|this, _: &NextPipelineLink, _, cx| this.step_link(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousPipelineLink, _, cx| this.step_link(-1, cx)))
            .on_action(
                cx.listener(|this, _: &OpenPipelineLink, window, cx| this.open_link(window, cx)),
            )
            .on_action(cx.listener(|this, _: &FocusPipelineList, window, cx| {
                this.full_width = false;
                window.focus(&this.list_focus, cx);
                cx.notify();
            }));
        match &self.detail {
            Detail::Run(detail) => container
                .overflow_y_scroll()
                .track_scroll(&self.detail_scroll)
                .child(self.render_run(detail, &theme, window, cx))
                .into_any_element(),
            Detail::Definition(ix) => container
                .overflow_y_scroll()
                .track_scroll(&self.detail_scroll)
                .child(self.render_definition(*ix, &theme, cx))
                .into_any_element(),
            Detail::Nothing => container
                .child(
                    EmptyState::new(IconName::Workflow, "Select a pipeline or run")
                        .body("↑ ↓ to move, ← → to switch tabs, Enter to show a pipeline's runs."),
                )
                .into_any_element(),
            Detail::Loading(_) => container.into_any_element(),
            Detail::Missing(id) => container
                .child(
                    EmptyState::new(IconName::CircleAlert, format!("Run {id} isn't synced yet"))
                        .body("Runs from the last 14 days appear here after the next sync."),
                )
                .into_any_element(),
            Detail::Failed(error) => container
                .child(
                    EmptyState::new(IconName::CircleX, "Couldn't read this run")
                        .body(format!("Your synced data is untouched. {error}")),
                )
                .into_any_element(),
        }
    }

    fn open_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("pipe-open-ado", "Open in Azure DevOps")
            .variant(ButtonVariant::Secondary)
            .icon(IconName::ExternalLink)
            .disabled(self.list.demo || self.list.org_url.is_none())
            .tooltip(if self.list.demo {
                "Demo org: fake data, nothing opens in Azure DevOps"
            } else {
                "Opens this run in your browser"
            })
            .on_press(press(cx, |this: &mut Self, _, cx| this.open_in_ado(cx)))
    }

    fn render_run(
        &self,
        detail: &RunDetail,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = &theme.colors;
        let run = &detail.run;
        let now = self.now();
        let outcome = Outcome::of(run);
        let (icon, tint) = outcome_style(outcome, colors);
        let focused = self.detail_focus.is_focused(window);
        let links = detail.links();
        let link_selected = |link: RunLink| focused && links.get(self.link) == Some(&link);
        let when = |at: Option<Millis>| at.map_or_else(|| "—".to_owned(), |t| age(now, t));
        let duration = match (model::duration(run, now), outcome) {
            (Some(d), Outcome::Running) => format!("Running for {}", model::format_duration(d)),
            (Some(d), _) => model::format_duration(d),
            (None, _) => "Not started".to_owned(),
        };
        let project = self
            .list
            .projects
            .get(&run.project_id)
            .cloned()
            .unwrap_or_else(|| run.project_id.clone());
        let fields = div()
            .flex()
            .flex_col()
            .gap(px(space::S6))
            .pt(px(space::S8))
            .type_style(typography::BODY, theme)
            .child(browse::field("Duration", duration, theme))
            .child(browse::field(
                "Requested for",
                match run.requested_for.as_deref() {
                    Some(id) => browse::person(id, self.list.name(id)),
                    None => div().child("—"),
                },
                theme,
            ))
            .child(browse::field(
                "Reason",
                model::reason_label(run.reason),
                theme,
            ))
            .child(browse::field(
                "Branch",
                format!("{} ({})", model::branch_label(&run.branch), run.branch),
                theme,
            ))
            .child(browse::field(
                "Commit",
                run.source_version
                    .as_deref()
                    .map_or_else(|| "—".to_owned(), |v| v.chars().take(8).collect()),
                theme,
            ))
            .child(browse::field("Queued", when(Some(run.queued)), theme))
            .child(browse::field("Started", when(run.started), theme))
            .child(browse::field("Finished", when(run.finish), theme))
            .child(browse::field("Project", project, theme));

        let stages_section = browse::section("Stages", theme)
            .children(detail.stages.iter().map(|stage| {
                let (s_icon, s_tint) = stage_style(stage.outcome, colors);
                let took = match (stage.started, stage.finished) {
                    (Some(a), Some(b)) => model::format_duration(b - a),
                    (Some(a), None) => format!("{} so far", model::format_duration(now - a)),
                    _ => String::new(),
                };
                div()
                    .h(px(size::CONTROL_LG))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .rounded(px(radius::SM))
                    .bg(colors.bg_raised)
                    .type_style(typography::BODY, theme)
                    .child(Icon::new(s_icon).size(size::ICON_SM).color(s_tint))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(stage.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(stage.outcome.label()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(72.0))
                            .text_right()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, theme)
                            .child(took),
                    )
            }))
            .when(detail.stages.is_empty(), |s| {
                s.child(
                    div()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::BODY, theme)
                        .child("Stage details aren't synced yet."),
                )
            });

        let link_row = |id: gpui::ElementId, selected: bool| {
            div()
                .id(id)
                .h(px(size::CONTROL_LG))
                .px(px(space::S8))
                .flex()
                .items_center()
                .gap(px(space::S8))
                .rounded(px(radius::SM))
                .cursor_pointer()
                .when(selected, |d| d.bg(colors.bg_selected))
                .when(!selected, |d| d.hover(|s| s.bg(colors.bg_hover)))
                .type_style(typography::BODY, theme)
        };
        let mut links_section = browse::section("Links", theme);
        if let Some(pr_id) = run.pr_id {
            let (pr_icon, pr_color, status) = match &detail.pr {
                Some(pr) => {
                    let (icon, color) = pr_style(pr.status, pr.is_draft, colors);
                    (icon, color, crate::palette::catalog::pr_status_label(pr))
                }
                None => (IconName::GitPullRequest, colors.text_tertiary, "Not synced"),
            };
            let title = detail.pr.as_ref().map_or_else(
                || format!("!{pr_id}"),
                |pr| format!("!{pr_id} {}", pr.title),
            );
            links_section = links_section.child(
                link_row(
                    ("pipe-pr", pr_id as usize).into(),
                    link_selected(RunLink::Pr(pr_id)),
                )
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(PipelinesEvent::OpenPr(pr_id))))
                .child(Icon::new(pr_icon).size(size::ICON_SM).color(pr_color))
                .child(div().flex_1().min_w_0().truncate().child(title))
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, theme)
                        .child(status),
                ),
            );
        }
        let pipeline_id = run.pipeline_id;
        links_section = links_section
            .child(
                link_row(
                    ("pipe-all-runs", pipeline_id as usize).into(),
                    link_selected(RunLink::Pipeline(pipeline_id)),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.list_focus, cx);
                    this.show_pipeline_runs(pipeline_id, cx);
                }))
                .child(
                    Icon::new(IconName::Workflow)
                        .size(size::ICON_SM)
                        .color(colors.text_secondary),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(format!("All runs of {}", run.pipeline_name)),
                ),
            )
            .child(
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child("Enter from the list, then ↑ ↓ and Enter to open. Esc goes back."),
            );

        div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .p(px(space::S24))
            .max_w(px(880.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(Icon::new(icon).size(size::ICON_MD).color(tint))
                    .child(
                        div()
                            .flex_1()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, theme)
                            .child(format!("Run · #{}", run.run_number)),
                    )
                    .child(crate::pin_actions::button(
                        adoken_model::PinSubject::Definition {
                            project_id: run.project_id.clone(),
                            id: run.pipeline_id,
                        },
                        run.pipeline_name.clone().into(),
                        cx,
                    ))
                    .child(self.open_button(cx)),
            )
            .child(
                div()
                    .text_color(colors.text_primary)
                    .type_style(typography::TITLE, theme)
                    .child(run.pipeline_name.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .child(Chip::new(outcome.label()).tone(outcome_tone(outcome)))
                    .child(Chip::new(model::reason_label(run.reason))),
            )
            .child(fields)
            .child(stages_section)
            .child(links_section)
    }

    fn render_definition(
        &self,
        ix: usize,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = &theme.colors;
        let now = self.now();
        let Some(def) = self.defs.get(ix) else {
            return div();
        };
        let pipeline = &self.list.pipelines[def.pipeline];
        let runs: Vec<&PipelineRun> = def.runs.iter().map(|&r| &self.list.runs[r]).collect();
        let stats = model::stats(&runs);
        let project = self
            .list
            .projects
            .get(&pipeline.project_id)
            .cloned()
            .unwrap_or_else(|| pipeline.project_id.clone());
        let fields = div()
            .flex()
            .flex_col()
            .gap(px(space::S6))
            .pt(px(space::S8))
            .type_style(typography::BODY, theme)
            .child(browse::field("Project", project, theme))
            .child(browse::field("Folder", pipeline.folder.clone(), theme))
            .child(browse::field(
                "Runs (14 days)",
                stats.runs.to_string(),
                theme,
            ))
            .child(browse::field(
                "Pass rate",
                stats
                    .pass_rate
                    .map_or_else(|| "—".to_owned(), |r| format!("{:.0}%", r * 100.0)),
                theme,
            ))
            .child(browse::field(
                "Median duration",
                stats
                    .median
                    .map_or_else(|| "—".to_owned(), model::format_duration),
                theme,
            ));
        let pipeline_id = pipeline.id;
        let recent = browse::section(format!("Last {} runs", def.spark.len()), theme)
            .child(sparkline(&def.spark, SPARK_DETAIL, colors))
            .child(
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child("Oldest to newest; bar height is duration."),
            );
        let list =
            browse::section("Recent runs", theme)
                .children(
                    runs.iter()
                        .take(DEFINITION_RUNS)
                        .enumerate()
                        .map(|(n, run)| {
                            let (r_icon, r_tint) = outcome_style(Outcome::of(run), colors);
                            let id = run.id;
                            div()
                                .id(("pipe-def-run", n))
                                .h(px(size::CONTROL_LG))
                                .px(px(space::S8))
                                .flex()
                                .items_center()
                                .gap(px(space::S8))
                                .rounded(px(radius::SM))
                                .cursor_pointer()
                                .hover(|s| s.bg(colors.bg_hover))
                                .type_style(typography::BODY, theme)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_run(id, window, cx)
                                }))
                                .child(Icon::new(r_icon).size(size::ICON_SM).color(r_tint))
                                .child(div().flex_1().min_w_0().truncate().child(format!(
                                    "#{} · {}",
                                    run.run_number,
                                    model::branch_label(&run.branch)
                                )))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(colors.text_tertiary)
                                        .type_style(typography::SMALL, theme)
                                        .child(format!(
                                            "{} · {}",
                                            model::duration(run, now)
                                                .map_or_else(String::new, model::format_duration),
                                            age(now, run.queued)
                                        )),
                                )
                        }),
                )
                .when(runs.is_empty(), |s| {
                    s.child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::BODY, theme)
                            .child("No runs in the last 14 days."),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(space::S8))
                        .pt(px(space::S4))
                        .child(
                            Button::new("pipe-show-runs", "Show all runs")
                                .variant(ButtonVariant::Secondary)
                                .shortcut(Kbd::parse("enter").expect("static binding"))
                                .on_press(press(cx, move |this: &mut Self, _, cx| {
                                    this.show_pipeline_runs(pipeline_id, cx)
                                })),
                        ),
                );
        div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .p(px(space::S24))
            .max_w(px(880.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Icon::new(IconName::Workflow)
                            .size(size::ICON_MD)
                            .color(colors.text_secondary),
                    )
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, theme)
                            .child("Pipeline"),
                    )
                    .child(div().flex_1())
                    .child(crate::pin_actions::button(
                        adoken_model::PinSubject::Definition {
                            project_id: pipeline.project_id.clone(),
                            id: pipeline.id,
                        },
                        pipeline.name.clone().into(),
                        cx,
                    )),
            )
            .child(
                div()
                    .text_color(colors.text_primary)
                    .type_style(typography::TITLE, theme)
                    .child(pipeline.name.clone()),
            )
            .child(fields)
            .child(recent)
            .child(list)
    }
}

/// Selection identity across reloads and tab switches.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKey {
    Run(i64),
    Pipeline(String, i64),
}

/// Bars bottom-aligned; queued runs draw a 2 px stub.
fn sparkline(
    bars: &[Bar],
    (bar_width, height): (f32, f32),
    colors: &ColorTokens<Hsla>,
) -> gpui::Div {
    div()
        .flex_none()
        .h(px(height))
        .flex()
        .items_end()
        .gap(px(1.0))
        .children(bars.iter().map(|bar| {
            let (_, color) = outcome_style(bar.outcome, colors);
            div()
                .w(px(bar_width))
                .h(px((bar.height * height).max(space::S2)))
                .rounded(px(1.0))
                .bg(color)
        }))
}

/// Icon + `pipeline.*` color (tokens.md §2.8).
pub fn outcome_style<C: Copy>(outcome: Outcome, colors: &ColorTokens<C>) -> (IconName, C) {
    match outcome {
        Outcome::Succeeded => (IconName::CircleCheck, colors.pipeline_succeeded),
        Outcome::Failed => (IconName::CircleX, colors.pipeline_failed),
        Outcome::Partial => (IconName::CircleAlert, colors.pipeline_partial),
        Outcome::Running => (IconName::LoaderCircle, colors.pipeline_running),
        Outcome::Queued => (IconName::CircleDotDashed, colors.pipeline_queued),
        Outcome::Canceled => (IconName::CircleSlash, colors.pipeline_canceled),
    }
}

fn stage_style<C: Copy>(outcome: StageOutcome, colors: &ColorTokens<C>) -> (IconName, C) {
    match outcome {
        StageOutcome::Succeeded => outcome_style(Outcome::Succeeded, colors),
        StageOutcome::Partial => outcome_style(Outcome::Partial, colors),
        StageOutcome::Failed => outcome_style(Outcome::Failed, colors),
        StageOutcome::Canceled => outcome_style(Outcome::Canceled, colors),
        StageOutcome::Running => outcome_style(Outcome::Running, colors),
        StageOutcome::Pending => outcome_style(Outcome::Queued, colors),
        StageOutcome::Skipped => (IconName::CircleDashed, colors.text_tertiary),
    }
}

fn outcome_tone(outcome: Outcome) -> ChipTone {
    match outcome {
        Outcome::Succeeded => ChipTone::Success,
        Outcome::Failed => ChipTone::Danger,
        Outcome::Partial => ChipTone::Warning,
        Outcome::Running => ChipTone::Accent,
        Outcome::Queued | Outcome::Canceled => ChipTone::Neutral,
    }
}

fn pr_style(status: PrStatus, draft: bool, colors: &ColorTokens<Hsla>) -> (IconName, Hsla) {
    match (status, draft) {
        (PrStatus::Active, true) => (IconName::GitPullRequestDraft, colors.pr_draft),
        (PrStatus::Active, false) => (IconName::GitPullRequest, colors.pr_active),
        (PrStatus::Completed, _) => (IconName::GitMerge, colors.pr_completed),
        (PrStatus::Abandoned, _) => (IconName::GitPullRequestClosed, colors.pr_abandoned),
    }
}

impl Focusable for PipelinesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.list_focus.clone()
    }
}

impl Render for PipelinesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        div()
            .id("pipelines")
            .size_full()
            .flex()
            .bg(colors.bg_app)
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                crate::pin_actions::toggle(this.pin_subject(), this.pin_title(), window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &crate::pin_actions::OpenRowMenu, window, cx| {
                    this.open_row_menu(
                        adoken_ui::action_menu::keyboard_position(&this.row_bounds, window),
                        window,
                        cx,
                    );
                }),
            )
            .on_action(cx.listener(|this, _: &ShowPipelineDefinitions, _, cx| {
                this.set_tab(Tab::Pipelines, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowMyRuns, _, cx| this.set_tab(Tab::Mine, cx)))
            .on_action(cx.listener(|this, _: &ShowFailedRuns, _, cx| this.set_tab(Tab::Failed, cx)))
            .on_action(cx.listener(|this, _: &ShowAllRuns, _, cx| this.set_tab(Tab::All, cx)))
            .on_action(cx.listener(|this, _: &NextRunBranch, _, cx| this.cycle_branch(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousRunBranch, _, cx| this.cycle_branch(-1, cx)))
            .on_action(cx.listener(|this, _: &ClearRunFilters, _, cx| this.clear_filters(cx)))
            .on_action(cx.listener(|this, _: &OpenRunInAdo, _, cx| this.open_in_ado(cx)))
            .on_action(cx.listener(|this, _: &ToggleFullWidth, _, cx| {
                this.full_width = !this.full_width;
                cx.notify();
            }))
            .when(!self.full_width, |this| this.child(self.render_list(cx)))
            .child(self.render_detail(window, cx))
            .children(self.row_menu.element())
    }
}

/// Gallery stories: the view on a seeded demo store at `DEMO_NOW`.
pub mod story {
    use super::*;

    /// A view over the demo store in `dir` with `tab` shown and the clock
    /// pinned; `run` opens that run's detail.
    pub fn view(
        dir: &std::path::Path,
        tab: Tab,
        run: Option<i64>,
        window: &mut Window,
        cx: &mut Context<PipelinesView>,
    ) -> PipelinesView {
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir, now).expect("seed the demo store");
        let mut view = PipelinesView::new(store, window, cx);
        view.clock = Some(now);
        view.tab = tab;
        if let Some(id) = run {
            view.detail = Detail::Loading(id);
            view.load_run(id, cx);
        }
        view
    }
}
