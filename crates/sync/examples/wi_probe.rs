//! Spec 0015 AC-28 real-org probe: work item writes, comments, metadata and
//! directory search through the production `WiWriter` path, on two work
//! items the probe creates itself, with a request ledger.
//!
//! ```sh
//! ADOKEN_PROBE_ORGS=my-scratch-org \
//!   cargo run -p adoken-sync --example wi_probe -- --org my-scratch-org
//! cargo run -p adoken-sync --example wi_probe -- --dry-run
//! ```
//!
//! Safety:
//! - `--org` must be listed in `ADOKEN_PROBE_ORGS` ([`ALLOWED_ORGS_ENV`],
//!   comma-separated): a scratch org of your own, safe for destructive
//!   tests. Anything else (and any org when the variable is unset) is
//!   refused before any request. The PAT comes from the Keychain (`adoken_ado::PAT_SERVICE`,
//!   that org's item; a macOS prompt may appear). Nothing secret is printed.
//! - Only items the probe creates (`adoken probe <timestamp> A|B`, task type,
//!   in the org's single project) are touched. They end Removed (or deleted
//!   when the type has no Removed state). Nothing else is written.
//! - Output is Markdown on stdout: no PAT, no payload text, names templated
//!   by the ledger's route table. The one URL printed is the commented item's
//!   web page, for the owner to check the Markdown rendering.
//! - `--dry-run` runs the whole flow against an in-process fake Azure DevOps
//!   (no network, no Keychain).

use std::collections::BTreeMap;
use std::sync::Arc;

use adoken_ado::{AuthProvider, Keychain, Organization, SecretStore};
use adoken_model::WorkItemType;
use adoken_model::{LinkKind, NodeKind, Person, Project, StateCategory, TextFormat, WorkItem};
use adoken_store::{Store, read, wi as store_wi, write};
use adoken_sync::attribution::{self, Attribution, Origin, UserAction};
use adoken_sync::ledger::{Entry, NetLog, Status};
use adoken_sync::transport::{ApiResponse, RateHeaders, TransportError};
use adoken_sync::wi::{WiChange, WiOutcome, WiWrite, patch};
use adoken_sync::wire::{ConnectionData, format_time};
use adoken_sync::{
    ApiRequest, Clock, GovError, Governor, Recording, SystemClock, Tier, Transport, WiWriter,
};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use serde_json::{Map, Value, json};
use url::Url;

/// The orgs this probe may write to: comma-separated, opt-in per machine.
const ALLOWED_ORGS_ENV: &str = "ADOKEN_PROBE_ORGS";

fn allowed_orgs() -> Vec<String> {
    std::env::var(ALLOWED_ORGS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}
const DRY_ORG: &str = "https://dev.azure.com/probe/";

/// The token preflight's verdicts (printed under "## Token").
const REJECTED: &str = "token rejected (401 on read)";
const NO_WORK_READ: &str = "token valid, but can't read work items (401) — replace the PAT with one that has vso.work_write";
const NO_WRITE_SCOPE: &str = "token valid for reads, lacks Work Items (Read & write) — replace the PAT with one that has vso.work_write";
const OK: &str = "ok";

/// Print `why` as the token verdict and stop before any write.
fn verdict(why: &str) -> Res<()> {
    println!("\n## Token\n{why}");
    Err(why.into())
}

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Res<()> {
    attribution::scope(Attribution::new(Origin::Startup), run()).await
}

/// Item creation and deletion: probe-only calls, not app code (the app
/// never creates or deletes work items).
trait Backend: Send + Sync {
    fn create<'a>(&'a self, project: &'a str, ops: Value) -> BoxFuture<'a, Res<Value>>;
    fn delete<'a>(&'a self, project: &'a str, id: i64) -> BoxFuture<'a, Res<u16>>;
}

struct Live {
    org_url: Url,
    auth: Arc<dyn AuthProvider>,
    http: reqwest::Client,
}

impl Live {
    async fn send(&self, mut req: reqwest::Request) -> Res<(u16, Vec<u8>)> {
        self.auth
            .authorize(&mut req)
            .await
            .map_err(|e| format!("auth: {e}"))?;
        let resp = self.http.execute(req).await?;
        let status = resp.status().as_u16();
        Ok((status, resp.bytes().await?.to_vec()))
    }
}

impl Backend for Live {
    fn create<'a>(&'a self, project: &'a str, ops: Value) -> BoxFuture<'a, Res<Value>> {
        Box::pin(async move {
            let url = self.org_url.join(&format!(
                "{project}/_apis/wit/workitems/$Task?api-version=7.1"
            ))?;
            let req = self
                .http
                .post(url)
                .header("Content-Type", "application/json-patch+json")
                .body(serde_json::to_vec(&ops)?)
                .build()?;
            let (status, body) = self.send(req).await?;
            // The app's own reading of a 401 on a work item write (FR-7):
            // the token lacks `vso.work_write`.
            if status == 401 || status == 403 {
                return Err(NO_WRITE_SCOPE.into());
            }
            if status != 200 {
                return Err(format!("create answered HTTP {status}").into());
            }
            Ok(serde_json::from_slice(&body)?)
        })
    }

    fn delete<'a>(&'a self, project: &'a str, id: i64) -> BoxFuture<'a, Res<u16>> {
        Box::pin(async move {
            let url = self.org_url.join(&format!(
                "{project}/_apis/wit/workitems/{id}?api-version=7.1"
            ))?;
            let req = self.http.delete(url).build()?;
            Ok(self.send(req).await?.0)
        })
    }
}

struct Findings {
    rows: Vec<(String, String)>,
}

impl Findings {
    fn add(&mut self, what: &str, value: impl Into<String>) {
        let value = value.into();
        eprintln!("- {what}: {value}");
        self.rows.push((what.to_owned(), value));
    }
}

async fn run() -> Res<()> {
    let args: Vec<String> = std::env::args().collect();
    let dry = args.iter().any(|a| a == "--dry-run");
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let log = Arc::new(NetLog::new(clock.clone()));
    let (org_url, transport, backend): (Url, Arc<dyn Transport>, Arc<dyn Backend>) = if dry {
        // `--read-only-token`: a token without `vso.work_write`.
        let fake = Arc::new(Fake::new(!args.iter().any(|a| a == "--read-only-token")));
        (
            Url::parse(DRY_ORG)?,
            Arc::new(Recording::new(FakeTransport(fake.clone()), log.clone())),
            fake,
        )
    } else {
        let name = args
            .iter()
            .position(|a| a == "--org")
            .and_then(|i| args.get(i + 1))
            .ok_or("--org <name> is required (or --dry-run)")?;
        let allowed = allowed_orgs();
        if !allowed.iter().any(|a| a.eq_ignore_ascii_case(name)) {
            return Err(format!(
                "refusing org {name:?}: wi_probe only writes to the orgs in \
                 {ALLOWED_ORGS_ENV} ({allowed:?}); set it to a scratch org of your own"
            )
            .into());
        }
        let org = Organization::parse(name)?;
        eprintln!("reading the PAT from the Keychain (a macOS prompt may appear)…");
        let pat = Keychain::new()
            .load(&org)?
            .ok_or("no PAT in the Keychain for that org")?;
        let auth: Arc<dyn AuthProvider> = Arc::new(adoken_ado::PatAuth::new(pat, None));
        let http = adoken_ado::http::Http::new();
        let base = org.base_url();
        (
            base.clone(),
            Arc::new(Recording::new(
                adoken_sync::transport::OptionalAdoTransport {
                    http,
                    auth: auth.clone(),
                },
                log.clone(),
            )),
            Arc::new(Live {
                org_url: base,
                auth,
                http: reqwest::Client::new(),
            }),
        )
    };
    let gov = Arc::new(Governor::new(clock.clone()));
    let get = |path: &str, query: &[(&str, &str)], version: &str| -> Res<ApiRequest> {
        let mut url = org_url.join(path)?;
        url.query_pairs_mut()
            .extend_pairs(query)
            .append_pair("api-version", version);
        Ok(ApiRequest::get(url))
    };
    let send = |req: ApiRequest| {
        let (gov, transport) = (gov.clone(), transport.clone());
        async move { gov.send_optional(transport.as_ref(), Tier::User, req).await }
    };

    println!(
        "# wi_probe — spec 0015 AC-28{}",
        if dry { " (dry run, fake org)" } else { "" }
    );
    // ── token preflight (reads only) ────────────────────────────────────
    let conn = match send(get("_apis/connectionData", &[], "7.1-preview")?).await {
        Ok(answer) => answer,
        Err(GovError::Status { status: 401, .. }) => return verdict(REJECTED),
        Err(e) => return Err(format!("connectionData: {e}").into()),
    };
    let conn: ConnectionData = serde_json::from_slice(&conn.body)?;
    let me = Person {
        id: conn.authenticated_user.id.to_ascii_lowercase(),
        display_name: conn.authenticated_user.provider_display_name.clone(),
        unique_name: conn.authenticated_user.account().map(str::to_owned),
        is_group: false,
        image_url: None,
    };
    #[derive(serde::Deserialize)]
    struct Projects {
        value: Vec<Project>,
    }
    let projects: Projects =
        serde_json::from_slice(&send(get("_apis/projects", &[], "7.1")?).await?.body)?;
    let [project] = projects.value.as_slice() else {
        return Err(format!(
            "expected exactly one project, found {}: refusing",
            projects.value.len()
        )
        .into());
    };
    match send(get(
        &format!("{}/_apis/wit/workitemtypes/Task", project.id),
        &[],
        "7.1",
    )?)
    .await
    {
        Ok(_) => {}
        Err(GovError::Status { status: 401, .. }) => return verdict(NO_WORK_READ),
        Err(e) => return Err(format!("work item read probe: {e}").into()),
    }
    let tmp = tempfile::tempdir()?;
    let store = Store::open(tmp.path().join("adoken.sqlite"))?;
    store.write(|tx| {
        write::upsert_person(tx, &me, true)?;
        write::upsert_project(tx, project)
    })?;
    let writer = WiWriter::new(
        store.clone(),
        transport.clone(),
        gov.clone(),
        clock.clone(),
        org_url.clone(),
    );

    // ── two items of the probe's own ────────────────────────────────────
    let stamp = format_time(clock.now_ms());
    let html = "<div>adoken probe description</div><ul><li>one</li><li>two</li></ul>";
    let md = "## adoken probe\n\n- one\n- two";
    let title = |s: &str| format!("adoken probe {stamp} {s}");
    let created_a = match backend
        .create(
            &project.id,
            json!([
                {"op": "add", "path": "/fields/System.Title", "value": title("A")},
                {"op": "add", "path": "/fields/System.Description", "value": html},
            ]),
        )
        .await
    {
        Ok(created) => created,
        Err(e) if e.to_string() == NO_WRITE_SCOPE => return verdict(NO_WRITE_SCOPE),
        Err(e) => return Err(e),
    };
    let created_b = backend
        .create(
            &project.id,
            json!([
                {"op": "add", "path": "/fields/System.Title", "value": title("B")},
                {"op": "add", "path": "/fields/System.Description", "value": md},
                {"op": "add", "path": "/multilineFieldsFormat/System.Description", "value": "Markdown"},
            ]),
        )
        .await?;
    println!("\n## Token\n{OK}");
    let (a, b) = (id_of(&created_a)?, id_of(&created_b)?);
    let mut f = Findings { rows: Vec::new() };
    f.add("created", format!("#{a} (HTML), #{b} (Markdown)"));
    for id in [a, b] {
        seed_row(&store, id, &project.id, &me.id)?;
        writer.sync_item(id).await?;
    }
    let result = steps(&writer, &store, &send, &get, &project.id, &me, a, b, &mut f).await;
    if let Err(e) = &result {
        f.add("step failed", e.to_string());
    }

    // ── cleanup: Removed, else delete ───────────────────────────────────
    let meta = writer
        .type_meta(&project.id, "Task", UserAction::ChangeState)
        .await?;
    let removed = meta
        .states
        .iter()
        .find(|s| s.category == StateCategory::Removed)
        .map(|s| s.name.clone());
    for id in [a, b] {
        let outcome = match &removed {
            Some(state) => {
                let item = stored(&store, id)?;
                let write = WiWrite {
                    id,
                    base_rev: item.rev,
                    change: WiChange::State {
                        to: state.clone(),
                        base: item.state.clone(),
                    },
                    extra: Vec::new(),
                };
                match writer.send(&write).await {
                    Ok(WiOutcome::Applied(_)) => format!("#{id} → {state}"),
                    other => format!(
                        "#{id} not removed ({other:?}); deleted: HTTP {}",
                        backend.delete(&project.id, id).await?
                    ),
                }
            }
            None => format!(
                "#{id} deleted (no Removed state): HTTP {}",
                backend.delete(&project.id, id).await?
            ),
        };
        f.add("cleanup", outcome);
    }

    println!("\n## Findings (record in spec 0015 §5)");
    println!("| finding | value |\n|---|---|");
    for (what, value) in &f.rows {
        println!("| {what} | {value} |");
    }
    print_endpoints(&log.entries());
    result
}

#[allow(clippy::too_many_arguments)]
async fn steps<S, SF, G>(
    writer: &WiWriter,
    store: &Store,
    send: &S,
    get: &G,
    project: &str,
    me: &Person,
    a: i64,
    b: i64,
    f: &mut Findings,
) -> Res<()>
where
    S: Fn(ApiRequest) -> SF,
    SF: Future<Output = Result<ApiResponse, GovError>>,
    G: Fn(&str, &[(&str, &str)], &str) -> Res<ApiRequest>,
{
    let edit = |id: i64, change: WiChange| -> Res<WiWrite> {
        Ok(WiWrite {
            id,
            base_rev: stored(store, id)?.rev,
            change,
            extra: Vec::new(),
        })
    };
    let outcome = |o: Result<WiOutcome, adoken_sync::WiError>| match o {
        Ok(WiOutcome::Applied(Some(i))) => format!("applied, rev {}", i.rev),
        Ok(other) => format!("{other:?}"),
        Err(e) => format!("error: {e}"),
    };

    // State (FR-11): a reachable state other than the current one.
    let item = stored(store, a)?;
    let meta = writer
        .type_meta(project, item.kind.as_str(), UserAction::ChangeState)
        .await?;
    let menu = meta.menu(&item.state);
    let target = menu
        .iter()
        .find(|s| s.name != item.state && s.category == StateCategory::InProgress)
        .or_else(|| {
            menu.iter()
                .find(|s| s.name != item.state && s.category != StateCategory::Removed)
        })
        .ok_or("no other state to move to")?;
    let r = writer
        .send(&edit(
            a,
            WiChange::State {
                to: target.name.clone(),
                base: item.state.clone(),
            },
        )?)
        .await;
    f.add("state change", outcome(r));

    // Assignee: me, then unassigned (FR-13).
    let r = writer
        .send(&edit(
            a,
            WiChange::Assign {
                to: Some(me.clone()),
                base: stored(store, a)?.assigned_to,
            },
        )?)
        .await;
    f.add("assign to me", outcome(r));
    let assigned_id = stored(store, a)?.assigned_to.unwrap_or_default();
    f.add(
        "AssignedTo id = connectionData id",
        (assigned_id.eq_ignore_ascii_case(&me.id)).to_string(),
    );
    let found = writer
        .search_people(&me.display_name, UserAction::Assign)
        .await;
    let directory = match &found {
        Ok(people) => match people.iter().find(|p| {
            p.unique_name.as_deref().map(str::to_ascii_lowercase)
                == me.unique_name.as_deref().map(str::to_ascii_lowercase)
        }) {
            Some(p) => format!(
                "found among {}; id equals AssignedTo id: {}",
                people.len(),
                p.id.eq_ignore_ascii_case(&assigned_id)
            ),
            None => format!("{} results, none with my sign-in name", people.len()),
        },
        Err(e) => format!("error: {e}"),
    };
    f.add("directory search", directory);
    let r = writer
        .send(&edit(
            a,
            WiChange::Assign {
                to: None,
                base: stored(store, a)?.assigned_to,
            },
        )?)
        .await;
    f.add("unassign", outcome(r));

    // Area and iteration on real nodes (FR-14).
    let tree = writer.class_tree(project, UserAction::ChangeArea).await?;
    for kind in [NodeKind::Area, NodeKind::Iteration] {
        let nodes = match kind {
            NodeKind::Area => &tree.areas,
            NodeKind::Iteration => &tree.iterations,
        };
        let label = match kind {
            NodeKind::Area => "area",
            NodeKind::Iteration => "iteration",
        };
        let item = stored(store, a)?;
        let current = match kind {
            NodeKind::Area => &item.area_path,
            NodeKind::Iteration => &item.iteration_path,
        };
        let Some(node) = nodes.iter().find(|n| n.depth > 0 && &n.path != current) else {
            f.add(
                label,
                format!(
                    "only the root exists ({} node(s)): not changed",
                    nodes.len()
                ),
            );
            continue;
        };
        let change = match kind {
            NodeKind::Area => WiChange::Area {
                to: node.path.clone(),
                base: current.clone(),
            },
            NodeKind::Iteration => WiChange::Iteration {
                to: node.path.clone(),
                base: current.clone(),
            },
        };
        let r = writer.send(&edit(a, change)?).await;
        f.add(
            &format!("{label} change (depth {})", node.depth),
            outcome(r),
        );
    }

    // Related link A → B, then remove it (FR-21, FR-22).
    let r = writer
        .send(&edit(
            a,
            WiChange::AddLink {
                kind: LinkKind::Related,
                target: b,
                replace_parent: None,
            },
        )?)
        .await;
    f.add("add Related link", outcome(r));
    let r = writer
        .send(&edit(
            a,
            WiChange::RemoveLink {
                kind: LinkKind::Related,
                target: b,
            },
        )?)
        .await;
    f.add("remove Related link", outcome(r));

    // HTML description through the FR-18 gate, saved as HTML (FR-17).
    let extras_a = extras(store, a)?;
    match extras_a.description {
        Some(d) if d.format == TextFormat::Html => {
            match adoken_richtext::editable_html(&d.source) {
                Ok(markdown) => {
                    let edited = format!("{markdown}\n\nAdded by the adoken probe.");
                    let html = adoken_richtext::markdown_to_html(&edited, &|_| None);
                    let r = writer
                        .send(&edit(
                            a,
                            WiChange::Description {
                                to: html,
                                format: TextFormat::Html,
                                base: Some(d.source),
                            },
                        )?)
                        .await;
                    f.add("HTML description edit (gate open)", outcome(r));
                    let after = self::extras(store, a)?.description;
                    f.add(
                        "HTML field still HTML after save",
                        after
                            .is_some_and(|d| d.format == TextFormat::Html)
                            .to_string(),
                    );
                }
                Err(blocked) => f.add(
                    "HTML description gate",
                    format!("blocked: {}", blocked.describe()),
                ),
            }
        }
        other => f.add(
            "HTML description",
            format!("unexpected stored format: {:?}", other.map(|d| d.format)),
        ),
    }

    // Markdown description: multilineFieldsFormat on the GET and the batch.
    let extras_b = extras(store, b)?;
    f.add(
        "item GET returns multilineFieldsFormat (Markdown item)",
        (extras_b.description.as_ref().map(|d| d.format) == Some(TextFormat::Markdown)).to_string(),
    );
    let batch_url = get("_apis/wit/workitemsbatch", &[], "7.1")?.url().clone();
    let batch = send(ApiRequest::workitems_batch(batch_url, &[b])).await?;
    let batch: Value = serde_json::from_slice(&batch.body)?;
    let in_batch = batch["value"][0]["multilineFieldsFormat"]["System.Description"]
        .as_str()
        .is_some_and(|f| f.eq_ignore_ascii_case("markdown"));
    f.add(
        "workitemsbatch returns multilineFieldsFormat",
        in_batch.to_string(),
    );
    let r = writer
        .send(&edit(
            b,
            WiChange::Description {
                to: "## adoken probe\n\n- one\n- two\n- three".into(),
                format: TextFormat::Markdown,
                base: extras_b.description.map(|d| d.source),
            },
        )?)
        .await;
    f.add("Markdown description edit", outcome(r));

    // A Markdown comment mentioning me (FR-25, FR-26).
    let r = writer
        .comment(a, &format!("@<{}> adoken probe comment", me.id))
        .await;
    f.add(
        "Markdown comment with mention",
        match &r {
            Ok(c) => format!("posted, stored format {}", c.format),
            Err(e) => format!("error: {e}"),
        },
    );
    f.add(
        "check the comment's rendering at",
        format!("{}_workitems/edit/{a}", writer.org_url().as_str()),
    );

    // One stale `test /rev` (FR-8 / §9): the request builder of the app,
    // sent with an old revision on purpose.
    let item = stored(store, b)?;
    let stale = WiWrite {
        id: b,
        base_rev: item.rev - 1,
        change: WiChange::Title {
            to: format!("{} (stale)", item.title),
            base: item.title.clone(),
        },
        extra: Vec::new(),
    };
    let req = patch::patch_request(writer.org_url(), &stale, stale.base_rev, &[])?;
    let stale_answer =
        attribution::with_origin(Origin::You(UserAction::EditTitle), send(req)).await;
    f.add(
        "stale test /rev answer",
        match stale_answer {
            Ok(r) => format!("HTTP {} (accepted!)", r.status),
            Err(GovError::Status {
                status, message, ..
            }) => format!(
                "HTTP {status}, {}",
                message
                    .as_deref()
                    .and_then(tf_code)
                    .unwrap_or_else(|| "no TF code".into())
            ),
            Err(e) => format!("error: {e}"),
        },
    );
    Ok(())
}

fn tf_code(message: &str) -> Option<String> {
    let at = message.find("TF")?;
    let code: String = message[at..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (code.len() > 2).then_some(code)
}

fn id_of(created: &Value) -> Res<i64> {
    created["id"]
        .as_i64()
        .ok_or_else(|| "created item has no id".into())
}

fn stored(store: &Store, id: i64) -> Res<WorkItem> {
    Ok(store
        .with_conn(|c| read::work_item(c, id))?
        .ok_or_else(|| format!("#{id} not stored"))?)
}

fn extras(store: &Store, id: i64) -> Res<adoken_model::WorkItemExtras> {
    Ok(store
        .with_conn(|c| store_wi::extras(c, id))?
        .unwrap_or_default())
}

/// A rev-0 row so the per-item sync knows the project; the first answer
/// replaces it.
fn seed_row(store: &Store, id: i64, project: &str, me: &str) -> Res<()> {
    store.write(|tx| {
        write::upsert_work_item(
            tx,
            &WorkItem {
                id,
                project_id: project.to_owned(),
                rev: 0,
                kind: WorkItemType::Task,
                title: String::new(),
                state: String::new(),
                state_category: StateCategory::Proposed,
                assigned_to: None,
                created_by: me.to_owned(),
                changed_by: me.to_owned(),
                area_path: String::new(),
                iteration_path: String::new(),
                priority: None,
                tags: Vec::new(),
                parent_id: None,
                linked_pr_ids: Vec::new(),
                description: None,
                comment_count: 0,
                created: 0,
                changed: 0,
            },
        )
        .map(|_| ())
    })?;
    Ok(())
}

/// Requests and TSTU per route (the ledger's templated routes: no names).
fn print_endpoints(entries: &[Entry]) {
    println!("\n## Requests (TSTU per request type)");
    println!("| method | route | origin | n | statuses | TSTU | TSTU/request |");
    println!("|---|---|---|---:|---|---:|---:|");
    #[derive(Default)]
    struct Stats {
        n: usize,
        cost: f64,
        statuses: BTreeMap<String, usize>,
    }
    let mut by: BTreeMap<(String, String, String), Stats> = BTreeMap::new();
    for e in entries {
        let s = by
            .entry((e.method.to_owned(), e.route.clone(), e.origin.label()))
            .or_default();
        s.n += 1;
        s.cost += e.rate.cost.unwrap_or(0.0);
        let status = match e.status {
            Status::Http(code) => code.to_string(),
            _ => "err".into(),
        };
        *s.statuses.entry(status).or_default() += 1;
    }
    for ((method, route, origin), s) in by {
        let statuses: Vec<String> = s.statuses.iter().map(|(c, k)| format!("{c}×{k}")).collect();
        println!(
            "| {method} | `{route}` | {origin} | {} | {} | {:.4} | {:.4} |",
            s.n,
            statuses.join(" "),
            s.cost,
            s.cost / s.n as f64
        );
    }
}

// ── the dry run's fake Azure DevOps ──────────────────────────────────────

const FAKE_ME: &str = "aaaaaaaa-0000-0000-0000-00000000000a";

/// In-process Azure DevOps with just enough behavior for the flow: items
/// with revisions and `test /rev`, relations, formats, comments, a Task type
/// with a Removed state, a two-level tree, a directory.
struct Fake {
    items: Mutex<BTreeMap<i64, Value>>,
    comments: Mutex<BTreeMap<i64, Vec<Value>>>,
    next: Mutex<i64>,
    /// The token has `vso.work_write`.
    writes: bool,
}

impl Fake {
    fn new(writes: bool) -> Self {
        Fake {
            items: Mutex::new(BTreeMap::new()),
            comments: Mutex::new(BTreeMap::new()),
            next: Mutex::new(900),
            writes,
        }
    }

    fn me() -> Value {
        json!({"id": FAKE_ME, "displayName": "Probe Owner", "uniqueName": "owner@example.com"})
    }

    fn route(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
        // parts[0] = org
        let rest = &parts[1..];
        match (method, rest) {
            ("GET", ["_apis", "connectionData"]) => (
                200,
                json!({"authenticatedUser": {"id": FAKE_ME, "providerDisplayName": "Probe Owner",
                    "properties": {"Account": {"$value": "owner@example.com"}}}}),
            ),
            ("GET", ["_apis", "projects"]) => (
                200,
                json!({"value": [{"id": "pp000000-0000-0000-0000-000000000001", "name": "Probe"}]}),
            ),
            ("GET", [_, "_apis", "wit", "workitems", id]) => {
                match self.items.lock().get(&id.parse().unwrap_or(0)) {
                    Some(item) => (200, item.clone()),
                    None => (404, json!({"message": "TF401232: does not exist"})),
                }
            }
            ("PATCH", ["_apis", "wit", "workitems", id]) => {
                self.patch(id.parse().unwrap_or(0), body.cloned().unwrap_or_default())
            }
            ("GET", [_, "_apis", "wit", "workItems", id, "comments"]) => (
                200,
                json!({"comments": self.comments.lock().get(&id.parse().unwrap_or(0)).cloned().unwrap_or_default()}),
            ),
            ("POST", [_, "_apis", "wit", "workItems", id, "comments"]) => {
                let id: i64 = id.parse().unwrap_or(0);
                let text = body.and_then(|b| b["text"].as_str()).unwrap_or_default();
                let comment = json!({"id": 1, "workItemId": id, "text": text, "format": "markdown",
                    "createdBy": Fake::me(), "createdDate": "2026-10-01T12:00:00Z"});
                self.comments
                    .lock()
                    .entry(id)
                    .or_default()
                    .push(comment.clone());
                if let Some(item) = self.items.lock().get_mut(&id) {
                    item["rev"] = json!(item["rev"].as_i64().unwrap_or(0) + 1);
                    item["fields"]["System.CommentCount"] = json!(1);
                }
                (200, comment)
            }
            ("GET", [_, "_apis", "wit", "workitemtypes", "Task"]) => (
                200,
                json!({"name": "Task", "states": [
                    {"name": "New", "category": "Proposed"}, {"name": "Active", "category": "InProgress"},
                    {"name": "Closed", "category": "Completed"}, {"name": "Removed", "category": "Removed"}],
                    "transitions": {"New": [{"to": "Active"}, {"to": "Removed"}],
                                    "Active": [{"to": "New"}, {"to": "Closed"}, {"to": "Removed"}]}}),
            ),
            ("GET", [_, "_apis", "wit", "classificationnodes"]) => (
                200,
                json!({"value": [
                    {"name": "Probe", "structureType": "area", "children": [{"name": "Infra", "structureType": "area"}]},
                    {"name": "Probe", "structureType": "iteration", "children": [{"name": "Sprint 1", "structureType": "iteration",
                        "attributes": {"startDate": "2026-09-28T00:00:00Z", "finishDate": "2026-10-09T00:00:00Z"}}]}]}),
            ),
            ("GET", ["_apis", "identities"]) => (
                200,
                json!({"value": [{"id": FAKE_ME, "providerDisplayName": "Probe Owner", "isActive": true,
                    "properties": {"Account": {"$value": "owner@example.com"}}}]}),
            ),
            ("POST", ["_apis", "wit", "workitemsbatch"]) => {
                let ids: Vec<i64> = body
                    .and_then(|b| b["ids"].as_array())
                    .map(|a| a.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                let items = self.items.lock();
                (
                    200,
                    json!({"value": ids.iter().filter_map(|i| items.get(i).cloned()).collect::<Vec<_>>()}),
                )
            }
            _ => (
                404,
                json!({"message": format!("fake: no route {method} {path}")}),
            ),
        }
    }

    fn patch(&self, id: i64, ops: Value) -> (u16, Value) {
        let mut items = self.items.lock();
        let Some(item) = items.get_mut(&id) else {
            return (404, json!({"message": "TF401232: does not exist"}));
        };
        let mut next = item.clone();
        for op in ops.as_array().cloned().unwrap_or_default() {
            let path = op["path"].as_str().unwrap_or_default();
            match (op["op"].as_str(), path) {
                (Some("test"), "/rev") => {
                    if op["value"] != item["rev"] {
                        return (
                            400,
                            json!({"message": "TF26071: This work item has been changed by someone else since you opened it."}),
                        );
                    }
                }
                (Some("add"), "/relations/-") => {
                    next["relations"]
                        .as_array_mut()
                        .expect("array")
                        .push(op["value"].clone());
                }
                (Some("remove"), p) if p.starts_with("/relations/") => {
                    let i: usize = p["/relations/".len()..].parse().unwrap_or(usize::MAX);
                    let rels = next["relations"].as_array_mut().expect("array");
                    if i >= rels.len() {
                        return (400, json!({"message": "VS402862: bad index"}));
                    }
                    rels.remove(i);
                }
                (Some("add"), p) if p.starts_with("/multilineFieldsFormat/") => {
                    next["multilineFieldsFormat"][&p["/multilineFieldsFormat/".len()..]] =
                        op["value"].clone();
                }
                (Some("add"), "/fields/System.AssignedTo") => match op["value"].as_str() {
                    Some("") | None => {
                        next["fields"]
                            .as_object_mut()
                            .expect("object")
                            .remove("System.AssignedTo");
                    }
                    Some(_) => next["fields"]["System.AssignedTo"] = Fake::me(),
                },
                (Some("add"), p) if p.starts_with("/fields/") => {
                    next["fields"][&p["/fields/".len()..]] = op["value"].clone();
                }
                _ => {
                    return (
                        400,
                        json!({"message": format!("fake: unsupported op {op}")}),
                    );
                }
            }
        }
        next["rev"] = json!(item["rev"].as_i64().unwrap_or(0) + 1);
        *item = next.clone();
        (200, next)
    }
}

impl Backend for Fake {
    fn create<'a>(&'a self, _project: &'a str, ops: Value) -> BoxFuture<'a, Res<Value>> {
        Box::pin(async move {
            if !self.writes {
                return Err(NO_WRITE_SCOPE.into());
            }
            let id = {
                let mut next = self.next.lock();
                *next += 1;
                *next
            };
            let mut fields = Map::new();
            let mut formats = Map::new();
            for op in ops.as_array().cloned().unwrap_or_default() {
                let path = op["path"].as_str().unwrap_or_default();
                if let Some(f) = path.strip_prefix("/fields/") {
                    fields.insert(f.to_owned(), op["value"].clone());
                } else if let Some(f) = path.strip_prefix("/multilineFieldsFormat/") {
                    formats.insert(f.to_owned(), op["value"].clone());
                }
            }
            for (k, v) in [
                ("System.TeamProject", json!("Probe")),
                ("System.WorkItemType", json!("Task")),
                ("System.State", json!("New")),
                ("System.AreaPath", json!("Probe")),
                ("System.IterationPath", json!("Probe")),
                ("System.CreatedBy", Fake::me()),
                ("System.ChangedBy", Fake::me()),
                ("System.CreatedDate", json!("2026-10-01T12:00:00Z")),
                ("System.ChangedDate", json!("2026-10-01T12:00:00Z")),
            ] {
                fields.insert(k.into(), v);
            }
            let item = json!({"id": id, "rev": 1, "fields": fields, "relations": [],
                "multilineFieldsFormat": formats});
            self.items.lock().insert(id, item.clone());
            Ok(item)
        })
    }

    fn delete<'a>(&'a self, _project: &'a str, id: i64) -> BoxFuture<'a, Res<u16>> {
        Box::pin(async move {
            Ok(if self.items.lock().remove(&id).is_some() {
                200
            } else {
                404
            })
        })
    }
}

struct FakeTransport(Arc<Fake>);

impl Transport for FakeTransport {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        let (status, body) = self
            .0
            .route(req.method().as_str(), req.url().path(), req.body());
        Box::pin(async move {
            Ok(ApiResponse {
                status,
                rate: RateHeaders {
                    cost: Some(0.01),
                    ..RateHeaders::default()
                },
                body: serde_json::to_vec(&body).unwrap_or_default(),
                content_type: Some("application/json".into()),
                validators: Default::default(),
            })
        })
    }
}
