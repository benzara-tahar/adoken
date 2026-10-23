//! Spec 0015 demo content: every demo work item's description source,
//! stored relations and freshness (as a full Azure DevOps answer would
//! record them), a few hand-written descriptions that exercise the rich
//! text renderer and the editor's three cases (Markdown field, HTML that
//! round-trips, HTML adoken can't edit), and dated iteration trees so the
//! iteration picker has Current / Next / Recent.
//!
//! A sixth of the items nobody I know touched stay "Partly synced"
//! (title-index rows, spec FR-29), as they would after a search.

use adoken_model::time::DAY;
use adoken_model::*;
use adoken_store::rusqlite::Connection;
use adoken_store::wi::{ACCEPTANCE, FullFields, REPRO_STEPS, put_class_tree, record_answer};

use crate::{Dataset, vocab};

/// Index of the scenario bug ("Timezone bug in daily settlement job").
pub const SCENARIO: usize = 100;
/// A Markdown-format description (spec FR-17: edited losslessly).
pub const MARKDOWN: usize = 101;
/// An HTML description that round-trips (FR-18: edited as Markdown).
pub const HTML: usize = 102;
/// An HTML description with a table and two images (FR-18: blocked).
pub const BLOCKED: usize = 103;

fn html(format: TextFormat, source: impl Into<String>) -> RichText {
    RichText {
        source: source.into(),
        format,
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Plain demo text as Azure DevOps' editor stores it.
fn as_html(plain: &str) -> String {
    plain
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| format!("<div>{}</div>", escape(p)))
        .collect()
}

fn mention(person: &Person) -> String {
    format!(
        "<a href=\"#\" data-vss-mention=\"version:2.0,{}\">@{}</a>",
        person.id, person.display_name
    )
}

fn work_item_url(org: &Org, id: i64) -> String {
    format!("{}/_apis/wit/workItems/{id}", org.url.trim_end_matches('/'))
}

fn wi_link(org: &Org, index: usize, kind: LinkKind, target: i64) -> WiLink {
    WiLink {
        index: index as u32,
        rel: kind.rel().to_owned(),
        kind: Some(kind),
        target_id: Some(target),
        pr_id: None,
        url: work_item_url(org, target),
        name: None,
        comment: None,
    }
}

/// Parent, children and linked PRs as stored relations, server order.
fn relations(ds: &Dataset, wi: &WorkItem) -> Vec<WiLink> {
    let mut links = Vec::new();
    if let Some(parent) = wi.parent_id {
        links.push(wi_link(&ds.org, links.len(), LinkKind::Parent, parent));
    }
    for child in ds.work_items.iter().filter(|c| c.parent_id == Some(wi.id)) {
        links.push(wi_link(&ds.org, links.len(), LinkKind::Child, child.id));
    }
    for &pr in &wi.linked_pr_ids {
        let (project, repo) = ds
            .pr(pr)
            .map(|p| (p.project_id.clone(), p.repo_id.clone()))
            .unwrap_or_default();
        links.push(WiLink {
            index: links.len() as u32,
            rel: "ArtifactLink".into(),
            kind: None,
            target_id: None,
            pr_id: Some(pr),
            url: format!("vstfs:///Git/PullRequestId/{project}%2F{repo}%2F{pr}"),
            name: Some("Pull Request".into()),
            comment: None,
        });
    }
    links
}

/// The scenario bug's description (design §2.2 mock): heading, list,
/// code, link, mention and one image (which blocks editing).
fn scenario_description(ds: &Dataset) -> String {
    let teammate = ds
        .people
        .iter()
        .find(|p| p.id != ds.me && !p.is_group)
        .expect("a teammate");
    format!(
        "<div>Settlement runs at 00:05 UTC, but merchants in UTC+10 see the previous day's totals.</div>\
         <h2>Repro</h2>\
         <ul><li>Set the merchant timezone to <b>Australia/Sydney</b></li>\
         <li>Run the daily settlement job<ul><li>Expected: totals for the local calendar day</li></ul></li></ul>\
         <pre><code>SELECT * FROM settlements WHERE day = :local_day</code></pre>\
         <div>Runbook: <a href=\"https://wiki.contoso.com/payments/settlement\">settlement job</a>. \
         {} saw it on 3 merchants this week.</div>\
         <div><img src=\"https://dev.azure.com/contoso/_apis/wit/attachments/1\" alt=\"Dashboard\"></div>",
        mention(teammate)
    )
}

const MARKDOWN_DESCRIPTION: &str = "## Goal\n\nFinance reconciles ledger entries **daily** instead of weekly.\n\n- [x] Export the ledger as CSV\n- [ ] Match entries against the bank feed\n- [ ] Flag mismatches over `0.01`\n\n> Finance signs off before we ship.\n";

const HTML_DESCRIPTION: &str = "<div>Refunds over the original amount should be refused before they reach the processor.</div><ol><li>Compare the refund with the captured amount</li><li>Show <i>Refund exceeds payment</i> in the console</li></ol><div>See <a href=\"https://wiki.contoso.com/payments/refunds\">refund rules</a>.</div>";

const BLOCKED_DESCRIPTION: &str = "<div>Latency by region after the cache change:</div><table><tr><th>Region</th><th>p50</th><th>p99</th></tr><tr><td>EU</td><td>41 ms</td><td>180 ms</td></tr><tr><td>US</td><td>38 ms</td><td>162 ms</td></tr></table><div><img src=\"https://dev.azure.com/contoso/_apis/wit/attachments/2\" alt=\"Before\"><img src=\"https://dev.azure.com/contoso/_apis/wit/attachments/3\" alt=\"After\"></div><div style=\"color: red\">Do not roll back without paging the on-call.</div>";

/// Iterations 41–44 two weeks apart: 43 is current at `now`, 44 next.
fn iteration_tree(project: &Project, now: Millis) -> ClassTree {
    let today = now.div_euclid(DAY);
    let current_start = today - 4;
    let node = |path: String, name: &str, depth: u16, dates: Option<(i64, i64)>| ClassNode {
        kind: NodeKind::Iteration,
        path,
        name: name.to_owned(),
        depth,
        start_ms: dates.map(|(s, _)| s * DAY),
        finish_ms: dates.map(|(_, f)| f * DAY),
    };
    let mut iterations = vec![node(project.name.clone(), &project.name, 0, None)];
    for (ix, sprint) in vocab::SPRINTS.iter().enumerate() {
        // Sprint 43 (ix 2) is current.
        let start = current_start + (ix as i64 - 2) * 14;
        iterations.push(node(
            format!("{}\\{sprint}", project.name),
            sprint,
            1,
            Some((start, start + 11)),
        ));
    }
    let area = |path: String, name: &str, depth: u16| ClassNode {
        kind: NodeKind::Area,
        path,
        name: name.to_owned(),
        depth,
        start_ms: None,
        finish_ms: None,
    };
    let mut areas = vec![area(project.name.clone(), &project.name, 0)];
    for name in vocab::AREAS {
        areas.push(area(format!("{}\\{name}", project.name), name, 1));
    }
    ClassTree {
        areas,
        iterations,
        fetched_ms: now,
    }
}

/// Write the 0015 content for the seeded work items (inside the seed
/// transaction, after the items themselves).
pub fn seed(tx: &Connection, ds: &Dataset) -> adoken_store::Result<()> {
    for project in &ds.projects {
        put_class_tree(tx, &project.id, &iteration_tree(project, ds.now))?;
    }
    for (ix, wi) in ds.work_items.iter().enumerate() {
        let mine = wi.assigned_to.as_deref() == Some(ds.me.as_str())
            || wi.created_by == ds.me
            || wi.changed_by == ds.me;
        let special = [SCENARIO, MARKDOWN, HTML, BLOCKED].contains(&ix);
        if !mine && !special && wi.id % 6 == 0 {
            // A title-index row: freshness reads "Partly synced".
            continue;
        }
        let mut full = FullFields {
            description: wi
                .description
                .as_deref()
                .map(|d| html(TextFormat::Html, as_html(d))),
            links: relations(ds, wi),
            ..FullFields::default()
        };
        match ix {
            SCENARIO => {
                full.description = Some(html(TextFormat::Html, scenario_description(ds)));
                let related = ds.work_items[HTML].id;
                full.links
                    .insert(0, wi_link(&ds.org, 0, LinkKind::Related, related));
                full.links.push(WiLink {
                    index: 0,
                    rel: "ArtifactLink".into(),
                    kind: None,
                    target_id: None,
                    pr_id: None,
                    url: "vstfs:///Git/Commit/payments%2Fpayments-api%2F3f9c2a1b7d4e".into(),
                    name: Some("Fixed in Commit".into()),
                    comment: None,
                });
                full.links.push(WiLink {
                    index: 0,
                    rel: "Hyperlink".into(),
                    kind: None,
                    target_id: None,
                    pr_id: None,
                    url: "https://status.contoso.com/incidents/42".into(),
                    name: None,
                    comment: Some("Incident 42".into()),
                });
                for (i, link) in full.links.iter_mut().enumerate() {
                    link.index = i as u32;
                }
                full.texts.push((
                    REPRO_STEPS.into(),
                    html(
                        TextFormat::Html,
                        "<ol><li>Set merchant timezone to Australia/Sydney</li><li>Wait for the 00:05 UTC run</li><li>Open the daily totals</li></ol>",
                    ),
                ));
            }
            MARKDOWN => {
                full.description = Some(html(TextFormat::Markdown, MARKDOWN_DESCRIPTION));
                full.texts.push((
                    ACCEPTANCE.into(),
                    html(
                        TextFormat::Markdown,
                        "- Daily export lands by 07:00\n- Mismatches are flagged in the report\n",
                    ),
                ));
            }
            HTML => full.description = Some(html(TextFormat::Html, HTML_DESCRIPTION)),
            BLOCKED => full.description = Some(html(TextFormat::Html, BLOCKED_DESCRIPTION)),
            _ => {}
        }
        record_answer(tx, wi.id, wi.rev, ds.now, Some(&full))?;
    }
    Ok(())
}
