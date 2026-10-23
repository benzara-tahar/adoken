//! Cost spike (spec 0002 §9, ADR 0001 gate B5): one baseline pass against the
//! connected org, into a throwaway store, with a request ledger.
//!
//! ```sh
//! cargo run -p adoken-sync --example sync_probe [-- --shapes <dir>] [--pins]
//! ```
//!
//! - Loads the verified account (`account.json`, `onboarding.json`) and its
//!   PAT from the macOS Keychain through `adoken_ado::Connector` (a Keychain
//!   prompt may appear). Nothing is written to the app's data directory.
//! - Runs warm → hot → cold once (the first-sync order) for the selected
//!   projects only, then a steady-state warm + hot round, then "load older
//!   runs" and one more round, printing requests per endpoint, bytes,
//!   latency, `X-RateLimit-*` / TSTU usage, wall time and store row counts.
//! - `--shapes <dir>` also records one sanitized response per endpoint
//!   (names, emails, free text masked; mention markup kept) plus a few
//!   read-only shape probes beyond the engine's "mine" scope (builds and
//!   timelines of anyone, work item comments, PR threads of any PR,
//!   `@RecentMentions` WIQL).
//!
//! - `--pins` measures pin refresh, availability and actor-attribution endpoints
//!   and compares the limited definition query with the newest queued run
//!   (spec 0014 AC-9). `--pins-only` skips the full reporting-index baseline.
//!
//! Read-only: GET, WIQL POST and workitemsbatch POST only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use adoken_ado::http::Http;
use adoken_ado::{Connector, OnboardingState};
use adoken_store::Store;
use adoken_sync::attribution::{self, Attribution, Origin};
use adoken_sync::ledger::{Entry, NetLog, Status};
use adoken_sync::routes::match_route;
use adoken_sync::transport::{ApiResponse, TransportError};
use adoken_sync::{
    AdoTransport, ApiRequest, Engine, Recording, SyncConfig, SystemClock, Tier, Transport,
};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use serde_json::Value;
use url::Url;

/// Keeps the first few response bodies per endpoint template (`--shapes`).
const BODIES_PER_ENDPOINT: usize = 4;

struct Capture<T> {
    inner: Arc<T>,
    bodies: Mutex<BTreeMap<String, Vec<Vec<u8>>>>,
}

impl<T: Transport> Transport for Capture<T> {
    fn send(&self, req: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, TransportError>> {
        Box::pin(async move {
            let key = format!(
                "{} {}",
                req.method().as_str(),
                match_route(req.method().as_str(), "", req.url().path()).route
            );
            let resp = self.inner.send(req).await?;
            let mut bodies = self.bodies.lock();
            let kept = bodies.entry(key).or_default();
            if kept.len() < BODIES_PER_ENDPOINT {
                kept.push(resp.body.clone());
            }
            drop(bodies);
            Ok(resp)
        })
    }
}

type Stack = Capture<Recording<AdoTransport>>;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Everything not inside an engine pass (the probes) is attributed as
    // startup traffic in the network log (spec 0016 FR-18).
    attribution::scope(Attribution::new(Origin::Startup), probe()).await
}

async fn probe() -> Result<(), Box<dyn std::error::Error>> {
    let shapes: Option<PathBuf> = {
        let args: Vec<String> = std::env::args().collect();
        args.iter()
            .position(|a| a == "--shapes")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--shapes <dir>")))
    };
    let data_dir = adoken_ado::data_dir();
    let probe_pins = std::env::args().any(|arg| arg == "--pins");
    let pins_only = std::env::args().any(|arg| arg == "--pins-only");
    let connector = Connector::new(data_dir.clone());
    let state = OnboardingState::load(&data_dir).ok_or("no onboarding.json: connect first")?;
    eprintln!("reading the PAT from the Keychain (a macOS prompt may appear)…");
    let client = connector
        .client()
        .await?
        .ok_or("no verified account + Keychain token")?;
    let org = client.organization().clone();
    let projects: Vec<String> = state.projects.iter().map(|p| p.id.clone()).collect();

    let recording = Arc::new(Recording::new(
        AdoTransport {
            http: Http::new(),
            auth: client.auth().clone(),
        },
        Arc::new(NetLog::new(Arc::new(SystemClock))),
    ));
    let transport: Arc<Stack> = Arc::new(Capture {
        inner: recording.clone(),
        bodies: Mutex::new(BTreeMap::new()),
    });
    let tmp = tempfile::tempdir()?;
    let store = Store::open(tmp.path().join("adoken.sqlite"))?;
    let mut cfg = SyncConfig::new(org.base_url());
    cfg.projects = Some(projects.clone());
    let engine = Engine::new(store.clone(), transport.clone(), Arc::new(SystemClock), cfg);

    println!(
        "# sync_probe — org {{org}}, {} selected project(s)",
        projects.len()
    );
    if pins_only {
        engine.run_tier(Tier::Warm).await?;
        pins_probe(&org.base_url(), &store, &engine, &transport, &recording).await?;
        return Ok(());
    }
    let wall = Instant::now();
    let mut passes = Vec::new();
    for tier in [Tier::Warm, Tier::Hot, Tier::Cold] {
        passes.push(run_pass(&engine, &recording, tier).await);
    }
    let wall_ms = wall.elapsed().as_millis();
    let baseline = recording.entries();
    print_passes(
        "Baseline pass (warm → hot → cold, first-sync order)",
        &passes,
    );
    println!("- wall time: {wall_ms} ms");
    print_endpoints(&baseline);
    print_rate_headers(&baseline);
    print_rows(&store)?;

    // One steady-state warm + hot round (no structure refresh, cursors set).
    let steady: Vec<Pass> = vec![
        run_pass(&engine, &recording, Tier::Warm).await,
        run_pass(&engine, &recording, Tier::Hot).await,
    ];
    print_passes("Steady-state sample", &steady);
    let per_5min = steady[1].cost * 5.0 + steady[0].cost * 2.5 + passes[2].cost / 6.0;
    let req_5min = steady[1].requests as f64 * 5.0
        + steady[0].requests as f64 * 2.5
        + passes[2].requests as f64 / 6.0;
    println!(
        "- foreground estimate (hot 60 s, warm 2 min, cold 30 min): {req_5min:.1} requests and {per_5min:.4} TSTU per 5 min (budget: ≤ 10 TSTU, hard cap 50)"
    );
    print_endpoints(&recording.entries()[baseline.len()..]);

    // "Pipelines: load older runs" (user-initiated), then one more round:
    // finished runs' timelines are fetched once and never again.
    let before = recording.entries().len();
    let t = Instant::now();
    let older = engine.load_older_runs().await;
    println!(
        "\n## Load older runs (user request)\n- {} run(s), {} ms{}",
        older.as_ref().copied().unwrap_or(0),
        t.elapsed().as_millis(),
        older
            .as_ref()
            .err()
            .map(|e| format!(", error: {e}"))
            .unwrap_or_default()
    );
    print_endpoints(&recording.entries()[before..]);
    let before = recording.entries().len();
    let after_older: Vec<Pass> = vec![
        run_pass(&engine, &recording, Tier::Warm).await,
        run_pass(&engine, &recording, Tier::Hot).await,
    ];
    print_passes("Steady state after loading older runs", &after_older);
    print_endpoints(&recording.entries()[before..]);
    print_rows(&store)?;

    print_header_names(&org, client.auth().as_ref()).await;
    if probe_pins {
        pins_probe(
            &org.base_url(),
            &store,
            &engine,
            transport.as_ref(),
            &recording,
        )
        .await?;
    }

    if let Some(dir) = &shapes {
        let before = recording.entries().len();
        extra_probes(&org.base_url(), &store, transport.as_ref()).await;
        println!("\n## Shape probes (not part of sync)");
        print_endpoints(&recording.entries()[before..]);
        let bodies = transport.bodies.lock();
        let secrets = secrets(&store, &org, &state, &bodies)?;
        write_shapes(dir, &bodies, &secrets)?;
        eprintln!("shapes written to {}", dir.display());
    }
    Ok(())
}

async fn pins_probe(
    base: &Url,
    store: &Store,
    engine: &Engine,
    transport: &Stack,
    recording: &Recording<AdoTransport>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (pr, wi, definitions) = store.with_conn(|c| {
        let pr = c
            .prepare("SELECT id FROM pull_request ORDER BY id LIMIT 1")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .next()
            .transpose()?;
        let wi = c
            .prepare("SELECT id FROM work_item ORDER BY id LIMIT 1")?
            .query_map([], |r| r.get::<_, i64>(0))?
            .next()
            .transpose()?;
        let definitions = c
            .prepare(
                "SELECT p.project_id, p.id FROM pipeline p WHERE p.project_id = \
             (SELECT project_id FROM pipeline GROUP BY project_id ORDER BY count(*) DESC LIMIT 1) \
             ORDER BY EXISTS(SELECT 1 FROM pipeline_run r WHERE r.project_id = p.project_id \
                 AND r.pipeline_id = p.id AND r.status != 'completed') DESC, p.id LIMIT 3",
            )?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok((pr, wi, definitions))
    })?;
    println!("\n## Pins endpoint ledger (read-only, throwaway store)");
    let before = recording.entries().len();
    if let Some(id) = pr {
        let response = engine
            .governor()
            .send(
                transport,
                Tier::Warm,
                ApiRequest::get(api(
                    base,
                    &format!("_apis/git/pullrequests/{id}"),
                    &[],
                    "7.1",
                )),
            )
            .await?;
        let pr: Value = serde_json::from_slice(&response.body)?;
        if let (Some(project), Some(repository)) = (
            pr["repository"]["project"]["id"].as_str(),
            pr["repository"]["id"].as_str(),
        ) {
            engine.governor().send(transport, Tier::Warm, ApiRequest::get(api(base,
                &format!("{project}/_apis/git/repositories/{repository}/pullrequests/{id}/iterations"),
                &[], "7.1"))).await?;
        } else {
            println!(
                "- Iteration actor lookup: not measured; PR response lacks repository metadata"
            );
        }
    } else {
        println!("- PR-by-id: not measured; no cached PR");
    }
    if let Some(id) = wi {
        engine
            .governor()
            .send(
                transport,
                Tier::Warm,
                ApiRequest::workitems_batch(
                    api(base, "_apis/wit/workitemsbatch", &[], "7.1"),
                    &[id],
                ),
            )
            .await?;
    } else {
        println!("- Work-item batch: not measured; no cached work item");
    }
    if let Some((project, _)) = definitions.first() {
        let ids = definitions
            .iter()
            .map(|(_, id)| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let path = format!("{project}/_apis/build/builds");
        let response = engine
            .governor()
            .send(
                transport,
                Tier::Warm,
                ApiRequest::get(api(
                    base,
                    &path,
                    &[("definitions", &ids), ("maxBuildsPerDefinition", "1")],
                    "7.1",
                )),
            )
            .await?;
        let limited: Value = serde_json::from_slice(&response.body)?;
        print_endpoints(&recording.entries()[before..]);
        let metadata_start = recording.entries().len();
        println!("\n### Availability and live-run metadata samples");
        println!(
            "- Definition GET is sampled for cost even when a retained run exists; production uses it only for an omitted definition."
        );
        for (_, definition) in &definitions {
            engine
                .governor()
                .send(
                    transport,
                    Tier::Warm,
                    ApiRequest::get(api(
                        base,
                        &format!("{project}/_apis/build/definitions/{definition}"),
                        &[],
                        "7.1",
                    )),
                )
                .await?;
        }
        if let Some(run) = limited["value"]
            .as_array()
            .and_then(|runs| {
                runs.iter()
                    .find(|run| matches!(run["status"].as_str(), Some("inProgress" | "notStarted")))
            })
            .and_then(|run| run["id"].as_i64())
        {
            engine
                .governor()
                .send(
                    transport,
                    Tier::Hot,
                    ApiRequest::get(api(
                        base,
                        &format!("{project}/_apis/build/builds/{run}"),
                        &[],
                        "7.1",
                    )),
                )
                .await?;
        } else {
            println!(
                "- Live-run GET: not measured; no in-progress/queued sample in the limited response"
            );
        }
        print_endpoints(&recording.entries()[metadata_start..]);
        let compare_start = recording.entries().len();
        let mut running = 0;
        for (slot, (_, definition_id)) in definitions.iter().enumerate() {
            let id = definition_id.to_string();
            let response = engine
                .governor()
                .send(
                    transport,
                    Tier::Warm,
                    ApiRequest::get(api(
                        base,
                        &path,
                        &[
                            ("definitions", &id),
                            ("queryOrder", "queueTimeDescending"),
                            ("$top", "1"),
                        ],
                        "7.1",
                    )),
                )
                .await?;
            let newest: Value = serde_json::from_slice(&response.body)?;
            let newest = newest["value"].as_array().and_then(|runs| runs.first());
            let selected = limited["value"].as_array().and_then(|runs| {
                runs.iter()
                    .find(|run| run["definition"]["id"].as_i64() == Some(*definition_id))
            });
            match (newest, selected) {
                (Some(newest), Some(selected)) => {
                    let status = newest["status"].as_str().unwrap_or("unknown");
                    if matches!(status, "inProgress" | "notStarted") {
                        running += 1;
                    }
                    println!(
                        "- Definition {}: newest status={status}; limited query selected newest={}; selected status={}",
                        slot + 1,
                        newest["id"] == selected["id"],
                        selected["status"].as_str().unwrap_or("unknown")
                    );
                }
                _ => println!("- Definition {}: no comparable retained run", slot + 1),
            }
        }
        println!(
            "- In-progress/queued newest runs observed: {running}; AC-9's live-run ordering check requires at least one."
        );
        println!("\n### Ordering comparison requests (excluded from pin-refresh cost)");
        print_endpoints(&recording.entries()[compare_start..]);
    } else {
        print_endpoints(&recording.entries()[before..]);
        println!("- Definition refresh: not measured; no cached definition");
    }
    Ok(())
}

struct Pass {
    tier: Tier,
    requests: usize,
    ms: u128,
    cost: f64,
    events: usize,
    error: Option<String>,
}

async fn run_pass(engine: &Engine, rec: &Recording<AdoTransport>, tier: Tier) -> Pass {
    let before = rec.entries().len();
    let t = Instant::now();
    let result = engine.run_tier(tier).await;
    let ms = t.elapsed().as_millis();
    let entries = rec.entries();
    let new = &entries[before..];
    Pass {
        tier,
        requests: new.len(),
        ms,
        cost: new.iter().filter_map(|e| e.rate.cost).sum(),
        events: result.as_ref().map(|r| r.events.len()).unwrap_or(0),
        error: result.err().map(|e| e.to_string()),
    }
}

fn print_passes(title: &str, passes: &[Pass]) {
    println!("\n## {title}");
    for p in passes {
        println!(
            "- {:?}: {} requests, {} ms, {:.4} TSTU, {} events{}",
            p.tier,
            p.requests,
            p.ms,
            p.cost,
            p.events,
            p.error
                .as_ref()
                .map(|e| format!(", error: {e}"))
                .unwrap_or_default()
        );
    }
}

fn print_endpoints(entries: &[Entry]) {
    println!("\n| method | host | path | n | statuses | bytes | ms | TSTU |");
    println!("|---|---|---|---:|---|---:|---:|---:|");
    #[derive(Default)]
    struct Stats {
        requests: usize,
        bytes: u64,
        elapsed_ms: u64,
        cost: f64,
        statuses: BTreeMap<u16, usize>,
        errors: usize,
    }
    let mut by_route: BTreeMap<(&str, &str, &str), Stats> = BTreeMap::new();
    for e in entries {
        let s = by_route
            .entry((e.method, e.host.as_str(), e.route.as_str()))
            .or_default();
        s.requests += 1;
        s.bytes += e.bytes.unwrap_or(0);
        s.elapsed_ms += e.elapsed_ms.unwrap_or(0);
        s.cost += e.rate.cost.unwrap_or(0.0);
        match e.status {
            Status::Http(code) => *s.statuses.entry(code).or_default() += 1,
            _ => s.errors += 1,
        }
    }
    let (mut n, mut bytes, mut cost) = (0, 0, 0.0);
    for ((method, host, path), s) in by_route {
        let statuses: Vec<String> = s
            .statuses
            .iter()
            .map(|(c, k)| format!("{c}×{k}"))
            .chain((s.errors > 0).then(|| format!("err×{}", s.errors)))
            .collect();
        println!(
            "| {method} | {host} | `{path}` | {} | {} | {} | {} | {:.4} |",
            s.requests,
            statuses.join(" "),
            s.bytes,
            s.elapsed_ms,
            s.cost
        );
        n += s.requests;
        bytes += s.bytes;
        cost += s.cost;
    }
    println!("| **total** | | | **{n}** | | **{bytes}** | | **{cost:.4}** |");
}

fn print_rate_headers(entries: &[Entry]) {
    println!("\n## X-RateLimit-* headers");
    let count = |f: &dyn Fn(&Entry) -> bool| entries.iter().filter(|e| f(e)).count();
    println!("- responses: {}", entries.len());
    println!(
        "- with X-RateLimit-Cost: {}",
        count(&|e| e.rate.cost.is_some())
    );
    println!(
        "- with X-RateLimit-Limit/Remaining: {}",
        count(&|e| e.rate.limit.is_some() || e.rate.remaining.is_some())
    );
    println!(
        "- with Retry-After / X-RateLimit-Delay: {}",
        count(&|e| e.rate.retry_after_ms.is_some() || e.rate.delay_ms.is_some())
    );
    println!(
        "- X-RateLimit-Resource values: {:?}",
        entries
            .iter()
            .filter_map(|e| e.rate.resource.as_deref())
            .collect::<std::collections::BTreeSet<_>>()
    );
    let costs: Vec<f64> = entries.iter().filter_map(|e| e.rate.cost).collect();
    if !costs.is_empty() {
        let max = costs.iter().cloned().fold(0.0, f64::max);
        println!(
            "- TSTU per request: min {:.5}, max {max:.5}, sum {:.5}",
            costs.iter().cloned().fold(f64::MAX, f64::min),
            costs.iter().sum::<f64>()
        );
    }
}

/// Names of the `X-*` response headers Azure DevOps sends (values only for
/// `X-RateLimit-*`), from one signed `connectionData` GET.
async fn print_header_names(org: &adoken_ado::Organization, auth: &dyn adoken_ado::AuthProvider) {
    let url = api(&org.base_url(), "_apis/connectionData", &[], "7.1-preview");
    let http = Http::new();
    match http.execute(http.get(url), auth).await {
        Ok(resp) => {
            println!("\n## Response header names (connectionData)");
            let mut names: Vec<String> = resp
                .headers()
                .iter()
                .filter(|(k, _)| k.as_str().starts_with("x-"))
                .map(|(k, v)| match k.as_str().starts_with("x-ratelimit") {
                    true => format!("{k}: {}", v.to_str().unwrap_or("?")),
                    false => k.to_string(),
                })
                .collect();
            names.sort();
            names.dedup();
            for n in names {
                println!("- {n}");
            }
        }
        Err(e) => println!("\n(header probe failed: {e})"),
    }
}

fn print_rows(store: &Store) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n## Store rows");
    let tables = [
        "org",
        "identity",
        "project",
        "repo",
        "pull_request",
        "pr_reviewer",
        "pr_thread",
        "comment",
        "mention",
        "work_item",
        "wi_pr_link",
        "pipeline",
        "pipeline_run",
        "pipelines_stage",
        "search_doc",
        "sync_event",
        "sync_cursor",
    ];
    store.with_conn(|c| {
        for t in tables {
            let n: i64 = c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))?;
            println!("- {t}: {n}");
        }
        Ok(())
    })?;
    Ok(())
}

fn api(base: &Url, path: &str, query: &[(&str, &str)], version: &str) -> Url {
    let mut url = base.join(path).expect("relative path");
    {
        let mut q = url.query_pairs_mut();
        for (k, v) in query {
            q.append_pair(k, v);
        }
        q.append_pair("api-version", version);
    }
    url
}

/// Read-only shape probes beyond the engine's "mine" scope (so a sparse org
/// still yields shapes): recent builds of anyone + their timelines, PRs of
/// any status + their threads, the latest work items + their comments, and
/// the `@RecentMentions` / `@Follows` spike. Captured for `--shapes` only,
/// never written to the store.
async fn extra_probes(base: &Url, store: &Store, t: &Stack) {
    let get = |url: Url| async move {
        let what = match_route("GET", "", url.path()).route;
        match t.send(ApiRequest::get(url)).await {
            Ok(r) => {
                eprintln!("probe GET {what} -> {} ({} B)", r.status, r.body.len());
                serde_json::from_slice::<Value>(&r.body).unwrap_or(Value::Null)
            }
            Err(e) => {
                eprintln!("probe GET {what} failed: {e}");
                Value::Null
            }
        }
    };
    let wiql = |q: &'static str, top: Option<&'static str>| async move {
        let query: Vec<(&str, &str)> = top.map(|n| ("$top", n)).into_iter().collect();
        let url = api(base, "_apis/wit/wiql", &query, "7.1");
        match t.send(ApiRequest::wiql(url, q)).await {
            Ok(r) => {
                let body: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
                let n = body["workItems"].as_array().map(Vec::len);
                let msg = body["message"]
                    .as_str()
                    .map(|m| m.chars().take(200).collect::<String>());
                eprintln!("wiql `{q}`: HTTP {} items={n:?} message={msg:?}", r.status);
                body
            }
            Err(e) => {
                eprintln!("wiql `{q}`: {e}");
                Value::Null
            }
        }
    };
    let ids = |v: &Value, key: &str| -> Vec<i64> {
        v[key]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x["id"].as_i64()).collect())
            .unwrap_or_default()
    };
    for p in store.projects().unwrap_or_default() {
        let builds = get(api(
            base,
            &format!("{}/_apis/build/builds", p.id),
            &[("$top", "5"), ("queryOrder", "queueTimeDescending")],
            "7.1",
        ))
        .await;
        for id in ids(&builds, "value").into_iter().take(3) {
            get(api(
                base,
                &format!("{}/_apis/build/builds/{id}/timeline", p.id),
                &[],
                "7.1",
            ))
            .await;
        }
        let prs = get(api(
            base,
            &format!("{}/_apis/git/pullrequests", p.id),
            &[("searchCriteria.status", "all"), ("$top", "5")],
            "7.1",
        ))
        .await;
        for pr in prs["value"].as_array().into_iter().flatten().take(3) {
            let (Some(id), Some(repo)) = (
                pr["pullRequestId"].as_i64(),
                pr["repository"]["id"].as_str(),
            ) else {
                continue;
            };
            get(api(
                base,
                &format!(
                    "{}/_apis/git/repositories/{repo}/pullRequests/{id}/threads",
                    p.id
                ),
                &[],
                "7.1",
            ))
            .await;
        }
    }
    let latest = wiql(
        "SELECT [System.Id] FROM WorkItems ORDER BY [System.ChangedDate] DESC",
        Some("5"),
    )
    .await;
    let latest: Vec<String> = ids(&latest, "workItems")
        .iter()
        .map(i64::to_string)
        .collect();
    if !latest.is_empty() {
        let items = get(api(
            base,
            "_apis/wit/workitems",
            &[
                ("ids", &latest.join(",")),
                ("$expand", "relations"),
                ("errorPolicy", "omit"),
            ],
            "7.1",
        ))
        .await;
        for w in items["value"].as_array().into_iter().flatten() {
            let count = w["fields"]["System.CommentCount"].as_i64().unwrap_or(0);
            let project = w["fields"]["System.TeamProject"]
                .as_str()
                .unwrap_or_default();
            if count > 0
                && let Some(id) = w["id"].as_i64()
            {
                get(api(
                    base,
                    &format!("{project}/_apis/wit/workItems/{id}/comments"),
                    &[],
                    "7.1-preview.4",
                ))
                .await;
            }
        }
        // Title-index feed without the 365-day horizon (shape of real rows).
        for p in store.projects().unwrap_or_default() {
            get(api(
                base,
                &format!("{}/_apis/wit/reporting/workitemrevisions", p.id),
                &[
                    ("includeLatestOnly", "true"),
                    ("includeDeleted", "true"),
                    ("fields", "System.Id,System.Title,System.WorkItemType,System.State,System.AssignedTo,System.CreatedBy,System.ChangedBy,System.ChangedDate,System.CreatedDate,System.TeamProject,System.CommentCount"),
                ],
                "7.1",
            ))
            .await;
        }
    }
    // Spike: do the portal-only macros work over REST?
    wiql(
        "SELECT [System.Id] FROM WorkItems WHERE [System.Id] IN (@RecentMentions)",
        None,
    )
    .await;
    wiql(
        "SELECT [System.Id] FROM WorkItems WHERE [System.Id] IN (@Follows)",
        None,
    )
    .await;
}

/// Strings to scrub from recorded shapes: org, project and people names
/// (from the store and from every identity object in the captured bodies).
fn secrets(
    store: &Store,
    org: &adoken_ado::Organization,
    state: &OnboardingState,
    bodies: &BTreeMap<String, Vec<Vec<u8>>>,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    fn harvest(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                for (k, x) in map {
                    match x.as_str() {
                        Some(s) if REDACT_KEYS.contains(&k.as_str()) => out.push(s.to_owned()),
                        _ => harvest(x, out),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|x| harvest(x, out)),
            _ => {}
        }
    }
    let mut out = vec![org.name().to_owned()];
    out.extend(state.projects.iter().map(|p| p.name.clone()));
    out.extend(store.projects()?.into_iter().map(|p| p.name));
    for p in store.people()? {
        out.push(p.display_name);
        out.extend(p.unique_name);
    }
    for body in bodies.values().flatten() {
        if let Ok(v) = serde_json::from_slice::<Value>(body) {
            harvest(&v, &mut out);
        }
    }
    // Name parts too ("First Last" → "First", "Last").
    let parts: Vec<String> = out
        .iter()
        .flat_map(|s| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .collect();
    out.extend(parts);
    out.retain(|s| s.len() >= 3 && !s.starts_with("aad.") && !s.starts_with("<"));
    out.sort();
    out.dedup();
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    Ok(out)
}

const REDACT_KEYS: &[&str] = &[
    "displayName",
    "uniqueName",
    "mailAddress",
    "providerDisplayName",
    "customDisplayName",
    "imageUrl",
    "descriptor",
    "subjectDescriptor",
];
/// Free text: letters masked, markup (`<…>`, `@<guid>`) kept.
const TEXT_KEYS: &[&str] = &[
    "content",
    "text",
    "renderedText",
    "System.Description",
    "System.History",
    "description",
    "title",
    "System.Title",
    "message",
    "ci.message",
    "comment",
];

fn write_shapes(
    dir: &Path,
    bodies: &BTreeMap<String, Vec<Vec<u8>>>,
    secrets: &[String],
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (key, bodies) in bodies {
        let name: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        for (i, body) in bodies.iter().enumerate() {
            let mut v: Value = match serde_json::from_slice(body) {
                Ok(v) => v,
                Err(_) => Value::String(format!("<{} non-JSON bytes>", body.len())),
            };
            sanitize(&mut v, secrets);
            std::fs::write(
                dir.join(format!("{}.{i}.json", name.trim_matches('_'))),
                serde_json::to_vec_pretty(&v)?,
            )?;
        }
    }
    Ok(())
}

fn sanitize(v: &mut Value, secrets: &[String]) {
    match v {
        Value::Object(map) => {
            map.remove("_links");
            for (k, x) in map.iter_mut() {
                if REDACT_KEYS.contains(&k.as_str()) && x.is_string() {
                    *x = Value::String("<redacted>".into());
                } else if TEXT_KEYS.contains(&k.as_str()) && x.is_string() {
                    let s = x.as_str().unwrap_or_default();
                    *x = Value::String(scrub(&mask_text(s), secrets));
                } else {
                    sanitize(x, secrets);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|x| sanitize(x, secrets)),
        Value::String(s) => *s = scrub(s, secrets),
        _ => {}
    }
}

fn is_email(token: &str) -> bool {
    match token.split_once('@') {
        Some((user, domain)) => !user.is_empty() && domain.contains('.'),
        None => false,
    }
}

/// Replace secrets (case-insensitive) and email-looking tokens.
fn scrub(s: &str, secrets: &[String]) -> String {
    let mut out = s.to_owned();
    for secret in secrets {
        let lower = out.to_lowercase();
        let needle = secret.to_lowercase();
        let mut rebuilt = String::with_capacity(out.len());
        let mut last = 0;
        for (i, _) in lower.match_indices(&needle) {
            // `to_lowercase` can change byte lengths for non-ASCII; skip then.
            if lower.len() != out.len() {
                break;
            }
            rebuilt.push_str(&out[last..i]);
            rebuilt.push_str("{redacted}");
            last = i + needle.len();
        }
        rebuilt.push_str(&out[last..]);
        out = rebuilt;
    }
    let delims = |c: char| c.is_whitespace() || "\"'=,;()<>".contains(c);
    let mut result = String::with_capacity(out.len());
    let mut token = String::new();
    for c in out.chars().chain(std::iter::once('\0')) {
        if delims(c) || c == '\0' {
            result.push_str(if is_email(&token) { "<email>" } else { &token });
            token.clear();
            if c != '\0' {
                result.push(c);
            }
        } else {
            token.push(c);
        }
    }
    result
}

/// Mask letters and digits outside `<…>` markup; keeps `@<guid>` tokens,
/// HTML tags and attributes (e.g. `data-vss-mention`) verbatim.
fn mask_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => {
                depth += 1;
                out.push(c);
            }
            '>' => {
                depth = depth.saturating_sub(1);
                out.push(c);
            }
            c if depth == 0 && c.is_alphanumeric() => out.push('x'),
            c => out.push(c),
        }
    }
    out
}
