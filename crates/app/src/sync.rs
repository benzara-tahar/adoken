//! Connected mode (spec 0002): the sync scheduler for the signed-in org runs
//! on the Tokio runtime, never on the UI thread, and writes the same store
//! the views read (they refresh on its commits, as in demo mode). The shell
//! shows the scheduler's state in the statusbar ([`label`]).
//!
//! macOS reachability and sleep/wake ([`crate::netwatch`]) feed the
//! scheduler (FR-5): offline shows at once and pauses requests; waking or
//! reconnecting runs a hot pass right away.
//!
//! The session's [`ActivityHub`] (spec 0016) exists in every mode: the
//! network log merges the three transports (main, work-item writes,
//! avatars) and the tracker records sync passes and lane state; the shell
//! reads them through ≤ 4 Hz [`activity_changes`] (FR-6).
//!
//! `ADOKEN_SYNC_LOG=<file>` appends one JSON line per completed request,
//! the same redacted record as Network log › Export (spec 0016 FR-25/26):
//! time, method, host, templated route (`{org}`, `{project}`, `{repo}`,
//! `{name}`, `{id}`, `{sha}`, `{n}`), purpose, origin, attempt, timing,
//! status, size, rate headers and `x-vss-activityid`. No query strings,
//! bodies, other headers, item ids, names or Azure DevOps messages, so it's
//! safe to share as proof of what adoken sends.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use adoken_ado::http::Http;
use adoken_ado::{AdoClient, Connector, Organization};
use adoken_model::Millis;
use adoken_model::time::{DAY, HOUR, MINUTE, SECOND};
use adoken_store::Store;
use adoken_sync::activity::{Coalescer, PUBLISH_INTERVAL_MS, SyncInputs, Tracker};
use adoken_sync::avatars::{AvatarCache, AvatarFetcher};
use adoken_sync::cadence::{Activity, Pause, Prefs};
use adoken_sync::ledger::NetLog;
use adoken_sync::transport::OptionalAdoTransport;
use adoken_sync::{
    AdoTransport, Control, Engine, Phase, PrFiles, Recording, Scheduler, SyncConfig, SyncStatus,
    SystemClock, Writer,
};
use adoken_theme::ColorTokens;
use gpui::{App, Global, Hsla, KeyBinding, Task, actions};
use gpui_tokio::Tokio;
use tokio::sync::{mpsc, watch};

use crate::palette::PaletteCommand;

actions!(
    sync,
    [
        /// Sync now.
        SyncNow,
        /// Fetch my pipeline runs older than the synced window.
        LoadOlderRuns,
        /// Pause scheduled sync until resumed, or resume it.
        PauseOrResumeSync,
        /// Pause scheduled sync for one hour.
        PauseSyncForHour,
        /// Open Settings on the Sync tab.
        OpenSyncSettings,
    ]
);

/// ⌘R (design/command-palette.md §keys).
pub const SYNC_NOW_KEYS: &str = "secondary-r";
const LOG_ENV: &str = "ADOKEN_SYNC_LOG";
/// "Foreground" = window key or active in the last 5 minutes (FR-4).
pub const BACKGROUND_AFTER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

pub fn bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new(SYNC_NOW_KEYS, SyncNow, None)]
}

/// Connected-mode commands: sync now (hot + warm; presses within 10 s are
/// coalesced, spec 0002 §3), load older pipeline runs (runs older than the
/// 7-day window, 50 per project per press, with their stages), and
/// Settings › Sync's pause controls (spec 0002 FR-4a).
pub fn commands() -> Vec<PaletteCommand> {
    let cmd = |title, keys, action: Box<dyn gpui::Action>| PaletteCommand {
        title,
        keys,
        action,
    };
    vec![
        cmd("Sync now", Some(SYNC_NOW_KEYS), Box::new(SyncNow)),
        cmd("Pipelines: load older runs", None, Box::new(LoadOlderRuns)),
        cmd("Sync: Pause or resume", None, Box::new(PauseOrResumeSync)),
        cmd("Sync: Pause for 1 hour", None, Box::new(PauseSyncForHour)),
        cmd("Sync: Settings…", None, Box::new(OpenSyncSettings)),
    ]
}

/// Settings › Sync as last read or edited, and the running scheduler's
/// control and activity (connected mode only).
#[derive(Default)]
struct Link {
    prefs: Prefs,
    /// `[sync]` has been read at least once.
    loaded: bool,
    control: Option<mpsc::UnboundedSender<Control>>,
    activity: Option<watch::Receiver<Activity>>,
}

impl Global for Link {}

/// Register the pause / resume / settings actions (palette, spec 0002
/// FR-4a; they edit `[sync]` like the Settings tab does) and the session's
/// [`ActivityHub`].
pub fn init(cx: &mut App) {
    cx.set_global(Link::default());
    install_activity_hub(ActivityHub::new(), cx);
    cx.on_action(|_: &PauseOrResumeSync, cx| {
        let now = time::OffsetDateTime::now_utc().unix_timestamp() * 1000;
        let pause = if prefs(cx).pause.active(now) {
            Pause::Off
        } else {
            Pause::UntilResumed
        };
        set_pause(pause, cx);
    });
    cx.on_action(|_: &PauseSyncForHour, cx| {
        let until = time::OffsetDateTime::now_utc().unix_timestamp() * 1000
            + crate::settings::sync_prefs::PAUSE_FOR;
        set_pause(Pause::Until(until), cx);
    });
    cx.on_action(|_: &OpenSyncSettings, cx| crate::settings::open(crate::settings::Tab::Sync, cx));
}

/// Settings › Sync as last read or edited.
pub fn prefs(cx: &App) -> Prefs {
    cx.try_global::<Link>()
        .map_or_else(Prefs::default, |l| l.prefs)
}

/// Apply Settings › Sync to the running scheduler now (no restart).
pub fn set_prefs(prefs: Prefs, cx: &mut App) {
    if !cx.has_global::<Link>() {
        return;
    }
    let link = cx.global_mut::<Link>();
    link.loaded = true;
    if link.prefs == prefs {
        return;
    }
    link.prefs = prefs;
    if let Some(control) = &link.control {
        control.send(Control::Prefs(prefs)).ok();
    }
}

/// Pause or resume (palette, Activity's sync tab): persisted in `[sync]`,
/// applied live.
pub(crate) fn set_pause(pause: Pause, cx: &mut App) {
    let prefs = Prefs { pause, ..prefs(cx) };
    set_prefs(prefs, cx);
    crate::settings::write_table(
        cx,
        crate::settings::sync_prefs::SYNC_TABLE,
        crate::settings::sync_prefs::table(&prefs),
    );
}

/// The running scheduler's last passes and measured costs; `None` until
/// sync runs (demo, signed out).
pub fn activity(cx: &App) -> Option<watch::Receiver<Activity>> {
    cx.try_global::<Link>().and_then(|l| l.activity.clone())
}

/// The session's activity sources (spec 0016): one network log for every
/// transport, one tracker for sync passes and lane state. Exists in every
/// mode; demo mode records its feed ticks into `passes`.
#[derive(Clone)]
pub struct ActivityHub {
    pub net: Arc<NetLog>,
    pub passes: Arc<Tracker>,
}

impl ActivityHub {
    pub fn new() -> Self {
        let clock = Arc::new(SystemClock);
        ActivityHub {
            net: Arc::new(NetLog::new(clock.clone())),
            passes: Arc::new(Tracker::new(clock)),
        }
    }

    /// Moves whenever the log or the tracker changed.
    pub fn generation(&self) -> u64 {
        self.net.generation().wrapping_add(self.passes.generation())
    }
}

impl Default for ActivityHub {
    fn default() -> Self {
        Self::new()
    }
}

/// The installed hub and its throttled change counter.
struct Hub {
    hub: ActivityHub,
    changes: watch::Receiver<u64>,
    _publisher: Task<()>,
}

impl Global for Hub {}

/// The session's hub (`None` before [`init`]).
pub fn activity_hub(cx: &App) -> Option<ActivityHub> {
    cx.try_global::<Hub>().map(|h| h.hub.clone())
}

/// Bumps at most 4 times a second, on the trailing edge, while the hub
/// changes; never while nothing changes (FR-6).
pub fn activity_changes(cx: &App) -> Option<watch::Receiver<u64>> {
    cx.try_global::<Hub>().map(|h| h.changes.clone())
}

/// Use `hub` from now on (gallery and smoke fixtures; [`init`] installs
/// the session's).
pub fn install_activity_hub(hub: ActivityHub, cx: &mut App) {
    let (tx, changes) = watch::channel(hub.generation());
    let executor = cx.background_executor().clone();
    let publisher = executor.spawn(publish(hub.clone(), tx, executor.clone()));
    cx.set_global(Hub {
        hub,
        changes,
        _publisher: publisher,
    });
}

/// FR-6 publisher: wakes on a change, publishes on the trailing edge of a
/// 250 ms window, and does nothing while nothing changes.
async fn publish(hub: ActivityHub, tx: watch::Sender<u64>, executor: gpui::BackgroundExecutor) {
    let start = std::time::Instant::now();
    let now = || start.elapsed().as_millis() as Millis;
    let mut coalescer = Coalescer::new(PUBLISH_INTERVAL_MS);
    let mut published = hub.generation();
    loop {
        if hub.generation() == published {
            let net = hub.net.signal().clone();
            let passes = hub.passes.signal().clone();
            futures::future::select(Box::pin(net.changed()), Box::pin(passes.changed())).await;
            continue;
        }
        let at = coalescer.change(now());
        let wait = at - now();
        if wait > 0 {
            executor
                .timer(std::time::Duration::from_millis(wait as u64))
                .await;
        }
        if coalescer.tick(now()) {
            published = hub.generation();
            if tx.send(published).is_err() {
                return;
            }
        }
    }
}

/// [`adoken_sync::activity::rows`]' inputs now: the tracker's lanes and
/// governor, with the statusbar's view of the token and reachability.
pub fn sync_inputs(cx: &App, state: &SyncState) -> SyncInputs {
    let mut inputs = activity_hub(cx).map_or_else(
        || SyncInputs {
            online: true,
            foreground: true,
            prefs: prefs(cx),
            ..SyncInputs::default()
        },
        |hub| hub.passes.inputs(),
    );
    if let SyncState::Running(status) = state {
        match status.phase {
            Phase::Unauthorized => inputs.gov.unauthorized = true,
            Phase::Offline => inputs.online = false,
            _ => {}
        }
    }
    inputs
}

/// Sync now from anywhere (Settings › Sync, ⌘R in the Settings window).
pub fn sync_now(cx: &App) {
    if let Some(control) = cx.try_global::<Link>().and_then(|l| l.control.as_ref()) {
        control.send(Control::RefreshNow).ok();
    }
}

/// A replacement token verified (spec 0001 FR-15, AC-13): the running
/// scheduler sends requests again and runs a hot pass now.
pub fn token_replaced(cx: &App) {
    if let Some(control) = cx.try_global::<Link>().and_then(|l| l.control.as_ref()) {
        control.send(Control::TokenReplaced).ok();
    }
}

/// The signed-in org the shell syncs.
#[derive(Clone, Debug)]
pub struct ConnectedOrg {
    pub data_dir: PathBuf,
    pub org: Organization,
    /// Selected project ids (onboarding / Settings → Projects).
    pub projects: Vec<String>,
    /// Already read from the Keychain at launch; `None` reads it on start.
    pub client: Option<AdoClient>,
    pub connector: Connector,
    /// Scopes the token was granted (`account.json`); avatars need
    /// `vso.identity` (spec 0012 FR-9).
    pub scopes: Vec<String>,
    /// Review access (0001 `AccessLevel::Review`): vote, comment, resolve,
    /// complete. Read-only tokens get no review controls (0004 AC-11).
    pub can_write: bool,
}

impl ConnectedOrg {
    pub fn store_path(&self) -> PathBuf {
        adoken_ado::org_store_path(&self.data_dir, &self.org)
    }
}

/// Blocking; call before opening the org's store. Creates `<data_dir>/orgs/`
/// `0700` (ADR 0001: the org directories hold synced data; the store makes
/// its own directory `0700`), tightening an existing one. Before v0.0.3 the
/// connected store lived at `<data_dir>/adoken.sqlite`: move it (with its
/// WAL files) to the org's directory once, so local state survives.
pub fn prepare_org_store(data_dir: &Path, path: &Path) -> std::io::Result<()> {
    let orgs = data_dir.join("orgs");
    std::fs::create_dir_all(&orgs)?;
    restrict_dir(&orgs)?;
    let legacy = data_dir.join("adoken.sqlite");
    if !legacy.exists() || path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        restrict_dir(dir)?;
    }
    for suffix in ["-wal", "-shm", ""] {
        let from = PathBuf::from(format!("{}{suffix}", legacy.display()));
        if from.exists() {
            std::fs::rename(&from, PathBuf::from(format!("{}{suffix}", path.display())))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_dir(_dir: &Path) -> std::io::Result<()> {
    Ok(())
}

/// What the statusbar knows about sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// Reading the token, building the engine.
    Starting,
    /// Nothing selected in Settings → Projects.
    NoProjects,
    /// The Keychain has no token for the org, or it couldn't be read.
    NoToken,
    Running(SyncStatus),
}

impl SyncState {
    /// A pass just finished (the store has new data).
    pub fn is_idle(&self) -> bool {
        matches!(self, SyncState::Running(s) if s.phase == Phase::Idle)
    }
}

/// Owns the scheduler task and the reachability / sleep observers;
/// dropping it stops sync.
pub struct SyncHandle {
    control: mpsc::UnboundedSender<Control>,
    state: watch::Receiver<SyncState>,
    avatars: watch::Receiver<Option<Arc<AvatarFetcher>>>,
    writer: watch::Receiver<Option<Writer>>,
    wi_writer: watch::Receiver<Option<adoken_sync::WiWriter>>,
    /// The token may link work items (spec 0004 FR-17): its scopes include
    /// `vso.work_write`, or they're unknown.
    link_work_items: bool,
    /// Review access (0001 `AccessLevel::Review`).
    can_write: bool,
    files: watch::Receiver<Option<Arc<PrFiles>>>,
    _net: crate::netwatch::NetWatch,
    _task: Task<Result<(), gpui_tokio::JoinError>>,
}

impl SyncHandle {
    pub fn watch(&self) -> watch::Receiver<SyncState> {
        self.state.clone()
    }

    /// The avatar fetcher, once the token is read (spec 0012).
    pub fn avatars(&self) -> watch::Receiver<Option<Arc<AvatarFetcher>>> {
        self.avatars.clone()
    }

    /// Review writes through this org's governor and ledger (spec 0004).
    pub fn write_access(&self) -> crate::review::writes::Remote {
        crate::review::writes::Remote {
            writer: self.writer.clone(),
            control: self.control.clone(),
            link_work_items: self.link_work_items,
        }
    }

    /// Work item edits through this org's governor and ledger (spec 0015):
    /// editable with Review access and `vso.work_write`; per-item sync
    /// works either way.
    pub fn work_item_access(&self) -> crate::workitems::writes::Access {
        crate::workitems::writes::Access::Remote {
            writer: self.wi_writer.clone(),
            can_write: self.can_write && self.link_work_items,
        }
    }

    /// A replacement changed what the token may do (spec 0001 FR-16: a
    /// feature gated on a newly granted scope turns on without a restart).
    /// The caller reinstalls the access globals from the accessors above.
    pub fn set_grants(&mut self, grants: crate::account::Grants, scopes: Vec<String>, cx: &App) {
        self.can_write = grants.can_write;
        self.link_work_items = grants.link_work_items;
        if let Some(hub) = activity_hub(cx) {
            hub.passes.set_scopes(scopes);
        }
    }

    /// Review file lists and blobs through this org's governor (spec 0004
    /// FR-1..FR-3), once the token is read.
    pub fn pr_files(&self) -> watch::Receiver<Option<Arc<PrFiles>>> {
        self.files.clone()
    }

    pub fn sync_now(&self) {
        self.control.send(Control::RefreshNow).ok();
    }

    /// My runs older than the synced window, with their stages.
    pub fn load_older_runs(&self) {
        self.control.send(Control::LoadOlderRuns).ok();
    }

    pub fn set_foreground(&self, foreground: bool) {
        self.control.send(Control::Foreground(foreground)).ok();
    }
}

/// Start syncing `org` into `store` on the Tokio runtime. Main thread (the
/// sleep/wake observers register with `NSWorkspace`).
pub fn start(store: Store, org: ConnectedOrg, cx: &mut App) -> SyncHandle {
    let (control, control_rx) = mpsc::unbounded_channel();
    let (state_tx, state) = watch::channel(SyncState::Starting);
    let (avatars_tx, avatars) = watch::channel(None);
    let (writer_tx, writer) = watch::channel(None);
    let (wi_writer_tx, wi_writer) = watch::channel(None);
    let (files_tx, files) = watch::channel(None);
    let (activity_tx, activity) = watch::channel(Activity {
        foreground: true,
        ..Activity::default()
    });
    let net = {
        let control = control.clone();
        crate::netwatch::watch(move |c| {
            control.send(c).ok();
        })
    };
    // Settings › Sync: as already read, else read on the Tokio side before
    // the first pass (a paused sync must not send anything at launch).
    let (prefs, settings) = match cx.try_global::<Link>() {
        Some(link) if link.loaded => (Some(link.prefs), None),
        _ => (None, crate::settings::path(cx)),
    };
    if cx.has_global::<Link>() {
        let link = cx.global_mut::<Link>();
        link.control = Some(control.clone());
        link.activity = Some(activity);
    }
    let outputs = Outputs {
        state: state_tx,
        avatars: avatars_tx,
        writer: writer_tx,
        wi_writer: wi_writer_tx,
        files: files_tx,
        activity: activity_tx,
    };
    let link_work_items = org.scopes.is_empty()
        || org
            .scopes
            .iter()
            .any(|s| s == adoken_sync::writes::WORK_WRITE_SCOPE);
    let can_write = org.can_write;
    let hub = activity_hub(cx).unwrap_or_default();
    let task = Tokio::spawn(
        cx,
        run(store, org, control_rx, prefs, settings, outputs, hub),
    );
    // Spec 0001 FR-15: a 401 on the main transport is the one quiet
    // rejected state.
    crate::account::watch_sync(state.clone(), cx);
    SyncHandle {
        control,
        state,
        avatars,
        writer,
        wi_writer,
        link_work_items,
        can_write,
        files,
        _net: net,
        _task: task,
    }
}

/// What the sync task reports back to the app.
struct Outputs {
    state: watch::Sender<SyncState>,
    avatars: watch::Sender<Option<Arc<AvatarFetcher>>>,
    writer: watch::Sender<Option<Writer>>,
    /// Spec 0015 work item edits, per-item sync and metadata.
    wi_writer: watch::Sender<Option<adoken_sync::WiWriter>>,
    files: watch::Sender<Option<Arc<PrFiles>>>,
    activity: watch::Sender<Activity>,
}

/// `[sync]` from `settings.toml` (missing or broken file: defaults).
async fn read_prefs(path: Option<PathBuf>) -> Prefs {
    let Some(path) = path else {
        return Prefs::default();
    };
    let text = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    toml::from_str::<toml::Table>(&text)
        .map(|doc| crate::settings::sync_prefs::parse(&doc).0)
        .unwrap_or_default()
}

async fn run(
    store: Store,
    org: ConnectedOrg,
    control: mpsc::UnboundedReceiver<Control>,
    prefs: Option<Prefs>,
    settings: Option<PathBuf>,
    outputs: Outputs,
    hub: ActivityHub,
) {
    let Outputs {
        state,
        avatars,
        writer,
        wi_writer,
        files,
        activity,
    } = outputs;
    if org.projects.is_empty() {
        state.send_replace(SyncState::NoProjects);
        return;
    }
    // The org's own directory: the avatar cache lives beside its store.
    let org_dir = org
        .store_path()
        .parent()
        .map_or_else(|| org.data_dir.clone(), Path::to_path_buf);
    let client = match org.client {
        Some(client) => client,
        None => match org.connector.client().await {
            Ok(Some(client)) => client,
            Ok(None) => {
                state.send_replace(SyncState::NoToken);
                return;
            }
            Err(error) => {
                eprintln!("adoken: sync can't read the token: {error}");
                state.send_replace(SyncState::NoToken);
                return;
            }
        },
    };
    let http = Http::new();
    // One session log for all three transports (spec 0016 FR-16).
    log_to_file(&hub.net);
    hub.passes.set_scopes(org.scopes.clone());
    let transport = Recording::new(
        AdoTransport {
            http: http.clone(),
            auth: client.auth().clone(),
        },
        hub.net.clone(),
    );
    let org_url = client.organization().base_url();
    let mut cfg = SyncConfig::new(org_url.clone());
    cfg.projects = Some(org.projects);
    let engine = Engine::new(store, Arc::new(transport), Arc::new(SystemClock), cfg)
        .with_tracker(hub.passes.clone());
    // Work item calls (spec 0004 FR-17, spec 0015): Azure DevOps answers 401
    // when the PAT lacks `vso.work_write`; that must not reject the token.
    let work_items = Arc::new(Recording::new(
        OptionalAdoTransport {
            http: http.clone(),
            auth: client.auth().clone(),
        },
        hub.net.clone(),
    ));
    let review_writer = engine.writer().with_work_item_transport(work_items);
    // Spec 0015: per-item sync needs only `vso.work`, so the work item
    // writer exists for read-only tokens too (edits are gated by access).
    wi_writer.send_replace(Some(review_writer.work_items()));
    // Gated by the review access global (Read-only tokens get none), so a
    // replacement that grants Review needs no new writer (0001 FR-16).
    writer.send_replace(Some(review_writer));
    files.send_replace(Some(engine.pr_files()));
    // Avatars share the governor and the request log, but a refusal from
    // their endpoint never rejects the token (0012 FR-9).
    let avatar_transport = Recording::new(
        OptionalAdoTransport {
            http,
            auth: client.auth().clone(),
        },
        hub.net.clone(),
    );
    let fetcher = AvatarFetcher::new(
        AvatarCache::new(&org_dir),
        Arc::new(avatar_transport),
        engine.shared_governor(),
        engine.clock().clone(),
        org_url,
        &org.scopes,
    )
    .with_sync_status(engine.status());
    avatars.send_replace(Some(Arc::new(fetcher)));
    let mut status = engine.status();
    let forward = async move {
        loop {
            let s = status.borrow_and_update().clone();
            state.send_replace(SyncState::Running(s));
            if status.changed().await.is_err() {
                return;
            }
        }
    };
    let prefs = match prefs {
        Some(prefs) => prefs,
        None => read_prefs(settings).await,
    };
    let scheduler = Scheduler::new(engine).with_prefs(prefs);
    let mut passes = scheduler.activity();
    let report = async move {
        loop {
            activity.send_replace(passes.borrow_and_update().clone());
            if passes.changed().await.is_err() {
                return;
            }
        }
    };
    tokio::join!(scheduler.run(control), forward, report);
}

/// `ADOKEN_SYNC_LOG`: also append every completed request to that file as
/// an export JSON line (spec 0016 FR-26).
fn log_to_file(log: &NetLog) {
    let Some(path) = std::env::var_os(LOG_ENV) else {
        return;
    };
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => log.set_sink(file),
        Err(error) => eprintln!("adoken: {LOG_ENV}: {error}"),
    }
}

/// Statusbar tone; the shell maps it to theme tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Busy,
    Warning,
    Error,
    Quiet,
}

/// Statusbar glyph and theme color for `tone`.
pub fn indicator(tone: Tone, colors: &ColorTokens<Hsla>) -> (&'static str, Hsla) {
    match tone {
        Tone::Ok => ("●", colors.status_success_fg),
        Tone::Busy => ("◔", colors.status_info_fg),
        Tone::Warning => ("▲", colors.status_warning_fg),
        Tone::Error => ("▲", colors.status_danger_fg),
        Tone::Quiet => ("○", colors.text_tertiary),
    }
}

/// `12s ago`, `3 min ago`, `2 h ago`, `4 d ago`.
fn ago(ms: Millis) -> String {
    let ms = ms.max(0);
    match ms {
        d if d < MINUTE => format!("{}s ago", d / SECOND),
        d if d < HOUR => format!("{} min ago", d / MINUTE),
        d if d < DAY => format!("{} h ago", d / HOUR),
        d => format!("{} d ago", d / DAY),
    }
}

/// Status pill text (spec 0002 §3): `Synced 12s ago` · `Syncing…` ·
/// `Offline — last synced 3 min ago` · `Throttled by Azure DevOps — resuming
/// in 40s` · `Token rejected` · `No projects`. `last_sync` is the store's
/// newest successful cursor (survives restarts).
pub fn label(state: &SyncState, last_sync: Option<Millis>, now: Millis) -> (Tone, String) {
    let status = match state {
        SyncState::Starting => return (Tone::Busy, "Connecting…".into()),
        SyncState::NoProjects => return (Tone::Quiet, "No projects".into()),
        SyncState::NoToken => return (Tone::Error, "No token in Keychain".into()),
        SyncState::Running(status) => status,
    };
    let last = [
        last_sync,
        status.last_ok_hot,
        status.last_ok_warm,
        status.last_ok_cold,
    ]
    .into_iter()
    .flatten()
    .max();
    match &status.phase {
        Phase::Unauthorized => (Tone::Error, "Token rejected".into()),
        Phase::Throttled { until, .. } => {
            let wait = (until - now).max(0);
            let wait = if wait < MINUTE {
                format!("{}s", (wait + SECOND - 1) / SECOND)
            } else {
                format!("{} min", (wait + MINUTE - 1) / MINUTE)
            };
            (
                Tone::Warning,
                format!("Throttled by Azure DevOps — resuming in {wait}"),
            )
        }
        Phase::Offline => (
            Tone::Warning,
            match last {
                Some(at) => format!("Offline — last synced {}", ago(now - at)),
                None => "Offline — not synced yet".into(),
            },
        ),
        Phase::Syncing(_) if last.is_none() => (Tone::Busy, "First sync…".into()),
        Phase::Syncing(_) => (Tone::Busy, "Syncing…".into()),
        Phase::Idle => match last {
            Some(at) => (Tone::Ok, format!("Synced {}", ago(now - at))),
            None => (Tone::Quiet, "Not synced yet".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use adoken_sync::Tier;

    use super::*;

    const NOW: Millis = 1_790_000_000_000;

    fn running(phase: Phase, last_ok_warm: Option<Millis>) -> SyncState {
        SyncState::Running(SyncStatus {
            phase,
            last_ok_hot: None,
            last_ok_warm,
            last_ok_cold: None,
        })
    }

    #[test]
    fn labels_follow_the_spec_states() {
        let l = |s: &SyncState, last| label(s, last, NOW);
        assert_eq!(
            l(&running(Phase::Idle, Some(NOW - 12 * SECOND)), None),
            (Tone::Ok, "Synced 12s ago".into())
        );
        assert_eq!(
            l(&running(Phase::Offline, None), Some(NOW - 3 * MINUTE)),
            (Tone::Warning, "Offline — last synced 3 min ago".into())
        );
        assert_eq!(
            l(
                &running(
                    Phase::Throttled {
                        until: NOW + 39_500,
                        resource: Some("Core".into())
                    },
                    None
                ),
                None
            ),
            (
                Tone::Warning,
                "Throttled by Azure DevOps — resuming in 40s".into()
            )
        );
        assert_eq!(
            l(&running(Phase::Unauthorized, Some(NOW)), None).1,
            "Token rejected"
        );
        assert_eq!(l(&SyncState::NoProjects, None).1, "No projects");
    }

    #[test]
    fn syncing_says_first_sync_until_anything_landed() {
        assert_eq!(
            label(&running(Phase::Syncing(Tier::Warm), None), None, NOW).1,
            "First sync…"
        );
        assert_eq!(
            label(&running(Phase::Syncing(Tier::Hot), None), Some(NOW), NOW).1,
            "Syncing…"
        );
    }

    #[test]
    fn newest_of_store_and_pass_times_wins() {
        let state = running(Phase::Idle, Some(NOW - 2 * HOUR));
        assert_eq!(
            label(&state, Some(NOW - 30 * SECOND), NOW).1,
            "Synced 30s ago"
        );
        assert_eq!(label(&state, None, NOW).1, "Synced 2 h ago");
    }

    #[test]
    fn legacy_root_store_moves_into_the_org_directory_once() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        std::fs::write(data.join("adoken.sqlite"), b"db").unwrap();
        std::fs::write(data.join("adoken.sqlite-wal"), b"wal").unwrap();
        let target = data.join("orgs/contoso/adoken.sqlite");

        prepare_org_store(data, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"db");
        assert_eq!(
            std::fs::read(data.join("orgs/contoso/adoken.sqlite-wal")).unwrap(),
            b"wal"
        );
        assert!(!data.join("adoken.sqlite").exists());

        // An existing org store is never overwritten.
        std::fs::write(data.join("adoken.sqlite"), b"stale").unwrap();
        prepare_org_store(data, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"db");
    }

    #[cfg(unix)]
    #[test]
    fn orgs_directory_is_private_even_when_it_already_existed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let orgs = dir.path().join("orgs");
        std::fs::create_dir_all(&orgs).unwrap();
        std::fs::set_permissions(&orgs, std::fs::Permissions::from_mode(0o755)).unwrap();

        prepare_org_store(dir.path(), &orgs.join("contoso/adoken.sqlite")).unwrap();
        let mode = std::fs::metadata(&orgs).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
    }
}
