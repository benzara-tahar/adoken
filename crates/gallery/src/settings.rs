//! Settings window stories (spec 0009): one per tab over a fixed
//! `settings.toml`, the broken-file state, and the settings controls.

use adoken_app::code_font;
use adoken_app::settings::{AccountStory, SettingsView, Tab};
use adoken_app::theme_picker::library::install_for_story;
use adoken_theme::{AppearanceSettings, CodeFont, CodeFontSettings};
use adoken_theme::{space, typography};
use adoken_ui::{ActiveTheme, Select, Slider, Stepper, Switch, TypeStyleExt};
use gpui::{AnyElement, App, Window, div, prelude::*, px};

/// Stand-ins for the installed-font scan: families every Mac has, so the
/// picker's previews render the same on any snapshot machine.
const STORY_FAMILIES: [&str; 5] = ["Andale Mono", "Courier New", "Menlo", "Monaco", "PT Mono"];

/// Values away from the defaults so every control shows a state.
const SETTINGS: &str = r#"
[general]
motion = "reduced"

[sound]
enabled = true
master = 0.45

[notify]
show_self_caused = true
badge = "AllUnread"
[notify.quiet_hours]
enabled = true
let_p0_through = true
[[notify.quiet_hours.schedule.spans]]
days = ["mon", "tue", "wed", "thu", "fri"]
from = "19:00"
to = "08:00"
[notify.delivery]
p0 = "BannerSound"
p1 = "Banner"
p2 = "Banner"
p3 = "InboxOnly"

[nudges]
daily_cap = 5
days = ["mon", "tue", "wed", "thu"]
start = "08:30"
end = "17:00"
[nudges.N7]
enabled = false
threshold = "5d"
[nudges.N1]
enabled = true
threshold = "8h"
"#;

fn view(
    key: &'static str,
    tab: Tab,
    text: &'static str,
    demo: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        // The picker library and code font state are app globals; reset them
        // so a previous story's selection (e.g. theme-picker-search) can't
        // leak into this one.
        install_for_story(AppearanceSettings::default(), Vec::new(), Vec::new(), cx);
        code_font::install_for_story(CodeFontSettings::default(), None, cx);
        code_font::set_families_for_story(&STORY_FAMILIES, cx);
        SettingsView::story(tab, text, demo, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// Settings › Appearance searched for "code font" (what `> Code font…`
/// opens): the font list with the bundled font in use, size, ligatures and
/// the live diff preview.
pub fn code_font_story(window: &mut Window, cx: &mut App) -> AnyElement {
    code_font_view(
        "settings-code-font",
        CodeFontSettings::default(),
        None,
        window,
        cx,
    )
}

/// A configured family that isn't installed: the fallback notice, code
/// still in JetBrains Mono, nothing checked in the list.
pub fn code_font_missing_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let settings = CodeFontSettings {
        font: CodeFont::Installed("Fira Code".into()),
        ..CodeFontSettings::default()
    };
    code_font_view(
        "settings-code-font-missing",
        settings,
        Some("Fira Code"),
        window,
        cx,
    )
}

fn code_font_view(
    key: &'static str,
    settings: CodeFontSettings,
    missing: Option<&'static str>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        install_for_story(AppearanceSettings::default(), Vec::new(), Vec::new(), cx);
        code_font::install_for_story(settings, missing, cx);
        code_font::set_families_for_story(&STORY_FAMILIES, cx);
        let mut view = SettingsView::story(Tab::Appearance, SETTINGS, true, window, cx);
        view.search_for(code_font::SETTINGS_QUERY, window, cx);
        view
    });
    div().size_full().child(view).into_any_element()
}

macro_rules! tab_story {
    ($name:ident, $key:literal, $tab:expr, $demo:expr) => {
        pub fn $name(window: &mut Window, cx: &mut App) -> AnyElement {
            view($key, $tab, SETTINGS, $demo, window, cx)
        }
    };
}

tab_story!(general_story, "settings-general", Tab::General, true);
tab_story!(
    appearance_story,
    "settings-appearance",
    Tab::Appearance,
    true
);
tab_story!(
    backgrounds_story,
    "settings-backgrounds",
    Tab::Backgrounds,
    true
);
tab_story!(sounds_story, "settings-sounds", Tab::Sounds, true);
tab_story!(
    notifications_story,
    "settings-notifications",
    Tab::Notifications,
    true
);
tab_story!(nudges_story, "settings-nudges", Tab::Nudges, true);
tab_story!(keyboard_story, "settings-keyboard", Tab::Keyboard, true);
tab_story!(ken_story, "settings-ken", Tab::Ken, true);
tab_story!(
    account_demo_story,
    "settings-account-demo",
    Tab::Account,
    true
);

fn account_view(
    key: &'static str,
    story: AccountStory,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        install_for_story(AppearanceSettings::default(), Vec::new(), Vec::new(), cx);
        code_font::install_for_story(CodeFontSettings::default(), None, cx);
        SettingsView::account_story(story, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

macro_rules! account_story {
    ($name:ident, $key:literal, $story:expr) => {
        pub fn $name(window: &mut Window, cx: &mut App) -> AnyElement {
            account_view($key, $story, window, cx)
        }
    };
}

// Settings › Account (spec 0009 FR-8/FR-9, design §10.1).
account_story!(account_story, "settings-account", AccountStory::Az);
account_story!(
    account_pasted_story,
    "settings-account-pasted",
    AccountStory::Pasted
);
account_story!(
    account_rejected_story,
    "settings-account-rejected",
    AccountStory::Rejected
);
account_story!(
    account_expired_story,
    "settings-account-expired",
    AccountStory::Expired
);
account_story!(
    account_replaced_story,
    "settings-account-replaced",
    AccountStory::ReplacedAutomatically
);
tab_story!(about_story, "settings-about", Tab::About, true);

/// The Sync tab's fixed clock: 2026-09-30 12:00 UTC.
const SYNC_NOW: adoken_model::Millis = 1_790_769_600_000;

/// A connected org mid-morning: passes a few seconds to minutes ago and
/// measured costs per lane (a busy reviewer: ~6 TSTU per 5 min, ~3%).
fn sync_activity() -> adoken_sync::cadence::Activity {
    use adoken_model::time::{MINUTE, SECOND};
    let ago = |ms| Some(SYNC_NOW - ms);
    adoken_sync::cadence::Activity {
        // MyPrs, LiveRuns, AllPrs, WorkItems, DoneRuns, Index.
        last_ok: [
            ago(12 * SECOND),
            ago(12 * SECOND),
            ago(75 * SECOND),
            ago(4 * MINUTE),
            ago(75 * SECOND),
            ago(14 * MINUTE),
        ],
        pass_cost: [
            Some(0.8),
            Some(0.05),
            Some(0.25),
            Some(0.3),
            Some(0.2),
            Some(0.4),
        ],
        window_cost: 5.4,
        foreground: true,
    }
}

fn sync_view(
    key: &'static str,
    text: &'static str,
    activity: adoken_sync::cadence::Activity,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let view = window.use_keyed_state(key, cx, move |window, cx| {
        install_for_story(AppearanceSettings::default(), Vec::new(), Vec::new(), cx);
        SettingsView::sync_story(text, activity, SYNC_NOW, window, cx)
    });
    div().size_full().child(view).into_any_element()
}

/// Settings › Sync, tuned: my PRs every minute, pipelines at 30 s, work
/// items every 5 min; last sync per kind and the rate-limit estimate.
pub fn sync_story(window: &mut Window, cx: &mut App) -> AnyElement {
    sync_view(
        "settings-sync",
        "[sync]\nmy_prs = \"1m\"\nall_prs = \"2m\"\nwork_items = \"5m\"\npipelines = \"30s\"\n",
        sync_activity(),
        window,
        cx,
    )
}

/// Paused until resumed: the pause choice and the status line say so.
pub fn sync_paused_story(window: &mut Window, cx: &mut App) -> AnyElement {
    sync_view(
        "settings-sync-paused",
        "[sync]\npaused = true\n",
        sync_activity(),
        window,
        cx,
    )
}

/// Everything manual only and paused in the background: nothing has
/// synced on a schedule, the estimate drops to the search index.
pub fn sync_manual_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let mut activity = sync_activity();
    activity.last_ok = [None, None, None, None, None, Some(SYNC_NOW - 20 * 60_000)];
    activity.window_cost = 0.02;
    sync_view(
        "settings-sync-manual",
        "[sync]\nmy_prs = \"manual\"\nall_prs = \"manual\"\nwork_items = \"manual\"\n\
         pipelines = \"manual\"\nbackground = \"paused\"\n",
        activity,
        window,
        cx,
    )
}

/// A syntax error: the notice shows and nothing would be written.
pub fn broken_story(window: &mut Window, cx: &mut App) -> AnyElement {
    view(
        "settings-broken",
        Tab::Sounds,
        "[sound\nenabled = true\n",
        true,
        window,
        cx,
    )
}

/// The generic controls (crates/ui): switch, slider, select, stepper, in
/// their on/off/disabled states.
pub fn controls_story(_: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let label = |text: &'static str| {
        div()
            .w(px(90.0))
            .text_color(theme.colors.text_secondary)
            .type_style(typography::SMALL, theme)
            .child(text)
    };
    let line = || div().flex().items_center().gap(px(space::S16));
    div()
        .size_full()
        .p(px(space::S24))
        .flex()
        .flex_col()
        .gap(px(space::S16))
        .bg(theme.colors.bg_surface)
        .child(
            line()
                .child(label("Switch"))
                .child(Switch::new("on", true).on_toggle(|_, _, _| {}))
                .child(Switch::new("off", false).on_toggle(|_, _, _| {}))
                .child(Switch::new("on-disabled", true).disabled(true))
                .child(Switch::new("off-disabled", false).disabled(true)),
        )
        .child(
            line()
                .child(label("Slider"))
                .child(
                    Slider::new("slider", 0.35, 0.0..=0.9)
                        .step(0.05)
                        .on_change(|_, _, _| {}),
                )
                .child(
                    Slider::new("slider-disabled", 24.0, 0.0..=64.0)
                        .width(140.0)
                        .disabled(true),
                ),
        )
        .child(
            line()
                .child(label("Select"))
                .child(
                    Select::new("select", ["Every view", "Inbox", "PR review"], 1)
                        .on_select(|_, _, _| {}),
                )
                .child(
                    Select::new("select-disabled", ["09:00"], 0)
                        .width(96.0)
                        .disabled(true),
                ),
        )
        .child(
            line()
                .child(label("Stepper"))
                .child(
                    Stepper::new("stepper", 24, 1..=720)
                        .suffix("h")
                        .on_change(|_, _, _| {}),
                )
                .child(Stepper::new("stepper-min", 0, 0..=10).on_change(|_, _, _| {}))
                .child(
                    Stepper::new("stepper-disabled", 5, 1..=30)
                        .suffix("days")
                        .disabled(true),
                ),
        )
        .into_any_element()
}
