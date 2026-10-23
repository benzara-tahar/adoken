//! Work items view (spec 0008, spec 0015): My work / Assigned / Recently
//! changed / By area / By iteration tabs, each grouped by state, and the
//! selected item's detail ([`detail`]): rich description, grouped links,
//! discussion and history, every field editable in place.
//!
//! Store reads run on the background executor; the list filters in memory
//! (`model`), so tab switches and selection moves never touch the store.
//! Edits go through the write controller ([`writes`]), which owns the
//! per-item write queue, pending states and per-item sync.

pub mod data;
pub mod detail;
pub mod model;
pub mod nav;
pub mod paths;
pub mod pick;
pub mod writes;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use adoken_model::{PersonId, StateCategory, TypeFamily, WorkItemType};
use adoken_store::Store;
use adoken_theme::{ColorTokens, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ChipTone, EmptyState, Icon, IconName, ListRow, Notice, Segmented, Theme,
    TypeStyleExt,
};
use gpui::{
    AnyWindowHandle, App, AsyncApp, Context, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ScrollHandle, ScrollStrategy, Subscription, Task, UniformListScrollHandle, Window, actions,
    canvas, div, prelude::*, px, uniform_list,
};

use crate::browse::{self, LIST_WIDTH, ROW_HEIGHT, ToggleFullWidth, press};
use crate::palette::PaletteCommand;
use crate::review::data::age;
use data::{DetailData, ListData};
use detail::model::{Arrangement, Stop};
use detail::{DETAIL_CONTEXT, DetailUi, LIST_CONTEXT, VIEW_CONTEXT};
use model::{Row, Tab};

actions!(
    work_items,
    [
        /// Move the work item selection down.
        SelectNextWorkItem,
        /// Move the work item selection up.
        SelectPreviousWorkItem,
        /// Move focus into the selected work item's detail.
        FocusWorkItemDetail,
        /// Return focus to the work item list.
        FocusWorkItemList,
        /// Next work item tab.
        NextWorkItemTab,
        /// Previous work item tab.
        PreviousWorkItemTab,
        /// Next area or iteration path.
        NextWorkItemPath,
        /// Previous area or iteration path.
        PreviousWorkItemPath,
        /// Show my open work.
        ShowMyWork,
        /// Show work items assigned to me.
        ShowAssigned,
        /// Show recently changed work items.
        ShowRecentlyChanged,
        /// Show open work items by area.
        ShowByArea,
        /// Show open work items by iteration.
        ShowByIteration,
        /// Open the selected work item in Azure DevOps.
        OpenWorkItemInAdo,
        /// Open the area / iteration dropdown of the shown path tab (`/`).
        OpenWorkItemPathFilter,
        /// By area, with its dropdown open (from any view).
        FilterWorkItemsByArea,
        /// By iteration, with its dropdown open (from any view).
        FilterWorkItemsByIteration,
    ]
);

const NEXT_PATH_KEYS: &str = "]";
const PREVIOUS_PATH_KEYS: &str = "[";
const PATH_FILTER_KEYS: &str = "/";
const SKELETON_ROWS: usize = 8;
/// principles §2: below this window width the detail slides over the list
/// (design §2.4).
const STACK_BELOW: f32 = 1100.0;
/// The pane width assumed before the first paint measures it.
const DEFAULT_PANE: f32 = 828.0;
/// The sticky compact header shows once the title row has scrolled away
/// (design §2.5): the toolbar plus the title's first line.
const STICKY_AFTER: f32 = 96.0;

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(LIST_CONTEXT);
    let mut bindings = vec![
        KeyBinding::new("down", SelectNextWorkItem, list),
        KeyBinding::new("j", SelectNextWorkItem, list),
        KeyBinding::new("up", SelectPreviousWorkItem, list),
        KeyBinding::new("k", SelectPreviousWorkItem, list),
        KeyBinding::new("enter", FocusWorkItemDetail, list),
        KeyBinding::new("right", NextWorkItemTab, list),
        KeyBinding::new("left", PreviousWorkItemTab, list),
        KeyBinding::new(NEXT_PATH_KEYS, NextWorkItemPath, list),
        KeyBinding::new(PREVIOUS_PATH_KEYS, PreviousWorkItemPath, list),
        KeyBinding::new(
            PATH_FILTER_KEYS,
            OpenWorkItemPathFilter,
            Some(&format!("{LIST_CONTEXT} && !TextInput")),
        ),
    ];
    bindings.extend(detail::bindings());
    bindings
}

/// Every work item action, for the Cmd+P command list.
pub fn palette_commands() -> Vec<PaletteCommand> {
    let command = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        command("Work items: my work", None, Box::new(ShowMyWork)),
        command("Work items: assigned to me", None, Box::new(ShowAssigned)),
        command(
            "Work items: recently changed",
            None,
            Box::new(ShowRecentlyChanged),
        ),
        command("Work items: by area", None, Box::new(ShowByArea)),
        command("Work items: by iteration", None, Box::new(ShowByIteration)),
        command(
            "Work items: next area or iteration",
            Some(NEXT_PATH_KEYS),
            Box::new(NextWorkItemPath),
        ),
        command(
            "Work items: previous area or iteration",
            Some(PREVIOUS_PATH_KEYS),
            Box::new(PreviousWorkItemPath),
        ),
        command(
            "Work items: Filter by area…",
            None,
            Box::new(FilterWorkItemsByArea),
        ),
        command(
            "Work items: Filter by iteration…",
            None,
            Box::new(FilterWorkItemsByIteration),
        ),
    ]
    .into_iter()
    .chain(detail::palette_commands())
    .collect()
}

pub fn new(store: Store, window: &mut Window, cx: &mut App) -> gpui::Entity<WorkItemsView> {
    cx.new(|cx| WorkItemsView::new(store, window, cx))
}

/// Requests for the shell (canonical views live elsewhere).
#[derive(Debug, Clone, PartialEq)]
pub enum WorkItemsEvent {
    OpenPr(i64),
    /// An item opened here (a link, the row menu): the view as it was
    /// before (spec 0018 FR-3.2).
    Navigated(Box<nav::WorkItemsNavState>),
}

enum Phase {
    Loading { skeleton_since: Option<Instant> },
    Ready,
    Failed(String),
}

pub(crate) enum Detail {
    Nothing,
    Loading(i64),
    Missing(i64),
    Failed(String),
    Ready(Box<DetailData>),
}

/// People and project names shared with detail jobs.
#[derive(Clone, Default)]
struct Names {
    people: Arc<HashMap<PersonId, String>>,
    projects: Arc<HashMap<String, String>>,
}

pub struct WorkItemsView {
    store: Store,
    phase: Phase,
    list: ListData,
    names: Names,
    tab: Tab,
    area_paths: Vec<String>,
    iteration_paths: Vec<String>,
    area: Option<String>,
    iteration: Option<String>,
    rows: Vec<Row>,
    /// Row index of the selection.
    selected: Option<usize>,
    detail: Detail,
    /// The detail's cursor, editors, pickers and overlays.
    ui: DetailUi,
    full_width: bool,
    /// Stacked (window < 1100): the detail covers the list until `Esc`.
    stacked_detail: bool,
    /// Fixed "now" for stories; `None` = wall clock.
    clock: Option<adoken_model::Millis>,
    list_focus: FocusHandle,
    detail_focus: FocusHandle,
    list_scroll: UniformListScrollHandle,
    detail_scroll: ScrollHandle,
    window: AnyWindowHandle,
    _load: Option<Task<()>>,
    _detail: Option<Task<()>>,
    _grace: Option<Task<()>>,
    _watch: Option<Task<()>>,
    row_menu: adoken_ui::action_menu::MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    _pins: Option<Subscription>,
    _edits: Option<Subscription>,
    /// Gallery stories: runs once, after the detail first loads.
    story_hook: Option<StoryHook>,
    /// The area / iteration dropdown (spec 0008 FR-8).
    paths: paths::PathFilter,
    /// Back / forward before the list landed (spec 0018 FR-8).
    pending_restore: Option<crate::shell::history::Pending<nav::WorkItemsNavState>>,
    /// The detail's scroll, cursor and focus, once its item loads.
    restore_detail: Option<crate::shell::history::Pending<nav::DetailRestore>>,
}

/// See [`WorkItemsView::story_hook`].
pub type StoryHook = Box<dyn FnOnce(&mut WorkItemsView, &mut Window, &mut Context<WorkItemsView>)>;

impl EventEmitter<WorkItemsEvent> for WorkItemsView {}

impl WorkItemsView {
    pub fn new(store: Store, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
            names: Names::default(),
            tab: Tab::MyWork,
            area_paths: Vec::new(),
            iteration_paths: Vec::new(),
            area: None,
            iteration: None,
            rows: Vec::new(),
            selected: None,
            detail: Detail::Nothing,
            ui: DetailUi::default(),
            full_width: false,
            stacked_detail: false,
            clock: None,
            list_focus: cx.focus_handle(),
            detail_focus: cx.focus_handle(),
            list_scroll: UniformListScrollHandle::new(),
            detail_scroll: ScrollHandle::new(),
            window: window.window_handle(),
            _load: None,
            _detail: None,
            _grace: Some(grace),
            _watch: Some(watch),
            row_menu: Default::default(),
            row_bounds: Default::default(),
            _pins: crate::pin_actions::observe(cx),
            _edits: None,
            story_hook: None,
            paths: Default::default(),
            pending_restore: None,
            restore_detail: None,
        };
        view._edits = view.observe_edits(window, cx);
        detail::register_gates(cx);
        view.load(cx);
        view
    }

    /// Pin "now" (Recently changed window, ages) for deterministic stories.
    pub fn set_clock(&mut self, now: adoken_model::Millis, cx: &mut Context<Self>) {
        self.clock = Some(now);
        self.refilter();
        cx.notify();
    }

    fn now(&self) -> adoken_model::Millis {
        self.clock.unwrap_or_else(browse::now_ms)
    }

    /// (Re)reads the list; the first read drives the loading states, later
    /// ones update in place.
    fn load(&mut self, cx: &mut Context<Self>) {
        let store = self.store.clone();
        self._load = Some(cx.spawn(async move |this, cx| {
            let data = cx
                .background_spawn(async move {
                    ListData::read(&store).map(|data| {
                        let prepared = paths::Prepared::read(&store, &data);
                        (data, prepared)
                    })
                })
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
                    Ok((data, prepared)) => {
                        this.set_list(data, cx);
                        this.set_paths(prepared, cx);
                    }
                    Err(error) => {
                        log::warn!("work items: couldn't read the store: {error}");
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
        self.names = Names {
            people: Arc::new(data.names.clone()),
            projects: Arc::new(data.projects.clone()),
        };
        self.area_paths = model::paths(&data.items, Tab::Area);
        self.iteration_paths = model::paths(&data.items, Tab::Iteration);
        // A chosen path lasts the session (FR-8), even once nothing is
        // open under it; only a first load picks one.
        let keep = |current: &Option<String>, paths: &[String], tab| match current {
            Some(path) => Some(path.clone()),
            None => model::default_path(&data.items, tab, &data.me, paths),
        };
        self.area = keep(&self.area, &self.area_paths, Tab::Area);
        self.iteration = keep(&self.iteration, &self.iteration_paths, Tab::Iteration);
        let selected_id = self.selected_id();
        self.list = data;
        // People arrived after a picker opened (cold start): it gets them.
        if let Some(picker) = self.ui.mention.picker().cloned() {
            let people: Vec<adoken_ui::picker::Person> =
                self.list.people.iter().map(detail::picker_person).collect();
            picker.update(cx, |picker, cx| picker.set_people(people, cx));
        }
        self.phase = Phase::Ready;
        self.refilter();
        self.selected = selected_id
            .and_then(|id| model::row_of(&self.rows, &self.list.items, id))
            .or_else(|| model::step(&self.rows, None, 0));
        match self.detail_id() {
            Some(id) => {
                if first {
                    // Opened before the list arrived (palette, Today):
                    // select it when this tab lists it, and re-read it now
                    // that names are known.
                    self.selected = model::row_of(&self.rows, &self.list.items, id);
                }
                self.load_detail(id, cx);
            }
            None => self.show_selected(cx),
        }
        if first {
            self.apply_pending_restore(cx);
        }
    }

    fn path(&self) -> Option<&str> {
        match self.tab {
            Tab::Area => self.area.as_deref(),
            Tab::Iteration => self.iteration.as_deref(),
            _ => None,
        }
    }

    fn refilter(&mut self) {
        let now = self.now();
        self.rows = model::rows(&self.list.items, self.tab, &self.list.me, now, self.path());
    }

    fn selected_id(&self) -> Option<i64> {
        match self.rows.get(self.selected?)? {
            Row::Item(ix) => self.list.items.get(*ix).map(|item| item.id),
            Row::Header { .. } => None,
        }
    }

    fn detail_id(&self) -> Option<i64> {
        match &self.detail {
            Detail::Loading(id) | Detail::Missing(id) => Some(*id),
            Detail::Ready(detail) => Some(detail.item.id),
            _ => None,
        }
    }

    pub fn pin_subject(&self) -> Option<adoken_model::PinSubject> {
        self.selected_id()
            .or_else(|| self.detail_id())
            .map(adoken_model::PinSubject::WorkItem)
    }

    pub fn pin_title(&self) -> gpui::SharedString {
        self.selected_id()
            .and_then(|id| self.list.items.iter().find(|item| item.id == id))
            .map(|item| item.title.clone())
            .or_else(|| match &self.detail {
                Detail::Ready(detail) => Some(detail.item.title.clone()),
                _ => None,
            })
            .unwrap_or_default()
            .into()
    }

    /// The row action menu (`⌘K` / ⋯, design §3.1): open, copy, sync,
    /// links and assignee, browser, pin.
    fn open_row_menu(
        &mut self,
        position: adoken_ui::action_menu::MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use adoken_ui::action_menu::MenuItem;
        let subject = self.pin_subject();
        let title = self.pin_title();
        let weak = cx.entity().downgrade();
        let from_list = self.list_focus.is_focused(window);
        let mut items = Vec::new();
        if from_list {
            items.push(
                MenuItem::new("Open", move |window, cx| {
                    weak.update(cx, |this, cx| {
                        if let Some(id) = this.selected_id() {
                            this.open_work_item(id, window, cx);
                        }
                    })
                    .ok();
                })
                .keys("enter"),
            );
        }
        use crate::pin_actions::action;
        items.extend([
            action("Copy link", detail::CopyWorkItemLink).keys(detail::COPY_LINK_KEYS),
            action("Copy id", detail::CopyWorkItemId).keys(detail::COPY_ID_KEYS),
            action("Sync this work item", detail::SyncWorkItem).keys(detail::SYNC_KEYS),
            action("Remove link…", detail::RemoveWorkItemLink),
            action("Unassign", detail::UnassignWorkItem),
            action("Open in Azure DevOps", OpenWorkItemInAdo)
                .keys(detail::OPEN_KEYS)
                .disabled(if self.list.demo {
                    Some(crate::review::web::DEMO_OPEN_REASON)
                } else {
                    self.web_url()
                        .is_none()
                        .then_some("No browser target available")
                }),
            crate::pin_actions::menu_item(subject, title, cx),
        ]);
        self.row_menu.open(
            "Work item",
            items,
            position,
            adoken_theme::z::DROPDOWN,
            |this| &mut this.row_menu,
            window,
            cx,
        );
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.after_filter_change(cx);
    }

    /// The list's tab and chosen paths (navigation history captures it).
    pub fn list_filter(&self) -> model::ListFilter {
        model::ListFilter {
            tab: self.tab,
            area: self.area.clone(),
            iteration: self.iteration.clone(),
        }
    }

    /// Puts back a [`Self::list_filter`].
    pub fn restore_list_filter(&mut self, filter: model::ListFilter, cx: &mut Context<Self>) {
        self.area = filter.area;
        self.iteration = filter.iteration;
        self.set_tab(filter.tab, cx);
    }

    fn after_filter_change(&mut self, cx: &mut Context<Self>) {
        let open = self.detail_id();
        self.refilter();
        self.selected = open
            .and_then(|id| model::row_of(&self.rows, &self.list.items, id))
            .or_else(|| model::step(&self.rows, None, 0));
        self.list_scroll.scroll_to_item(0, ScrollStrategy::Top);
        if let Some(ix) = self.selected {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
        if self.selected_id() != open {
            self.show_selected(cx);
        }
        cx.notify();
    }

    fn cycle_path(&mut self, delta: isize, cx: &mut Context<Self>) {
        // From a non-path tab, `[`/`]` jump to By area.
        if !self.tab.has_path() {
            self.tab = Tab::Area;
            self.after_filter_change(cx);
            return;
        }
        let (paths, current) = match self.tab {
            Tab::Iteration => (&self.iteration_paths, &mut self.iteration),
            _ => (&self.area_paths, &mut self.area),
        };
        *current = model::cycle(paths, current.as_deref(), delta);
        self.after_filter_change(cx);
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(next) = model::step(&self.rows, self.selected, delta) else {
            return;
        };
        if Some(next) == self.selected {
            return;
        }
        self.selected = Some(next);
        self.list_scroll
            .scroll_to_item(next, ScrollStrategy::Nearest);
        self.show_selected(cx);
        cx.notify();
    }

    fn show_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected_id() {
            Some(id) => {
                crate::pin_actions::mark_opened(adoken_model::PinSubject::WorkItem(id), cx);
                self.load_detail(id, cx);
            }
            None => {
                self.detail = Detail::Nothing;
                self._detail = None;
            }
        }
    }

    /// Opens work item `id` (palette, Today, inbox): selects it when the
    /// current tab lists it, and shows its detail either way.
    pub fn open_work_item(&mut self, id: i64, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.nav_state(window, cx);
        crate::pin_actions::mark_opened(adoken_model::PinSubject::WorkItem(id), cx);
        self.selected = model::row_of(&self.rows, &self.list.items, id);
        if let Some(ix) = self.selected {
            self.list_scroll.scroll_to_item(ix, ScrollStrategy::Center);
        }
        self.detail = Detail::Loading(id);
        self.load_detail(id, cx);
        window.focus(&self.list_focus, cx);
        cx.emit(WorkItemsEvent::Navigated(Box::new(before)));
        cx.notify();
    }

    fn load_detail(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.detail_id() != Some(id) {
            self.detail = Detail::Loading(id);
            self.reset_detail_ui();
            self.detail_scroll.set_offset(gpui::point(px(0.0), px(0.0)));
        }
        let store = self.store.clone();
        let names = self.names.clone();
        let window = self.window;
        self._detail = Some(cx.spawn(async move |this, cx: &mut AsyncApp| {
            let loaded = cx
                .background_spawn(async move {
                    data::load_detail(&store, id, &names.people, &names.projects)
                })
                .await;
            window
                .update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        if this.detail_id() != Some(id) {
                            return;
                        }
                        this.detail = match loaded {
                            Ok(Some(detail)) => Detail::Ready(Box::new(detail)),
                            Ok(None) => Detail::Missing(id),
                            Err(error) => Detail::Failed(error.to_string()),
                        };
                        this.detail_loaded(window, cx);
                        this.apply_detail_restore(window, cx);
                        cx.notify();
                    })
                })
                .ok();
        }));
    }

    fn web_url(&self) -> Option<String> {
        let org = self.list.org_url.as_deref()?;
        if let Some(item) = self
            .selected_id()
            .and_then(|id| self.list.items.iter().find(|item| item.id == id))
        {
            let project = self
                .list
                .projects
                .get(&item.project_id)
                .map_or(item.project_id.as_str(), String::as_str);
            return Some(data::web_url(org, project, item.id));
        }
        match &self.detail {
            Detail::Ready(detail) => Some(data::web_url(org, &detail.project, detail.item.id)),
            _ => None,
        }
    }

    fn open_in_ado(&mut self, cx: &mut Context<Self>) {
        if self.list.demo || self.web_url().is_none() {
            return;
        }
        if let Some(subject) = self.pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        if let Some(url) = self.web_url() {
            cx.open_url(&url);
        }
    }

    // ── rendering ────────────────────────────────────────────────────────

    fn render_list(&self, fill: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let selected_tab = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let tabs = Segmented::new("wi-tabs", Tab::ALL.map(short_label), selected_tab).on_select({
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
                            .child("Work items"),
                    )
                    .children(stale.map(|label| {
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, &theme)
                            .child(label)
                    })),
            )
            .child(tabs)
            .when(self.tab.has_path(), |header| {
                header.child(self.render_path_picker(&theme, cx))
            })
            .when(matches!(self.phase, Phase::Ready), |header| {
                header.child(
                    div()
                        .text_color(colors.text_tertiary)
                        .type_style(typography::SMALL, &theme)
                        .child(self.summary()),
                )
            });

        let body = match &self.phase {
            Phase::Loading { skeleton_since } => match skeleton_since {
                Some(_) => browse::skeleton_rows("wi-skeleton", SKELETON_ROWS).into_any_element(),
                None => div().flex_1().into_any_element(),
            },
            Phase::Failed(error) => div()
                .p(px(space::S12))
                .child(
                    Notice::new(
                        ChipTone::Danger,
                        "Couldn't read work items from the local store.",
                    )
                    .icon(IconName::CircleX)
                    .detail(format!("Your synced data is untouched. {error}"))
                    .child(
                        Button::new("wi-retry", "Retry")
                            .on_press(press(cx, |this: &mut Self, _, cx| this.retry(cx))),
                    ),
                )
                .into_any_element(),
            Phase::Ready if self.rows.is_empty() => {
                let (icon, headline, body) = self.empty_copy();
                div()
                    .flex_1()
                    .child(EmptyState::new(icon, headline).body(body))
                    .into_any_element()
            }
            Phase::Ready => uniform_list(
                "wi-list",
                self.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    let theme = cx.theme().clone();
                    range
                        .map(|ix| this.render_row(ix, &theme, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.list_scroll)
            .flex_1()
            .into_any_element(),
        };

        div()
            .id("wi-list-pane")
            .key_context(LIST_CONTEXT)
            .track_focus(&self.list_focus)
            .flex()
            .flex_col()
            .when(fill, |pane| pane.flex_1())
            .when(!fill, |pane| pane.flex_none().w(px(LIST_WIDTH)))
            .h_full()
            .bg(colors.bg_surface)
            .border_r_1()
            .border_color(colors.border_subtle)
            .on_action(cx.listener(|this, _: &SelectNextWorkItem, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPreviousWorkItem, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &FocusWorkItemDetail, window, cx| {
                if matches!(this.detail, Detail::Ready(_)) {
                    this.ui.cursor = Some(Stop::Title);
                    this.stacked_detail = true;
                    window.focus(&this.detail_focus, cx);
                    cx.notify();
                }
            }))
            .on_action(
                cx.listener(|this, _: &NextWorkItemTab, _, cx| this.set_tab(this.tab.step(1), cx)),
            )
            .on_action(cx.listener(|this, _: &PreviousWorkItemTab, _, cx| {
                this.set_tab(this.tab.step(-1), cx)
            }))
            .child(header)
            .child(body)
    }

    fn summary(&self) -> String {
        let n = self
            .rows
            .iter()
            .filter(|row| matches!(row, Row::Item(_)))
            .count();
        let noun = if n == 1 { "item" } else { "items" };
        match self.tab {
            Tab::MyWork => format!("{n} open {noun} you own or touched"),
            Tab::Assigned => format!("{n} {noun} assigned to you"),
            Tab::Recent => format!("{n} {noun} changed in the last 7 days"),
            Tab::Area | Tab::Iteration => format!("{n} open {noun}"),
        }
    }

    fn empty_copy(&self) -> (IconName, &'static str, &'static str) {
        if self.list.items.is_empty() {
            return (
                IconName::ListTodo,
                "No work items synced yet",
                "They appear here after the first sync.",
            );
        }
        match self.tab {
            Tab::MyWork => (
                IconName::CircleCheck,
                "Nothing open on your plate",
                "Open items assigned to you, or that you filed or changed, show up here.",
            ),
            Tab::Assigned => (
                IconName::ListTodo,
                "Nothing assigned to you",
                "Work items assigned to you show up here.",
            ),
            Tab::Recent => (
                IconName::Clock,
                "No work items changed this week",
                "Items anyone changed in the last 7 days show up here.",
            ),
            Tab::Area | Tab::Iteration if self.list.items.iter().any(|i| i.is_open()) => (
                IconName::FolderKanban,
                "Nothing open here",
                "Press / to choose another path, or [ ] to step through the busy ones.",
            ),
            Tab::Area | Tab::Iteration => (
                IconName::FolderKanban,
                "No open work items synced",
                "Open items in the newest 5,000 synced show up here by path.",
            ),
        }
    }

    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = &theme.colors;
        let item = match &self.rows[ix] {
            Row::Header { state, count } => {
                return browse::group_header(state.clone(), *count, theme).into_any_element();
            }
            Row::Item(item) => &self.list.items[*item],
        };
        let (icon, tint) = type_style(&item.kind, colors);
        let assignee = item
            .assigned_to
            .as_deref()
            .map_or_else(|| "Unassigned".to_owned(), |id| self.list.name(id));
        let subtitle = format!(
            "#{} · {} · {assignee} · {}",
            item.id,
            item.state,
            model::leaf(&item.iteration_path)
        );
        let id = item.id;
        let mut row = ListRow::new(("wi-row", ix), item.title.clone())
            .height(ROW_HEIGHT)
            .leading(Icon::new(icon).size(size::ICON_MD).color(tint))
            .subtitle(subtitle)
            .selected(self.selected == Some(ix))
            .track_selected_bounds(self.row_bounds.clone())
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.selected = Some(ix);
                    if event.modifiers().platform {
                        this.open_in_ado(cx);
                    } else {
                        this.open_work_item(id, window, cx);
                    }
                }),
            )
            .on_secondary_click(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.selected = Some(ix);
                    this.open_row_menu(
                        adoken_ui::action_menu::MenuPosition::at_pointer(event.position),
                        window,
                        cx,
                    );
                },
            ));
        if !item.linked_pr_ids.is_empty() {
            row = row.trailing(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S2))
                    .child(
                        Icon::new(IconName::GitPullRequest)
                            .size(size::ICON_XS)
                            .color(colors.text_tertiary),
                    )
                    .child(item.linked_pr_ids.len().to_string()),
            );
        }
        if let Some(p) = item.priority {
            let (p_icon, p_color) = priority_style(p, colors);
            row = row.trailing(Icon::new(p_icon).size(size::ICON_SM).color(p_color));
        }
        row.trailing(div().child(age(self.now(), item.changed)))
            .into_any_element()
    }

    fn render_detail(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let container = div()
            .id("wi-detail")
            .key_context(DETAIL_CONTEXT)
            .track_focus(&self.detail_focus)
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(theme.colors.bg_surface)
            .text_color(theme.colors.text_primary)
            .on_action(cx.listener(|this, _: &FocusWorkItemList, window, cx| {
                this.full_width = false;
                this.stacked_detail = false;
                window.focus(&this.list_focus, cx);
                cx.notify();
            }));
        match &self.detail {
            Detail::Ready(detail) => {
                let label = format!("Work item #{}, {}", detail.item.id, detail.item.title);
                container
                    .role(gpui::Role::Region)
                    .aria_label(label)
                    .child(self.render_item(window, cx))
                    .into_any_element()
            }
            Detail::Nothing => container
                .child(
                    EmptyState::new(IconName::ListTodo, "Select a work item").body(
                        "↑ ↓ to move, ← → to switch tabs, ↩ to open, letters to edit (press ? for keys).",
                    ),
                )
                .into_any_element(),
            // principles §3.2: local reads finish well under 150 ms.
            Detail::Loading(_) => container.into_any_element(),
            Detail::Missing(id) => container
                .child(
                    EmptyState::new(IconName::CircleAlert, format!("#{id} isn't synced yet"))
                        .body("It appears here after the next sync."),
                )
                .into_any_element(),
            Detail::Failed(error) => container
                .child(
                    EmptyState::new(IconName::CircleX, "Couldn't read this work item")
                        .body(format!("Your synced data is untouched. {error}")),
                )
                .into_any_element(),
        }
    }

    /// The pane's content (design §2): single column under 1000 px pane
    /// width, the properties rail from 1000 px; the sticky compact header
    /// once the title has scrolled away; Add link / Remove link dialogs.
    fn render_item(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let detail = self.detail().expect("rendered with a detail");
        let item = detail.item.clone();
        let project = detail.project.clone();
        let repro = detail.repro_steps.clone();
        let acceptance = detail.acceptance.clone();
        let measured = self.ui.anchors.pane.get();
        let pane = if measured > 0.0 {
            measured
        } else {
            DEFAULT_PANE
        };
        let arrangement = detail::model::arrangement(pane);
        // Text scale follows the window's rem (16 px at 100 %).
        let font_scale = f32::from(window.rem_size()) / 16.0;
        let columns = detail::model::grid_columns(pane, font_scale);
        let stacked = self.is_stacked(window);
        let prose = pane.min(size::DETAIL_PROSE_MAX + detail::model::PANE_PADDING_X)
            - detail::model::PANE_PADDING_X;

        // Re-layout when a resize crosses the rail or grid thresholds.
        let pane_slot = self.ui.anchors.pane.clone();
        let view = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let width = f32::from(bounds.size.width);
                pane_slot.set(width);
                if detail::model::arrangement(width) != arrangement
                    || detail::model::grid_columns(width, font_scale) != columns
                {
                    view.update(cx, |_, cx| cx.notify()).ok();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let toolbar = self.render_toolbar(&item, stacked, pane, cx);
        let (inner_toolbar, outer_toolbar) = match arrangement {
            Arrangement::Single => (Some(toolbar), None),
            Arrangement::Wide => (None, Some(toolbar)),
        };
        let banner = self.render_not_available(window, cx);
        let title = self.render_title(&item, window, cx);
        let chips = self.render_chip_row(&item, arrangement == Arrangement::Single, window, cx);
        // Spec 0020: the description section is a tour anchor.
        let mut sections = vec![
            div()
                .child(self.render_description(window, cx))
                .children(adoken_ui::tour::anchor(detail::anchors::DESCRIPTION, cx))
                .into_any_element(),
        ];
        if let Some(doc) = &repro {
            sections.push(self.render_extra_text("wi-repro", "Repro steps", doc, window, cx));
        }
        if let Some(doc) = &acceptance {
            sections.push(self.render_extra_text(
                "wi-acceptance",
                "Acceptance criteria",
                doc,
                window,
                cx,
            ));
        }
        sections.push(self.render_links(window, cx));
        sections.push(
            div()
                .child(self.render_discussion(window, cx))
                .children(adoken_ui::tour::anchor(detail::anchors::DISCUSSION, cx))
                .into_any_element(),
        );
        sections.push(self.render_history(window, cx));

        let mut head = div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .children(banner)
            .child(title)
            .child(chips);
        if arrangement == Arrangement::Single {
            head = head
                .child(
                    div()
                        .pt(px(space::S8))
                        .child(self.render_grid(&item, &project, columns, prose, window, cx))
                        .children(adoken_ui::tour::anchor(detail::anchors::FIELDS, cx)),
                )
                .child(self.render_meta(&item, cx));
        }
        let scroll = div()
            .id("wi-detail-scroll")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .track_scroll(&self.detail_scroll)
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.notify()))
            .child(
                div()
                    .pl(px(space::S24))
                    .pr(px(space::S24))
                    .pt(px(space::S16))
                    .pb(px(space::S48))
                    .flex()
                    .flex_col()
                    .gap(px(space::S24))
                    .children(inner_toolbar)
                    .child(head)
                    .children(sections),
            );
        let sticky = self.detail_scroll.offset().y < px(-STICKY_AFTER);
        let body = match arrangement {
            Arrangement::Single => div().size_full().flex().child(scroll),
            Arrangement::Wide => div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(space::S24))
                        .pt(px(space::S16))
                        .pb(px(space::S8))
                        .children(outer_toolbar),
                )
                .child(
                    div().flex_1().min_h_0().flex().child(scroll).child(
                        div()
                            .id("wi-rail-scroll")
                            .h_full()
                            .overflow_y_scroll()
                            .child(self.render_rail(&item, &project, window, cx))
                            .children(adoken_ui::tour::anchor(detail::anchors::FIELDS, cx)),
                    ),
                ),
        };
        div()
            .relative()
            .size_full()
            .child(measure)
            // FR-36: "Saved" for assistive technology only (polite).
            .child(crate::activity::statusbar::announcement(
                self.announcement(),
            ))
            .child(body)
            .child(detail::reveal_pass(
                self.ui.reveal.clone(),
                self.ui.anchors.composer.clone(),
                self.detail_scroll.clone(),
            ))
            .when(
                self.story_hook.is_some() && matches!(self.phase, Phase::Ready),
                |pane| {
                    // Stories reach their state once the detail has painted
                    // (pickers anchor to real cells).
                    let view = cx.entity().downgrade();
                    pane.child(
                        canvas(
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    if let Some(hook) = this.story_hook.take() {
                                        hook(this, window, cx);
                                        cx.notify();
                                    }
                                })
                                .ok();
                            },
                            |_, _, _, _| {},
                        )
                        .absolute(),
                    )
                },
            )
            .when(sticky && self.ui.title.is_none(), |pane| {
                pane.child(self.render_sticky(&item, window, cx))
            })
            .children(self.render_link_overlays(cx))
            .into_any_element()
    }

    /// Window under 1100 px: the detail slides over the list (design §2.4).
    fn is_stacked(&self, window: &Window) -> bool {
        window.viewport_size().width < px(STACK_BELOW)
    }
}

fn short_label(tab: Tab) -> &'static str {
    match tab {
        Tab::MyWork => "Mine",
        Tab::Assigned => "Assigned",
        Tab::Recent => "Recent",
        Tab::Area => "Area",
        Tab::Iteration => "Iteration",
    }
}

/// Type icon + `wi.*` color (tokens.md §2.8).
/// The icon follows the type's family, so Scrum's Product Backlog Item and
/// CMMI's Requirement look like a User Story; custom types get a plain square.
pub fn type_style<C: Copy>(kind: &WorkItemType, colors: &ColorTokens<C>) -> (IconName, C) {
    match kind.family() {
        TypeFamily::Bug => (IconName::Bug, colors.wi_bug),
        TypeFamily::Task => (IconName::SquareCheck, colors.wi_task),
        TypeFamily::Requirement => (IconName::BookOpen, colors.wi_story),
        TypeFamily::Feature => (IconName::Trophy, colors.wi_feature),
        TypeFamily::Epic => (IconName::Crown, colors.wi_epic),
        TypeFamily::Issue => (IconName::CircleAlert, colors.wi_issue),
        TypeFamily::Other => (IconName::Square, colors.text_secondary),
    }
}

/// Priority glyph + `priority.*` color (tokens.md §2.8).
pub fn priority_style<C: Copy>(priority: u8, colors: &ColorTokens<C>) -> (IconName, C) {
    match priority {
        1 => (IconName::ChevronsUp, colors.priority_p1),
        2 => (IconName::ChevronUp, colors.priority_p2),
        3 => (IconName::Minus, colors.priority_p3),
        _ => (IconName::ChevronDown, colors.priority_p4),
    }
}

pub(crate) fn state_tone(category: StateCategory) -> ChipTone {
    match category {
        StateCategory::InProgress => ChipTone::Accent,
        StateCategory::Resolved => ChipTone::Success,
        StateCategory::Proposed | StateCategory::Completed | StateCategory::Removed => {
            ChipTone::Neutral
        }
    }
}

impl Focusable for WorkItemsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.list_focus.clone()
    }
}

impl Render for WorkItemsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_path_placement(window, cx);
        let colors = &cx.theme().colors;
        let stacked = self.is_stacked(window);
        let detail_open = matches!(self.detail, Detail::Ready(_) | Detail::Loading(_));
        let show_detail = !stacked || self.full_width || (self.stacked_detail && detail_open);
        let show_list = !self.full_width && (!stacked || !show_detail);
        let root = div()
            .id("work-items")
            .key_context(VIEW_CONTEXT)
            .size_full()
            .flex()
            .bg(colors.bg_app);
        self.on_detail_actions(root, cx)
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
            .on_action(cx.listener(|this, _: &ShowMyWork, _, cx| this.set_tab(Tab::MyWork, cx)))
            .on_action(cx.listener(|this, _: &ShowAssigned, _, cx| this.set_tab(Tab::Assigned, cx)))
            .on_action(
                cx.listener(|this, _: &ShowRecentlyChanged, _, cx| this.set_tab(Tab::Recent, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowByArea, _, cx| this.set_tab(Tab::Area, cx)))
            .on_action(
                cx.listener(|this, _: &ShowByIteration, _, cx| this.set_tab(Tab::Iteration, cx)),
            )
            .on_action(cx.listener(|this, _: &NextWorkItemPath, _, cx| this.cycle_path(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousWorkItemPath, _, cx| this.cycle_path(-1, cx)))
            .on_action(cx.listener(|this, _: &OpenWorkItemInAdo, _, cx| this.open_in_ado(cx)))
            .on_action(cx.listener(|this, _: &OpenWorkItemPathFilter, window, cx| {
                if this.tab.has_path() {
                    this.open_path_filter(this.tab, true, window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &FilterWorkItemsByArea, window, cx| {
                this.open_path_filter(Tab::Area, true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &FilterWorkItemsByIteration, window, cx| {
                    this.open_path_filter(Tab::Iteration, true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleFullWidth, _, cx| {
                this.full_width = !this.full_width;
                cx.notify();
            }))
            .when(show_list, |this| this.child(self.render_list(stacked, cx)))
            .when(show_detail, |this| {
                this.child(self.render_detail(window, cx))
            })
            .children(self.row_menu.element())
            .children(self.ui.state_menu.element())
            .children(self.ui.people.element())
            .children(self.ui.tree.element())
            .children(self.ui.link_menu.element())
            .children(self.paths.element())
    }
}

/// Gallery stories: the view on a seeded demo store at `DEMO_NOW`.
pub mod story {
    use adoken_sync::wi::{WiChange, WiError, WiWrite};

    use super::writes::{Access, Answer, WiEdits, WiOutcome};
    use super::*;

    /// A view over the demo store in `dir`, `tab` shown, clock pinned;
    /// work item edits apply to the demo store (spec FR-37).
    pub fn view(
        dir: &std::path::Path,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<WorkItemsView>,
    ) -> WorkItemsView {
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir, now).expect("seed the demo store");
        writes::set_access(Access::Local, cx);
        let edits = WiEdits::install(store.clone(), cx);
        edits.update(cx, |edits, cx| edits.set_clock(now, cx));
        let mut view = WorkItemsView::new(store, window, cx);
        view.clock = Some(now);
        view.tab = tab;
        view
    }

    /// [`view`] with the demo dataset's work item at `index` open
    /// (`adoken_demo::rich::SCENARIO` etc.), then `hook` once it loads.
    pub fn detail(
        dir: &std::path::Path,
        index: usize,
        hook: Option<StoryHook>,
        window: &mut Window,
        cx: &mut Context<WorkItemsView>,
    ) -> WorkItemsView {
        let mut view = view(dir, Tab::MyWork, window, cx);
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, adoken_demo::DEMO_NOW);
        view.story_hook = hook;
        view.open_work_item(ds.work_items[index].id, window, cx);
        view
    }

    /// A view over an empty (never synced) store in `dir`.
    pub fn empty(
        dir: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<WorkItemsView>,
    ) -> WorkItemsView {
        let store = Store::open(dir.join("adoken.sqlite")).expect("open an empty store");
        let mut view = WorkItemsView::new(store, window, cx);
        view.clock = Some(adoken_demo::DEMO_NOW);
        view
    }

    impl WorkItemsView {
        /// The open item (stories, smoke tests).
        pub fn detail_item(&self) -> Option<adoken_model::WorkItem> {
            self.detail().map(|d| d.item.clone())
        }

        /// The keyboard cursor's stop (tests).
        pub fn cursor(&self) -> Option<Stop> {
            self.ui.cursor
        }

        /// Run `intent` as its key would (stories, tests).
        pub fn run_intent(
            &mut self,
            intent: detail::Intent,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            self.intend(intent, window, cx);
        }

        /// The title editor's text, while it's open (tests).
        pub fn title_editor_text(&self, cx: &App) -> Option<String> {
            self.ui
                .title
                .as_ref()
                .map(|t| t.input.read(cx).text().to_owned())
        }

        /// The title editor's refusal, if any (tests).
        pub fn title_error(&self) -> Option<&'static str> {
            self.ui.title.as_ref().and_then(|t| t.error)
        }

        /// Type `text` into the open title editor (stories).
        pub fn story_title_text(&mut self, text: &str, cx: &mut Context<Self>) {
            if let Some(editor) = &self.ui.title {
                editor
                    .input
                    .update(cx, |input, cx| input.set_text(text, cx));
            }
        }

        /// Replace the description editor's text (stories).
        pub fn story_description_text(&mut self, text: &str, cx: &mut Context<Self>) {
            if let Some(editor) = &self.ui.description {
                editor
                    .area
                    .update(cx, |area, cx| area.set_text(text, Vec::new(), cx));
            }
        }

        /// The description editor: open, previewing, text (tests).
        pub fn description_editor(&self, cx: &App) -> Option<(bool, String)> {
            self.ui
                .description
                .as_ref()
                .map(|e| (e.previewing, e.area.read(cx).text().to_owned()))
        }

        /// Toggle Write / Preview (stories).
        pub fn story_toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            self.toggle_preview(window, cx);
        }

        /// Type into the composer and follow `@` as typing would (stories).
        pub fn story_composer_text(
            &mut self,
            text: &str,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            if let Some(composer) = &self.ui.composer {
                let area = composer.area.clone();
                window.focus(&area.focus_handle(cx), cx);
                area.update(cx, |area, cx| {
                    area.set_lines(3, 12, cx);
                    area.set_text(text, Vec::new(), cx)
                });
            }
        }

        /// The composer's text as it would be sent (tests).
        pub fn composer_markdown(&self, cx: &App) -> Option<String> {
            self.ui
                .composer
                .as_ref()
                .map(|c| c.area.read(cx).serialize(adoken_richtext::mention_token))
        }

        /// Expand groups / comments / history (stories).
        pub fn story_expand_all(&mut self) {
            self.ui.show_all_comments = true;
            self.ui.show_all_history = true;
        }

        /// Put the keyboard cursor on `stop` with the detail focused.
        pub fn story_cursor(&mut self, stop: Stop, window: &mut Window, cx: &mut Context<Self>) {
            self.ui.cursor = Some(stop);
            window.focus(&self.detail_focus, cx);
            cx.notify();
        }

        /// Scroll the detail down by `px` (the sticky header story).
        pub fn story_scroll(&mut self, by: f32) {
            self.detail_scroll.set_offset(gpui::point(px(0.0), px(-by)));
        }

        /// Leave the list for full width (wide stories).
        pub fn story_full_width(&mut self) {
            self.full_width = true;
        }

        /// Open the Remove link confirmation for link row `row`.
        pub fn story_remove_link(
            &mut self,
            nth: usize,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) {
            if let Some(&row) = self.removable_rows().get(nth) {
                self.request_remove_row(row, window, cx);
            }
        }

        /// The shown link row pointing at work item `target` (tests).
        pub fn link_row_of(&self, target: i64) -> Option<usize> {
            let detail = self.detail()?;
            let shown = self.shown_link_rows(detail);
            shown
                .rows
                .iter()
                .position(|&ix| detail.links[ix].work_item_id() == Some(target))
        }

        /// Whether a picker or menu of the detail is open (tests).
        pub fn picker_open(&self) -> bool {
            self.ui.state_menu.is_open()
                || self.ui.people.is_open()
                || self.ui.tree.is_open()
                || self.ui.link_menu.is_open()
                || self.ui.link_picker.is_some()
                || self.ui.mention.is_open()
        }

        /// Stacked: the detail covers the list (as `↩` from the list).
        pub fn story_stacked(&mut self) {
            self.stacked_detail = true;
        }

        /// The open assignee picker: `query` typed, `directory` results in.
        pub fn story_people(
            &mut self,
            query: &str,
            directory: Vec<adoken_ui::picker::Person>,
            cx: &mut Context<Self>,
        ) {
            if let Some(picker) = self.ui.people.picker().cloned() {
                picker.update(cx, |picker, cx| {
                    picker.set_query(query, cx);
                    picker.set_directory(adoken_ui::picker::Directory::Results(directory), cx);
                });
            }
        }

        /// The open tree picker with `query` typed.
        pub fn story_tree_query(&mut self, query: &str, cx: &mut Context<Self>) {
            if let Some(picker) = self.ui.tree.picker().cloned() {
                picker.update(cx, |picker, cx| picker.set_query(query, cx));
            }
        }
    }

    /// A write the controller holds as sent but unanswered (pending
    /// stories): nothing is ever sent.
    pub fn pend(item: &adoken_model::WorkItem, change: WiChange, cx: &mut App) {
        let Some(edits) = WiEdits::global(cx) else {
            return;
        };
        let write = WiWrite {
            id: item.id,
            base_rev: item.rev,
            change,
            extra: Vec::new(),
        };
        edits.update(cx, |edits, cx| {
            let now = edits.now();
            // Dropping the job keeps it Sending.
            let _ = edits.edits.submit(write, now);
            cx.notify();
        });
    }

    /// `change` answered with `outcome` (failed / conflict / rule stories).
    pub fn answer(
        item: &adoken_model::WorkItem,
        change: WiChange,
        outcome: Result<WiOutcome, WiError>,
        cx: &mut App,
    ) {
        let Some(edits) = WiEdits::global(cx) else {
            return;
        };
        let write = WiWrite {
            id: item.id,
            base_rev: item.rev,
            change,
            extra: Vec::new(),
        };
        edits.update(cx, |edits, cx| {
            let now = edits.now();
            let _ = edits.edits.submit(write.clone(), now);
            let _ = edits.edits.answered(item.id, Answer::Write(write, outcome));
            cx.notify();
        });
    }

    /// A comment I posted, unanswered (or failed with `cause`).
    pub fn comment(
        item: &adoken_model::WorkItem,
        markdown: &str,
        cause: Option<WiError>,
        cx: &mut App,
    ) {
        let Some(edits) = WiEdits::global(cx) else {
            return;
        };
        edits.update(cx, |edits, cx| {
            let now = edits.now();
            let (local, _) = edits
                .edits
                .submit_comment(item.id, markdown.to_owned(), now);
            if let Some(cause) = cause {
                let _ = edits
                    .edits
                    .answered(item.id, Answer::Comment(local, Err(cause)));
            }
            cx.notify();
        });
    }
}
