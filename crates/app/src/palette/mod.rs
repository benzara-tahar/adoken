//! Cmd+P palette (docs/design/command-palette.md, spec 0003) over the local
//! store: sigil/keyword scopes, `key:value` filters, direct id and link
//! jumps, per-type row templates from `settings.toml`, and the app's
//! commands. Opening a result emits [`PaletteEvent::Open`]; the shell routes.
//!
//! Threading: the catalog (index + domain rows) is built on a background
//! executor once the store opens; until then commands are searchable. Every
//! query runs on the background executor too. A new keystroke replaces the
//! in-flight search task (dropping it cancels it) and bumps a generation
//! counter the job checks before starting, so stale results never land.
//!
//! Latency: keystroke → results painted is recorded in an
//! `adoken_perf::Samples` series and logged in debug builds.

pub mod catalog;
pub mod gates;
pub mod query;
pub mod rows;

/// Guided tour anchors (spec 0020 FR-10).
pub mod anchors {
    pub const INPUT: &str = "palette.input";
    pub const RESULTS: &str = "palette.results";
    pub const FOOTER: &str = "palette.footer";
}

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use adoken_perf::{Ms, Samples};
use adoken_search::DocKind;
use adoken_store::Store;
use adoken_theme::{radius, size, space, typography, z};
use adoken_ui::{
    ActiveTheme, Chip, ChipTone, Icon, IconName, Kbd, ListRow, TextInput, TextInputEvent, Theme,
    TypeStyleExt, match_ranges,
};
use gpui::{
    Action, AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    KeyBinding, MouseButton, ScrollStrategy, SharedString, Subscription, Task,
    UniformListScrollHandle, Window, actions, anchored, canvas, deferred, div, point, prelude::*,
    px, uniform_list,
};

use crate::avatar::Avatar;
use crate::pins::Pins;
use crate::{FocusNext, FocusPrevious};
use adoken_model::PinSubject;
use adoken_ui::action_menu::{MenuPosition, MenuSlot};
use catalog::{Catalog, CatalogData, CommandEntry, Item, Row, Target};
use query::Query;
use rows::{Density, Leading, PaletteSettings, RowType};

actions!(
    palette,
    [
        /// Open or close the command palette.
        Toggle,
        /// Open the palette scoped to commands.
        ToggleCommands,
        OpenUnpinPalette,
        /// Select the next result (wraps).
        SelectNext,
        /// Select the previous result (wraps).
        SelectPrev,
        /// Move the selection one page down.
        PageDown,
        /// Move the selection one page up.
        PageUp,
        /// Open the selected result in the browser.
        ConfirmBrowser,
        /// Run the selected result's primary action.
        Confirm,
        /// Complete a scope keyword or fix an unknown filter key.
        Complete,
        /// Clear the query, or close the palette when it is already empty.
        Dismiss,
    ]
);

pub const TOGGLE_KEYS: &str = "secondary-p";
pub const COMMANDS_KEYS: &str = "secondary-shift-p";
const KEY_CONTEXT: &str = "Palette";

/// Visible rows before the list scrolls; the half row hints there is more (§2).
const MAX_VISIBLE_ROWS: f32 = 8.5;
const INPUT_ROW_HEIGHT: f32 = 52.0;
const FOOTER_HEIGHT: f32 = 32.0;
const PLACEHOLDER: &str = "Search pull requests, work items, pipelines, people…  (> for commands)";
const SCOPE_HINT: &str = "! pull requests · # work items · @ people · > commands · p: pipelines · r: repos · proj: projects";
const FILTER_HINT: &str =
    "Filters: author:@me reviewer: is:open state: type:bug assigned: project:";

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new(TOGGLE_KEYS, Toggle, None),
        KeyBinding::new(COMMANDS_KEYS, ToggleCommands, None),
        KeyBinding::new("down", SelectNext, cx),
        KeyBinding::new("up", SelectPrev, cx),
        KeyBinding::new("pagedown", PageDown, cx),
        KeyBinding::new("pageup", PageUp, cx),
        KeyBinding::new("enter", Confirm, cx),
        KeyBinding::new("secondary-enter", ConfirmBrowser, cx),
        KeyBinding::new("tab", Complete, cx),
        KeyBinding::new("escape", Dismiss, cx),
    ]
}

/// An app action exposed as a `>` command row (keyboard-first: every action
/// is reachable from Cmd+P).
pub struct PaletteCommand {
    pub title: &'static str,
    pub keys: Option<&'static str>,
    pub action: Box<dyn Action>,
}

/// A content row was confirmed. Commands are run by the palette itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteEvent {
    Open(Target),
}

/// Selection after moving `delta` rows in a list of `len`: single steps wrap
/// (§7.1), page jumps clamp at the ends.
pub fn step_selection(selected: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let len_i = len as isize;
    let current = selected.min(len - 1) as isize;
    if delta.abs() == 1 {
        (current + delta).rem_euclid(len_i) as usize
    } else {
        (current + delta).clamp(0, len_i - 1) as usize
    }
}

/// Glyph and label per result kind (the label is spoken and shown in the
/// footer, so color is never the only signal).
pub fn kind_meta(kind: DocKind) -> (&'static str, &'static str) {
    match kind {
        DocKind::PullRequest => ("⇄", "Pull request"),
        DocKind::WorkItem => ("▣", "Work item"),
        DocKind::Pipeline => ("▶", "Pipeline run"),
        DocKind::Repo => ("◫", "Repository"),
        DocKind::Project => ("◇", "Project"),
        DocKind::Person => ("@", "Person"),
        DocKind::Command => ("›", "Command"),
    }
}

pub fn scope_label(kind: DocKind) -> &'static str {
    match kind {
        DocKind::PullRequest => "PRs",
        DocKind::WorkItem => "Work items",
        DocKind::Pipeline => "Pipelines",
        DocKind::Repo => "Repos",
        DocKind::Project => "Projects",
        DocKind::Person => "People",
        DocKind::Command => "Commands",
    }
}

/// Unix ms now (row times are relative to it).
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A results change waiting for its frame to paint.
struct PaintProbe {
    started: Instant,
    query_len: usize,
    hits: usize,
}

pub struct Palette {
    visible: bool,
    input: Entity<TextInput>,
    commands: Vec<PaletteCommand>,
    catalog: Arc<Catalog>,
    settings: Arc<PaletteSettings>,
    settings_path: Option<PathBuf>,
    settings_text: Option<String>,
    settings_issues: Vec<String>,
    load_task: Option<Task<()>>,
    load_error: Option<String>,
    query: Query,
    /// Query the current `results` answer; differs from `query` while a
    /// search is in flight (old results stay up: no empty flash, §4.3).
    results_query: Query,
    results: Vec<Row>,
    /// Items opened this session, newest first (empty query "Recent").
    recent: Vec<u32>,
    selected: usize,
    scroll: UniformListScrollHandle,
    generation: u64,
    /// The generation `results` came from; behind `generation` while a
    /// search is in flight.
    applied: u64,
    latest: Arc<AtomicU64>,
    search_task: Option<Task<()>>,
    typed_at: Option<Instant>,
    paint_probe: Option<PaintProbe>,
    latency: Samples,
    previous_focus: Option<FocusHandle>,
    /// Fixed "now" for relative times (gallery snapshots); wall clock if unset.
    clock: Option<i64>,
    /// Render in place instead of as a window overlay (gallery stories).
    inline: bool,
    _input_events: Subscription,
    pin_events: Option<Subscription>,
    action_menu: MenuSlot,
    row_bounds: adoken_ui::action_menu::BoundsSlot,
    unpin_mode: bool,
}

impl EventEmitter<PaletteEvent> for Palette {}

impl Focusable for Palette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

fn command_entries(commands: &[PaletteCommand]) -> Vec<CommandEntry> {
    commands
        .iter()
        .map(|c| CommandEntry {
            title: c.title,
            keys: c.keys,
        })
        .collect()
}

impl Palette {
    /// Creates the (hidden) palette with only commands searchable; call
    /// [`Palette::load_store`] or [`Palette::set_catalog`] for content.
    pub fn new(commands: Vec<PaletteCommand>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(PLACEHOLDER, cx));
        let input_events = cx.subscribe(&input, |this, input, event, cx| match event {
            TextInputEvent::Changed => {
                let text = input.read(cx).text().to_owned();
                this.query_changed(&text, cx);
            }
            TextInputEvent::Pasted => {}
        });
        let catalog = Arc::new(Catalog::commands_only(command_entries(&commands)));
        Self {
            visible: false,
            input,
            commands,
            catalog,
            settings: Arc::new(PaletteSettings::default()),
            settings_path: None,
            settings_text: None,
            settings_issues: Vec::new(),
            load_task: None,
            load_error: None,
            query: Query::default(),
            results_query: Query::default(),
            results: Vec::new(),
            recent: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            generation: 0,
            applied: 0,
            latest: Arc::new(AtomicU64::new(0)),
            search_task: None,
            typed_at: None,
            paint_probe: None,
            latency: Samples::new("palette.keystroke_to_paint"),
            previous_focus: None,
            clock: None,
            inline: false,
            pin_events: None,
            action_menu: MenuSlot::default(),
            row_bounds: Default::default(),
            unpin_mode: false,
            _input_events: input_events,
        }
    }

    /// Reads the store and `settings.toml` on a background executor and
    /// swaps in the full catalog when done.
    pub fn load_store(&mut self, store: Store, settings_path: PathBuf, cx: &mut Context<Self>) {
        let commands = command_entries(&self.commands);
        self.settings_path = Some(settings_path.clone());
        let job = cx.background_executor().spawn(async move {
            let started = Instant::now();
            let settings_text = std::fs::read_to_string(&settings_path).ok();
            let data = CatalogData::load(&store).map_err(|error| error.to_string());
            let catalog = data.map(|data| Catalog::new(data, commands));
            (catalog, settings_text, started.elapsed())
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let (catalog, settings_text, took) = job.await;
            this.update(cx, |this, cx| {
                this.load_task = None;
                this.apply_settings_text(settings_text);
                match catalog {
                    Ok(catalog) => {
                        if cfg!(debug_assertions) {
                            eprintln!(
                                "adoken: palette catalog items={} took={}",
                                catalog.len(),
                                Ms(took)
                            );
                        }
                        this.set_catalog(Arc::new(catalog), cx);
                    }
                    Err(error) => {
                        eprintln!("adoken: couldn't load the palette index: {error}");
                        this.load_error = Some(error);
                        cx.notify();
                    }
                }
            })
            .ok();
        }));
    }

    pub fn set_catalog(&mut self, catalog: Arc<Catalog>, cx: &mut Context<Self>) {
        self.catalog = catalog;
        self.recent.clear();
        self.search(cx);
    }

    /// Pin "now" (relative times) for deterministic snapshots.
    pub fn set_clock(&mut self, now: i64, cx: &mut Context<Self>) {
        self.clock = Some(now);
        cx.notify();
    }

    /// Render inside the parent element instead of over the window.
    pub fn set_inline(&mut self, inline: bool, cx: &mut Context<Self>) {
        self.inline = inline;
        cx.notify();
    }

    fn now(&self) -> i64 {
        self.clock.unwrap_or_else(now_ms)
    }

    pub fn set_settings(&mut self, settings: PaletteSettings, cx: &mut Context<Self>) {
        self.settings = Arc::new(settings);
        cx.notify();
    }

    fn apply_settings_text(&mut self, text: Option<String>) {
        if text == self.settings_text && self.settings_text.is_some() {
            return;
        }
        let (settings, issues) = PaletteSettings::from_toml(text.as_deref().unwrap_or(""));
        for issue in &issues {
            eprintln!("adoken: {issue}");
        }
        self.settings = Arc::new(settings);
        self.settings_issues = issues;
        self.settings_text = text;
    }

    /// Re-reads `settings.toml` off the UI thread; row changes apply on the
    /// next frame (spec 0003 FR-8).
    fn reload_settings(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.settings_path.clone() else {
            return;
        };
        let job = cx
            .background_executor()
            .spawn(async move { std::fs::read_to_string(path).ok() });
        cx.spawn(async move |this, cx| {
            let text = job.await;
            this.update(cx, |this, cx| {
                this.apply_settings_text(text);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Nothing typed (so no scope chip either): the guided tour's first use
    /// (spec 0020 FR-2 item 3).
    pub fn query_is_empty(&self, cx: &App) -> bool {
        self.input.read(cx).text().is_empty()
    }

    /// What's typed (smoke runs: nothing typed during a tour is lost).
    pub fn query_text<'a>(&self, cx: &'a App) -> &'a str {
        self.input.read(cx).text()
    }

    pub fn selected_pin_subject(&self) -> Option<PinSubject> {
        match self.results.get(self.selected)? {
            Row::Pin(subject) => Some(subject.clone()),
            Row::Item { item, .. } => self.catalog.pin_subject(*item),
            Row::NotSynced(Target::PullRequest(id)) => Some(PinSubject::Pr(*id)),
            Row::NotSynced(Target::WorkItem(id)) => Some(PinSubject::WorkItem(*id)),
            _ => None,
        }
    }

    pub fn selected_title(&self) -> SharedString {
        match self.results.get(self.selected) {
            Some(Row::Item { item, .. }) => self.catalog.title(*item).to_owned().into(),
            _ => self
                .selected_pin_subject()
                .and_then(|subject| subject.reference())
                .unwrap_or_else(|| "Pipeline".to_owned())
                .into(),
        }
    }

    /// Why the selected command can't run now (it's listed disabled).
    pub fn selected_gate(&self, cx: &App) -> Option<SharedString> {
        match self.results.get(self.selected)? {
            Row::Item { item, .. } => match self.catalog.item(*item) {
                Item::Command(command) => {
                    gates::blocked(self.commands[*command].action.as_ref(), cx)
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn browser_url(&self, cx: &App) -> Option<String> {
        match self.results.get(self.selected)? {
            Row::Item { item, .. } => self.catalog.web_url(*item),
            Row::Pin(subject) => Pins::global(cx)?
                .read(cx)
                .snapshot()
                .pins
                .iter()
                .find(|pin| pin.subject() == subject)?
                .web_url
                .clone(),
            _ => None,
        }
    }

    fn open_selected_browser(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = self.browser_url(cx) else {
            return;
        };
        if let Some(subject) = self.selected_pin_subject() {
            crate::pin_actions::mark_opened(subject, cx);
        }
        self.close(window, cx);
        cx.open_url(&url);
    }

    fn open_row_menu(
        &mut self,
        position: MenuPosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        let ix = self.selected;
        let primary = match self.results.get(ix) {
            Some(Row::Item { item, .. }) => match self.catalog.item(*item) {
                Item::Command(_) => "Run",
                Item::Person(_) | Item::Repo(_) | Item::Project(_) => "Show PRs",
                _ => "Open",
            },
            Some(Row::Pin(_)) if self.unpin_mode => "Unpin",
            _ => "Open",
        };
        let mut items = vec![
            adoken_ui::action_menu::MenuItem::new(primary, move |window, cx| {
                weak.update(cx, |this, cx| this.confirm_at(ix, window, cx))
                    .ok();
            })
            .keys("enter"),
            crate::pin_actions::menu_item(self.selected_pin_subject(), self.selected_title(), cx),
        ];
        let weak = cx.entity().downgrade();
        items.insert(
            1,
            adoken_ui::action_menu::MenuItem::new("Open in browser", move |window, cx| {
                weak.update(cx, |this, cx| this.open_selected_browser(window, cx))
                    .ok();
            })
            .keys("secondary-enter")
            .disabled(
                self.browser_url(cx)
                    .is_none()
                    .then_some("No browser target available"),
            ),
        );
        // Spec 0015 FR-35: a work item row also edits; each action opens the
        // item in Work items with that editor open.
        if let Some(Row::Item { item, .. }) = self.results.get(ix)
            && let Item::WorkItem(wi) = self.catalog.item(*item)
        {
            use crate::workitems::detail::Intent;
            let id = wi.id;
            let read_only = !crate::workitems::writes::access(cx).can_write();
            for (label, intent) in [
                ("Change state…", Intent::State),
                ("Assign…", Intent::Assign),
                ("Assign to me", Intent::AssignToMe),
                ("Comment…", Intent::Comment),
                ("Sync this work item", Intent::Sync),
            ] {
                let weak = cx.entity().downgrade();
                let item = adoken_ui::action_menu::MenuItem::new(label, move |window, cx| {
                    crate::workitems::detail::queue_intent(id, intent, cx);
                    weak.update(cx, |this, cx| this.confirm_at(ix, window, cx))
                        .ok();
                });
                let reason = (read_only && intent != Intent::Sync)
                    .then_some(crate::workitems::writes::READ_ONLY_HINT);
                items.push(item.disabled(reason));
            }
        }
        if let Some(subject) = self.selected_pin_subject()
            && let Some(pins) = Pins::global(cx)
            && pins.read(cx).is_pinned(&subject)
        {
            items.push(adoken_ui::action_menu::MenuItem::new(
                "Rename pin…",
                move |window, cx| {
                    window.dispatch_action(Box::new(crate::pins::RenamePin), cx);
                },
            ));
        }
        self.action_menu.open(
            "Actions",
            items,
            position,
            z::PALETTE + 1,
            |this| &mut this.action_menu,
            window,
            cx,
        );
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.close(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            return;
        }
        self.visible = true;
        if self.pin_events.is_none()
            && let Some(pins) = Pins::global(cx)
        {
            self.pin_events = Some(cx.observe(&pins, |this, _, cx| {
                if this.visible {
                    this.search(cx);
                }
            }));
        }
        self.previous_focus = window.focused(cx);
        window.focus(&self.input.focus_handle(cx), cx);
        self.reload_settings(cx);
        self.search(cx);
        cx.notify();
    }

    /// Opens with `text` typed (`>` for commands, refinements).
    pub fn open_with(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.unpin_mode = false;
        self.open(window, cx);
        self.set_query(text, cx);
    }

    pub fn open_unpin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_with("pin: ", window, cx);
        self.unpin_mode = true;
        cx.notify();
    }

    pub fn set_query(&mut self, text: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text(text, cx));
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.unpin_mode = false;
        // The query is cleared on close (§1).
        self.input.update(cx, |input, cx| input.set_text("", cx));
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
        if cfg!(debug_assertions)
            && let Some(summary) = self.latency.summary()
        {
            eprintln!("adoken: {summary}");
        }
        self.latency.clear();
        cx.notify();
    }

    fn query_changed(&mut self, text: &str, cx: &mut Context<Self>) {
        self.typed_at = Some(Instant::now());
        self.query = Query::parse(text);
        self.search(cx);
    }

    /// Start (or restart) the search for `self.query`, cancelling any
    /// in-flight one.
    fn search(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.latest.store(generation, Ordering::Release);
        let catalog = self.catalog.clone();
        let query = self.query.clone();
        let latest = self.latest.clone();
        let limit = self.settings.max_results;
        let now = self.now();
        let recent = if query.is_blank() {
            self.recent.clone()
        } else {
            Vec::new()
        };
        let pins = Pins::global(cx)
            .map(|pins| pins.read(cx).snapshot())
            .unwrap_or_default();
        let job = cx.background_executor().spawn(async move {
            if latest.load(Ordering::Acquire) != generation {
                return None;
            }
            let rows = palette_results(&pins, &query, &catalog, &recent, limit, now);
            Some((query, rows))
        });
        self.search_task = Some(cx.spawn(async move |this, cx| {
            if let Some((query, rows)) = job.await {
                this.update(cx, |this, cx| this.apply(generation, query, rows, cx))
                    .ok();
            }
        }));
    }

    fn apply(&mut self, generation: u64, query: Query, rows: Vec<Row>, cx: &mut Context<Self>) {
        if generation != self.generation {
            return;
        }
        self.applied = generation;
        if let Some(started) = self.typed_at.take() {
            self.paint_probe = Some(PaintProbe {
                started,
                query_len: query.text.chars().count(),
                hits: rows.len(),
            });
        }
        self.results = rows;
        self.results_query = query;
        self.selected = 0;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.results.is_empty() {
            return;
        }
        self.selected = step_selection(self.selected, self.results.len(), delta);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn page_rows() -> isize {
        MAX_VISIBLE_ROWS as isize
    }

    /// Tab: `pr:` → `pr: ` (chip), or `auther:` → `author:` (§3.3).
    fn complete(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).text().to_owned();
        let fixed = if let Some(unknown) = self.query.unknown.first()
            && let Some(key) = unknown.suggestion
        {
            text.replacen(&format!("{}:", unknown.key), &format!("{}:", key.name()), 1)
        } else if matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "pin:" | "pr:" | "wi:" | "cmd:" | "people:" | "p:" | "pipe:" | "r:" | "repo:" | "proj:"
        ) {
            format!("{} ", text.trim())
        } else {
            return;
        };
        self.set_query(&fixed, cx);
    }

    fn confirm_at(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Row::Pin(subject)) = self.results.get(ix).cloned() {
            let unpin = self.unpin_mode;
            self.close(window, cx);
            if let Some(pins) = Pins::global(cx) {
                pins.update(cx, |pins, cx| {
                    if unpin {
                        pins.unpin(&subject, cx);
                    } else {
                        pins.open(&subject, cx);
                    }
                });
            }
            return;
        }
        let Some(Row::Item { item, .. }) = self.results.get(ix).cloned() else {
            // Not-synced / other-org rows are informational (disabled).
            return;
        };
        let catalog = self.catalog.clone();
        match catalog.item(item) {
            Item::Command(command) => {
                let action = self.commands[*command].action.boxed_clone();
                // Gated (command-palette.md §4.3): explain, run nothing.
                if let Some(reason) = gates::blocked(action.as_ref(), cx) {
                    adoken_ui::ToastStack::show(
                        adoken_ui::Toast::new("palette-gated", IconName::Lock, reason)
                            .tone(adoken_ui::ToastTone::Warning),
                        cx,
                    );
                    return;
                }
                self.close(window, cx);
                // After close, so the action sees the restored focus.
                window.dispatch_action(action, cx);
            }
            Item::Person(_) | Item::Repo(_) | Item::Project(_) => {
                if let Some(refine) = catalog.refine_query(item) {
                    self.set_query(&refine, cx);
                }
            }
            Item::PullRequest(_) | Item::WorkItem(_) | Item::Run(_) => {
                let Some(target) = catalog.target(item) else {
                    return;
                };
                self.recent.retain(|&r| r != item);
                if let Some(subject) = catalog.pin_subject(item) {
                    crate::pin_actions::mark_opened(subject, cx);
                }
                self.recent.insert(0, item);
                self.recent.truncate(self.settings.recent_count);
                self.close(window, cx);
                cx.emit(PaletteEvent::Open(target));
            }
        }
    }

    fn row_height(&self) -> f32 {
        match self.settings.density {
            Density::Compact => size::PALETTE_ROW_COMPACT,
            Density::Default => size::PALETTE_ROW_DEFAULT,
            Density::Comfortable => size::PALETTE_ROW_COMFORTABLE,
        }
    }

    fn render_row(&self, ix: usize, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let selected = ix == self.selected;
        let height = self.row_height();
        let group = if self.results_query.is_blank() {
            let label = |row: &Row| match row {
                Row::Pin(_) => "Pinned",
                Row::Item { item, .. } if self.recent.contains(item) => "Recent",
                _ => "My work",
            };
            (ix == 0 || label(&self.results[ix]) != label(&self.results[ix - 1]))
                .then(|| label(&self.results[ix]))
        } else {
            None
        };
        let caption = || {
            group.map(|label| {
                div()
                    .flex_none()
                    .px(px(space::S12))
                    .text_color(theme.colors.text_tertiary)
                    .type_style(typography::LABEL, theme)
                    .child(label)
            })
        };
        let row = match &self.results[ix] {
            Row::Pin(subject) => {
                let snapshot = Pins::global(cx)
                    .map(|pins| pins.read(cx).snapshot())
                    .unwrap_or_default();
                let Some(view) = snapshot.pins.iter().find(|pin| pin.subject() == subject) else {
                    return div().into_any_element();
                };
                let look = crate::pins::render::look(view, self.now());
                let pin_row = adoken_ui::PinRow::new(
                    ("palette-pin", ix),
                    look.glyph,
                    look.tint.color(theme),
                    view.display_title().to_owned(),
                )
                .changed(view.changed)
                .trailing(look.trailing.resolve(theme, 0.0))
                .state(adoken_ui::PinRowState {
                    cursor: selected,
                    ..Default::default()
                })
                .aria_label(format!(
                    "Pinned {}, {}, {}, {}",
                    subject.key(),
                    view.display_title(),
                    view.headline.word(self.now()),
                    if view.changed {
                        "changed since you opened it"
                    } else {
                        "unchanged"
                    },
                ))
                .on_click(cx.listener(
                    move |this, event: &gpui::ClickEvent, window, cx| {
                        this.selected = ix;
                        if event.modifiers().platform {
                            this.open_selected_browser(window, cx);
                        } else {
                            this.confirm_at(ix, window, cx);
                        }
                    },
                ));
                return div()
                    .relative()
                    .w_full()
                    .h(px(height))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .children(caption())
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(
                                    move |this, event: &gpui::MouseDownEvent, window, cx| {
                                        this.selected = ix;
                                        this.open_row_menu(
                                            MenuPosition::at_pointer(event.position),
                                            window,
                                            cx,
                                        );
                                    },
                                ),
                            )
                            .child(div().flex_1().min_w_0().child(pin_row)),
                    )
                    .children(
                        selected
                            .then(|| adoken_ui::action_menu::track_bounds(self.row_bounds.clone())),
                    )
                    .into_any_element();
            }
            Row::Item { item, positions } => self.render_item(ix, *item, positions, theme),
            Row::NotSynced(target) => {
                let (kind, id) = match target {
                    Target::PullRequest(id) => (DocKind::PullRequest, format!("!{id}")),
                    Target::WorkItem(id) => (DocKind::WorkItem, format!("#{id}")),
                    Target::Run(id) => (DocKind::Pipeline, format!("run {id}")),
                };
                ListRow::new(("palette-row", ix), format!("{id} isn't synced yet"))
                    .subtitle("Not in the local store. It appears here once sync picks it up.")
                    .leading(kind_glyph(kind, theme, true))
            }
            Row::OtherOrg { org } => {
                ListRow::new(("palette-row", ix), "This link is for another organization")
                    .subtitle(format!("{org} isn't connected to adoken"))
                    .leading(
                        Icon::new(IconName::CircleAlert)
                            .size(size::ICON_MD)
                            .color(theme.colors.text_disabled),
                    )
            }
        };
        // A gated command shows why it can't run, dimmed (§4.3).
        let gated = match &self.results[ix] {
            Row::Item { item, .. } => match self.catalog.item(*item) {
                Item::Command(command) => {
                    gates::blocked(self.commands[*command].action.as_ref(), cx)
                }
                _ => None,
            },
            _ => None,
        };
        let row = match &gated {
            Some(reason) => row.subtitle(reason.clone()),
            None => row,
        };
        let row = row
            .height(
                height
                    - if group.is_some() {
                        typography::LABEL.line_height
                    } else {
                        0.0
                    },
            )
            .selected(selected)
            .track_selected_bounds(self.row_bounds.clone())
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.selected = ix;
                    if event.modifiers().platform {
                        this.open_selected_browser(window, cx);
                    } else {
                        this.confirm_at(ix, window, cx);
                    }
                }),
            )
            .on_secondary_click(cx.listener(
                move |this, event: &gpui::MouseDownEvent, window, cx| {
                    this.selected = ix;
                    this.open_row_menu(MenuPosition::at_pointer(event.position), window, cx);
                },
            ))
            .into_any_element();
        div()
            .h(px(height))
            .flex()
            .flex_col()
            .when(gated.is_some(), |wrap| {
                wrap.opacity(adoken_theme::opacity::DIMMED)
            })
            .children(caption())
            .child(row)
            .into_any_element()
    }

    fn render_item(&self, ix: usize, item: u32, positions: &[u32], theme: &Theme) -> ListRow {
        let catalog = &self.catalog;
        let now = self.now();
        let entry = catalog.item(item);
        let row_type = RowType::of(entry.kind());
        let template = self.settings.row(row_type);
        let field = |name: &str| catalog.field(item, name, now);

        let title = template.title.render(field);
        let title = if title.is_empty() {
            catalog.title(item).to_owned()
        } else {
            title
        };
        // Matches are char positions in the indexed title; shift them to
        // where the title sits inside the rendered template.
        let doc_title = catalog.title(item);
        let highlights = match title.find(doc_title) {
            Some(offset) => match_ranges(doc_title, positions)
                .into_iter()
                .map(|r| r.start + offset..r.end + offset)
                .collect(),
            None => Vec::new(),
        };
        let mut row = ListRow::new(("palette-row", ix), title)
            .highlights(highlights)
            .leading(leading(
                template.leading,
                entry,
                catalog,
                self.settings.avatars,
                theme,
            ));
        if self.settings.density != Density::Compact {
            row = row.subtitle(template.subtitle.render(field));
        }
        for badge in &template.badges {
            let value = field(badge);
            if !value.is_empty() {
                row = row.badge(Chip::new(value).tone(badge_tone(badge)));
            }
        }
        for name in &template.trailing {
            if row_type == RowType::Command && name == "shortcut" {
                if let Item::Command(ix) = entry
                    && let Some(kbd) = catalog.command(*ix).keys.and_then(|k| Kbd::parse(k).ok())
                {
                    row = row.trailing(kbd);
                }
                continue;
            }
            let value = field(name);
            if !value.is_empty() {
                row = row.trailing(value);
            }
        }
        row
    }

    fn render_message(&self, title: SharedString, hint: SharedString, theme: &Theme) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .gap(px(space::S4))
            .px(px(space::S16))
            .py(px(space::S24))
            .child(
                div()
                    .text_color(theme.colors.text_primary)
                    .type_style(typography::BODY_STRONG, theme)
                    .child(title),
            )
            .child(
                div()
                    .text_color(theme.colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(hint),
            )
    }

    fn render_body(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        if self.results.is_empty() {
            let (title, hint) = if self.results_query.pins {
                if self.results_query.text.is_empty() && self.results_query.filters.is_empty() {
                    ("No pins yet".to_owned(), "Pin what you use daily · ⌘D")
                } else {
                    ("No pins match".to_owned(), FILTER_HINT)
                }
            } else if self.results_query.is_blank() {
                (
                    "Search pull requests, work items, pipelines, people".to_owned(),
                    SCOPE_HINT,
                )
            } else if !self.catalog.is_loaded()
                && self.results_query.scope != Some(DocKind::Command)
            {
                (
                    "Loading local data…".to_owned(),
                    "Commands work now; everything else appears in a moment.",
                )
            } else if self.results_query.text.is_empty() {
                let what = match self.results_query.scope {
                    Some(kind) => format!("No {} match", scope_label(kind).to_lowercase()),
                    None => "No matches".to_owned(),
                };
                (what, FILTER_HINT)
            } else {
                (
                    format!("No matches for “{}”", self.results_query.text),
                    SCOPE_HINT,
                )
            };
            return self
                .render_message(title.into(), hint.into(), theme)
                .into_any_element();
        }

        let row_height = self.row_height();
        let height = (self.results.len() as f32).min(MAX_VISIBLE_ROWS) * row_height;
        let theme = theme.clone();
        // Tour anchor (spec 0020): the results once there are some.
        let tour_anchor = adoken_ui::tour::anchor(anchors::RESULTS, cx);
        div()
            .my(px(space::S4))
            .child(
                uniform_list(
                    "palette-results",
                    self.results.len(),
                    cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                        range
                            .map(|ix| this.render_row(ix, &theme, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .h(px(height)),
            )
            .children(tour_anchor)
            .into_any_element()
    }

    /// Footer notice: unknown filter, settings issue, or load failure.
    fn notice(&self) -> Option<String> {
        if let Some(unknown) = self.query.unknown.first() {
            return Some(match unknown.suggestion {
                Some(key) => format!(
                    "Unknown filter `{}:` — did you mean `{}:`? Tab to fix",
                    unknown.key,
                    key.name()
                ),
                None => format!("Unknown filter `{}:`", unknown.key),
            });
        }
        if let Some(error) = &self.load_error {
            return Some(format!("Couldn't read local data: {error}"));
        }
        self.settings_issues
            .first()
            .map(|issue| format!("Row settings: {issue}"))
    }

    fn render_footer(&self, theme: &Theme, cx: &App) -> gpui::Div {
        let hint = |keys: &str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .child(Kbd::parse(keys).expect("static binding"))
                .child(label)
        };
        let selected_kind = match self.results.get(self.selected) {
            Some(Row::Item { item, .. }) => Some(self.catalog.item(*item).kind()),
            Some(Row::Pin(subject)) => Some(match subject {
                PinSubject::Pr(_) => DocKind::PullRequest,
                PinSubject::WorkItem(_) => DocKind::WorkItem,
                PinSubject::Definition { .. } => DocKind::Pipeline,
            }),
            _ => None,
        };
        // Informational rows (not synced, other org) have no primary action.
        let primary = match selected_kind {
            Some(DocKind::Command) => Some("Run"),
            Some(DocKind::Person | DocKind::Repo | DocKind::Project) => Some("Show PRs"),
            Some(_) => Some("Open"),
            None => None,
        };
        let count = match self.results.len() {
            1 => "1 result".to_owned(),
            n => format!("{n} results"),
        };
        let status = match (self.catalog.is_loaded(), selected_kind) {
            (false, _) => "Loading local data…".to_owned(),
            (true, _) if self.results_query.pins => count,
            (true, _) if self.results.len() >= self.settings.max_results => {
                format!(
                    "Showing first {} — refine your search",
                    self.settings.max_results
                )
            }
            (true, Some(kind)) => format!("{} · {count}", kind_meta(kind).1),
            (true, None) => count,
        };
        let footer = div()
            .flex_none()
            .h(px(FOOTER_HEIGHT))
            .px(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S16))
            .border_t_1()
            .border_color(theme.colors.border_subtle)
            .text_color(theme.colors.text_tertiary)
            .type_style(typography::SMALL, theme);
        match self.notice() {
            Some(notice) => footer
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size(size::ICON_SM)
                        .color(theme.colors.status_warning_fg),
                )
                .child(div().flex_1().truncate().child(notice)),
            None => footer
                .children(primary.map(|label| hint("enter", label)))
                .children((!self.results_query.pins).then(|| hint("up", "Previous")))
                .child(hint(
                    "down",
                    if self.results_query.pins {
                        "Navigate"
                    } else {
                        "Next"
                    },
                ))
                .children((!self.results_query.pins).then(|| hint("tab", "Complete")))
                .child(hint("escape", "Close"))
                .child(div().flex_1())
                .child(status)
                .children(if self.results_query.pins {
                    Pins::global(cx)
                        .map(|pins| pins.read(cx).opens_this_week())
                        .filter(|count| *count > 0)
                        .map(|count| div().child(format!("Opened from pins {count}× this week")))
                } else {
                    None
                }),
        }
    }
}

fn palette_results(
    snapshot: &crate::pins::snapshot::Snapshot,
    query: &Query,
    catalog: &Catalog,
    recent: &[u32],
    limit: usize,
    now: i64,
) -> Vec<Row> {
    let mut rows = pin_rows(snapshot, query, catalog, now);
    if !query.pins {
        let recent = recent.iter().copied().map(|item| Row::Item {
            item,
            positions: Vec::new(),
        });
        for row in recent.chain(catalog.run(query, limit, now)) {
            let duplicate_pin = match &row {
                Row::Item { item, .. } => catalog.pin_subject(*item).is_some_and(|subject| {
                    rows.iter()
                        .any(|row| matches!(row, Row::Pin(pin) if pin == &subject))
                }),
                _ => false,
            };
            if !duplicate_pin && !rows.contains(&row) {
                rows.push(row);
            }
        }
    }
    rows.truncate(limit);
    rows
}

fn pin_rows(
    snapshot: &crate::pins::snapshot::Snapshot,
    query: &Query,
    catalog: &Catalog,
    now: i64,
) -> Vec<Row> {
    if !query.pins && !query.is_blank() {
        return Vec::new();
    }
    let needle = query.text.to_lowercase();
    snapshot
        .pins
        .iter()
        .filter(|pin| {
            (needle.is_empty()
                || pin.display_title().to_lowercase().contains(&needle)
                || pin.title.to_lowercase().contains(&needle)
                || pin
                    .subject()
                    .reference()
                    .is_some_and(|reference| reference.contains(&needle)))
                && catalog.pin_matches(pin.subject(), query, now)
        })
        .take(if query.pins { usize::MAX } else { 9 })
        .map(|pin| Row::Pin(pin.subject().clone()))
        .collect()
}

fn kind_color(kind: DocKind, theme: &Theme) -> Hsla {
    let colors = &theme.colors;
    match kind {
        DocKind::PullRequest => colors.pr_active,
        DocKind::WorkItem => colors.wi_story,
        DocKind::Pipeline => colors.pipeline_running,
        DocKind::Repo | DocKind::Project | DocKind::Person | DocKind::Command => {
            colors.text_secondary
        }
    }
}

fn kind_glyph(kind: DocKind, theme: &Theme, disabled: bool) -> gpui::Div {
    let color = if disabled {
        theme.colors.text_disabled
    } else {
        kind_color(kind, theme)
    };
    div()
        .text_color(color)
        .type_style(typography::BODY_STRONG, theme)
        .child(kind_meta(kind).0)
}

fn badge_tone(field: &str) -> ChipTone {
    match field {
        "my_vote" => ChipTone::Accent,
        _ => ChipTone::Neutral,
    }
}

/// The leading glyph for a row (§5.1).
fn leading(
    leading: Leading,
    item: &Item,
    catalog: &Catalog,
    avatars: bool,
    theme: &Theme,
) -> AnyElement {
    let colors = &theme.colors;
    match (leading, item) {
        (Leading::None, _) => div().into_any_element(),
        (Leading::StatusIcon, Item::PullRequest(pr)) => {
            use adoken_model::PrStatus;
            let color = match (pr.status, pr.is_draft) {
                (PrStatus::Active, true) => colors.pr_draft,
                (PrStatus::Active, false) => colors.pr_active,
                (PrStatus::Completed, _) => colors.pr_completed,
                (PrStatus::Abandoned, _) => colors.pr_abandoned,
            };
            Icon::new(IconName::GitPullRequest)
                .size(size::ICON_MD)
                .color(color)
                .into_any_element()
        }
        (Leading::StatusIcon, Item::Run(run)) => {
            use adoken_model::{RunResult, RunStatus};
            let (icon, color) = match (run.status, run.result) {
                (_, Some(RunResult::Succeeded)) => {
                    (IconName::CircleCheck, colors.pipeline_succeeded)
                }
                (_, Some(RunResult::Failed)) => (IconName::CircleX, colors.pipeline_failed),
                (_, Some(RunResult::PartiallySucceeded)) => {
                    (IconName::CircleAlert, colors.pipeline_partial)
                }
                (_, Some(RunResult::Canceled)) => (IconName::CircleX, colors.pipeline_canceled),
                (RunStatus::InProgress | RunStatus::Cancelling, None) => {
                    (IconName::LoaderCircle, colors.pipeline_running)
                }
                _ => (IconName::CircleDashed, colors.pipeline_queued),
            };
            Icon::new(icon)
                .size(size::ICON_MD)
                .color(color)
                .into_any_element()
        }
        (Leading::TypeIcon | Leading::StatusIcon, Item::WorkItem(wi)) => {
            use adoken_model::TypeFamily;
            let color = match wi.kind.family() {
                TypeFamily::Bug => colors.wi_bug,
                TypeFamily::Task => colors.wi_task,
                TypeFamily::Requirement => colors.wi_story,
                TypeFamily::Feature => colors.wi_feature,
                TypeFamily::Epic => colors.wi_epic,
                TypeFamily::Issue => colors.wi_issue,
                TypeFamily::Other => colors.text_secondary,
            };
            div()
                .text_color(color)
                .type_style(typography::BODY_STRONG, theme)
                .child("▣")
                .into_any_element()
        }
        // `[palette] avatars = false` (0003 FR-8): the type glyph, and no
        // picture is ever asked for (0012 FR-14).
        (Leading::Avatar, _) if avatars => {
            let person = match item {
                Item::PullRequest(pr) => catalog.person(&pr.author),
                Item::WorkItem(wi) => wi.assigned_to.as_deref().and_then(|id| catalog.person(id)),
                Item::Person(p) => Some(p),
                _ => None,
            };
            match person.filter(|p| !p.display_name.is_empty()) {
                Some(person) => Avatar::person(person).into_any_element(),
                None => kind_glyph(item.kind(), theme, false).into_any_element(),
            }
        }
        (_, item) => kind_glyph(item.kind(), theme, false).into_any_element(),
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        // Guided tours wait for the results their anchor covers (spec 0020).
        if self.load_task.is_some() || self.applied != self.generation {
            adoken_ui::tour::unsettled(window, cx);
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let viewport = window.viewport_size();
        let width = size::PALETTE_WIDTH.min(f32::from(viewport.width) - 2.0 * space::S16);
        let top = if self.inline {
            space::S24
        } else {
            (f32::from(viewport.height) * 0.14).max(size::TITLEBAR + space::S48)
        };

        let probe = self.paint_probe.take().map(|probe| {
            let palette = cx.entity().downgrade();
            canvas(
                |_, _, _| {},
                move |_, _, _, cx| {
                    let elapsed = probe.started.elapsed();
                    if cfg!(debug_assertions) {
                        eprintln!(
                            "adoken: palette.latency query_len={} hits={} ms={}",
                            probe.query_len,
                            probe.hits,
                            Ms(elapsed)
                        );
                    }
                    palette
                        .update(cx, |this, _| this.latency.record(elapsed))
                        .ok();
                },
            )
            .size_0()
        });

        let scope_chip = self
            .query
            .scope
            .map(scope_label)
            .or(self.query.pins.then_some("Pins"))
            .map(|label| {
                div()
                    .flex_none()
                    .px(px(space::S6))
                    .rounded(px(radius::XS))
                    .bg(colors.accent_wash)
                    .text_color(colors.text_primary)
                    .type_style(typography::LABEL, &theme)
                    .child(label)
            });
        let filter_chips = self.query.filters.iter().map(|filter| {
            let label = format!(
                "{}{}:{}",
                if filter.negated { "-" } else { "" },
                filter.key.name(),
                filter.values.join(",")
            );
            Chip::new(label).tone(ChipTone::Accent)
        });

        let input_row = div()
            .flex_none()
            .h(px(INPUT_ROW_HEIGHT))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::HEADING, &theme)
                    .child("⌕"),
            )
            .children(scope_chip)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.text_primary)
                    .type_style(typography::HEADING, &theme)
                    .child(self.input.clone()),
            )
            .children(filter_chips)
            .children(adoken_ui::tour::anchor(anchors::INPUT, cx));

        let card = div()
            .id("palette")
            .key_context(KEY_CONTEXT)
            .occlude()
            .w(px(width))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(radius::LG))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .on_action(cx.listener(|this, _: &ConfirmBrowser, window, cx| {
                this.open_selected_browser(window, cx)
            }))
            .shadow(theme.shadows(adoken_theme::elevation::LEVEL_3))
            .on_action(cx.listener(|this, _: &crate::pins::TogglePin, window, cx| {
                crate::pin_actions::toggle(
                    this.selected_pin_subject(),
                    this.selected_title(),
                    window,
                    cx,
                );
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
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)))
            .on_action(
                cx.listener(|this, _: &PageDown, _, cx| this.move_selection(Self::page_rows(), cx)),
            )
            .on_action(
                cx.listener(|this, _: &PageUp, _, cx| this.move_selection(-Self::page_rows(), cx)),
            )
            .on_action(cx.listener(|this, _: &Confirm, window, cx| {
                this.confirm_at(this.selected, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Complete, _, cx| this.complete(cx)))
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                if this.input.read(cx).text().is_empty() {
                    this.close(window, cx);
                } else {
                    this.input.update(cx, |input, cx| input.set_text("", cx));
                }
            }))
            // Modal focus trap: Tab must not leave the query field.
            .on_action(|_: &FocusNext, _, _| {})
            .on_action(|_: &FocusPrevious, _, _| {})
            .child(input_row)
            .child(self.render_body(&theme, cx))
            .child(
                self.render_footer(&theme, cx)
                    .children(adoken_ui::tour::anchor(anchors::FOOTER, cx)),
            )
            .children(self.action_menu.element())
            .children(probe);

        let scrim = div()
            .id("palette-scrim")
            .occlude()
            .bg(colors.bg_scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .flex()
            .flex_col()
            .items_center()
            .pt(px(top))
            .child(crate::background::frosted(radius::LG, card));
        if self.inline {
            return scrim.size_full().into_any_element();
        }
        deferred(
            anchored()
                .position(point(px(0.0), px(0.0)))
                .child(scrim.w(viewport.width).h(viewport.height)),
        )
        .with_priority(usize::from(z::PALETTE))
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_steps_wrap_and_pages_clamp() {
        assert_eq!(step_selection(0, 5, -1), 4);
        assert_eq!(step_selection(4, 5, 1), 0);
        assert_eq!(step_selection(2, 5, 8), 4);
        assert_eq!(step_selection(2, 5, -8), 0);
        assert_eq!(step_selection(3, 0, 1), 0);
        // A stale selection past the end is clamped before stepping.
        assert_eq!(step_selection(9, 3, -1), 1);
    }

    /// Spec 0025 AC-1: ⌘1…⌘9, ⌘↑/⌘↓ and ⌃N/⌃P do nothing in the palette;
    /// ↑/↓ still step (and wrap, above).
    #[test]
    fn ac1_number_first_last_and_ctrl_keys_are_unbound_and_arrows_still_step() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(crate::key_bindings());
        let workspace = [KeyContext::parse("Workspace").expect("context")];
        let palette = [
            KeyContext::parse("Workspace").expect("context"),
            KeyContext::parse(KEY_CONTEXT).expect("context"),
        ];
        let resolve = |keys: &str, stack: &[KeyContext]| {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, stack);
            bindings.first().map(|binding| binding.action().name())
        };
        for keys in [
            "secondary-1",
            "secondary-5",
            "secondary-9",
            "secondary-up",
            "secondary-down",
            "ctrl-n",
            "ctrl-p",
        ] {
            assert_eq!(
                resolve(keys, &palette),
                resolve(keys, &workspace),
                "the palette binds {keys}"
            );
        }
        assert_eq!(resolve("down", &palette), Some(SelectNext.name()));
        assert_eq!(resolve("up", &palette), Some(SelectPrev.name()));
    }

    fn snapshot(
        subjects: Vec<(PinSubject, &str, Option<&str>)>,
    ) -> crate::pins::snapshot::Snapshot {
        crate::pins::snapshot::Snapshot {
            pins: subjects
                .into_iter()
                .enumerate()
                .map(|(position, (subject, title, label))| {
                    crate::pins::snapshot::PinView::provisional(adoken_model::Pin {
                        subject,
                        position: position as u32,
                        title: title.to_owned(),
                        label: label.map(str::to_owned),
                        pinned_at: 0,
                        opened_at: None,
                        seen_result: None,
                        unavailable: None,
                    })
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn pins_scope_keeps_local_order_matches_labels_and_caps_only_blank_global_query() {
        let pins = snapshot(
            (0..12)
                .map(|id| {
                    (
                        PinSubject::Pr(id),
                        "Remote title",
                        (id == 10).then_some("Fix SSO redirect"),
                    )
                })
                .collect(),
        );
        let catalog = Catalog::commands_only(Vec::new());
        assert_eq!(
            pin_rows(&pins, &Query::parse("pin:"), &catalog, 0),
            (0..12)
                .map(|id| Row::Pin(PinSubject::Pr(id)))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            pin_rows(&pins, &Query::parse("pin: ssO"), &catalog, 0),
            [Row::Pin(PinSubject::Pr(10))]
        );
        assert_eq!(
            pin_rows(&pins, &Query::default(), &catalog, 0),
            (0..9)
                .map(|id| Row::Pin(PinSubject::Pr(id)))
                .collect::<Vec<_>>()
        );
        assert!(pin_rows(&pins, &Query::parse("pin: unmatched"), &catalog, 0).is_empty());
        assert!(pin_rows(&pins, &Query::parse("!"), &catalog, 0).is_empty());
    }

    #[test]
    fn pinned_results_precede_recents_without_duplicate_subjects_and_filters_apply() {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let catalog = Catalog::new(CatalogData::from_dataset(&ds), Vec::new());
        let first = &ds.prs[0];
        let pins = snapshot(vec![(
            PinSubject::Pr(first.id),
            &first.title,
            Some("My review"),
        )]);
        let item = (0..catalog.len() as u32)
            .find(|ix| catalog.pin_subject(*ix) == Some(PinSubject::Pr(first.id)))
            .unwrap();
        let rows = palette_results(
            &pins,
            &Query::default(),
            &catalog,
            &[item],
            30,
            adoken_demo::DEMO_NOW,
        );
        assert_eq!(rows[0], Row::Pin(PinSubject::Pr(first.id)));
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, Row::Item { item: ix, .. } if *ix == item))
        );
        assert!(
            pin_rows(
                &pins,
                &Query::parse("pin: type:bug"),
                &catalog,
                adoken_demo::DEMO_NOW
            )
            .is_empty()
        );
        assert_eq!(catalog.pin_subject(item), Some(PinSubject::Pr(first.id)));
        let run = (0..catalog.len() as u32)
            .find(|ix| matches!(catalog.item(*ix), Item::Run(_)))
            .unwrap();
        let Item::Run(run_model) = catalog.item(run) else {
            unreachable!()
        };
        assert_eq!(
            catalog.pin_subject(run),
            Some(PinSubject::Definition {
                project_id: run_model.project_id.clone(),
                id: run_model.pipeline_id
            })
        );
    }

    #[test]
    fn definition_filter_uses_latest_run_not_an_older_failure_and_keeps_project_scope_without_runs()
    {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let mut data = CatalogData::from_dataset(&ds);
        let mut latest = data.runs[0].clone();
        latest.result = Some(adoken_model::RunResult::Succeeded);
        latest.status = adoken_model::RunStatus::Completed;
        let subject = PinSubject::Definition {
            project_id: latest.project_id.clone(),
            id: latest.pipeline_id,
        };
        let mut old = latest.clone();
        old.id += 1_000_000;
        old.result = Some(adoken_model::RunResult::Failed);
        old.queued -= adoken_model::time::HOUR;
        data.runs = vec![latest, old];
        let catalog = Catalog::new(data.clone(), Vec::new());
        assert!(!catalog.pin_matches(
            &subject,
            &Query::parse("pin: status:failed"),
            adoken_demo::DEMO_NOW
        ));
        assert!(catalog.pin_matches(
            &subject,
            &Query::parse("pin: status:succeeded"),
            adoken_demo::DEMO_NOW
        ));
        data.runs.clear();
        let catalog = Catalog::new(data, Vec::new());
        let PinSubject::Definition { project_id, .. } = &subject else {
            unreachable!()
        };
        assert!(catalog.pin_matches(
            &subject,
            &Query::parse(&format!("pin: project:{project_id}")),
            adoken_demo::DEMO_NOW
        ));
        assert!(!catalog.pin_matches(
            &subject,
            &Query::parse("pin: project:unrelated"),
            adoken_demo::DEMO_NOW
        ));
    }
}
