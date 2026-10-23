//! Pipelines view stories (spec 0008) over a seeded demo store (fake
//! Contoso data) with the clock pinned at `DEMO_NOW`.

use std::sync::LazyLock;

use adoken_app::pipelines::model::Tab;
use adoken_app::pipelines::story;
use adoken_demo::Dataset;
use gpui::{AnyElement, App, Window, div, prelude::*};

static DATASET: LazyLock<Dataset> =
    LazyLock::new(|| adoken_demo::dataset(adoken_demo::DEFAULT_SEED));

/// The demo's failed build on my PR !4291.
fn failed_pr_run() -> i64 {
    DATASET
        .runs
        .iter()
        .find(|r| r.pr_id == Some(4291) && r.result.is_some_and(|x| x.as_str() == "failed"))
        .map(|r| r.id)
        .expect("demo scenario run")
}

fn render(
    key: &'static str,
    tab: Tab,
    run: Option<i64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, |window, cx| {
        let dir = crate::story_dir(key);
        story::view(&dir, tab, run, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// Definitions with their last-runs sparklines; the busiest one's detail.
pub fn definitions_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render("pipelines", Tab::Pipelines, None, window, cx)
}

/// Failed runs with the failed PR build's stages, duration and linked PR.
pub fn failed_run_story(window: &mut Window, cx: &mut App) -> AnyElement {
    render(
        "pipelines-run",
        Tab::Failed,
        Some(failed_pr_run()),
        window,
        cx,
    )
}
