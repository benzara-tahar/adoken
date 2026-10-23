//! Work item edits in the app (spec 0015 slices A and B): who may edit
//! ([`Access`], [`gate`]), the per-item serial write queue (FR-10) with the
//! pending / failed / conflict / rule states the detail renders (FR-6..FR-9,
//! FR-28), comments (FR-25), drafts (FR-20) and recent assignees (FR-13),
//! the freshness label (FR-29), Sync this item and the partly-synced
//! auto-fetch (FR-30, FR-31), metadata on first use (FR-33) and the
//! directory search (FR-13; the view debounces it).
//!
//! The state machines ([`Edits`], [`ItemSyncs`], [`freshness_with`]) are
//! GPUI-free and unit-tested; [`WiEdits`] is the GPUI entity that runs
//! them, sending through the org's [`WiWriter`] on Tokio (or the demo
//! store's `apply_local`), never on the UI thread.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use adoken_model::time::{DAY, HOUR, MINUTE, SECOND};
use adoken_model::{
    ClassNode, ClassTree, Millis, NodeKind, Person, PinSubject, StateCategory, TypeFamily,
    TypeMeta, WiState, WorkItem, WorkItemExtras, WorkItemType,
};
use adoken_store::Store;
use adoken_store::wi::{self as store_wi, DraftKind, WiComment, WiDraft};
use adoken_sync::attribution::UserAction;
pub use adoken_sync::wi::{
    Conflict, READ_ONLY_HINT, RuleViolation, WiChange, WiError, WiField as Field, WiOutcome,
    WiWrite,
};
use adoken_sync::wi::{WiWriter, apply_local, comment_local, is_stale, link_guard};
use gpui::{App, AppContext, Context, Entity, Global, Task};
use gpui_tokio::Tokio;
use serde_json::Value;
use tokio::sync::watch;

/// After this long pending, the line reads "Waiting for Azure DevOps…"
/// (spec §States, design §9.1).
pub const WAITING_AFTER_MS: Millis = 3 * SECOND;
/// A second Sync this item within this of a successful one sends nothing
/// (FR-30).
pub const JUST_SYNCED_MS: Millis = 10 * SECOND;
/// "Just synced" shows this long (design §3.4).
pub const JUST_SYNCED_LABEL_MS: Millis = 2 * SECOND;
/// FR-31: a partly synced item fetches itself at most this often.
pub const AUTO_FETCH_EVERY_MS: Millis = 10 * MINUTE;
/// FR-29: stale from this age.
pub const STALE_AFTER_MS: Millis = 15 * MINUTE;
/// FR-20: drafts save this long after the last keystroke.
pub const DRAFT_DELAY_MS: Millis = 300;
/// The demo org's fake round trip (spec §States: ≤ 300 ms).
pub const DEMO_DELAY_MS: Millis = 250;

/// Offline tooltip on disabled controls (principles §3.5).
pub const OFFLINE_HINT: &str = "Offline — reconnect to send";
/// The demo org's reason on Sync this item.
pub const DEMO_SYNC_HINT: &str = "Demo org: nothing syncs from Azure DevOps";

// ── access (FR-28) ───────────────────────────────────────────────────────

/// Who may change work items from this window.
#[derive(Clone, Default)]
pub enum Access {
    /// The demo org: writes apply to the local store (FR-37).
    Local,
    /// Signed in. `can_write`: the token has `vso.work_write` (Review
    /// level); without it the detail is read-only but Sync this item still
    /// works (it only needs `vso.work`).
    Remote {
        writer: watch::Receiver<Option<WiWriter>>,
        can_write: bool,
    },
    /// No org (signed out, stories).
    #[default]
    None,
}

impl Access {
    pub fn can_write(&self) -> bool {
        match self {
            Access::Local => true,
            Access::Remote { can_write, .. } => *can_write,
            Access::None => false,
        }
    }

    pub fn is_demo(&self) -> bool {
        matches!(self, Access::Local)
    }

    /// The org's work item writer, once sync has read the token.
    pub fn writer(&self) -> Option<WiWriter> {
        match self {
            Access::Remote { writer, .. } => writer.borrow().clone(),
            _ => None,
        }
    }
}

struct WiAccess(Access);

impl Global for WiAccess {}

/// The window's work item access; `None` until the shell sets one.
pub fn access(cx: &App) -> Access {
    cx.try_global::<WiAccess>()
        .map_or(Access::None, |a| a.0.clone())
}

pub fn set_access(access: Access, cx: &mut App) {
    cx.set_global(WiAccess(access));
}

/// What the detail may offer for one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Editable,
    /// No `vso.work_write` (FR-28): plain values plus one passive hint.
    ReadOnly,
    /// Controls visible, disabled, "Offline — reconnect to send".
    Offline,
    /// 404 on sync or write: last data kept, editing disabled.
    Gone,
}

/// The gate of item `id` (FR-28, §3.4 Offline, §States Error).
pub fn gate(cx: &App, id: i64) -> Gate {
    let edits = WiEdits::global(cx);
    let edits = edits.as_ref().map(|e| e.read(cx));
    if edits.is_some_and(|e| e.edits.is_gone(id) || e.syncs.is_gone(id)) {
        return Gate::Gone;
    }
    if !access(cx).can_write() || edits.is_some_and(|e| e.edits.is_read_only(id)) {
        return Gate::ReadOnly;
    }
    if edits.is_some_and(|e| e.offline) {
        return Gate::Offline;
    }
    Gate::Editable
}

// ── the per-item queue (FR-6..FR-10, FR-25, FR-28) ───────────────────────

/// What the detail shows under an element while (or after) its edit is out.
#[derive(Clone, Debug, PartialEq)]
pub enum EditStatus {
    /// Sent or queued behind another write of the item (FR-10): the new
    /// value with the clock and "Sending…".
    Sending { write: WiWrite, since: Millis },
    /// Reverted to the stored value; `write` is the Retry (FR-7, FR-28).
    Failed { write: WiWrite, cause: WiError },
    /// The server's value is shown; mine waits (FR-8).
    Conflict { write: WiWrite, conflict: Conflict },
    /// A process rule needs another field (FR-9).
    Rule { write: WiWrite, rule: RuleViolation },
}

impl EditStatus {
    pub fn write(&self) -> &WiWrite {
        match self {
            EditStatus::Sending { write, .. }
            | EditStatus::Failed { write, .. }
            | EditStatus::Conflict { write, .. }
            | EditStatus::Rule { write, .. } => write,
        }
    }

    /// My value, as the pending line or "Yours:" shows it.
    pub fn value(&self) -> String {
        self.write().change.display()
    }

    pub fn is_sending(&self) -> bool {
        matches!(self, EditStatus::Sending { .. })
    }

    /// "Waiting for Azure DevOps…" instead of "Sending…".
    pub fn waiting(&self, now: Millis) -> bool {
        matches!(self, EditStatus::Sending { since, .. } if now - since >= WAITING_AFTER_MS)
    }

    pub fn rule(&self) -> Option<&RuleViolation> {
        match self {
            EditStatus::Rule { rule, .. } => Some(rule),
            _ => None,
        }
    }

    pub fn conflict(&self) -> Option<&Conflict> {
        match self {
            EditStatus::Conflict { conflict, .. } => Some(conflict),
            _ => None,
        }
    }

    pub fn failure(&self) -> Option<&WiError> {
        match self {
            EditStatus::Failed { cause, .. } => Some(cause),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommentStatus {
    Sending { since: Millis },
    Failed { cause: WiError },
}

/// A comment I posted that Azure DevOps hasn't confirmed (FR-4, FR-25).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingComment {
    /// Local id (not Azure DevOps').
    pub local: u64,
    pub markdown: String,
    pub status: CommentStatus,
}

/// One unit of the per-item queue.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Write(WiWrite),
    Comment { local: u64, markdown: String },
}

/// A job's answer. One exists per answered job, briefly: boxing the write
/// outcome would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Answer {
    Write(WiWrite, Result<WiOutcome, WiError>),
    Comment(u64, Result<WiComment, WiError>),
}

#[derive(Default)]
struct ItemEdits {
    queue: VecDeque<Job>,
    in_flight: bool,
    status: HashMap<Field, EditStatus>,
    comments: Vec<PendingComment>,
    read_only: bool,
    gone: bool,
}

/// FR-10: per item, one job in flight and the rest in order; different
/// items run in parallel. Pure state; [`WiEdits`] sends what it returns.
#[derive(Default)]
pub struct Edits {
    items: HashMap<i64, ItemEdits>,
    next_local: u64,
}

impl Edits {
    fn item(&mut self, id: i64) -> &mut ItemEdits {
        self.items.entry(id).or_default()
    }

    /// Queue `write`; returns the job to start now, if none is in flight.
    pub fn submit(&mut self, write: WiWrite, now: Millis) -> Option<Job> {
        let id = write.id;
        let item = self.item(id);
        item.status.insert(
            write.change.field(),
            EditStatus::Sending {
                write: write.clone(),
                since: now,
            },
        );
        item.queue.push_back(Job::Write(write));
        self.next(id)
    }

    /// Show `write` pending while a local check runs (FR-21 link guards).
    pub fn checking(&mut self, write: &WiWrite, now: Millis) {
        self.item(write.id).status.insert(
            write.change.field(),
            EditStatus::Sending {
                write: write.clone(),
                since: now,
            },
        );
    }

    /// The local check refused `write`: its reason, nothing sent.
    pub fn refused(&mut self, write: WiWrite, reason: String) {
        self.item(write.id).status.insert(
            write.change.field(),
            EditStatus::Failed {
                write,
                cause: WiError::Invalid(reason),
            },
        );
    }

    /// Queue a comment (pending at once); returns its local id and the job
    /// to start now, if any.
    pub fn submit_comment(&mut self, id: i64, markdown: String, now: Millis) -> (u64, Option<Job>) {
        self.next_local += 1;
        let local = self.next_local;
        let item = self.item(id);
        item.comments.push(PendingComment {
            local,
            markdown: markdown.clone(),
            status: CommentStatus::Sending { since: now },
        });
        item.queue.push_back(Job::Comment { local, markdown });
        (local, self.next(id))
    }

    fn next(&mut self, id: i64) -> Option<Job> {
        let item = self.item(id);
        if item.in_flight {
            return None;
        }
        let job = item.queue.pop_front()?;
        item.in_flight = true;
        Some(job)
    }

    /// Whether `id` has a job out (FR-10: never two).
    pub fn in_flight(&self, id: i64) -> bool {
        self.items.get(&id).is_some_and(|i| i.in_flight)
    }

    /// Record the answer of `id`'s job in flight; returns the next job.
    pub fn answered(&mut self, id: i64, answer: Answer) -> Option<Job> {
        let item = self.item(id);
        item.in_flight = false;
        let mut stop: Option<WiError> = None;
        match answer {
            Answer::Write(write, result) => {
                let field = write.change.field();
                let queued_same = item
                    .queue
                    .iter()
                    .any(|j| matches!(j, Job::Write(w) if w.change.field() == field));
                match result {
                    Ok(WiOutcome::Applied(_)) => {
                        if !queued_same {
                            item.status.remove(&field);
                        }
                    }
                    Ok(WiOutcome::Conflict(conflict)) => {
                        item.status
                            .insert(field, EditStatus::Conflict { write, conflict });
                    }
                    Ok(WiOutcome::Rule(rule)) => {
                        item.status.insert(field, EditStatus::Rule { write, rule });
                    }
                    Err(cause) => {
                        if matches!(cause, WiError::MissingScope | WiError::Gone) {
                            stop = Some(cause.clone());
                        }
                        item.status
                            .insert(field, EditStatus::Failed { write, cause });
                    }
                }
            }
            Answer::Comment(local, result) => match result {
                Ok(_) => item.comments.retain(|c| c.local != local),
                Err(cause) => {
                    if matches!(cause, WiError::MissingScope | WiError::Gone) {
                        stop = Some(cause.clone());
                    }
                    if let Some(c) = item.comments.iter_mut().find(|c| c.local == local) {
                        c.status = CommentStatus::Failed { cause };
                    }
                }
            },
        }
        if let Some(cause) = stop {
            // FR-28: the token can't write (or the item is gone): nothing
            // else queued would get through; each keeps its Retry.
            match cause {
                WiError::MissingScope => item.read_only = true,
                _ => item.gone = true,
            }
            for job in item.queue.drain(..) {
                match job {
                    Job::Write(write) => {
                        item.status.insert(
                            write.change.field(),
                            EditStatus::Failed {
                                write,
                                cause: cause.clone(),
                            },
                        );
                    }
                    Job::Comment { local, .. } => {
                        if let Some(c) = item.comments.iter_mut().find(|c| c.local == local) {
                            c.status = CommentStatus::Failed {
                                cause: cause.clone(),
                            };
                        }
                    }
                }
            }
            return None;
        }
        self.next(id)
    }

    pub fn status(&self, id: i64, field: Field) -> Option<&EditStatus> {
        self.items.get(&id)?.status.get(&field)
    }

    pub fn statuses(&self, id: i64) -> impl Iterator<Item = (&Field, &EditStatus)> {
        self.items
            .get(&id)
            .into_iter()
            .flat_map(|i| i.status.iter())
    }

    pub fn pending_comments(&self, id: i64) -> &[PendingComment] {
        self.items.get(&id).map_or(&[], |i| i.comments.as_slice())
    }

    pub fn is_read_only(&self, id: i64) -> bool {
        self.items.get(&id).is_some_and(|i| i.read_only)
    }

    pub fn is_gone(&self, id: i64) -> bool {
        self.items.get(&id).is_some_and(|i| i.gone)
    }

    /// The token was replaced (or access changed): try again.
    pub fn clear_read_only(&mut self) {
        for item in self.items.values_mut() {
            item.read_only = false;
        }
    }

    fn take(&mut self, id: i64, field: Field) -> Option<EditStatus> {
        let item = self.items.get_mut(&id)?;
        match item.status.get(&field) {
            Some(EditStatus::Sending { .. }) | None => None,
            Some(_) => item.status.remove(&field),
        }
    }

    /// Retry a failed write with my value (FR-7).
    pub fn retry(&mut self, id: i64, field: Field, now: Millis) -> Option<Job> {
        match self.take(id, field)? {
            EditStatus::Failed { write, .. } => {
                let item = self.item(id);
                item.read_only = false;
                item.gone = false;
                self.submit(write, now)
            }
            other => {
                self.item(id).status.insert(field, other);
                None
            }
        }
    }

    /// Apply mine (FR-8): resend my value on their revision.
    pub fn apply_mine(&mut self, id: i64, field: Field, now: Millis) -> Option<Job> {
        match self.take(id, field)? {
            EditStatus::Conflict { write, conflict } => self.submit(
                WiWrite {
                    id,
                    base_rev: conflict.server_rev,
                    change: conflict.rebased,
                    extra: write.extra,
                },
                now,
            ),
            other => {
                self.item(id).status.insert(field, other);
                None
            }
        }
    }

    /// Keep theirs, Dismiss, Cancel (rule): drop my value.
    pub fn dismiss(&mut self, id: i64, field: Field) {
        self.take(id, field);
    }

    /// FR-9: resend the original change with the field the rule asked for,
    /// in the same PATCH. `None` (and the notice stays) when the field's
    /// reference name is unknown: only Open in Azure DevOps is offered then.
    pub fn resend_with_rule(
        &mut self,
        id: i64,
        field: Field,
        value: Value,
        now: Millis,
    ) -> Option<Job> {
        match self.take(id, field)? {
            EditStatus::Rule { mut write, rule } => {
                let Some(reference) = rule.reference.clone() else {
                    self.item(id)
                        .status
                        .insert(field, EditStatus::Rule { write, rule });
                    return None;
                };
                write.extra.retain(|(f, _)| *f != reference);
                write.extra.push((reference, value));
                write.base_rev = rule.server_rev;
                self.submit(write, now)
            }
            other => {
                self.item(id).status.insert(field, other);
                None
            }
        }
    }

    /// Retry a failed comment.
    pub fn retry_comment(&mut self, id: i64, local: u64, now: Millis) -> Option<Job> {
        let item = self.item(id);
        let comment = item.comments.iter_mut().find(|c| c.local == local)?;
        if !matches!(comment.status, CommentStatus::Failed { .. }) {
            return None;
        }
        comment.status = CommentStatus::Sending { since: now };
        let markdown = comment.markdown.clone();
        item.read_only = false;
        item.queue.push_back(Job::Comment { local, markdown });
        self.next(id)
    }

    /// Take a failed comment's text back (to the composer).
    pub fn take_comment(&mut self, id: i64, local: u64) -> Option<String> {
        let item = self.items.get_mut(&id)?;
        let at = item
            .comments
            .iter()
            .position(|c| c.local == local && matches!(c.status, CommentStatus::Failed { .. }))?;
        Some(item.comments.remove(at).markdown)
    }
}

// ── Sync this item (FR-30, FR-31) ────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
pub enum ItemSyncState {
    #[default]
    Idle,
    Syncing {
        since: Millis,
    },
    /// Last success at `at`.
    Synced {
        at: Millis,
    },
    /// Pressed again within 10 s of `at`: "Just synced" until `until`.
    JustSynced {
        at: Millis,
        until: Millis,
    },
    Failed {
        cause: WiError,
    },
}

/// What a press of Sync this item did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncPress {
    Sent,
    /// Within 10 s of a successful sync: nothing sent.
    JustSynced,
    /// Already syncing.
    Busy,
    Disabled(&'static str),
}

/// FR-30 debounce and FR-31's once-per-10-minutes auto-fetch.
#[derive(Default)]
pub struct ItemSyncs {
    state: HashMap<i64, ItemSyncState>,
    auto: HashMap<i64, Millis>,
    gone: std::collections::HashSet<i64>,
}

impl ItemSyncs {
    /// The state at `now` ("Just synced" lasts 2 s).
    pub fn state(&self, id: i64, now: Millis) -> ItemSyncState {
        match self.state.get(&id) {
            Some(ItemSyncState::JustSynced { at, until }) if now >= *until => {
                ItemSyncState::Synced { at: *at }
            }
            Some(state) => state.clone(),
            None => ItemSyncState::Idle,
        }
    }

    pub fn is_gone(&self, id: i64) -> bool {
        self.gone.contains(&id)
    }

    /// `⌘⇧R` / the header button / the palette (FR-30).
    pub fn press(&mut self, id: i64, now: Millis, offline: bool, demo: bool) -> SyncPress {
        if demo {
            return SyncPress::Disabled(DEMO_SYNC_HINT);
        }
        if offline {
            return SyncPress::Disabled("Offline — reconnect to sync");
        }
        match self.state.get(&id) {
            Some(ItemSyncState::Syncing { .. }) => SyncPress::Busy,
            Some(ItemSyncState::Synced { at } | ItemSyncState::JustSynced { at, .. })
                if now - *at < JUST_SYNCED_MS =>
            {
                let at = *at;
                self.state.insert(
                    id,
                    ItemSyncState::JustSynced {
                        at,
                        until: now + JUST_SYNCED_LABEL_MS,
                    },
                );
                SyncPress::JustSynced
            }
            _ => {
                self.state.insert(id, ItemSyncState::Syncing { since: now });
                SyncPress::Sent
            }
        }
    }

    /// FR-31: the selection rested 400 ms on `id`. Sends (returns `true`)
    /// only for a partly synced item, online, once per 10 minutes.
    pub fn rested(
        &mut self,
        id: i64,
        partly: bool,
        now: Millis,
        offline: bool,
        demo: bool,
    ) -> bool {
        if !partly || offline || demo {
            return false;
        }
        if self
            .auto
            .get(&id)
            .is_some_and(|at| now - at < AUTO_FETCH_EVERY_MS)
        {
            return false;
        }
        if matches!(self.state.get(&id), Some(ItemSyncState::Syncing { .. })) {
            return false;
        }
        self.auto.insert(id, now);
        self.state.insert(id, ItemSyncState::Syncing { since: now });
        true
    }

    pub fn done(&mut self, id: i64, result: &Result<(), WiError>, now: Millis) {
        let state = match result {
            Ok(()) => {
                self.gone.remove(&id);
                ItemSyncState::Synced { at: now }
            }
            Err(cause) => {
                if *cause == WiError::Gone {
                    self.gone.insert(id);
                }
                ItemSyncState::Failed {
                    cause: cause.clone(),
                }
            }
        };
        self.state.insert(id, state);
    }
}

// ── freshness (FR-29) ────────────────────────────────────────────────────

/// Which lane would have fetched the item had it changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Coverage {
    /// Only Sync this item refreshes it.
    #[default]
    None,
    /// The "mine" WIQL lane (assigned to or created by me, synced project).
    Mine,
    /// The pinned batch (0014 FR-11).
    Pinned,
}

/// What the freshness label needs from the store ([`load_freshness_input`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FreshnessInput {
    pub synced_ms: Option<Millis>,
    pub full_synced_ms: Option<Millis>,
    pub coverage: Coverage,
    /// The `wi.hot` cursor's last good pass (`sync_cursor.last_ok_ms`).
    pub lane_last_ok: Option<Millis>,
}

impl FreshnessInput {
    /// FR-29: the later of the item's own fetch and its covering lane's
    /// last good pass (the pinned batch refetches the item itself).
    pub fn confirmed(&self) -> Option<Millis> {
        match self.coverage {
            Coverage::Mine => self.synced_ms.max(self.lane_last_ok),
            _ => self.synced_ms,
        }
    }

    /// Description and links were never fetched (title-index row).
    pub fn partly(&self) -> bool {
        self.full_synced_ms.is_none()
    }
}

/// Blocking: the freshness inputs of item `id`.
pub fn load_freshness_input(store: &Store, id: i64) -> adoken_store::Result<FreshnessInput> {
    store.with_conn(|c| {
        let extras = store_wi::extras(c, id)?.unwrap_or_default();
        let me = adoken_store::read::me(c)?.map(|p| p.id);
        let item = adoken_store::read::work_item(c, id)?;
        let synced: std::collections::HashSet<String> = adoken_store::read::synced_projects(c)?
            .into_iter()
            .map(|p| p.id)
            .collect();
        let mine = item.as_ref().is_some_and(|wi| {
            me.as_deref()
                .is_some_and(|me| wi.assigned_to.as_deref() == Some(me) || wi.created_by == me)
                && synced.contains(&wi.project_id)
        });
        let coverage = if mine {
            Coverage::Mine
        } else if adoken_store::read::pin(c, &PinSubject::WorkItem(id))?.is_some() {
            Coverage::Pinned
        } else {
            Coverage::None
        };
        Ok(FreshnessInput {
            synced_ms: extras.synced_ms,
            full_synced_ms: extras.full_synced_ms,
            coverage,
            lane_last_ok: adoken_store::read::cursor(c, "wi.hot", "")?.and_then(|c| c.last_ok_ms),
        })
    })
}

/// The rest of what the label depends on.
#[derive(Clone, Debug)]
pub struct FreshCtx<'a> {
    pub now: Millis,
    pub offline: bool,
    pub demo: bool,
    pub sync: &'a ItemSyncState,
    /// The Work items lane's interval (`None`: manual only).
    pub lane_every: Option<Millis>,
    /// The user's UTC offset at `now`: tooltip times are local.
    pub offset: time::UtcOffset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreshKind {
    Fresh,
    Stale,
    Partly,
    Syncing,
    Waiting,
    JustSynced,
    Failed,
    Offline,
    Demo,
}

/// The header's freshness button (design §3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Freshness {
    pub kind: FreshKind,
    pub label: String,
    /// Tooltip and accessible label: the full timestamp and the lane.
    pub tooltip: String,
    pub confirmed: Option<Millis>,
}

fn span(ms: Millis) -> String {
    match ms {
        ms if ms < MINUTE => format!("{} s", (ms / SECOND).max(1)),
        ms if ms < HOUR => format!("{} min", ms / MINUTE),
        ms if ms < DAY => format!("{} h", ms / HOUR),
        ms => format!("{} d", ms / DAY),
    }
}

fn ago(ms: Millis) -> String {
    match ms.max(0) {
        ms if ms < HOUR => format!("{}m ago", ms / MINUTE),
        ms if ms < DAY => format!("{}h ago", ms / HOUR),
        ms => format!("{}d ago", ms / DAY),
    }
}

/// `14:02:11` in local time (`offset`), with the date when it isn't today
/// there.
pub fn timestamp(at: Millis, now: Millis, offset: time::UtcOffset) -> String {
    let t = |ms: Millis| {
        time::OffsetDateTime::from_unix_timestamp(ms.div_euclid(1000))
            .ok()
            .map(|t| t.to_offset(offset))
    };
    let (Some(at_t), Some(now_t)) = (t(at), t(now)) else {
        return String::new();
    };
    let clock = format!(
        "{:02}:{:02}:{:02}",
        at_t.hour(),
        at_t.minute(),
        at_t.second()
    );
    if at_t.date() == now_t.date() {
        clock
    } else {
        format!("{} {} {clock}", &at_t.month().to_string()[..3], at_t.day())
    }
}

/// FR-29's label for `input` under `ctx` (pure).
pub fn freshness_with(input: &FreshnessInput, ctx: &FreshCtx<'_>) -> Freshness {
    let now = ctx.now;
    let confirmed = input.confirmed();
    let lane = match (input.coverage, ctx.lane_every) {
        (Coverage::None, _) => "only when you sync it".to_owned(),
        (_, Some(every)) => format!("refreshes every {} with Work items", span(every)),
        (_, None) => "refreshes with Work items on Sync now".to_owned(),
    };
    let base = match confirmed {
        Some(at) => format!("Last confirmed {} · {lane}", timestamp(at, now, ctx.offset)),
        None => format!("Never fetched in full · {lane}"),
    };
    let make = |kind, label: String, tooltip: String| Freshness {
        kind,
        label,
        tooltip,
        confirmed,
    };
    let action = "Sync this work item  ⌘⇧R";
    if ctx.demo {
        return make(FreshKind::Demo, "Demo data".into(), DEMO_SYNC_HINT.into());
    }
    if ctx.offline {
        let label = match confirmed {
            Some(at) => format!("Offline · synced {}", ago(now - at)),
            None => "Offline".to_owned(),
        };
        return make(
            FreshKind::Offline,
            label,
            format!("{base} · Offline — reconnect to sync"),
        );
    }
    match ctx.sync {
        ItemSyncState::Syncing { since } if now - since >= WAITING_AFTER_MS => {
            return make(
                FreshKind::Waiting,
                "Waiting for Azure DevOps…".into(),
                "Azure DevOps asked adoken to slow down. This item syncs as soon as it allows."
                    .into(),
            );
        }
        ItemSyncState::Syncing { .. } => {
            return make(FreshKind::Syncing, "Syncing…".into(), base);
        }
        ItemSyncState::JustSynced { until, .. } if now < *until => {
            return make(
                FreshKind::JustSynced,
                "Just synced".into(),
                format!("{base} · {action}"),
            );
        }
        ItemSyncState::Failed { cause } => {
            return make(
                FreshKind::Failed,
                "Couldn't sync · Retry".into(),
                format!("Couldn't sync: {cause} · Retry  ⌘⇧R"),
            );
        }
        _ => {}
    }
    let tooltip = format!("{base} · {action}");
    match confirmed {
        Some(at) if !input.partly() && now - at < MINUTE => {
            make(FreshKind::Fresh, "Synced just now".into(), tooltip)
        }
        Some(at) if !input.partly() && now - at < STALE_AFTER_MS => make(
            FreshKind::Fresh,
            format!("Synced {}", ago(now - at)),
            tooltip,
        ),
        Some(at) if !input.partly() => make(
            FreshKind::Stale,
            format!("Updated {}", ago(now - at)),
            tooltip,
        ),
        _ => make(FreshKind::Partly, "Partly synced".into(), tooltip),
    }
}

// ── metadata fallbacks (FR-14, FR-37) ────────────────────────────────────

/// Stock states of a type (Agile, plus the "Doing" some teams add), used
/// when no metadata can be fetched (the demo org): every state reachable.
pub fn stock_type_meta(kind: &WorkItemType) -> TypeMeta {
    let names: &[&str] = match kind.family() {
        TypeFamily::Task => &["New", "Active", "Doing", "Closed", "Removed"],
        TypeFamily::Issue => &["Active", "Resolved", "Closed"],
        _ => &["New", "Active", "Resolved", "Closed", "Removed"],
    };
    TypeMeta {
        states: names
            .iter()
            .map(|n| WiState {
                name: (*n).to_owned(),
                category: StateCategory::of_state(n),
                color: None,
            })
            .collect(),
        transitions: HashMap::new(),
        fetched_ms: 0,
    }
}

/// FR-14: until the tree arrives, the paths seen in the loaded items.
pub fn fallback_tree<'a>(
    areas: impl IntoIterator<Item = &'a str>,
    iterations: impl IntoIterator<Item = &'a str>,
) -> ClassTree {
    fn nodes<'a>(kind: NodeKind, paths: impl IntoIterator<Item = &'a str>) -> Vec<ClassNode> {
        let mut all: Vec<String> = Vec::new();
        for path in paths {
            let mut acc = String::new();
            for seg in path.split('\\').filter(|s| !s.is_empty()) {
                if !acc.is_empty() {
                    acc.push('\\');
                }
                acc.push_str(seg);
                if !all.contains(&acc) {
                    all.push(acc.clone());
                }
            }
        }
        all.sort_by(|a, b| {
            a.split('\\')
                .map(str::to_lowercase)
                .cmp(b.split('\\').map(str::to_lowercase))
        });
        all.into_iter()
            .map(|path| ClassNode {
                kind,
                name: path.rsplit('\\').next().unwrap_or_default().to_owned(),
                depth: path.matches('\\').count() as u16,
                path,
                start_ms: None,
                finish_ms: None,
            })
            .collect()
    }
    ClassTree {
        areas: nodes(NodeKind::Area, areas),
        iterations: nodes(NodeKind::Iteration, iterations),
        fetched_ms: 0,
    }
}

/// People the picker may assign (FR-13): no groups, nobody without a known
/// sign-in name.
pub fn assignable(people: impl IntoIterator<Item = Person>) -> Vec<Person> {
    people
        .into_iter()
        .filter(|p| !p.is_group && p.unique_name.as_deref().is_some_and(|u| !u.is_empty()))
        .collect()
}

// ── the entity ───────────────────────────────────────────────────────────

enum Meta<T> {
    Loading,
    Ready(T),
}

/// The window's work item edit controller (one per store).
pub struct WiEdits {
    store: Store,
    pub edits: Edits,
    pub syncs: ItemSyncs,
    offline: bool,
    /// Stories pin the clock.
    clock: Option<Millis>,
    types: HashMap<(String, String), Meta<TypeMeta>>,
    trees: HashMap<String, Meta<ClassTree>>,
    recents: Vec<Person>,
    drafts: HashMap<(i64, DraftKind), Task<()>>,
    /// The latest confirmed answer per item (FR-7: server wins), shown
    /// until the store reload catches up so the pending clock never
    /// clears back to the old value.
    applied: HashMap<i64, WorkItem>,
}

struct GlobalEdits(Entity<WiEdits>);

impl Global for GlobalEdits {}

impl WiEdits {
    /// The controller for `store`, created (and made global) on first use.
    pub fn install(store: Store, cx: &mut App) -> Entity<WiEdits> {
        if let Some(existing) = cx.try_global::<GlobalEdits>()
            && existing.0.read(cx).store.path() == store.path()
        {
            return existing.0.clone();
        }
        let entity = cx.new(|_| WiEdits::new(store));
        cx.set_global(GlobalEdits(entity.clone()));
        let weak = entity.downgrade();
        let store = entity.read(cx).store.clone();
        cx.spawn(async move |cx| {
            let recents = cx
                .background_executor()
                .spawn(async move {
                    store.with_conn(|c| store_wi::recent_assignees(c, store_wi::RECENT_ASSIGNEES))
                })
                .await
                .unwrap_or_default();
            weak.update(cx, |this, cx| {
                this.recents = recents;
                cx.notify();
            })
            .ok();
        })
        .detach();
        entity
    }

    pub fn global(cx: &App) -> Option<Entity<WiEdits>> {
        cx.try_global::<GlobalEdits>().map(|g| g.0.clone())
    }

    fn new(store: Store) -> Self {
        WiEdits {
            store,
            edits: Edits::default(),
            syncs: ItemSyncs::default(),
            offline: false,
            clock: None,
            types: HashMap::new(),
            trees: HashMap::new(),
            recents: Vec::new(),
            drafts: HashMap::new(),
            applied: HashMap::new(),
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Azure DevOps' latest confirmed copy of item `id` this session.
    pub fn applied(&self, id: i64) -> Option<&WorkItem> {
        self.applied.get(&id)
    }

    pub fn now(&self) -> Millis {
        self.clock.unwrap_or_else(crate::browse::now_ms)
    }

    /// Stories: pin the clock.
    pub fn set_clock(&mut self, now: Millis, cx: &mut Context<Self>) {
        self.clock = Some(now);
        cx.notify();
    }

    pub fn set_offline(&mut self, offline: bool, cx: &mut Context<Self>) {
        if self.offline != offline {
            self.offline = offline;
            cx.notify();
        }
    }

    /// The token was replaced or its access changed (spec 0001 FR-16):
    /// items a refusal made read-only may be edited again.
    pub fn token_changed(&mut self, cx: &mut Context<Self>) {
        self.edits.clear_read_only();
        cx.notify();
    }

    pub fn offline(&self) -> bool {
        self.offline
    }

    // ── edits ────────────────────────────────────────────────────────────

    /// Start an edit of `item` (FR-6: pending on the next frame). Refused
    /// at once (nothing queued) for an empty title, offline, or without
    /// write access. A new link is first checked against FR-21's local
    /// guards off the UI thread; a refusal shows as the link's failure with
    /// the reason, and nothing is sent.
    pub fn edit(
        &mut self,
        item: &WorkItem,
        change: WiChange,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        change.validate().map_err(|e| e.to_string())?;
        let access = access(cx);
        if !access.can_write() || self.edits.is_read_only(item.id) {
            return Err(READ_ONLY_HINT.into());
        }
        if self.offline {
            return Err(OFFLINE_HINT.into());
        }
        let write = WiWrite {
            id: item.id,
            base_rev: item.rev,
            change,
            extra: Vec::new(),
        };
        let now = self.now();
        if let WiChange::AddLink { .. } = &write.change {
            self.edits.checking(&write, now);
            let store = self.store.clone();
            let checked = write.clone();
            let guard = cx.background_spawn(async move {
                store
                    .with_conn(|c| {
                        let Some(item) = adoken_store::read::work_item(c, checked.id)? else {
                            return Ok(Ok(()));
                        };
                        let extras = store_wi::extras(c, checked.id)?.unwrap_or_default();
                        Ok(link_guard(c, &item, &extras, &checked.change))
                    })
                    .unwrap_or_else(|e| Err(e.to_string()))
            });
            cx.spawn(async move |this, cx| {
                let verdict = guard.await;
                this.update(cx, |this, cx| {
                    let id = write.id;
                    match verdict {
                        Ok(()) => {
                            let now = this.now();
                            if let Some(job) = this.edits.submit(write, now) {
                                this.start(id, job, cx);
                            }
                        }
                        Err(reason) => this.edits.refused(write, reason),
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        } else if let Some(job) = self.edits.submit(write, now) {
            self.start(item.id, job, cx);
        }
        cx.notify();
        Ok(())
    }

    /// Post a comment (FR-25): pending at once; empty text is refused.
    pub fn comment(
        &mut self,
        item: &WorkItem,
        markdown: String,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        if markdown.trim().is_empty() {
            return Err("Nothing to post".into());
        }
        if !access(cx).can_write() || self.edits.is_read_only(item.id) {
            return Err(READ_ONLY_HINT.into());
        }
        if self.offline {
            return Err(OFFLINE_HINT.into());
        }
        let now = self.now();
        let (local, job) = self.edits.submit_comment(item.id, markdown, now);
        if let Some(job) = job {
            self.start(item.id, job, cx);
        }
        cx.notify();
        Ok(local)
    }

    pub fn status(&self, id: i64, field: Field) -> Option<&EditStatus> {
        self.edits.status(id, field)
    }

    pub fn pending_comments(&self, id: i64) -> &[PendingComment] {
        self.edits.pending_comments(id)
    }

    pub fn read_only(&self, id: i64) -> bool {
        self.edits.is_read_only(id)
    }

    pub fn retry(&mut self, id: i64, field: Field, cx: &mut Context<Self>) {
        if self.offline {
            return;
        }
        let now = self.now();
        if let Some(job) = self.edits.retry(id, field, now) {
            self.start(id, job, cx);
        }
        cx.notify();
    }

    pub fn dismiss(&mut self, id: i64, field: Field, cx: &mut Context<Self>) {
        self.edits.dismiss(id, field);
        cx.notify();
    }

    pub fn keep_theirs(&mut self, id: i64, field: Field, cx: &mut Context<Self>) {
        self.dismiss(id, field, cx);
    }

    pub fn apply_mine(&mut self, id: i64, field: Field, cx: &mut Context<Self>) {
        if self.offline {
            return;
        }
        let now = self.now();
        if let Some(job) = self.edits.apply_mine(id, field, now) {
            self.start(id, job, cx);
        }
        cx.notify();
    }

    /// FR-9: `value` is the allowed value, the text, or a person's distinct
    /// name (`adoken_sync::wi::distinct_name`).
    pub fn resend_with_rule(
        &mut self,
        id: i64,
        field: Field,
        value: String,
        cx: &mut Context<Self>,
    ) {
        if self.offline {
            return;
        }
        let now = self.now();
        if let Some(job) = self
            .edits
            .resend_with_rule(id, field, Value::String(value), now)
        {
            self.start(id, job, cx);
        }
        cx.notify();
    }

    pub fn retry_comment(&mut self, id: i64, local: u64, cx: &mut Context<Self>) {
        if self.offline {
            return;
        }
        let now = self.now();
        if let Some(job) = self.edits.retry_comment(id, local, now) {
            self.start(id, job, cx);
        }
        cx.notify();
    }

    pub fn take_comment(&mut self, id: i64, local: u64, cx: &mut Context<Self>) -> Option<String> {
        let text = self.edits.take_comment(id, local);
        cx.notify();
        text
    }

    /// Send `job` (one per item at a time) and feed its answer back.
    fn start(&mut self, id: i64, job: Job, cx: &mut Context<Self>) {
        let access = access(cx);
        let store = self.store.clone();
        let now = self.now();
        let run: Task<Answer> = match access {
            Access::Local => {
                let executor = cx.background_executor().clone();
                cx.background_spawn(async move {
                    executor
                        .timer(Duration::from_millis(DEMO_DELAY_MS as u64))
                        .await;
                    match job {
                        Job::Write(write) => {
                            let result = apply_local(&store, &write, now)
                                .map(|item| WiOutcome::Applied(Some(item)));
                            Answer::Write(write, result)
                        }
                        Job::Comment { local, markdown } => {
                            Answer::Comment(local, comment_local(&store, id, &markdown, now))
                        }
                    }
                })
            }
            Access::Remote { .. } => match access.writer() {
                Some(writer) => {
                    let spawned = Tokio::spawn(cx, async move {
                        match job {
                            Job::Write(write) => {
                                let result = writer.send(&write).await;
                                Answer::Write(write, result)
                            }
                            Job::Comment { local, markdown } => {
                                Answer::Comment(local, writer.comment(id, &markdown).await)
                            }
                        }
                    });
                    cx.background_spawn(async move {
                        spawned.await.unwrap_or_else(|e| {
                            Answer::Comment(0, Err(WiError::Local(e.to_string())))
                        })
                    })
                }
                None => Task::ready(not_sent(job, WiError::NotConnected)),
            },
            Access::None => Task::ready(not_sent(job, WiError::MissingScope)),
        };
        cx.spawn(async move |this, cx| {
            let answer = run.await;
            this.update(cx, |this, cx| this.answered(id, answer, cx))
                .ok();
        })
        .detach();
    }

    fn answered(&mut self, id: i64, answer: Answer, cx: &mut Context<Self>) {
        // Spec 0001 FR-17: a scope refusal is remembered in account.json; a
        // write Azure DevOps accepted proves an untested scope.
        match &answer {
            Answer::Write(_, Ok(WiOutcome::Applied(_))) | Answer::Comment(_, Ok(_)) => {
                crate::account::record_write(adoken_sync::writes::WORK_WRITE_SCOPE, false, cx)
            }
            Answer::Write(_, Err(WiError::MissingScope))
            | Answer::Comment(_, Err(WiError::MissingScope)) => {
                crate::account::record_write(adoken_sync::writes::WORK_WRITE_SCOPE, true, cx)
            }
            _ => {}
        }
        let confirmed = match &answer {
            Answer::Write(write, Ok(WiOutcome::Applied(_))) => {
                self.after_confirmed_write(write, cx);
                true
            }
            Answer::Comment(_, Ok(_)) => {
                self.delete_draft(id, DraftKind::Comment, cx);
                true
            }
            _ => false,
        };
        let failed = matches!(
            &answer,
            Answer::Write(_, Err(_)) | Answer::Comment(_, Err(_))
        );
        if confirmed {
            // A write's answer is the re-read: "Synced just now" (design §3.4).
            let now = self.now();
            self.syncs.done(id, &Ok(()), now);
            // Spec 0022: a confirmed forward state move plays its
            // celebration sound instead (FR-6: never both).
            let celebrated = match &answer {
                Answer::Write(write, Ok(WiOutcome::Applied(item))) => {
                    self.celebrate(write, item.as_ref(), cx)
                }
                _ => false,
            };
            if !celebrated {
                crate::sound::play_ui(adoken_sound::SoundEvent::UiActionDone, true, cx).ok();
            }
        } else if failed {
            crate::sound::play_ui(adoken_sound::SoundEvent::UiError, true, cx).ok();
        }
        if let Answer::Write(_, Ok(WiOutcome::Applied(Some(item)))) = &answer
            && self
                .applied
                .get(&id)
                .is_none_or(|known| known.rev < item.rev)
        {
            self.applied.insert(id, item.clone());
        }
        if let Some(next) = self.edits.answered(id, answer) {
            self.start(id, next, cx);
        }
        cx.notify();
    }

    /// Spec 0022 FR-1/FR-2: my confirmed state write may celebrate. The
    /// answer's state wins (0015 FR-7); categories come from the type
    /// metadata (FR-33) when cached, else the stock names. `true` when the
    /// celebration took care of the sound.
    fn celebrate(&self, write: &WiWrite, item: Option<&WorkItem>, cx: &mut App) -> bool {
        let WiChange::State { to, base } = &write.change else {
            return false;
        };
        let to = item.map_or(to.as_str(), |item| item.state.as_str());
        let meta = item.and_then(|item| {
            match self
                .types
                .get(&(item.project_id.clone(), item.kind.as_str().to_owned()))
            {
                Some(Meta::Ready(meta)) => Some(meta),
                _ => None,
            }
        });
        let category = |state: &str| {
            meta.map_or_else(
                || StateCategory::of_state(state),
                |meta| meta.category(state),
            )
        };
        crate::celebrate::work_item_confirmed(
            crate::celebrate::WorkItemMove {
                id: write.id,
                title: item.map(|item| item.title.clone()).unwrap_or_default(),
                from: base.clone(),
                from_category: category(base),
                to: to.to_owned(),
                to_category: category(to),
            },
            cx,
        )
    }

    /// FR-20: a confirmed description save deletes its draft; FR-13: a
    /// confirmed assignment joins the recent assignees.
    fn after_confirmed_write(&mut self, write: &WiWrite, cx: &mut Context<Self>) {
        match &write.change {
            WiChange::Description { .. } => self.delete_draft(write.id, DraftKind::Description, cx),
            WiChange::Assign {
                to: Some(person), ..
            } => {
                self.recents.retain(|p| p.id != person.id);
                self.recents.insert(0, person.clone());
                self.recents.truncate(store_wi::RECENT_ASSIGNEES);
                let (store, person, now) = (self.store.clone(), person.clone(), self.now());
                cx.background_spawn(async move {
                    store
                        .write(|tx| store_wi::touch_recent_assignee(tx, &person, now))
                        .ok();
                })
                .detach();
            }
            _ => {}
        }
    }

    // ── drafts (FR-20) ───────────────────────────────────────────────────

    /// Save the draft as I type (debounced, off the UI thread); empty text
    /// deletes it.
    pub fn save_draft(
        &mut self,
        item: &WorkItem,
        kind: DraftKind,
        body: String,
        cx: &mut Context<Self>,
    ) {
        let draft = WiDraft {
            work_item_id: item.id,
            kind,
            body,
            base_rev: item.rev,
            updated: self.now(),
        };
        let store = self.store.clone();
        let executor = cx.background_executor().clone();
        let task = cx.background_spawn(async move {
            executor
                .timer(Duration::from_millis(DRAFT_DELAY_MS as u64))
                .await;
            store.write(|tx| store_wi::save_draft(tx, &draft)).ok();
        });
        self.drafts.insert((item.id, kind), task);
    }

    /// Discard (or a confirmed save / post).
    pub fn delete_draft(&mut self, id: i64, kind: DraftKind, cx: &mut Context<Self>) {
        self.drafts.remove(&(id, kind));
        let store = self.store.clone();
        cx.background_spawn(async move {
            store.write(|tx| store_wi::delete_draft(tx, id, kind)).ok();
        })
        .detach();
    }

    // ── Sync this item (FR-30, FR-31) ────────────────────────────────────

    pub fn item_sync(&self, id: i64) -> ItemSyncState {
        self.syncs.state(id, self.now())
    }

    /// `⌘⇧R`, the header button, the palette.
    pub fn sync_item(&mut self, item: &WorkItem, cx: &mut Context<Self>) -> SyncPress {
        let now = self.now();
        let demo = access(cx).is_demo();
        let press = self.syncs.press(item.id, now, self.offline, demo);
        if press == SyncPress::Sent {
            self.run_sync(item.id, cx);
        }
        cx.notify();
        press
    }

    /// FR-31: call after the selection rested 400 ms on `item`.
    pub fn rested_on(
        &mut self,
        item: &WorkItem,
        extras: Option<&WorkItemExtras>,
        cx: &mut Context<Self>,
    ) {
        let partly = extras.is_none_or(|x| x.full_synced_ms.is_none());
        let now = self.now();
        let demo = !matches!(access(cx), Access::Remote { .. });
        if self.syncs.rested(item.id, partly, now, self.offline, demo) {
            self.run_sync(item.id, cx);
            cx.notify();
        }
    }

    fn run_sync(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(writer) = access(cx).writer() else {
            let now = self.now();
            self.syncs.done(id, &Err(WiError::NotConnected), now);
            return;
        };
        let spawned = Tokio::spawn(cx, async move { writer.sync_item(id).await.map(|_| ()) });
        cx.spawn(async move |this, cx| {
            let result = spawned
                .await
                .unwrap_or_else(|e| Err(WiError::Local(e.to_string())));
            this.update(cx, |this, cx| {
                let now = this.now();
                this.syncs.done(id, &result, now);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// FR-29's label for an item whose inputs `input` were loaded with
    /// [`load_freshness_input`].
    pub fn freshness(&self, input: &FreshnessInput, id: i64, cx: &App) -> Freshness {
        let now = self.now();
        let sync = self.syncs.state(id, now);
        freshness_with(
            input,
            &FreshCtx {
                now,
                offline: self.offline,
                demo: access(cx).is_demo(),
                sync: &sync,
                lane_every: crate::sync::prefs(cx)
                    .cadence(adoken_sync::cadence::Kind::WorkItems)
                    .every(),
                offset: crate::activity::model::local_offset(now),
            },
        )
    }

    /// Every status line of item `id`.
    pub fn statuses(&self, id: i64) -> impl Iterator<Item = (&Field, &EditStatus)> {
        self.edits.statuses(id)
    }

    // ── metadata (FR-11, FR-14, FR-33) ───────────────────────────────────

    /// The state menu of `item` (FR-11); `None` while its type metadata
    /// loads (the entity notifies when it lands). Cached metadata paints at
    /// once; older than 24 h it refreshes in the background.
    pub fn states(&mut self, item: &WorkItem, cx: &mut Context<Self>) -> Option<Vec<WiState>> {
        let key = (item.project_id.clone(), item.kind.as_str().to_owned());
        match self.types.get(&key) {
            Some(Meta::Ready(meta)) => return Some(meta.menu(&item.state)),
            Some(Meta::Loading) => return None,
            None => {}
        }
        self.types.insert(key.clone(), Meta::Loading);
        let store = self.store.clone();
        let access = access(cx);
        let kind = item.kind.clone();
        let now = self.now();
        cx.spawn(async move |this, cx| {
            let (p, t) = key.clone();
            let cached = cx
                .background_executor()
                .spawn(async move { store.with_conn(|c| store_wi::type_meta(c, &p, &t)) })
                .await
                .ok()
                .flatten();
            let writer = access.writer();
            let fetch = match (&cached, &writer) {
                (_, None) => false,
                (Some(meta), Some(_)) => is_stale(meta.fetched_ms, now),
                (None, Some(_)) => true,
            };
            let shown = cached
                .clone()
                .or_else(|| writer.is_none().then(|| stock_type_meta(&kind)));
            if let Some(meta) = shown {
                this.update(cx, |this, cx| {
                    this.types.insert(key.clone(), Meta::Ready(meta));
                    cx.notify();
                })
                .ok();
            }
            if let (true, Some(writer)) = (fetch, writer) {
                let (p, t) = key.clone();
                let meta = Tokio::spawn(cx, async move {
                    writer.type_meta(&p, &t, UserAction::ChangeState).await
                })
                .await
                .ok()
                .and_then(Result::ok);
                let meta = meta.or(cached).unwrap_or_else(|| stock_type_meta(&kind));
                this.update(cx, |this, cx| {
                    this.types.insert(key, Meta::Ready(meta));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        None
    }

    /// A project's area and iteration trees (FR-14); `None` while loading
    /// (the picker offers [`fallback_tree`] meanwhile).
    pub fn tree(&mut self, project_id: &str, cx: &mut Context<Self>) -> Option<ClassTree> {
        match self.trees.get(project_id) {
            Some(Meta::Ready(tree)) => return Some(tree.clone()),
            Some(Meta::Loading) => return None,
            None => {}
        }
        self.trees.insert(project_id.to_owned(), Meta::Loading);
        let store = self.store.clone();
        let access = access(cx);
        let now = self.now();
        let project = project_id.to_owned();
        cx.spawn(async move |this, cx| {
            let p = project.clone();
            let cached = cx
                .background_executor()
                .spawn(async move { store.with_conn(|c| store_wi::class_tree(c, &p)) })
                .await
                .ok()
                .flatten();
            let writer = access.writer();
            let fetch = match (&cached, &writer) {
                (_, None) => false,
                (Some(tree), Some(_)) => is_stale(tree.fetched_ms, now),
                (None, Some(_)) => true,
            };
            if let Some(tree) = cached.clone() {
                this.update(cx, |this, cx| {
                    this.trees.insert(project.clone(), Meta::Ready(tree));
                    cx.notify();
                })
                .ok();
            } else if writer.is_none() {
                // No network (demo): keep the fallback for good.
                this.update(cx, |this, cx| {
                    this.trees
                        .insert(project.clone(), Meta::Ready(ClassTree::default()));
                    cx.notify();
                })
                .ok();
            }
            if let (true, Some(writer)) = (fetch, writer) {
                let p = project.clone();
                let tree = Tokio::spawn(cx, async move {
                    writer.class_tree(&p, UserAction::ChangeArea).await
                })
                .await
                .ok()
                .and_then(Result::ok);
                this.update(cx, |this, cx| {
                    match tree.or(cached) {
                        Some(tree) => {
                            this.trees.insert(project, Meta::Ready(tree));
                        }
                        // Failed: try again on the next open.
                        None => {
                            this.trees.remove(&project);
                        }
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        None
    }

    /// People I assigned in adoken recently (FR-13), newest first.
    pub fn recent_assignees(&self) -> &[Person] {
        &self.recents
    }

    /// FR-13 directory search, sent now at user priority (the view
    /// debounces it: 300 ms pause, 3+ characters, fewer than 5 local
    /// matches; dropping the task cancels it). The demo org and a missing
    /// connection answer an empty list.
    pub fn search_people(
        &mut self,
        text: String,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<Person>, WiError>> {
        search_people(text, cx)
    }

    /// A project's trees if already in memory (no fetch, no notify).
    pub fn cached_tree(&self, project_id: &str) -> Option<&ClassTree> {
        match self.trees.get(project_id) {
            Some(Meta::Ready(tree)) => Some(tree),
            _ => None,
        }
    }
}

/// See [`WiEdits::search_people`].
pub fn search_people(text: String, cx: &mut App) -> Task<Result<Vec<Person>, WiError>> {
    let Some(writer) = access(cx).writer() else {
        return Task::ready(Ok(Vec::new()));
    };
    let search = Tokio::spawn(cx, async move {
        writer.search_people(&text, UserAction::Assign).await
    });
    cx.background_spawn(async move {
        search
            .await
            .unwrap_or_else(|e| Err(WiError::Local(e.to_string())))
    })
}

/// See [`WiEdits::cached_tree`].
pub fn cached_tree(project_id: &str, cx: &App) -> Option<ClassTree> {
    WiEdits::global(cx)?
        .read(cx)
        .cached_tree(project_id)
        .cloned()
}

fn not_sent(job: Job, cause: WiError) -> Answer {
    match job {
        Job::Write(write) => Answer::Write(write, Err(cause)),
        Job::Comment { local, .. } => Answer::Comment(local, Err(cause)),
    }
}

// ── free functions for the detail ────────────────────────────────────────

/// See [`WiEdits::states`]; `None` without a controller.
pub fn states(item: &WorkItem, cx: &mut App) -> Option<Vec<WiState>> {
    WiEdits::global(cx)?.update(cx, |e, cx| e.states(item, cx))
}

/// See [`WiEdits::tree`].
pub fn tree(project_id: &str, cx: &mut App) -> Option<ClassTree> {
    WiEdits::global(cx)?.update(cx, |e, cx| e.tree(project_id, cx))
}

pub fn recent_assignees(cx: &App) -> Vec<Person> {
    WiEdits::global(cx).map_or_else(Vec::new, |e| e.read(cx).recent_assignees().to_vec())
}

/// Blocking: the drafts of item `id` (the detail's background load).
pub fn load_drafts(store: &Store, id: i64) -> adoken_store::Result<Vec<WiDraft>> {
    store.with_conn(|c| store_wi::drafts(c, id))
}

#[cfg(test)]
mod tests;
