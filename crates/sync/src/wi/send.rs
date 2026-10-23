//! [`WiWriter`]: spec 0015's network paths at the governor's user priority
//! through the optional work-item transport (a 401 there means the token
//! lacks `vso.work_write`, never that it was rejected, FR-7/FR-28).

use std::sync::Arc;

use adoken_model::{
    ClassTree, FieldKind, FieldMeta, Millis, Person, PersonId, TypeMeta, WorkItem, WorkItemExtras,
};
use adoken_store::wi::{self, WiComment};
use adoken_store::{Store, read, write};
use serde::de::DeserializeOwned;
use url::Url;

use super::WiError;
use super::local::{in_tx, stored_comment};
use super::meta::{self, is_stale};
use super::patch::{self, COMMENT_PAGES, Merge, WiChange, WiWrite, merge};
use crate::Tier;
use crate::activity::{ChangeSet, Outcome, Tracker, Trigger, collect_changes};
use crate::attribution::{Origin, UserAction, with_origin};
use crate::clock::Clock;
use crate::engine::{apply_wi_comments, apply_work_items, name};
use crate::governor::{GovError, Governor, Hold};
use crate::transport::{ApiRequest, ApiResponse, Transport};
use crate::wire::{WireCommentList, WireWiComment, WireWorkItem};

/// What a write ended as (FR-6..FR-9). Failures are [`WiError`]s.
#[derive(Clone, Debug, PartialEq)]
pub enum WiOutcome {
    /// Azure DevOps' answer, applied (`None`: its project isn't stored).
    Applied(Option<WorkItem>),
    /// Someone changed what I edited (FR-8): the server value is stored and
    /// shown; my value waits for Apply mine / Keep theirs.
    Conflict(Conflict),
    /// A process rule needs another field (FR-9).
    Rule(RuleViolation),
}

/// FR-8: who changed what I edited, and when.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// Apply mine resends on this revision.
    pub server_rev: i64,
    /// Their value, display form (`None`: empty / unassigned).
    pub theirs: Option<String>,
    pub by: PersonId,
    pub by_name: String,
    pub at: Millis,
    /// My change rebased on the server's value: Apply mine sends it on
    /// `server_rev`.
    pub rebased: WiChange,
}

/// FR-9: Azure DevOps refused the change for a field rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleViolation {
    /// The field's display name as Azure DevOps named it ("Root cause").
    pub field: String,
    /// Its reference name, when the type's fields list has it; the value
    /// goes back as `WiWrite::extra` under this name.
    pub reference: Option<String>,
    pub kind: FieldKind,
    /// Azure DevOps' message.
    pub message: String,
    /// The revision to resend on.
    pub server_rev: i64,
}

/// A per-item sync's result (FR-30).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemSynced {
    pub item: Option<WorkItem>,
    /// Events persisted (others' changes notify as usual).
    pub events: Vec<i64>,
}

type Stored = (WorkItem, WorkItemExtras);

/// Spec 0015's writes, per-item sync and metadata for one org. Cheap to
/// clone; methods need a Tokio runtime (store work runs on its blocking
/// pool).
#[derive(Clone)]
pub struct WiWriter {
    store: Store,
    transport: Arc<dyn Transport>,
    gov: Arc<Governor>,
    clock: Arc<dyn Clock>,
    org_url: Url,
    /// Per-item syncs are passes in the session's sync activity (0016
    /// FR-11: "You: Sync #20101").
    tracker: Option<Arc<Tracker>>,
}

impl WiWriter {
    /// `transport` must hand 401 back as a status (the app's
    /// `OptionalAdoTransport`).
    pub fn new(
        store: Store,
        transport: Arc<dyn Transport>,
        gov: Arc<Governor>,
        clock: Arc<dyn Clock>,
        org_url: Url,
    ) -> Self {
        let mut org_url = org_url;
        if !org_url.path().ends_with('/') {
            org_url.set_path(&format!("{}/", org_url.path()));
        }
        WiWriter {
            store,
            transport,
            gov,
            clock,
            org_url,
            tracker: None,
        }
    }

    /// Report per-item syncs into `tracker` (the engine's).
    pub fn with_tracker(mut self, tracker: Arc<Tracker>) -> Self {
        self.tracker = Some(tracker);
        self
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn org_url(&self) -> &Url {
        &self.org_url
    }

    /// Why a request sent now would wait ("Waiting for Azure DevOps…").
    pub fn hold(&self) -> Option<Hold> {
        self.gov.hold()
    }

    pub fn now(&self) -> Millis {
        self.clock.now_ms()
    }

    async fn call(&self, request: ApiRequest) -> Result<ApiResponse, GovError> {
        self.gov
            .send_optional(self.transport.as_ref(), Tier::User, request)
            .await
    }

    async fn db<R: Send + 'static>(
        &self,
        f: impl FnOnce(&Store) -> Result<R, WiError> + Send + 'static,
    ) -> Result<R, WiError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&store))
            .await
            .map_err(|e| WiError::Local(e.to_string()))?
    }

    async fn stored(&self, id: i64) -> Result<Option<Stored>, WiError> {
        self.db(move |s| {
            Ok(s.with_conn(|c| {
                Ok(read::work_item(c, id)?.map(|item| {
                    let extras = wi::extras(c, id).ok().flatten().unwrap_or_default();
                    (item, extras)
                }))
            })?)
        })
        .await
    }

    /// Apply a work item answer in one transaction with its events (FR-7):
    /// a write's answer always derives them (they come out self-caused); a
    /// read like a sync pass would (after the baseline).
    async fn apply(
        &self,
        body: Vec<u8>,
        write_answer: bool,
    ) -> Result<(Option<Stored>, Vec<i64>, ChangeSet), WiError> {
        let wire: WireWorkItem = decode(&body)?;
        let now = self.clock.now_ms();
        self.db(move |s| {
            let me = my_id(s)?;
            let id = wire.id;
            let (result, changes) = collect_changes(|| {
                in_tx(s, &me, now, true, |em| {
                    if !write_answer {
                        em.enabled =
                            read::cursor(em.tx, "wi.hot", "")?.is_some_and(|c| c.baseline_done);
                    }
                    apply_work_items(em, vec![wire])?;
                    Ok(read::work_item(em.tx, id)?.map(|item| {
                        let extras = wi::extras(em.tx, id).ok().flatten().unwrap_or_default();
                        (item, extras)
                    }))
                })
            });
            result.map(|(stored, events)| (stored, events, changes))
        })
        .await
    }

    /// One item GET at user priority, applied (FR-8 re-read, FR-25, FR-30).
    async fn read_item(
        &self,
        project_id: &str,
        id: i64,
    ) -> Result<(Option<Stored>, Vec<i64>, ChangeSet), WiError> {
        let answer = self
            .call(patch::item_request(&self.org_url, project_id, id))
            .await
            .map_err(WiError::from_read)?;
        self.apply(answer.body, false).await
    }

    /// Send `write` (FR-5..FR-9): rebase on a newer stored revision when the
    /// edited field is unchanged there (else a conflict, no request), PATCH
    /// with `test /rev`, apply the answer; on any other 4xx re-read the item
    /// and decide: same revision → a rule (FR-9) or Azure DevOps' refusal;
    /// newer and my field untouched → resend once; touched → conflict.
    pub async fn send(&self, write: &WiWrite) -> Result<WiOutcome, WiError> {
        with_origin(Origin::You(write.change.action()), self.send_inner(write)).await
    }

    async fn send_inner(&self, write: &WiWrite) -> Result<WiOutcome, WiError> {
        write.change.validate()?;
        let id = write.id;
        let mut view = self
            .stored(id)
            .await?
            .ok_or_else(|| WiError::Local(format!("#{id} isn't stored")))?;
        let project_id = view.0.project_id.clone();
        let mut base_rev = write.base_rev;
        if view.0.rev > base_rev {
            match merge(&write.change, &view.0, &view.1) {
                Merge::Same => base_rev = view.0.rev,
                Merge::Done => return Ok(WiOutcome::Applied(Some(view.0))),
                Merge::Changed { theirs } => {
                    return Ok(WiOutcome::Conflict(
                        self.conflict(&write.change, &view, theirs).await?,
                    ));
                }
            }
        }
        let mut resent = false;
        loop {
            let request = patch::patch_request(&self.org_url, write, base_rev, &view.1.links)?;
            let refused = match self.call(request).await {
                Ok(answer) => {
                    let (stored, ..) = self.apply(answer.body, true).await?;
                    return Ok(WiOutcome::Applied(stored.map(|s| s.0)));
                }
                Err(GovError::Status {
                    status, message, ..
                }) if (400..500).contains(&status) && !matches!(status, 401 | 403 | 404) => {
                    (status, message)
                }
                Err(e) => return Err(WiError::from_write(e)),
            };
            let (status, message) = refused;
            let reread = with_origin(
                Origin::YouReread(write.change.action()),
                self.read_item(&project_id, id),
            )
            .await;
            let server = match reread {
                Ok((Some(server), ..)) => server,
                Ok((None, ..)) => return Err(WiError::Rejected { status, message }),
                Err(e) => return Err(e),
            };
            if server.0.rev == base_rev {
                if let Some(field) = message.as_deref().and_then(patch::parse_rule_error) {
                    let rule = self
                        .rule(
                            &server.0,
                            field,
                            message.clone().unwrap_or_default(),
                            write.change.action(),
                        )
                        .await?;
                    return Ok(WiOutcome::Rule(rule));
                }
                return Err(WiError::Rejected { status, message });
            }
            if server.0.rev < base_rev {
                return Err(WiError::Rejected { status, message });
            }
            match merge(&write.change, &server.0, &server.1) {
                Merge::Same if !resent => {
                    resent = true;
                    base_rev = server.0.rev;
                    view = server;
                }
                Merge::Same => return Err(WiError::Rejected { status, message }),
                Merge::Done => return Ok(WiOutcome::Applied(Some(server.0))),
                Merge::Changed { theirs } => {
                    return Ok(WiOutcome::Conflict(
                        self.conflict(&write.change, &server, theirs).await?,
                    ));
                }
            }
        }
    }

    async fn conflict(
        &self,
        change: &WiChange,
        (server, extras): &Stored,
        theirs: Option<String>,
    ) -> Result<Conflict, WiError> {
        let by = server.changed_by.clone();
        let assigned = matches!(change, WiChange::Assign { .. });
        let (by_s, theirs_s) = (by.clone(), theirs.clone());
        let (by_name, theirs) = self
            .db(move |s| {
                Ok(s.with_conn(|c| {
                    let theirs = match theirs_s {
                        Some(id) if assigned => Some(name(c, &id)),
                        other => other,
                    };
                    Ok((name(c, &by_s), theirs))
                })?)
            })
            .await?;
        Ok(Conflict {
            server_rev: server.rev,
            theirs,
            by,
            by_name,
            at: server.changed,
            rebased: change.rebased_on(server, extras),
        })
    }

    async fn rule(
        &self,
        server: &WorkItem,
        field: String,
        message: String,
        action: UserAction,
    ) -> Result<RuleViolation, WiError> {
        let meta = self
            .field_meta(&server.project_id, server.kind.as_str(), &field, action)
            .await
            .unwrap_or(None);
        Ok(RuleViolation {
            reference: meta.as_ref().map(|m| m.reference.clone()),
            kind: meta.map_or(FieldKind::Unknown, |m| m.kind),
            field,
            message,
            server_rev: server.rev,
        })
    }

    /// FR-25: post `markdown` as a Markdown comment, store it (raw source and
    /// format), then read the item once (a comment is a new revision).
    pub async fn comment(&self, id: i64, markdown: &str) -> Result<WiComment, WiError> {
        if markdown.trim().is_empty() {
            return Err(WiError::Invalid("Nothing to post".into()));
        }
        with_origin(Origin::You(UserAction::Comment), async {
            let project_id = self
                .stored(id)
                .await?
                .map(|(item, _)| item.project_id)
                .ok_or_else(|| WiError::Local(format!("#{id} isn't stored")))?;
            let answer = self
                .call(patch::post_comment_request(
                    &self.org_url,
                    &project_id,
                    id,
                    markdown,
                ))
                .await
                .map_err(WiError::from_write)?;
            let wire: WireWiComment = decode(&answer.body)?;
            let now = self.clock.now_ms();
            let comment = self
                .db(move |s| {
                    let me = my_id(s)?;
                    let (comment, _) = in_tx(s, &me, now, true, |em| {
                        write::upsert_person(em.tx, &wire.created_by.person(), false)?;
                        let mut cm = wire.into_model();
                        cm.work_item_id = id;
                        let cid = cm.id;
                        write::upsert_wi_comment(em.tx, &cm)?;
                        stored_comment(em.tx, id, cid)
                    })?;
                    Ok(comment)
                })
                .await?;
            // A stale rev would make the next edit a needless conflict.
            let reread = with_origin(
                Origin::YouReread(UserAction::Comment),
                self.read_item(&project_id, id),
            )
            .await;
            if let Err(e) = reread {
                log::debug!(target: "adoken_sync", "re-read after comment on #{id}: {e}");
            }
            Ok(comment)
        })
        .await
    }

    /// FR-30: one item GET (all fields and relations), then the comment
    /// pages (newest first, at most 10), at user priority. Runs while sync
    /// is paused; never touches the scheduler.
    pub async fn sync_item(&self, id: i64) -> Result<ItemSynced, WiError> {
        let Some(tracker) = &self.tracker else {
            return with_origin(
                Origin::You(UserAction::SyncItem(id)),
                self.sync_item_inner(id),
            )
            .await
            .map(|(synced, _)| synced);
        };
        let pass = tracker.begin_user_pass(Trigger::ItemSync(id));
        let result = pass.scope(self.sync_item_inner(id)).await;
        let host = self.org_url.host_str().unwrap_or_default().to_owned();
        match result {
            Ok((synced, changes)) => {
                for set in changes {
                    pass.add_changes(set);
                }
                pass.finish(Outcome::Ok, synced.events.len());
                Ok(synced)
            }
            Err(e) => {
                pass.finish(e.outcome(&host), 0);
                Err(e)
            }
        }
    }

    async fn sync_item_inner(&self, id: i64) -> Result<(ItemSynced, Vec<ChangeSet>), WiError> {
        let project_id = self
            .stored(id)
            .await?
            .map(|(item, _)| item.project_id)
            .ok_or_else(|| WiError::Local(format!("#{id} isn't stored")))?;
        let (stored, mut events, item_changes) = self.read_item(&project_id, id).await?;
        let mut changes = vec![item_changes];
        let mut comments: Vec<WireWiComment> = Vec::new();
        let mut token: Option<String> = None;
        let mut complete = true;
        for page in 0..COMMENT_PAGES {
            let request = patch::comments_request(&self.org_url, &project_id, id, token.as_deref());
            match self.call(request).await {
                Ok(answer) => {
                    let page_body: WireCommentList = decode(&answer.body)?;
                    comments.extend(page_body.comments);
                    token = page_body.continuation_token.filter(|t| !t.is_empty());
                }
                Err(GovError::Status { status: 404, .. }) => {
                    complete = false;
                    token = None;
                }
                Err(e) => return Err(WiError::from_read(e)),
            }
            if token.is_none() {
                break;
            }
            if page + 1 == COMMENT_PAGES {
                complete = false;
            }
        }
        if complete && stored.is_some() {
            let now = self.clock.now_ms();
            let (new, set) = self
                .db(move |s| {
                    let me = my_id(s)?;
                    let (result, set) = collect_changes(|| {
                        in_tx(s, &me, now, true, |em| {
                            em.enabled =
                                read::cursor(em.tx, "wi.hot", "")?.is_some_and(|c| c.baseline_done);
                            apply_wi_comments(em, id, comments)?;
                            write::set_wi_comments_due(em.tx, id, false)?;
                            Ok(())
                        })
                    });
                    result.map(|(_, new)| (new, set))
                })
                .await?;
            events.extend(new);
            changes.push(set);
        }
        Ok((
            ItemSynced {
                item: stored.map(|s| s.0),
                events,
            },
            changes,
        ))
    }

    /// States and transitions of a type (FR-11), fetched and cached for
    /// 24 h (FR-33). `action` is the user action that needed them.
    pub async fn type_meta(
        &self,
        project_id: &str,
        type_name: &str,
        action: UserAction,
    ) -> Result<TypeMeta, WiError> {
        with_origin(Origin::You(action), async {
            let answer = self
                .call(patch::type_request(&self.org_url, project_id, type_name))
                .await
                .map_err(WiError::from_read)?;
            let meta = meta::parse_type(&answer.body, self.clock.now_ms())
                .map_err(|e| WiError::Local(format!("response: {e}")))?;
            let (p, t, m) = (project_id.to_owned(), type_name.to_owned(), meta.clone());
            self.db(move |s| Ok(s.write(|tx| wi::put_type_meta(tx, &p, &t, &m))?))
                .await?;
            Ok(meta)
        })
        .await
    }

    /// A project's area and iteration trees in one call (FR-14), cached
    /// 24 h (FR-33).
    pub async fn class_tree(
        &self,
        project_id: &str,
        action: UserAction,
    ) -> Result<ClassTree, WiError> {
        with_origin(Origin::You(action), async {
            let answer = self
                .call(patch::class_tree_request(&self.org_url, project_id))
                .await
                .map_err(WiError::from_read)?;
            let tree = meta::parse_class_tree(&answer.body, self.clock.now_ms())
                .map_err(|e| WiError::Local(format!("response: {e}")))?;
            let (p, t) = (project_id.to_owned(), tree.clone());
            self.db(move |s| Ok(s.write(|tx| wi::put_class_tree(tx, &p, &t))?))
                .await?;
            Ok(tree)
        })
        .await
    }

    /// The field a rule error names (FR-9), by display or reference name:
    /// the type's fields come from cache (24 h) or one fetch; a field with no
    /// allowed values gets its kind from its definition (one more GET, then
    /// cached). `None` when the type has no such field.
    pub async fn field_meta(
        &self,
        project_id: &str,
        type_name: &str,
        field: &str,
        action: UserAction,
    ) -> Result<Option<FieldMeta>, WiError> {
        with_origin(Origin::You(action), async {
            let (p, t) = (project_id.to_owned(), type_name.to_owned());
            let now = self.clock.now_ms();
            let cached = self
                .db(move |s| Ok(s.with_conn(|c| wi::type_fields(c, &p, &t))?))
                .await?;
            let fields = match cached {
                Some((fields, at)) if !is_stale(at, now) => fields,
                _ => {
                    let answer = self
                        .call(patch::fields_request(&self.org_url, project_id, type_name))
                        .await
                        .map_err(WiError::from_read)?;
                    let listed = meta::parse_fields(&answer.body)
                        .map_err(|e| WiError::Local(format!("response: {e}")))?;
                    let (p, t) = (project_id.to_owned(), type_name.to_owned());
                    self.db(move |s| {
                        Ok(s.write(|tx| {
                            wi::put_type_fields(tx, &p, &t, &listed, now)?;
                            wi::type_fields(tx, &p, &t)
                        })?)
                    })
                    .await?
                    .map(|(f, _)| f)
                    .unwrap_or_default()
                }
            };
            let Some(mut found) = fields.into_iter().find(|f| {
                f.name.eq_ignore_ascii_case(field) || f.reference.eq_ignore_ascii_case(field)
            }) else {
                return Ok(None);
            };
            let (p, t, r) = (
                project_id.to_owned(),
                type_name.to_owned(),
                found.reference.clone(),
            );
            let resolved = self
                .db(move |s| Ok(s.with_conn(|c| wi::field_resolved(c, &p, &t, &r))?))
                .await?;
            if !resolved {
                let answer = self
                    .call(patch::field_request(&self.org_url, &found.reference))
                    .await
                    .map_err(WiError::from_read)?;
                found.kind = meta::parse_field_kind(&answer.body)
                    .map_err(|e| WiError::Local(format!("response: {e}")))?;
                let (p, t, f) = (project_id.to_owned(), type_name.to_owned(), found.clone());
                self.db(move |s| {
                    Ok(s.write(|tx| wi::set_field_kind(tx, &p, &t, &f.reference, &f.kind))?)
                })
                .await?;
            }
            Ok(Some(found))
        })
        .await
    }

    /// FR-13 directory search: active people and groups matching `text`.
    pub async fn search_people(
        &self,
        text: &str,
        action: UserAction,
    ) -> Result<Vec<Person>, WiError> {
        with_origin(Origin::You(action), async {
            let answer = self
                .call(patch::people_request(&self.org_url, text))
                .await
                .map_err(WiError::from_read)?;
            meta::parse_people(&answer.body).map_err(|e| WiError::Local(format!("response: {e}")))
        })
        .await
    }
}

fn decode<T: DeserializeOwned>(body: &[u8]) -> Result<T, WiError> {
    serde_json::from_slice(body).map_err(|e| WiError::Local(format!("response: {e}")))
}

fn my_id(store: &Store) -> Result<PersonId, WiError> {
    store
        .me()?
        .map(|p| p.id)
        .ok_or_else(|| WiError::Local("signed-in identity unknown".into()))
}
