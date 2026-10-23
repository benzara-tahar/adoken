//! Spec 0015 §7: rich text parse of a 20 KB description ≤ 5 ms on a
//! background executor. Run: `cargo bench -p adoken-richtext --profile dev`.

use adoken_richtext::{Format, parse};
use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

/// ~20 KB of Azure DevOps editor HTML: styled div soup, lists, links,
/// mentions, a table, code.
pub fn html_fixture() -> String {
    let unit = "<div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">Settlement runs at <b>00:05 UTC</b>, but merchants in UTC+10 see the previous day's totals. See <a href=\"https://dev.azure.com/org/Payments/_workitems/edit/4821\">#4821</a> and https://learn.microsoft.com/azure/devops.</span><br style=\"box-sizing:border-box;\"> </div>\
<ul><li>Repro: set merchant tz to <code>Australia/Sydney</code></li><li>Expected: totals for the local calendar day<ul><li>nested <i>detail</i></li></ul></li></ul>\
<div><a href=\"#\" data-vss-mention=\"version:2.0,d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607\">@Dana Kim</a> can you verify on staging?</div>\
<table><tr><th>Region</th><th>Offset</th></tr><tr><td>AU</td><td>+10</td></tr></table>\
<pre>fn settle(day: Date) -&gt; Total {\n    todo()\n}</pre>";
    let mut html = String::new();
    while html.len() < 20 * 1024 {
        html.push_str(unit);
    }
    html
}

fn markdown_fixture() -> String {
    let unit = "## Settlement\n\nSettlement runs at **00:05 UTC**, but merchants in UTC+10 see the previous day's totals. See #4821 and <https://learn.microsoft.com/azure/devops>.\n\n- Repro: set merchant tz to `Australia/Sydney`\n- Expected: totals for the local calendar day\n  - nested *detail*\n- [x] checked\n\n@<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> can you verify on staging?\n\n| Region | Offset |\n| --- | --- |\n| AU | +10 |\n\n```\nfn settle(day: Date) -> Total {\n    todo()\n}\n```\n\n";
    let mut md = String::new();
    while md.len() < 20 * 1024 {
        md.push_str(unit);
    }
    md
}

fn bench(c: &mut Criterion) {
    let html = html_fixture();
    let md = markdown_fixture();
    c.bench_function("parse 20 KB html", |b| {
        b.iter(|| parse(black_box(&html), Format::Html))
    });
    c.bench_function("parse 20 KB markdown", |b| {
        b.iter(|| parse(black_box(&md), Format::Markdown))
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
