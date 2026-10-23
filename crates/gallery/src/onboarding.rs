//! Onboarding screens (spec 0001 AC-24, AC-25): Welcome with its image, and
//! the three screens that list every scope of the chosen level.

use adoken_app::onboarding::{Onboarding, Story};
use gpui::{AnyElement, App, Window, div, prelude::*};

fn view(key: &'static str, story: Story, window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        Onboarding::story(story, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// Screen 0: the welcome image above the title, description and Get started.
pub fn welcome_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view("onboarding-welcome", Story::Welcome, window, cx)
}

/// 2b: Review access through `az`, every scope with its id and what it allows.
pub fn review_access_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view("onboarding-review-access", Story::ReviewAccess, window, cx)
}

/// 2p: create the token in the browser, the same scope list to tick.
pub fn create_in_browser_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "onboarding-create-in-browser",
        Story::CreateInBrowser,
        window,
        cx,
    )
}

/// 2q after verification: write scopes untested, Build (Read) missing.
pub fn paste_verify_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view("onboarding-paste-verify", Story::PasteVerify, window, cx)
}
