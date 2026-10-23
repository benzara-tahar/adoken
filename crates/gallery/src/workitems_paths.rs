//! The work item list's area and iteration dropdown (spec 0008 FR-8,
//! AC-14) over a small fixture store: P1 "Storefront" with stored trees,
//! P2 "Payments" with or without them. The clock is pinned at `DEMO_NOW`;
//! edits are local, nothing is sent.

use adoken_app::workitems::StoryHook;
use adoken_app::workitems::model::Tab;
use adoken_app::workitems::paths::story;
use gpui::{AnyElement, App, Window, div, prelude::*};

struct Setup {
    two_projects: bool,
    p2_tree: bool,
    tab: Tab,
    query: &'static str,
    p2_loading: bool,
}

fn render(key: &'static str, setup: Setup, window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        let now = adoken_demo::DEMO_NOW;
        let dir = crate::story_dir(key);
        let store = story::seed(&dir, setup.two_projects, setup.p2_tree, now);
        let (tab, query, loading) = (setup.tab, setup.query, setup.p2_loading);
        let hook: StoryHook = Box::new(move |view, window, cx| {
            view.story_path_filter(tab, query, window, cx);
            if loading {
                view.story_tree_loading(story::P2, cx);
            }
        });
        story::view(store, setup.tab, now, Some(hook), window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// One project: one root, expanded, counts per node, the current path
/// selected.
pub fn area_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let setup = Setup {
        two_projects: false,
        p2_tree: false,
        tab: Tab::Area,
        query: "",
        p2_loading: false,
    };
    render("work-items-area-dropdown", setup, window, cx)
}

/// Two projects, both trees stored: two roots.
pub fn area_projects_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let setup = Setup {
        two_projects: true,
        p2_tree: true,
        tab: Tab::Area,
        query: "",
        p2_loading: false,
    };
    render("work-items-area-dropdown-projects", setup, window, cx)
}

/// "infra" typed: matches in both projects with their ancestors.
pub fn area_filtered_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let setup = Setup {
        two_projects: true,
        p2_tree: true,
        tab: Tab::Area,
        query: "infra",
        p2_loading: false,
    };
    render("work-items-area-dropdown-filtered", setup, window, cx)
}

/// P2's tree still on its way: its loaded paths meanwhile, and the footer.
pub fn area_loading_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let setup = Setup {
        two_projects: true,
        p2_tree: false,
        tab: Tab::Area,
        query: "",
        p2_loading: true,
    };
    render("work-items-area-dropdown-loading", setup, window, cx)
}

/// Iterations: Current / Next / Recent per project, each row naming it.
pub fn iteration_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let setup = Setup {
        two_projects: true,
        p2_tree: true,
        tab: Tab::Iteration,
        query: "",
        p2_loading: false,
    };
    render("work-items-iteration-dropdown", setup, window, cx)
}
