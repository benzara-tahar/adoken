//! Local fake Azure DevOps REST server (wiremock) serving the demo dataset.
//! Loopback only; records every request with the virtual clock time;
//! answers any write-method request other than WIQL with 405 and records a
//! violation.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard};

use adoken_demo::Dataset;
use adoken_model::*;
use adoken_sync::ledger::NetLog;
use adoken_sync::wire::{format_time, parse_time};
use adoken_sync::{Clock, LoopbackTransport, ManualClock, Recording, Transport};
use serde_json::{Value, json};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub const ORG: &str = "contoso";

#[derive(Clone, Debug)]
pub struct Logged {
    pub at: Millis,
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    /// JSON body of read-only query POSTs (WIQL, `workitemsbatch`).
    pub body: Option<Value>,
}

impl Logged {
    pub fn param(&self, k: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug)]
pub struct Fault {
    pub path_contains: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub times: usize,
    /// Let this many matching requests through first.
    pub skip: usize,
}

pub struct State {
    pub clock: Arc<ManualClock>,
    pub me: Person,
    pub people: BTreeMap<PersonId, Person>,
    pub projects: Vec<Project>,
    pub repos: Vec<Repo>,
    pub prs: BTreeMap<i64, PullRequest>,
    pub threads: BTreeMap<i64, Vec<PrThread>>,
    /// Changed files of each PR's latest iteration vs its merge base, with
    /// real git blob ids (the demo's ids are re-keyed so fetched contents
    /// verify against them).
    pub pr_files: BTreeMap<i64, Vec<PrFileChange>>,
    pub iterations: BTreeMap<i64, Vec<PrIteration>>,
    pub push_actors: BTreeMap<(i64, String), PersonId>,
    /// `includeCommits=true` lists per iteration (spec 0017 FR-4): the
    /// commit ids and `hasMoreCommits`. Iterations without an entry list
    /// their source commit only.
    pub iteration_commits: BTreeMap<(i64, u32), (Vec<String>, bool)>,
    /// Blob contents by git object id.
    pub blobs: BTreeMap<String, Vec<u8>>,
    /// Largest changes page served, whatever `$top` asks (ADO's default
    /// page is 100).
    pub changes_page: usize,
    /// Work item + change sequence (reporting watermark).
    pub work_items: BTreeMap<i64, (WorkItem, u64)>,
    pub wi_transition_dates: BTreeMap<i64, (Option<Millis>, Option<Millis>)>,
    pub runs: BTreeMap<i64, PipelineRun>,
    pub pipelines: BTreeMap<i64, Pipeline>,
    /// Build timelines, served as `Stage` records (unordered).
    pub stages: BTreeMap<i64, Vec<RunStage>>,
    /// Work item discussions, served as HTML with mention anchors.
    pub wi_comments: BTreeMap<i64, Vec<WorkItemComment>>,
    pub seq: u64,
    /// Work items deleted since the dataset was loaded.
    pub deleted_wis: BTreeSet<i64>,
    pub revisions_page: usize,
    pub log: Vec<Logged>,
    pub violations: Vec<String>,
    pub faults: VecDeque<Fault>,
    /// Added to every response (e.g. X-RateLimit-*).
    pub headers: Vec<(String, String)>,
    /// `workitemtypes/{type}` states as `(name, category)`, by type name;
    /// types not listed answer the stock Agile states.
    pub type_states: BTreeMap<String, Vec<(String, String)>>,
}

#[derive(Clone)]
pub struct MockAdo {
    pub state: Arc<Mutex<State>>,
}

pub struct Harness {
    pub server: MockServer,
    pub ado: MockAdo,
    pub clock: Arc<ManualClock>,
    /// Every request the suites send through [`Self::transport`] (spec
    /// 0016 AC-B3: the recorder debug-asserts that each matches the route
    /// table and carries an origin).
    pub log: Arc<NetLog>,
}

impl Harness {
    pub async fn start(ds: &Dataset) -> Harness {
        let clock = Arc::new(ManualClock::new(ds.now));
        let ado = MockAdo::new(ds, clock.clone());
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ado.clone())
            .mount(&server)
            .await;
        let log = Arc::new(NetLog::new(clock.clone()));
        Harness {
            server,
            ado,
            clock,
            log,
        }
    }

    /// The loopback transport, recorded into [`Self::log`].
    pub fn transport(&self) -> Arc<dyn Transport> {
        Arc::new(Recording::new(
            LoopbackTransport::default(),
            self.log.clone(),
        ))
    }

    pub fn org_url(&self) -> url::Url {
        url::Url::parse(&format!("{}/{ORG}/", self.server.uri())).unwrap()
    }

    pub fn state(&self) -> MutexGuard<'_, State> {
        self.ado.state.lock()
    }
}

impl MockAdo {
    pub fn new(ds: &Dataset, clock: Arc<ManualClock>) -> Self {
        let mut seq = 0;
        let git_ids: BTreeMap<&str, String> = ds
            .blobs
            .iter()
            .map(|(id, text)| {
                (
                    id.as_str(),
                    adoken_sync::pr_files::git_blob_id(text.as_bytes()),
                )
            })
            .collect();
        let remap = |id: &Option<String>| {
            id.as_deref()
                .map(|i| git_ids.get(i).cloned().unwrap_or_else(|| i.to_owned()))
        };
        let mut work_items = BTreeMap::new();
        let mut sorted: Vec<&WorkItem> = ds.work_items.iter().collect();
        sorted.sort_by_key(|w| (w.changed, w.id));
        for w in sorted {
            seq += 1;
            work_items.insert(w.id, (w.clone(), seq));
        }
        let state = State {
            clock,
            me: ds.person(&ds.me).unwrap().clone(),
            people: ds
                .people
                .iter()
                .map(|p| (p.id.clone(), p.clone()))
                .collect(),
            projects: ds.projects.clone(),
            repos: ds.repos.clone(),
            prs: ds.prs.iter().map(|p| (p.id, p.clone())).collect(),
            threads: ds
                .pr_extras
                .iter()
                .map(|(id, e)| (*id, e.threads.clone()))
                .collect(),
            pr_files: ds
                .pr_extras
                .iter()
                .map(|(id, e)| {
                    let files = e
                        .files
                        .iter()
                        .map(|f| PrFileChange {
                            old_content: remap(&f.old_content),
                            new_content: remap(&f.new_content),
                            ..f.clone()
                        })
                        .collect();
                    (*id, files)
                })
                .collect(),
            iterations: ds
                .pr_extras
                .iter()
                .map(|(id, e)| (*id, e.iterations.clone()))
                .collect(),
            push_actors: BTreeMap::new(),
            iteration_commits: BTreeMap::new(),
            blobs: ds
                .blobs
                .iter()
                .map(|(id, text)| (git_ids[id.as_str()].clone(), text.as_bytes().to_vec()))
                .collect(),
            changes_page: 2_000,
            work_items,
            runs: ds.runs.iter().map(|r| (r.id, r.clone())).collect(),
            pipelines: ds.pipelines.iter().map(|p| (p.id, p.clone())).collect(),
            stages: ds.stages.clone(),
            wi_comments: ds.wi_comments.iter().fold(BTreeMap::new(), |mut m, c| {
                m.entry(c.work_item_id)
                    .or_insert_with(Vec::new)
                    .push(c.clone());
                m
            }),
            seq,
            deleted_wis: BTreeSet::new(),
            wi_transition_dates: BTreeMap::new(),
            revisions_page: 1000,
            log: Vec::new(),
            violations: Vec::new(),
            faults: VecDeque::new(),
            headers: Vec::new(),
            type_states: BTreeMap::new(),
        };
        MockAdo {
            state: Arc::new(Mutex::new(state)),
        }
    }
}

impl State {
    pub fn now(&self) -> Millis {
        self.clock.now_ms()
    }

    pub fn fail(
        &mut self,
        path_contains: &str,
        status: u16,
        headers: &[(&str, &str)],
        times: usize,
    ) {
        self.fail_after(0, path_contains, status, headers, times);
    }

    pub fn fail_after(
        &mut self,
        skip: usize,
        path_contains: &str,
        status: u16,
        headers: &[(&str, &str)],
        times: usize,
    ) {
        self.faults.push_back(Fault {
            skip,
            path_contains: path_contains.into(),
            status,
            headers: headers
                .iter()
                .map(|(a, b)| ((*a).into(), (*b).into()))
                .collect(),
            times,
        });
    }

    pub fn requests(&self, path_contains: &str) -> Vec<Logged> {
        self.log
            .iter()
            .filter(|l| l.path.contains(path_contains))
            .cloned()
            .collect()
    }

    pub fn touch_wi(&mut self, id: i64, f: impl FnOnce(&mut WorkItem)) {
        self.seq += 1;
        let now = self.now();
        let seq = self.seq;
        let (wi, s) = self.work_items.get_mut(&id).unwrap();
        f(wi);
        wi.rev += 1;
        wi.changed = now;
        *s = seq;
    }

    pub fn push_pr(&mut self, id: i64, commit: &str, actor: &str) {
        let now = self.now();
        self.prs.get_mut(&id).unwrap().last_source_commit = Some(commit.into());
        let iterations = self.iterations.entry(id).or_default();
        let next = iterations.iter().map(|it| it.id).max().unwrap_or(0) + 1;
        iterations.push(PrIteration {
            id: next,
            source_commit: commit.into(),
            created: now,
            description: None,
            commit_count: None,
            commits_truncated: false,
        });
        self.push_actors.insert((id, commit.into()), actor.into());
    }

    /// Delete a work item: a new revision with `System.IsDeleted = true`.
    pub fn delete_wi(&mut self, id: i64) {
        self.touch_wi(id, |_| {});
        self.deleted_wis.insert(id);
    }

    fn identity(&self, id: &str) -> Value {
        let p = self.people.get(id);
        json!({
            "id": id,
            "displayName": p.map(|p| p.display_name.as_str()).unwrap_or("Unknown"),
            "uniqueName": p.and_then(|p| p.unique_name.clone()),
            "isContainer": p.is_some_and(|p| p.is_group),
        })
    }

    fn project(&self, id: &str) -> Value {
        let p = self.projects.iter().find(|p| p.id == id || p.name == id);
        json!({ "id": p.map(|p| p.id.as_str()).unwrap_or(id), "name": p.map(|p| p.name.as_str()).unwrap_or("") })
    }

    fn repo_json(&self, r: &Repo) -> Value {
        json!({
            "id": r.id, "name": r.name, "defaultBranch": r.default_branch,
            "project": self.project(&r.project_id),
        })
    }

    fn pr_json(&self, pr: &PullRequest) -> Value {
        let repo = self.repos.iter().find(|r| r.id == pr.repo_id).unwrap();
        let mut reviewers = Vec::new();
        for r in &pr.reviewers {
            let mut v = self.identity(&r.person);
            v["vote"] = json!(r.vote.as_i8());
            v["isRequired"] = json!(r.is_required);
            v["hasDeclined"] = json!(r.has_declined);
            reviewers.push(v);
        }
        json!({
            "pullRequestId": pr.id,
            "repository": self.repo_json(repo),
            "title": pr.title,
            "description": pr.description.chars().take(400).collect::<String>(),
            "status": pr.status.as_str(),
            "isDraft": pr.is_draft,
            "createdBy": self.identity(&pr.author),
            "creationDate": format_time(pr.created),
            "closedDate": pr.closed.map(format_time),
            "sourceRefName": pr.source_branch,
            "targetRefName": pr.target_branch,
            "mergeStatus": pr.merge_status.as_str(),
            "lastMergeSourceCommit": pr.last_source_commit.as_ref().map(|c| json!({"commitId": c})),
            "reviewers": reviewers,
        })
    }

    fn thread_json(&self, t: &PrThread) -> Value {
        let ctx = t.anchor.as_ref().map(|a| {
            json!({
                "filePath": a.file_path,
                "rightFileStart": a.right.map(|r| json!({"line": r.start, "offset": 1})),
                "rightFileEnd": a.right.map(|r| json!({"line": r.end, "offset": 1})),
                "leftFileStart": a.left.map(|r| json!({"line": r.start, "offset": 1})),
                "leftFileEnd": a.left.map(|r| json!({"line": r.end, "offset": 1})),
            })
        });
        let comments: Vec<Value> = t
            .comments
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "parentCommentId": c.parent_id.unwrap_or(0),
                    "author": self.identity(&c.author),
                    "content": c.body,
                    "publishedDate": format_time(c.created),
                    "lastUpdatedDate": format_time(c.updated),
                    "commentType": if c.is_system { "system" } else { "text" },
                    "isDeleted": c.is_deleted,
                })
            })
            .collect();
        json!({
            "id": t.id,
            "status": t.status.as_str(),
            "threadContext": ctx,
            "pullRequestThreadContext": t.iteration.map(|i| json!({"iterationContext": {"firstComparingIteration": 1, "secondComparingIteration": i}})),
            "comments": comments,
            "lastUpdatedDate": format_time(t.updated),
        })
    }

    fn wi_json(&self, w: &WorkItem) -> Value {
        let project = self.projects.iter().find(|p| p.id == w.project_id).unwrap();
        let mut fields = serde_json::Map::new();
        let mut put = |k: &str, v: Value| {
            if !v.is_null() {
                fields.insert(k.into(), v);
            }
        };
        put("System.Id", json!(w.id));
        put("System.TeamProject", json!(project.name));
        put("System.WorkItemType", json!(w.kind.as_str()));
        put("System.Title", json!(w.title));
        put("System.State", json!(w.state));
        put(
            "System.AssignedTo",
            w.assigned_to
                .as_deref()
                .map(|a| self.identity(a))
                .unwrap_or(Value::Null),
        );
        put("System.CreatedBy", self.identity(&w.created_by));
        put("System.ChangedBy", self.identity(&w.changed_by));
        put("System.AreaPath", json!(w.area_path));
        put("System.IterationPath", json!(w.iteration_path));
        put("Microsoft.VSTS.Common.Priority", json!(w.priority));
        put("System.Tags", json!(w.tags.join("; ")));
        put("System.Parent", json!(w.parent_id));
        put("System.CommentCount", json!(w.comment_count));
        put("System.CreatedDate", json!(format_time(w.created)));
        put("System.ChangedDate", json!(format_time(w.changed)));
        if let Some((closed, changed)) = self.wi_transition_dates.get(&w.id) {
            put(
                "Microsoft.VSTS.Common.ClosedDate",
                json!(closed.map(format_time)),
            );
            put(
                "Microsoft.VSTS.Common.StateChangeDate",
                json!(changed.map(format_time)),
            );
        }
        put("System.Description", json!(w.description));
        let relations: Vec<Value> = w
            .linked_pr_ids
            .iter()
            .map(|id| json!({"rel": "ArtifactLink", "url": format!("vstfs:///Git/PullRequestId/{}%2Frepo%2F{id}", project.id)}))
            .collect();
        json!({ "id": w.id, "rev": w.rev, "fields": fields, "relations": relations })
    }

    /// Reporting revisions row, as dev.azure.com sends it: identities are
    /// `"Name <unique name>"` strings and only the requested fields appear.
    fn revision_json(&self, w: &WorkItem, fields: Option<&[&str]>) -> Value {
        let mut v = self.wi_json(w);
        v.as_object_mut().unwrap().remove("relations");
        let map = v["fields"].as_object_mut().unwrap();
        for k in ["System.AssignedTo", "System.CreatedBy", "System.ChangedBy"] {
            if let Some(identity) = map.get(k).cloned() {
                let name = identity["displayName"].as_str().unwrap_or_default();
                let text = match identity["uniqueName"].as_str() {
                    Some(u) => format!("{name} <{u}>"),
                    None => name.to_owned(),
                };
                map.insert(k.into(), json!(text));
            }
        }
        map.insert(
            "System.IsDeleted".into(),
            json!(self.deleted_wis.contains(&w.id)),
        );
        if let Some(fields) = fields {
            map.retain(|k, _| fields.contains(&k.as_str()));
        }
        v
    }

    fn build_json(&self, r: &PipelineRun) -> Value {
        let def = self.pipelines.get(&r.pipeline_id);
        json!({
            "id": r.id,
            "buildNumber": r.run_number,
            "status": r.status.as_str(),
            "result": r.result.map(|x| x.as_str()),
            "reason": r.reason.as_str(),
            "sourceBranch": r.branch,
            "sourceVersion": r.source_version,
            "requestedFor": r.requested_for.as_deref().map(|p| self.identity(p)),
            "queueTime": format_time(r.queued),
            "startTime": r.started.map(format_time),
            "finishTime": r.finish.map(format_time),
            "definition": { "id": r.pipeline_id, "name": r.pipeline_name, "path": def.map(|d| d.folder.as_str()).unwrap_or("\\") },
            "project": self.project(&r.project_id),
            "triggerInfo": r.pr_id.map(|id| json!({"pr.number": id.to_string()})).unwrap_or(json!({})),
        })
    }

    /// Stage records in reverse pipeline order (the real API sends records
    /// unordered), under one Checkpoint with `order: null` to be ignored.
    fn timeline_json(&self, run_id: i64) -> Value {
        let mut records: Vec<Value> = self
            .stages
            .get(&run_id)
            .map(|stages| {
                stages
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let (state, result) = match s.outcome {
                            StageOutcome::Running => ("inProgress", None),
                            StageOutcome::Pending => ("pending", None),
                            o => ("completed", Some(o.as_str())),
                        };
                        json!({
                            "id": format!("stage-{i}"), "parentId": null, "type": "Stage",
                            "name": s.name, "identifier": s.name.replace(' ', "_"),
                            "order": i + 1, "state": state, "result": result,
                            "startTime": s.started.map(format_time),
                            "finishTime": s.finished.map(format_time).unwrap_or_else(|| "0001-01-01T00:00:00".into()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        records.reverse();
        records.push(json!({"id": "cp", "parentId": "stage-0", "type": "Checkpoint", "name": "Checkpoint", "order": null, "state": "completed", "result": "succeeded"}));
        json!({"id": format!("timeline-{run_id}"), "changeId": 1, "records": records})
    }

    /// A work item comment as 7.1-preview.4 sends it: HTML text, mentions as
    /// `data-vss-mention` anchors plus `mentions[]`.
    fn wi_comment_json(&self, c: &WorkItemComment) -> Value {
        let mut html = String::from("<div>");
        let mut mentions = Vec::new();
        let mut rest = c.body.as_str();
        while let Some(i) = rest.find("@<") {
            html.push_str(&rest[..i]);
            let after = &rest[i + 2..];
            let end = after.find('>').unwrap();
            let id = &after[..end];
            let name = self
                .people
                .get(id)
                .map_or("Someone", |p| p.display_name.as_str());
            html.push_str(&format!(
                "<a href=\"#\" data-vss-mention=\"version:2.0,{}\">@{name}</a>",
                id.to_ascii_uppercase()
            ));
            mentions.push(json!({"artifactId": c.work_item_id.to_string(), "commentId": c.id, "mentionedArtifact": id, "type": "person"}));
            rest = &after[end + 1..];
        }
        html.push_str(rest);
        html.push_str("</div>");
        json!({
            "id": c.id, "workItemId": c.work_item_id, "version": 1, "format": "html",
            "text": html, "renderedText": "",
            "createdBy": self.identity(&c.author), "createdDate": format_time(c.created),
            "modifiedBy": self.identity(&c.author), "modifiedDate": format_time(c.updated),
            "mentions": mentions, "isDeleted": false,
        })
    }

    fn is_me(&self, who: &str) -> bool {
        who == self.me.id || Some(who) == self.me.unique_name.as_deref()
    }

    fn route(
        &mut self,
        method: &str,
        path: &str,
        q: &[(String, String)],
        body: &[u8],
    ) -> (u16, Value) {
        let param = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
        if segs.first() != Some(&ORG) {
            return (404, json!({"message": "unknown org"}));
        }
        let segs = &segs[1..];
        if method != "GET" {
            if method == "POST" && segs == ["_apis", "wit", "wiql"] {
                let query = serde_json::from_slice::<Value>(body).ok();
                let text = query
                    .as_ref()
                    .and_then(|v| v["query"].as_str())
                    .unwrap_or("")
                    .to_owned();
                return (200, self.wiql(&text));
            }
            // Read-only batch read; `errorPolicy: omit` answers `null` for
            // ids that don't exist (deleted) instead of failing the batch.
            if method == "POST" && segs == ["_apis", "wit", "workitemsbatch"] {
                let request = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
                let omit = request["errorPolicy"].as_str() == Some("omit");
                let ids: Vec<i64> = request["ids"]
                    .as_array()
                    .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                let mut value = Vec::new();
                for id in ids {
                    match self.work_items.get(&id) {
                        Some((w, _)) if !self.deleted_wis.contains(&id) => {
                            value.push(self.wi_json(w))
                        }
                        _ if omit => value.push(Value::Null),
                        _ => {
                            return (
                                404,
                                json!({"message": format!("TF401232: Work item {id} does not exist")}),
                            );
                        }
                    }
                }
                return (200, json!({"count": value.len(), "value": value}));
            }
            self.violations.push(format!("{method} {path}"));
            return (405, json!({"message": "read-only mock"}));
        }
        match segs {
            // Real connectionData is preview-only: `7.1` is a 400.
            ["_apis", "connectionData"]
                if !param("api-version").is_some_and(|v| v.contains("-preview")) =>
            {
                (
                    400,
                    json!({"typeKey": "VssInvalidPreviewVersionException", "message": "The requested version \"7.1\" of the resource is under preview."}),
                )
            }
            ["_apis", "connectionData"] => (
                200,
                json!({
                    "authenticatedUser": {
                        "id": self.me.id,
                        "providerDisplayName": self.me.display_name,
                        "properties": {"Account": {"$type": "System.String", "$value": self.me.unique_name}},
                    },
                    "instanceId": "00000000-0000-0000-0000-0000c0ffee00",
                }),
            ),
            ["_apis", "projects"] => (
                200,
                json!({"count": self.projects.len(), "value": self.projects.iter().map(|p| json!({"id": p.id, "name": p.name})).collect::<Vec<_>>()}),
            ),
            // Project by id or name (a pinned item in an unselected project).
            ["_apis", "projects", p] => {
                let p = p.replace("%20", " ");
                match self.projects.iter().find(|x| x.id == p || x.name == p) {
                    Some(x) => (
                        200,
                        json!({"id": x.id, "name": x.name, "state": "wellFormed"}),
                    ),
                    None => (
                        404,
                        json!({"message": format!("TF200016: The project {p} does not exist.")}),
                    ),
                }
            }
            [p, "_apis", "git", "repositories"] => {
                let v: Vec<Value> = self
                    .repos
                    .iter()
                    .filter(|r| r.project_id == *p)
                    .map(|r| self.repo_json(r))
                    .collect();
                (200, json!({"count": v.len(), "value": v}))
            }
            [p, "_apis", "git", "pullrequests"] => {
                let status = param("searchCriteria.status").unwrap_or("active");
                let top: usize = param("$top").and_then(|v| v.parse().ok()).unwrap_or(101);
                let skip: usize = param("$skip").and_then(|v| v.parse().ok()).unwrap_or(0);
                let mut prs: Vec<&PullRequest> = self
                    .prs
                    .values()
                    .filter(|pr| {
                        pr.project_id == *p && (status == "all" || pr.status.as_str() == status)
                    })
                    .collect();
                prs.sort_by_key(|pr| std::cmp::Reverse(pr.id));
                let v: Vec<Value> = prs
                    .into_iter()
                    .skip(skip)
                    .take(top)
                    .map(|pr| self.pr_json(pr))
                    .collect();
                (200, json!({"count": v.len(), "value": v}))
            }
            [_, "_apis", "git", "pullrequests", id] | ["_apis", "git", "pullrequests", id] => {
                match id.parse::<i64>().ok().and_then(|id| self.prs.get(&id)) {
                    Some(pr) => (200, self.pr_json(pr)),
                    None => (404, json!({"message": "no such PR"})),
                }
            }
            [
                _p,
                "_apis",
                "git",
                "repositories",
                _r,
                "pullRequests",
                id,
                "threads",
            ] => {
                let id: i64 = id.parse().unwrap_or(0);
                let v: Vec<Value> = self
                    .threads
                    .get(&id)
                    .map(|ts| ts.iter().map(|t| self.thread_json(t)).collect())
                    .unwrap_or_default();
                (200, json!({"count": v.len(), "value": v}))
            }
            [
                _p,
                "_apis",
                "git",
                "repositories",
                _r,
                "pullRequests",
                id,
                "iterations",
            ] => {
                let id: i64 = id.parse().unwrap_or(0);
                if !self.prs.contains_key(&id) {
                    return (404, json!({"message": "no such PR"}));
                }
                let commits = param("includeCommits") == Some("true");
                let v: Vec<Value> = self
                    .iterations
                    .get(&id)
                    .map(|its| {
                        its.iter()
                            .map(|it| self.iteration_json(id, it, commits))
                            .collect()
                    })
                    .unwrap_or_default();
                (200, json!({"count": v.len(), "value": v}))
            }
            [
                _p,
                "_apis",
                "git",
                "repositories",
                _r,
                "pullRequests",
                id,
                "iterations",
                _it,
                "changes",
            ] => {
                let id: i64 = id.parse().unwrap_or(0);
                let top: usize = param("$top").and_then(|v| v.parse().ok()).unwrap_or(100);
                let skip: usize = param("$skip").and_then(|v| v.parse().ok()).unwrap_or(0);
                let files = self.pr_files.get(&id).cloned().unwrap_or_default();
                let page = top.min(self.changes_page);
                let entries: Vec<Value> = files
                    .iter()
                    .enumerate()
                    .skip(skip)
                    .take(page)
                    .map(|(ix, f)| change_json(ix, f))
                    .collect();
                let next = skip + entries.len();
                let mut body = json!({"changeEntries": entries});
                if next < files.len() {
                    body["nextSkip"] = json!(next);
                    body["nextTop"] = json!(page);
                }
                (200, body)
            }
            ["_apis", "wit", "workitems"] => {
                let v: Vec<Value> = param("ids")
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|id| id.parse::<i64>().ok())
                    .filter_map(|id| self.work_items.get(&id))
                    .map(|(w, _)| self.wi_json(w))
                    .collect();
                (200, json!({"count": v.len(), "value": v}))
            }
            [p, "_apis", "wit", "reporting", "workitemrevisions"] => {
                let after: u64 = param("continuationToken")
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0);
                let start = param("startDateTime").and_then(parse_time).unwrap_or(0);
                let mut items: Vec<&(WorkItem, u64)> = self
                    .work_items
                    .values()
                    .filter(|(w, s)| w.project_id == *p && *s > after && w.changed >= start)
                    .collect();
                items.sort_by_key(|(_, s)| *s);
                let total = items.len();
                let page: Vec<&(WorkItem, u64)> =
                    items.into_iter().take(self.revisions_page).collect();
                let token = page.last().map(|(_, s)| *s).unwrap_or(after.max(self.seq));
                let fields: Option<Vec<&str>> = param("fields").map(|f| f.split(',').collect());
                let values: Vec<Value> = page
                    .iter()
                    .map(|(w, _)| self.revision_json(w, fields.as_deref()))
                    .collect();
                (
                    200,
                    json!({"values": values, "continuationToken": token.to_string(), "isLastBatch": total <= self.revisions_page}),
                )
            }
            [p, "_apis", "build", "definitions", id] => {
                let id = id.parse::<i64>().unwrap_or(0);
                match self.pipelines.get(&id).filter(|def| def.project_id == *p) {
                    Some(def) => (
                        200,
                        json!({
                            "id": def.id, "name": def.name, "path": def.folder,
                            "project": self.project(&def.project_id),
                        }),
                    ),
                    None => (404, json!({"message": "no such definition"})),
                }
            }
            [p, "_apis", "build", "builds"] => {
                let statuses: Vec<&str> = param("statusFilter")
                    .map(|s| s.split(',').collect())
                    .unwrap_or_default();
                let who = param("requestedFor");
                let pr_only = param("reasonFilter") == Some("pullRequest");
                let min = param("minTime").and_then(parse_time);
                let max = param("maxTime").and_then(parse_time);
                let top: usize = param("$top")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(usize::MAX);
                let definitions: Option<Vec<i64>> = param("definitions")
                    .map(|d| d.split(',').filter_map(|id| id.parse().ok()).collect());
                let per_definition: Option<usize> =
                    param("maxBuildsPerDefinition").and_then(|v| v.parse().ok());
                let mut runs: Vec<&PipelineRun> = self
                    .runs
                    .values()
                    .filter(|r| r.project_id == *p)
                    .filter(|r| statuses.is_empty() || statuses.contains(&r.status.as_str()))
                    .filter(|r| {
                        who.is_none_or(|w| {
                            r.requested_for.as_deref().is_some_and(|id| {
                                id == w
                                    || self.people.get(id).and_then(|p| p.unique_name.as_deref())
                                        == Some(w)
                            })
                        })
                    })
                    .filter(|r| !pr_only || r.reason == RunReason::PullRequest)
                    .filter(|r| min.is_none_or(|m| r.finish.is_some_and(|f| f >= m)))
                    .filter(|r| max.is_none_or(|m| r.finish.is_some_and(|f| f <= m)))
                    .filter(|r| {
                        definitions
                            .as_ref()
                            .is_none_or(|d| d.contains(&r.pipeline_id))
                    })
                    .collect();
                runs.sort_by_key(|r| (r.finish.unwrap_or(r.queued), r.id));
                if param("queryOrder") == Some("finishTimeDescending") {
                    runs.reverse();
                }
                if let Some(n) = per_definition {
                    // Newest queued first per definition. [INFERENCE] which
                    // run the real API keeps is what 0014 AC-9 records.
                    runs.sort_by_key(|r| std::cmp::Reverse((r.queued, r.id)));
                    let mut kept: BTreeMap<i64, usize> = BTreeMap::new();
                    runs.retain(|r| {
                        let count = kept.entry(r.pipeline_id).or_default();
                        *count += 1;
                        *count <= n
                    });
                }
                let v: Vec<Value> = runs
                    .into_iter()
                    .take(top)
                    .map(|r| self.build_json(r))
                    .collect();
                (200, json!({"count": v.len(), "value": v}))
            }
            [_p, "_apis", "build", "builds", id, "timeline"] => {
                let id: i64 = id.parse().unwrap_or(0);
                if !self.runs.contains_key(&id) {
                    return (404, json!({"message": "no such build"}));
                }
                (200, self.timeline_json(id))
            }
            [_p, "_apis", "wit", "workItems", id, "comments"] => {
                if param("api-version") != Some("7.1-preview.4") {
                    return (400, json!({"message": "comments are preview-only"}));
                }
                let id: i64 = id.parse().unwrap_or(0);
                let comments: Vec<Value> = self
                    .wi_comments
                    .get(&id)
                    .map(|cs| cs.iter().rev().map(|c| self.wi_comment_json(c)).collect())
                    .unwrap_or_default();
                (
                    200,
                    json!({"totalCount": comments.len(), "count": comments.len(), "comments": comments}),
                )
            }
            [_p, "_apis", "wit", "workitemtypes", t] => {
                let t = t.replace("%20", " ");
                let stock = [
                    ("New", "Proposed"),
                    ("Active", "InProgress"),
                    ("Doing", "InProgress"),
                    ("Resolved", "Resolved"),
                    ("Closed", "Completed"),
                    ("Removed", "Removed"),
                ]
                .map(|(n, c)| (n.to_owned(), c.to_owned()))
                .to_vec();
                let states = self.type_states.get(&t).cloned().unwrap_or(stock);
                (
                    200,
                    json!({
                        "name": t,
                        "states": states
                            .iter()
                            .map(|(name, category)| json!({"name": name, "color": "b2b2b2", "category": category}))
                            .collect::<Vec<_>>(),
                    }),
                )
            }
            [_p, "_apis", "build", "builds", id] => {
                match id.parse::<i64>().ok().and_then(|id| self.runs.get(&id)) {
                    Some(r) => (200, self.build_json(r)),
                    None => (404, json!({"message": "no such build"})),
                }
            }
            _ => (
                404,
                json!({"message": format!("mock: no route for {path}")}),
            ),
        }
    }

    /// Supports the sync engine's "mine changed since" query shape.
    fn wiql(&self, text: &str) -> Value {
        let since = text
            .split("[System.ChangedDate] > '")
            .nth(1)
            .and_then(|r| r.split('\'').next())
            .and_then(parse_time)
            .unwrap_or(0);
        let who = text
            .split("[System.AssignedTo] = '")
            .nth(1)
            .and_then(|r| r.split('\'').next())
            .unwrap_or("");
        let projects = text
            .split("AND [System.TeamProject] IN (")
            .nth(1)
            .and_then(|tail| tail.split(')').next())
            .map(|names| {
                names
                    .split(", ")
                    .map(|name| name.trim_matches('\'').replace("''", "'"))
                    .collect::<Vec<_>>()
            });
        let mut items: Vec<&WorkItem> = self
            .work_items
            .values()
            .map(|(w, _)| w)
            .filter(|w| w.changed > since)
            .filter(|w| {
                projects.as_ref().is_none_or(|names| {
                    self.projects
                        .iter()
                        .any(|p| p.id == w.project_id && names.contains(&p.name))
                })
            })
            .filter(|w| {
                w.assigned_to
                    .as_deref()
                    .is_some_and(|a| a == who || self.is_me(a) && self.is_me(who))
                    || self.is_me(&w.created_by) && self.is_me(who)
            })
            .collect();
        items.sort_by_key(|w| (w.changed, w.id));
        json!({"workItems": items.iter().map(|w| json!({"id": w.id})).collect::<Vec<_>>()})
    }
}

/// One `changeEntries[]` item as live Azure DevOps sends it: upper-case
/// object ids, `originalObjectId` only for edits/renames.
fn change_json(ix: usize, f: &PrFileChange) -> Value {
    let mut item = json!({"path": f.path});
    if let Some(new) = &f.new_content {
        item["objectId"] = json!(new.to_ascii_uppercase());
    }
    if let Some(old) = &f.old_content {
        let key = if f.new_content.is_some() {
            "originalObjectId"
        } else {
            "objectId"
        };
        item[key] = json!(old.to_ascii_uppercase());
    }
    let mut entry = json!({
        "changeTrackingId": f.change_tracking_id.unwrap_or(ix as u32 + 1),
        "changeId": ix + 1,
        "item": item,
        "changeType": f.change_type.as_str(),
    });
    if let Some(original) = &f.original_path {
        entry["originalPath"] = json!(original);
    }
    entry
}

/// Fake commit ids per PR: the merge base differs from the target tip, as
/// after the target branch moved on.
pub fn merge_base(pr: i64) -> String {
    adoken_sync::pr_files::git_blob_id(format!("base:{pr}").as_bytes())
}

impl State {
    fn iteration_json(&self, pr: i64, it: &PrIteration, include_commits: bool) -> Value {
        let mut v = json!({
            "id": it.id,
            "description": it.description,
            "createdDate": format_time(it.created),
            "push": self.push_actors.get(&(pr, it.source_commit.clone())).map(|actor| json!({
                "pushedBy": self.identity(actor),
            })),
            "sourceRefCommit": {"commitId": it.source_commit},
            "targetRefCommit": {"commitId": adoken_sync::pr_files::git_blob_id(format!("target:{pr}").as_bytes())},
            "commonRefCommit": {"commitId": merge_base(pr)},
        });
        if include_commits {
            let (ids, more) = self
                .iteration_commits
                .get(&(pr, it.id))
                .cloned()
                .unwrap_or_else(|| (vec![it.source_commit.clone()], false));
            v["commits"] = ids
                .iter()
                .map(|id| json!({"commitId": id, "comment": "change"}))
                .collect();
            if more {
                v["hasMoreCommits"] = json!(true);
            }
        }
        v
    }
}

impl Respond for MockAdo {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        // Tools on the dev machine probe new localhost ports (seen: `HEAD /`,
        // `Host: localhost:<port>`, no User-Agent). adoken never sends HEAD
        // (no HEAD call exists in crates/), so ignoring exactly `HEAD /` keeps
        // the read-only and timing assertions about adoken's own requests.
        if req.method.as_str() == "HEAD" && req.url.path() == "/" {
            return ResponseTemplate::new(404);
        }
        let mut s = self.state.lock();
        let query: Vec<(String, String)> = req
            .url
            .query_pairs()
            .map(|(a, b)| (a.into_owned(), b.into_owned()))
            .collect();
        let path = req.url.path().to_owned();
        let at = s.now();
        s.log.push(Logged {
            at,
            method: req.method.to_string(),
            path: path.clone(),
            query: query.clone(),
            body: serde_json::from_slice(&req.body).ok(),
        });
        let hit = s
            .faults
            .iter()
            .position(|f| path.contains(&f.path_contains));
        if let Some(i) = hit
            && s.faults[i].skip > 0
        {
            s.faults[i].skip -= 1;
        } else if let Some(i) = hit {
            let f = s.faults[i].clone();
            if f.times <= 1 {
                s.faults.remove(i);
            } else {
                s.faults[i].times -= 1;
            }
            let mut t = ResponseTemplate::new(f.status)
                .set_body_json(json!({"message": "TF400733: Request was blocked due to exceeding usage of resource 'Core' in namespace 'User'."}));
            for (k, v) in &f.headers {
                t = t.insert_header(k.as_str(), v.as_str());
            }
            return t;
        }
        if let Some(id) = path
            .split_once("/blobs/")
            .map(|(_, id)| id.to_ascii_lowercase())
        {
            return match s.blobs.get(&id) {
                Some(bytes) => ResponseTemplate::new(200)
                    .set_body_raw(bytes.clone(), "application/octet-stream"),
                None => {
                    ResponseTemplate::new(404).set_body_json(json!({"message": "no such blob"}))
                }
            };
        }
        let (status, body) = s.route(req.method.as_ref(), &path, &query, &req.body);
        let mut t = ResponseTemplate::new(status).set_body_json(body);
        for (k, v) in &s.headers {
            t = t.insert_header(k.as_str(), v.as_str());
        }
        t
    }
}
