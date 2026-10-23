//! Settings › Keyboard › Global shortcut (spec 0021 AC-14): the section in
//! each state over a fake registrar (no Carbon), one story per state.

use adoken_app::hotkey::fixtures::{Scene, install_for_story, remove_for_story};
use adoken_app::settings::{SettingsView, Tab};
use gpui::{AnyElement, App, Window, div, prelude::*};

fn keyboard(key: &'static str, scene: Scene, window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        install_for_story(scene, cx);
        let view = SettingsView::story(Tab::Keyboard, "", true, window, cx);
        // The view holds its shortcuts; other stories never see them.
        remove_for_story(cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

macro_rules! scene_story {
    ($name:ident, $key:literal, $scene:expr) => {
        pub fn $name(window: &mut Window, cx: &mut App) -> AnyElement {
            keyboard($key, $scene, window, cx)
        }
    };
}

scene_story!(
    untested,
    "settings-keyboard-global-untested",
    Scene::Untested
);
scene_story!(works, "settings-keyboard-global-works", Scene::Works);
scene_story!(
    recording,
    "settings-keyboard-global-recording",
    Scene::Recording
);
scene_story!(refused, "settings-keyboard-global-refused", Scene::Refused);
scene_story!(failed, "settings-keyboard-global-failed", Scene::Failed);
scene_story!(off, "settings-keyboard-global-off", Scene::Off);
