//! The window's activity state (spec 0016 FR-1–FR-6, FR-21): the drawer's
//! open/tab/height, the statusbar indicator's sampled view, and the data
//! both tabs read from the session's [`ActivityHub`]. One per shell; the
//! drawer's view ([`super::panel::ActivityPanel`]) is created on first
//! open (cold start, spec §7).
//!
//! Updates arrive as the hub's ≤ 4 Hz generation (FR-6). Each one samples
//! the log's summary for the indicator and notifies the shell only when
//! what it paints changed; the Network rows are pulled (by sequence
//! number) only while that tab shows. The sampler's own timers (150 ms
//! busy delay, 500 ms bridge, 5-minute age-out) run only when one is due.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use adoken_model::Millis;
use adoken_store::Store;
use adoken_sync::activity::{KindRow, PassRecord, RowState, SyncInputs};
use adoken_sync::ledger::{NetSummary, StatusClass};
use gpui::{Context, EventEmitter, Task};

use super::indicator::{
    Indicator, IndicatorView, NetAnnouncer, SyncAnnouncer, SyncMood, visibility,
};
use super::net::NetView;
use super::passes::{self, ItemKind, PassLine};
use super::settings::{self as codec, ActivitySettings, Tab};
use crate::palette::catalog::Target;
use crate::palette::now_ms;
use crate::sync::{ActivityHub, SyncState};

/// What the drawer asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityEvent {
    /// Open an item in its canonical view; the drawer stays open (FR-13).
    Open(Target),
    /// "2 notifications" opens the inbox.
    OpenInbox,
    /// The drawer closed: give focus back (FR-3).
    Closed,
}

/// A changed item's title as the store has it now; `None`: no longer
/// in the store ("No longer synced", not openable).
pub type Title = Option<String>;

pub struct Activity {
    hub: Option<ActivityHub>,
    demo: bool,
    /// Stories pin "now" so snapshots don't drift.
    clock: Option<Millis>,
    offset: time::UtcOffset,
    store: Option<Store>,

    // ── drawer ──
    open: bool,
    tab: Tab,
    /// `[activity]` as last read or written.
    settings: ActivitySettings,
    maximized: bool,
    window_height: f32,
    /// The content column's width (two columns, table columns).
    content_width: f32,
    /// The drawer opened by pointer (animates) vs keyboard (instant).
    pub(super) opened_by_pointer: bool,
    /// Opens so far: keys the open animation.
    pub(super) opens: usize,

    // ── statusbar & Network ──
    /// The Network tab's rows, filters and cursor.
    pub net: NetView,
    summary: Option<NetSummary>,
    seen_through: u64,
    indicator: Indicator,
    view: IndicatorView,
    net_announcer: NetAnnouncer,
    sync_announcer: SyncAnnouncer,
    announcement: Option<String>,
    offline: bool,
    no_token: bool,

    // ── Sync ──
    sync_state: Option<SyncState>,
    /// Stories and the smoke run feed `rows()` directly.
    inputs: Option<SyncInputs>,
    passes: Vec<PassRecord>,
    lines: Vec<PassLine>,
    pass_generation: Option<u64>,
    pub(super) expanded: HashSet<u64>,
    expanded_once: bool,
    titles: HashMap<(ItemKind, i64), Title>,
    title_reads: Option<Task<()>>,

    wake: Option<(Millis, Task<()>)>,
    _watch: Option<Task<()>>,
    _live: gpui::Subscription,
}

impl EventEmitter<ActivityEvent> for Activity {}

impl Activity {
    pub fn new(demo: bool, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            hub: None,
            demo,
            clock: None,
            offset: local_offset(now_ms()),
            store: None,
            open: false,
            tab: Tab::Sync,
            settings: ActivitySettings::default(),
            maximized: false,
            window_height: 760.0,
            content_width: 968.0,
            opened_by_pointer: false,
            opens: 0,
            net: NetView::default(),
            summary: None,
            seen_through: 0,
            indicator: Indicator::default(),
            view: IndicatorView::Hidden,
            net_announcer: NetAnnouncer::default(),
            sync_announcer: SyncAnnouncer::default(),
            announcement: None,
            offline: false,
            no_token: false,
            sync_state: None,
            inputs: None,
            passes: Vec::new(),
            lines: Vec::new(),
            pass_generation: None,
            expanded: HashSet::new(),
            expanded_once: false,
            titles: HashMap::new(),
            title_reads: None,
            wake: None,
            _watch: None,
            _live: cx.observe_global::<super::LiveSettings>(|this, cx| {
                let settings = cx.global::<super::LiveSettings>().0;
                this.apply_settings(settings, cx);
            }),
        };
        if let Some(hub) = crate::sync::activity_hub(cx) {
            this.set_hub(hub, cx);
        }
        this
    }

    /// Read the session's hub (or a fixture's) from now on.
    pub fn set_hub(&mut self, hub: ActivityHub, cx: &mut Context<Self>) {
        self.hub = Some(hub);
        self.net = NetView::default();
        self.pass_generation = None;
        self._watch = crate::sync::activity_changes(cx).map(|mut changes| {
            cx.spawn(async move |this, cx| {
                loop {
                    if changes.changed().await.is_err() {
                        return;
                    }
                    if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                        return;
                    }
                }
            })
        });
        self.refresh(cx);
    }

    pub fn hub(&self) -> Option<&ActivityHub> {
        self.hub.as_ref()
    }

    /// Stories: pin "now", the time zone, and the sync rows' inputs.
    pub fn set_fixture(&mut self, now: Millis, inputs: Option<SyncInputs>, cx: &mut Context<Self>) {
        self.clock = Some(now);
        self.offset = time::UtcOffset::UTC;
        self.inputs = inputs;
        self.refresh(cx);
    }

    pub fn set_store(&mut self, store: Store, cx: &mut Context<Self>) {
        self.store = Some(store);
        // Passes expanded before the store opened get their titles now.
        self.read_titles(cx);
    }

    pub fn now(&self) -> Millis {
        self.clock.unwrap_or_else(now_ms)
    }

    pub fn offset(&self) -> time::UtcOffset {
        self.offset
    }

    pub fn is_demo(&self) -> bool {
        self.demo
    }

    // ── connection state from the shell ─────────────────────────────────

    pub fn set_sync_state(&mut self, state: Option<SyncState>, cx: &mut Context<Self>) {
        let offline =
            matches!(&state, Some(SyncState::Running(s)) if s.phase == adoken_sync::Phase::Offline);
        let no_token = matches!(state, Some(SyncState::NoToken));
        let changed = state != self.sync_state;
        self.sync_state = state;
        self.offline = offline;
        self.no_token = no_token;
        if changed {
            self.refresh(cx);
        }
    }

    pub fn sync_state(&self) -> Option<&SyncState> {
        self.sync_state.as_ref()
    }

    // ── drawer state ────────────────────────────────────────────────────

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The tab showing in an open drawer.
    pub fn showing(&self, tab: Tab) -> bool {
        self.open && self.tab == tab
    }

    pub fn settings(&self) -> ActivitySettings {
        self.settings
    }

    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// The window and content column size, from the shell's layout.
    pub fn set_geometry(&mut self, window_height: f32, content_width: f32) {
        self.window_height = window_height;
        self.content_width = content_width;
    }

    pub fn window_height(&self) -> f32 {
        self.window_height
    }

    pub fn content_width(&self) -> f32 {
        self.content_width
    }

    /// The drawer's height in the current window (FR-1).
    pub fn height(&self) -> f32 {
        if self.maximized {
            codec::height_range(self.window_height).1
        } else {
            codec::effective_height(self.settings.panel_height, self.window_height)
        }
    }

    /// Open on `tab` (`pointer`: animate; keyboard opens are instant).
    pub fn open(&mut self, tab: Tab, pointer: bool, cx: &mut Context<Self>) {
        let was_open = self.open;
        self.open = true;
        if !was_open {
            self.opens += 1;
            self.opened_by_pointer = pointer && !cx.reduce_motion();
        }
        self.set_tab(tab, cx);
        self.refresh(cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.maximized = false;
        cx.emit(ActivityEvent::Closed);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.persist(cx);
            self.refresh(cx);
            cx.notify();
        }
    }

    pub fn toggle_maximized(&mut self, cx: &mut Context<Self>) {
        self.maximized = !self.maximized;
        cx.notify();
    }

    /// Resize (drag or splitter keys); persisted as `[activity]
    /// panel_height` (coalesced while dragging).
    pub fn set_height(&mut self, height: f32, cx: &mut Context<Self>) {
        let height = codec::clamp_height(height, self.window_height).round();
        self.maximized = false;
        if self.settings.panel_height != Some(height) {
            self.settings.panel_height = Some(height);
            self.persist(cx);
            cx.notify();
        }
    }

    /// `[activity] resource_meter` from the palette or the meter's menu
    /// (spec 0023 FR-10); the other keys are written unchanged.
    pub fn set_resource_meter(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.settings.resource_meter != on {
            self.settings.resource_meter = on;
            self.persist(cx);
            cx.notify();
        }
    }

    fn persist(&mut self, cx: &mut Context<Self>) {
        self.settings.tab = self.tab;
        crate::settings::write_table(cx, codec::TABLE, codec::table(&self.settings));
    }

    /// Apply `[activity]` as read from the file. A read that matches what
    /// we wrote changes nothing; an outside edit applies on the next frame.
    pub fn apply_settings(&mut self, read: ActivitySettings, cx: &mut Context<Self>) {
        if read == self.settings {
            return;
        }
        let indicator = read.network_indicator != self.settings.network_indicator;
        self.settings = read;
        if !self.open {
            self.tab = read.tab;
        }
        if indicator {
            self.refresh(cx);
        }
        cx.notify();
    }

    /// Read `[activity]` off the UI thread.
    pub fn load_settings(&mut self, cx: &mut Context<Self>) {
        let Some(path) = crate::settings::path(cx) else {
            return;
        };
        if crate::settings::is_writing(cx) {
            return;
        }
        let height = Some(self.window_height);
        let read = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            codec::from_toml(&text, height)
        });
        cx.spawn(async move |this, cx| {
            let settings = read.await;
            this.update(cx, |this, cx| this.apply_settings(settings, cx))
                .ok();
        })
        .detach();
    }

    // ── statusbar ───────────────────────────────────────────────────────

    pub fn indicator(&self) -> &IndicatorView {
        &self.view
    }

    pub fn summary(&self) -> Option<&NetSummary> {
        self.summary.as_ref()
    }

    pub fn announcement(&self) -> Option<&str> {
        self.announcement.as_deref()
    }

    /// The newest unseen failure the indicator counts (tooltip, spoken).
    pub fn latest_problem(&self) -> Option<adoken_sync::ledger::Entry> {
        let hub = self.hub.as_ref()?;
        let summary = self.summary.as_ref()?;
        let (seq, _, _) = summary
            .recent_problems
            .iter()
            .filter(|(seq, _, _)| *seq > self.seen_through)
            .max_by_key(|(seq, _, _)| *seq)?;
        hub.net.get(&[*seq]).into_iter().next()
    }

    pub fn tooltip(&self) -> String {
        match &self.summary {
            Some(summary) => super::indicator::tooltip(
                &self.view,
                summary,
                self.latest_problem().as_ref(),
                self.offset,
                self.now(),
            ),
            None => "No requests yet · Network log ⌘⇧A".into(),
        }
    }

    /// Opening the Network tab marks every current failure as seen; called
    /// after the tab painted so the chip clears on the next frame (§2.3).
    pub fn mark_seen(&mut self, cx: &mut Context<Self>) {
        let Some(hub) = &self.hub else { return };
        let last = hub.net.last_seq();
        if last > self.seen_through {
            self.seen_through = last;
            self.refresh(cx);
        }
    }

    // ── Sync tab ────────────────────────────────────────────────────────

    /// The `rows()` inputs: a fixture's, else the running sync's.
    pub fn sync_inputs(&self, cx: &gpui::App) -> Option<SyncInputs> {
        if let Some(inputs) = &self.inputs {
            return Some(inputs.clone());
        }
        let state = self.sync_state.as_ref()?;
        Some(crate::sync::sync_inputs(cx, state))
    }

    pub fn rows(&self, cx: &gpui::App) -> Option<(SyncInputs, [KindRow; 6])> {
        let inputs = self.sync_inputs(cx)?;
        let rows = adoken_sync::activity::rows(&inputs, self.now());
        Some((inputs, rows))
    }

    pub fn passes(&self) -> &[PassRecord] {
        &self.passes
    }

    pub fn lines(&self) -> &[PassLine] {
        &self.lines
    }

    pub fn pass(&self, id: u64) -> Option<&PassRecord> {
        self.passes.iter().find(|p| p.id == id)
    }

    pub fn is_expanded(&self, key: u64) -> bool {
        self.expanded.contains(&key)
    }

    pub fn toggle_expanded(&mut self, key: u64, expand: Option<bool>, cx: &mut Context<Self>) {
        let open = expand.unwrap_or(!self.expanded.contains(&key));
        if open {
            self.expanded.insert(key);
            self.read_titles(cx);
        } else {
            self.expanded.remove(&key);
        }
        cx.notify();
    }

    pub fn title(&self, kind: ItemKind, id: i64) -> Option<&Title> {
        self.titles.get(&(kind, id))
    }

    /// Read the current titles of expanded passes' items off the UI
    /// thread (FR-13: "with their current titles from the store").
    fn read_titles(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let wanted: Vec<(ItemKind, i64)> = self
            .passes
            .iter()
            .filter(|p| self.expanded.contains(&p.id))
            .flat_map(|p| {
                passes::items(&p.changes)
                    .into_iter()
                    .flat_map(|(kind, changes)| changes.ids.iter().map(move |(id, _)| (kind, *id)))
                    .collect::<Vec<_>>()
            })
            .filter(|key| !self.titles.contains_key(key))
            .collect();
        if wanted.is_empty() {
            return;
        }
        let read = cx.background_executor().spawn(async move {
            wanted
                .into_iter()
                .map(|(kind, id)| {
                    let title = store
                        .with_conn(|c| {
                            Ok(match kind {
                                ItemKind::Pr => adoken_store::read::pr(c, id)?.map(|p| p.title),
                                ItemKind::WorkItem => {
                                    adoken_store::read::work_item(c, id)?.map(|w| w.title)
                                }
                                ItemKind::Run => adoken_store::read::run(c, id)?
                                    .map(|r| format!("{} {}", r.pipeline_name, r.run_number)),
                            })
                        })
                        .ok()
                        .flatten();
                    ((kind, id), title)
                })
                .collect::<Vec<_>>()
        });
        self.title_reads = Some(cx.spawn(async move |this, cx| {
            let titles = read.await;
            this.update(cx, |this, cx| {
                this.titles.extend(titles);
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn emit(&mut self, event: ActivityEvent, cx: &mut Context<Self>) {
        cx.emit(event);
    }

    /// `⌘↩` on a changed item: its Azure DevOps page, built from the store
    /// off the UI thread (org URL, project and repository names).
    pub fn open_in_browser(&mut self, kind: ItemKind, id: i64, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        if self.demo {
            adoken_ui::ToastStack::show(
                adoken_ui::Toast::new(
                    "activity-browser",
                    adoken_ui::IconName::Info,
                    "Demo data has no Azure DevOps page.",
                ),
                cx,
            );
            return;
        }
        let url = cx.background_executor().spawn(async move {
            let org = store.org().ok()??.url;
            let projects = store.projects().ok()?;
            let project_name = |pid: &str| {
                projects
                    .iter()
                    .find(|p| p.id == pid)
                    .map_or_else(|| pid.to_owned(), |p| p.name.clone())
            };
            match kind {
                ItemKind::Pr => {
                    let pr = store.with_conn(|c| adoken_store::read::pr(c, id)).ok()??;
                    let repos = store.repos().ok()?;
                    let repo = repos
                        .iter()
                        .find(|r| r.id == pr.repo_id)
                        .map_or_else(|| pr.repo_id.clone(), |r| r.name.clone());
                    crate::review::web::web_url(&org, &project_name(&pr.project_id), &repo, id)
                }
                ItemKind::WorkItem => {
                    let item = store
                        .with_conn(|c| adoken_store::read::work_item(c, id))
                        .ok()??;
                    Some(crate::workitems::data::web_url(
                        &org,
                        &project_name(&item.project_id),
                        id,
                    ))
                }
                ItemKind::Run => {
                    let run = store.with_conn(|c| adoken_store::read::run(c, id)).ok()??;
                    let mut url = url::Url::parse(&org).ok()?;
                    url.path_segments_mut().ok()?.pop_if_empty().extend([
                        project_name(&run.project_id).as_str(),
                        "_build",
                        "results",
                    ]);
                    url.query_pairs_mut()
                        .append_pair("buildId", &id.to_string());
                    Some(url.into())
                }
            }
        });
        cx.spawn(async move |_, cx| {
            if let Some(url) = url.await {
                cx.update(|cx| cx.open_url(&url));
            }
        })
        .detach();
    }

    // ── updates ─────────────────────────────────────────────────────────

    /// Sample everything the window shows; notify only on visible change.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let mut changed = false;
        let mut wake: Option<Millis> = None;
        let mut wake_at = |at: Option<Millis>| {
            if let Some(at) = at {
                wake = Some(wake.map_or(at, |w: Millis| w.min(at)));
            }
        };
        if let Some(hub) = self.hub.clone() {
            if self.showing(Tab::Network) {
                // Seen while the tab shows; the first paint shows them.
                let ingest = self.net.update(&hub.net);
                changed |= ingest != Default::default() || self.net_has_in_flight();
            }
            let summary = hub.net.summary(now);
            let vis = visibility(
                self.demo,
                self.no_token,
                self.settings.network_indicator,
                self.offline,
            );
            let sample = self.indicator.sample(&summary, self.seen_through, vis, now);
            wake_at(sample.wake_at);
            if sample.view != self.view {
                self.view = sample.view;
                changed = true;
            }
            let (danger, _) = summary.unseen(self.seen_through);
            let (said, later) = self.net_announcer.observe(
                danger,
                || {
                    summary
                        .recent_problems
                        .iter()
                        .filter(|(_, _, class)| *class == StatusClass::Danger)
                        .max_by_key(|(seq, _, _)| *seq)
                        .and_then(|(seq, _, _)| hub.net.get(&[*seq]).into_iter().next())
                        .map(|e| {
                            format!(
                                "{}, {}",
                                super::net::status_text(&e.status),
                                e.purpose.to_lowercase()
                            )
                        })
                        .unwrap_or_default()
                },
                now,
            );
            wake_at(later);
            if let Some(text) = said {
                self.announcement = Some(text);
                changed = true;
            }
            self.summary = Some(summary);
            let generation = hub.passes.generation();
            if self.pass_generation != Some(generation) && self.showing(Tab::Sync) {
                self.pass_generation = Some(generation);
                self.passes = hub.passes.passes();
                self.lines = passes::fold(&self.passes);
                if !self.expanded_once {
                    self.expanded_once = true;
                    if let Some(first) = self
                        .passes
                        .iter()
                        .find(|p| !passes::changes_empty(&p.changes) && p.ended.is_some())
                    {
                        self.expanded.insert(first.id);
                    }
                }
                self.read_titles(cx);
                changed = true;
            }
        }
        let mood = self.mood(cx);
        let (said, later) = self.sync_announcer.observe(&mood, now);
        wake_at(later);
        if let Some(text) = said {
            self.announcement = Some(text);
            changed = true;
        }
        self.schedule(wake, cx);
        if changed {
            cx.notify();
        }
    }

    fn net_has_in_flight(&self) -> bool {
        self.net
            .held()
            .rev()
            .take(64)
            .any(|e| matches!(e.status, adoken_sync::ledger::Status::InFlight))
    }

    /// The coarse sync state announcements follow (FR-4).
    fn mood(&self, cx: &gpui::App) -> SyncMood {
        if self.demo {
            return SyncMood::Normal;
        }
        match &self.sync_state {
            Some(SyncState::Running(s)) => match s.phase {
                adoken_sync::Phase::Offline => return SyncMood::Offline,
                adoken_sync::Phase::Throttled { .. } => return SyncMood::Throttled,
                _ => {}
            },
            Some(_) => return SyncMood::Normal,
            None if self.inputs.is_none() => return SyncMood::Normal,
            None => {}
        }
        let Some((_, rows)) = self.rows(cx) else {
            return SyncMood::Normal;
        };
        let failing: Vec<&str> = rows
            .iter()
            .filter(|r| matches!(r.state, RowState::Failed { .. }))
            .map(|r| passes::kind_title(r.kind))
            .collect();
        if failing.is_empty() {
            SyncMood::Normal
        } else {
            SyncMood::Failed(failing.join(", "))
        }
    }

    /// One timer for the earliest pending wake; none while nothing is due.
    fn schedule(&mut self, wake: Option<Millis>, cx: &mut Context<Self>) {
        let Some(at) = wake else {
            self.wake = None;
            return;
        };
        if self.wake.as_ref().is_some_and(|(due, _)| *due == at) {
            return;
        }
        let delay = Duration::from_millis((at - self.now()).max(0) as u64);
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                this.wake = None;
                this.refresh(cx);
            })
            .ok();
        });
        self.wake = Some((at, task));
    }
}

/// The local UTC offset at `at` (`localtime_r`, safe with many threads).
pub fn local_offset(at: Millis) -> time::UtcOffset {
    use adoken_notify::Clock as _;
    time::UtcOffset::from_whole_seconds(adoken_notify::SystemClock.offset_at(at))
        .unwrap_or(time::UtcOffset::UTC)
}
