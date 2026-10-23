//! The statusbar's activity items (spec 0016 FR-5, FR-9, FR-21; activity.md
//! §2): the sync item (a button opening Activity › Sync, now with the
//! paused state), the network indicator (a button opening Activity ›
//! Network) and the resource meter (spec 0023, §2.7), plus the 900 px
//! collapse order (§2.5).

use adoken_model::Millis;
use adoken_sync::Phase;
use adoken_sync::cadence::Pause;
use adoken_theme::{motion, radius, size, space, typography};
use adoken_ui::{Icon, IconName, Theme, Tooltip, TypeStyleExt};
use gpui::{
    AnyElement, App, ClickEvent, SharedString, Transformation, Window, div, prelude::*, px, radians,
};

use super::indicator::{IndicatorView, ProblemTone};
use super::passes::{clock, spoken};
use crate::sync::{SyncState, Tone};

/// Where the store is (the shell's load state).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Opening,
    Failed,
    Ready,
}

/// The sync item's leading glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncGlyph {
    Dot(Tone),
    /// Lucide `pause`, `text.secondary` (FR-9).
    Paused,
}

/// What the sync item says (activity.md §2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncItem {
    pub glyph: SyncGlyph,
    pub label: String,
    /// The §2.5 short form.
    pub short: String,
    /// Accessible label (state + how to open).
    pub spoken: String,
    pub tooltip: String,
}

/// `label` → its short form: "Synced 12s ago" → "12s ago", …
fn short_form(label: &str) -> String {
    if let Some(rest) = label.strip_prefix("Synced ") {
        return rest.to_owned();
    }
    if let Some(rest) = label.strip_prefix("Offline — last synced ") {
        return format!("Offline · {rest}");
    }
    if let Some(rest) = label.strip_prefix("Throttled by Azure DevOps — resuming in ") {
        return format!("Throttled · {rest}");
    }
    match label {
        "Opening local data…" => "Opening…".into(),
        "Couldn't open local data" => "Local data error".into(),
        "No token in Keychain" => "No token".into(),
        "Not synced yet" => "Not synced".into(),
        other => other.to_owned(),
    }
}

/// The sync item at `now`. Paused (FR-9) shows while a pause is active and
/// no pass runs; unauthorized, throttled and offline still win, and a ⌘R
/// pass during a pause reads "Syncing…". `token_expired`: the stored token's
/// `validTo` is known and past, so a rejection reads "Token expired" (spec
/// 0001 FR-15).
#[allow(clippy::too_many_arguments)]
pub fn sync_item(
    load: LoadState,
    demo: bool,
    state: Option<&SyncState>,
    last_sync: Option<Millis>,
    pause: Pause,
    token_expired: bool,
    offset: time::UtcOffset,
    now: Millis,
) -> SyncItem {
    let (glyph, label) = match load {
        LoadState::Opening => (SyncGlyph::Dot(Tone::Busy), "Opening local data…".to_owned()),
        LoadState::Failed => (
            SyncGlyph::Dot(Tone::Error),
            "Couldn't open local data".to_owned(),
        ),
        LoadState::Ready if demo => (SyncGlyph::Dot(Tone::Busy), "Demo data".to_owned()),
        LoadState::Ready => {
            let held = match state {
                Some(SyncState::Running(status)) => status.phase == Phase::Idle,
                None => true,
                Some(_) => false,
            };
            if held && pause.active(now) {
                let label = match pause {
                    Pause::Until(at) => format!("Sync paused · until {}", clock(at, offset)),
                    _ => "Sync paused".to_owned(),
                };
                (SyncGlyph::Paused, label)
            } else {
                match state {
                    Some(SyncState::Running(status))
                        if token_expired && status.phase == Phase::Unauthorized =>
                    {
                        (SyncGlyph::Dot(Tone::Error), "Token expired".to_owned())
                    }
                    Some(state) => {
                        let (tone, label) = crate::sync::label(state, last_sync, now);
                        (SyncGlyph::Dot(tone), label)
                    }
                    None => match last_sync {
                        Some(at) => (
                            SyncGlyph::Dot(Tone::Ok),
                            format!("Synced {}", super::passes::ago(now - at)),
                        ),
                        None => (SyncGlyph::Dot(Tone::Quiet), "Not synced yet".to_owned()),
                    },
                }
            }
        }
    };
    let paused = glyph == SyncGlyph::Paused;
    let short = if paused {
        "Paused".to_owned()
    } else {
        short_form(&label)
    };
    let state_spoken = match label.as_str() {
        "Demo data" => "Demo data, not synced".to_owned(),
        "Token rejected" => "Token rejected. Azure DevOps refused the token".to_owned(),
        "Token expired" => "Token expired. Azure DevOps refused the token".to_owned(),
        _ if paused => format!("{}. Command R still syncs once", spoken(&label)),
        other => spoken(other),
    };
    let sentence = match (last_sync, label.starts_with("Synced ")) {
        (Some(at), true) => format!("Synced at {}", super::net::time_text(at, offset, false)),
        _ => label.clone(),
    };
    let mut tooltip = format!("{sentence} · Sync activity ⌘⇧A · Sync now ⌘R");
    if paused {
        tooltip.push_str(" · ⌘R still syncs once");
    }
    SyncItem {
        glyph,
        spoken: format!("{state_spoken}. Show sync activity, Command Shift A"),
        label,
        short,
        tooltip,
    }
}

// ── width priority (activity.md §2.5) ───────────────────────────────────

/// ~6.3 px per `type.small` character at `font_scale` 1.0.
const CHAR_WIDTH: f32 = 6.3;
/// The org truncates to this before it hides.
pub const ORG_MIN: f32 = 64.0;
const KEN_CHIP: f32 = 95.0;
const GLYPH: f32 = 12.0;
/// The meter's reserved text (spec 0023 FR-8): values inside this range
/// never move a neighbor.
pub const METER_RESERVE: &str = "990 MB · 99% CPU";
pub const METER_RESERVE_SHORT: &str = "990 MB";

/// What the statusbar must fit.
#[derive(Clone, Debug, Default)]
pub struct BarParts<'a> {
    pub sync_label: &'a str,
    pub sync_short: &'a str,
    pub org: Option<&'a str>,
    /// Network value text (`None` when hidden or glyph only).
    pub network_value: Option<&'a str>,
    pub network_shown: bool,
    pub network_chip: bool,
    pub ken: bool,
    /// Demo pill: full and short text.
    pub demo: Option<(&'a str, &'a str)>,
    /// Resource meter: full and short (memory only) text, when shown.
    pub meter: Option<(&'a str, &'a str)>,
}

/// How each item fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Collapse {
    pub org_truncated: bool,
    pub org_hidden: bool,
    pub demo_short: bool,
    pub network_value_hidden: bool,
    pub sync_short: bool,
    /// The meter drops its CPU part (§2.5 step 2).
    pub meter_short: bool,
    /// The meter hides (§2.5 step 4).
    pub meter_hidden: bool,
}

/// Collapse items in the §2.5 order until the bar fits `width` (the
/// notice always truncates first: it takes no room of its own).
pub fn collapse(width: f32, font_scale: f32, parts: &BarParts) -> Collapse {
    let ch = CHAR_WIDTH * font_scale;
    let text = |s: &str| s.chars().count() as f32 * ch;
    let inner = width - 2.0 * space::S12;
    let measure = |c: &Collapse| {
        let sync = GLYPH
            + space::S6
            + text(if c.sync_short {
                parts.sync_short
            } else {
                parts.sync_label
            })
            + 2.0 * space::S6;
        let org = match parts.org {
            Some(_) if c.org_hidden => 0.0,
            Some(_) if c.org_truncated => ORG_MIN + space::S6,
            Some(org) => text(&format!("· {org}")) + space::S6,
            None => 0.0,
        };
        let network = if parts.network_shown {
            let value = match parts.network_value {
                Some(v) if !c.network_value_hidden => space::S4 + text(v).max(3.0 * ch),
                _ => 0.0,
            };
            let chip = if parts.network_chip {
                space::S4 + GLYPH + space::S4 + 2.0 * ch + 2.0 * space::S4
            } else {
                0.0
            };
            GLYPH + value + chip + 2.0 * space::S6 + space::S8
        } else {
            0.0
        };
        let ken = if parts.ken {
            KEN_CHIP * font_scale + space::S8
        } else {
            0.0
        };
        let demo = match parts.demo {
            Some((_, short)) if c.demo_short => text(short) + 2.0 * space::S6 + space::S8,
            Some((full, _)) => text(full) + 2.0 * space::S6 + space::S8,
            None => 0.0,
        };
        let meter = match parts.meter {
            Some(_) if c.meter_hidden => 0.0,
            Some((full, short)) => {
                let (shown, reserve) = if c.meter_short {
                    (short, METER_RESERVE_SHORT)
                } else {
                    (full, METER_RESERVE)
                };
                text(shown).max(text(reserve)) + 2.0 * space::S6 + space::S8
            }
            None => 0.0,
        };
        sync + org + network + ken + meter + demo
    };
    let mut c = Collapse::default();
    let meter = parts.meter.is_some();
    let org = parts.org.is_some();
    // Order (amended by spec 0023 FR-8): meter drops CPU, org truncates,
    // org hides, meter hides, demo pill short, network value, sync label.
    type Step = (bool, fn(&mut Collapse));
    let steps: [Step; 7] = [
        (meter, |c| c.meter_short = true),
        (org, |c| c.org_truncated = true),
        (org, |c| c.org_hidden = true),
        (meter, |c| c.meter_hidden = true),
        (parts.demo.is_some(), |c| c.demo_short = true),
        (parts.network_value.is_some(), |c| {
            c.network_value_hidden = true
        }),
        (true, |c| c.sync_short = true),
    ];
    for (applies, step) in steps {
        if measure(&c) <= inner {
            break;
        }
        if applies {
            step(&mut c);
        }
    }
    c
}

// ── rendering ───────────────────────────────────────────────────────────

/// The sync item as a button that opens Activity › Sync.
pub fn render_sync_item(
    item: &SyncItem,
    short: bool,
    selected: bool,
    expanded: bool,
    theme: &Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let colors = &theme.colors;
    let glyph = match item.glyph {
        SyncGlyph::Dot(tone) => {
            let (dot, color) = crate::sync::indicator(tone, colors);
            div().text_color(color).child(dot).into_any_element()
        }
        SyncGlyph::Paused => Icon::new(IconName::Pause)
            .size(size::ICON_XS)
            .color(colors.text_secondary)
            .into_any_element(),
    };
    let label: SharedString = if short {
        item.short.clone().into()
    } else {
        item.label.clone().into()
    };
    let tooltip = item.tooltip.clone();
    let inner = div()
        .h_full()
        .px(px(space::S6))
        .flex()
        .items_center()
        .gap(px(space::S6))
        .rounded(px(radius::XS))
        .when(selected, |d| d.bg(colors.bg_selected_muted))
        .child(glyph)
        .child(div().min_w_0().truncate().child(label));
    // The wrapper is the polite live region (principles §6).
    div()
        .id("statusbar-sync-status")
        .min_w_0()
        .h_full()
        .role(gpui::Role::Status)
        .child(
            div()
                .id("statusbar-sync")
                .min_w_0()
                .h_full()
                .py(px(space::S2))
                .role(gpui::Role::Button)
                .aria_label(item.spoken.clone())
                .aria_expanded(expanded)
                .cursor_pointer()
                .group("statusbar-sync")
                .child(inner.group_hover("statusbar-sync", |d| d.bg(colors.bg_hover)))
                .tooltip(Tooltip::text(tooltip))
                .on_click(on_click),
        )
        .into_any_element()
}

/// The network indicator (activity.md §2.3), or nothing when hidden.
#[allow(clippy::too_many_arguments)]
pub fn render_network(
    view: &IndicatorView,
    value_hidden: bool,
    selected: bool,
    spin: f32,
    theme: &Theme,
    tooltip: impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    let colors = &theme.colors;
    let content = match view {
        IndicatorView::Hidden => return None,
        IndicatorView::Offline => div()
            .h(px(16.0))
            .px(px(space::S6))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .rounded(px(radius::FULL))
            .bg(colors.bg_raised)
            .text_color(colors.text_secondary)
            .type_style(typography::LABEL, theme)
            .child(
                Icon::new(IconName::WifiOff)
                    .size(size::ICON_XS)
                    .color(colors.text_secondary),
            )
            .child("Offline"),
        IndicatorView::Shown {
            busy,
            value,
            problems,
        } => {
            let glyph = if *busy {
                Icon::new(IconName::LoaderCircle)
                    .size(size::ICON_XS)
                    .color(colors.status_info_fg)
                    .transform(Transformation::rotate(radians(
                        spin * std::f32::consts::TAU,
                    )))
            } else {
                Icon::new(IconName::ArrowUpDown)
                    .size(size::ICON_XS)
                    .color(colors.text_tertiary)
            };
            let value_color = if *busy {
                colors.text_secondary
            } else {
                colors.text_tertiary
            };
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .type_style(typography::LABEL, theme)
                .child(glyph)
                .when_some(value.clone().filter(|_| !value_hidden), |d, value| {
                    // Three tabular digits wide, so "200" → "3" → "200"
                    // never moves the glyph.
                    d.child(
                        div()
                            .min_w(px(3.0 * 7.0))
                            .text_color(value_color)
                            .child(value),
                    )
                })
                .when_some(*problems, |d, p| {
                    let (icon, fg, wash) = match p.tone {
                        ProblemTone::Danger => (
                            IconName::CircleX,
                            colors.status_danger_fg,
                            colors.status_danger_wash,
                        ),
                        ProblemTone::Warning => (
                            IconName::CircleAlert,
                            colors.status_warning_fg,
                            colors.status_warning_wash,
                        ),
                    };
                    d.child(
                        div()
                            .h(px(16.0))
                            .px(px(space::S4))
                            .flex()
                            .items_center()
                            .gap(px(space::S2))
                            .rounded(px(radius::FULL))
                            .bg(wash)
                            .text_color(colors.text_primary)
                            .child(Icon::new(icon).size(size::ICON_XS).color(fg))
                            .child(p.count().to_string()),
                    )
                })
        }
    };
    Some(
        div()
            .id("statusbar-network")
            .flex_none()
            .h_full()
            .relative()
            .cursor_pointer()
            .role(gpui::Role::Button)
            .aria_label(view.label())
            .aria_expanded(selected)
            .group("statusbar-network")
            // Hover / selected wash, inset 2 px top and bottom (§2.2).
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .my(px(space::S2))
                    .rounded(px(radius::XS))
                    .when(selected, |d| d.bg(colors.bg_selected_muted))
                    .group_hover("statusbar-network", |d| d.bg(colors.bg_hover)),
            )
            .child(
                div()
                    .relative()
                    .h_full()
                    .px(px(space::S6))
                    .flex()
                    .items_center()
                    .child(content),
            )
            .tooltip(tooltip)
            .on_click(on_click)
            .into_any_element(),
    )
}

/// The resource meter (spec 0023, activity.md §2.7): a button with a menu,
/// `type.small` in `text.tertiary` at every value (no tone, FR-13). Its
/// width is reserved for "990 MB · 99% CPU" so values inside that range
/// never move a neighbor (FR-8). `focus_tooltip` is the tooltip shown on
/// keyboard focus (FR-9); hover uses `tooltip`.
#[allow(clippy::too_many_arguments)]
pub fn render_meter(
    text: &crate::resource::meter::MeterText,
    short: bool,
    expanded: bool,
    focus: &gpui::FocusHandle,
    focus_tooltip: Option<gpui::AnyView>,
    bounds: adoken_ui::action_menu::BoundsSlot,
    theme: &Theme,
    tooltip: impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static,
    on_open: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let colors = &theme.colors;
    let (shown, reserve) = if short {
        (text.short(), METER_RESERVE_SHORT)
    } else {
        (text.full(), METER_RESERVE)
    };
    // The reserve sizes the slot; a longer value ("1.2 GB · 100% CPU")
    // grows it instead.
    let label = if shown.chars().count() > reserve.chars().count() {
        div().child(shown).into_any_element()
    } else {
        div()
            .relative()
            .child(div().invisible().child(reserve))
            .child(div().absolute().top_0().right_0().child(shown))
            .into_any_element()
    };
    let on_open = std::rc::Rc::new(on_open);
    let (on_click, on_right, on_key) = (on_open.clone(), on_open.clone(), on_open);
    div()
        .id("statusbar-meter")
        .track_focus(focus)
        .flex_none()
        .h_full()
        .relative()
        .cursor_pointer()
        .role(gpui::Role::Button)
        .aria_label(text.spoken())
        .aria_expanded(expanded)
        .group("statusbar-meter")
        .child(adoken_ui::action_menu::track_bounds(bounds))
        // Hover / pressed wash, inset 2 px top and bottom (§2.2).
        .child(
            div()
                .absolute()
                .inset_0()
                .my(px(space::S2))
                .rounded(px(radius::XS))
                .when(expanded, |d| d.bg(colors.bg_active))
                .group_hover("statusbar-meter", |d| d.bg(colors.bg_hover)),
        )
        .child(
            div()
                .relative()
                .h_full()
                .px(px(space::S6))
                .flex()
                .items_center()
                .text_color(colors.text_tertiary)
                .type_style(typography::SMALL, theme)
                .child(label),
        )
        .when(focus_tooltip.is_some(), |d| {
            d.child(adoken_ui::focus_ring(theme))
                .children(focus_tooltip.map(|view| {
                    // Origin at the meter's top-right; the tooltip hangs
                    // above it, right-aligned.
                    div().absolute().top_0().right_0().child(gpui::deferred(
                        gpui::anchored()
                            .anchor(gpui::Anchor::BottomRight)
                            .snap_to_window_with_margin(px(space::S8))
                            .child(div().mb(px(space::S4)).child(view)),
                    ))
                }))
        })
        .tooltip(tooltip)
        .on_click(move |_, window, cx| on_click(window, cx))
        .on_mouse_down(gpui::MouseButton::Right, move |_, window, cx| {
            cx.stop_propagation();
            on_right(window, cx)
        })
        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
            let keystroke = &event.keystroke;
            let plain = !keystroke.modifiers.modified();
            if plain && matches!(keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                on_key(window, cx);
            }
        })
        .into_any_element()
}

/// A zero-size polite live region carrying the latest announcement
/// (FR-4: state changes and failures, never one per request).
pub fn announcement(text: Option<&str>) -> AnyElement {
    div()
        .id("statusbar-announcement")
        .absolute()
        .size_0()
        .overflow_hidden()
        .role(gpui::Role::Status)
        .when_some(text.map(str::to_owned), |d, text| d.aria_label(text))
        .into_any_element()
}

/// Busy glyph rotation for this frame (0 under reduce motion).
pub fn spin_phase(busy: bool, window: &Window, cx: &mut App) -> f32 {
    if busy {
        adoken_ui::motion::pulse_phase(
            adoken_ui::motion::ms(motion::GLYPH_SPIN),
            window.current_view(),
            cx,
        )
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_sync::{SyncStatus, Tier};

    const NOW: Millis = super::super::fixtures::NOW;

    fn running(phase: Phase) -> SyncState {
        SyncState::Running(SyncStatus {
            phase,
            last_ok_hot: Some(NOW - 12_000),
            last_ok_warm: None,
            last_ok_cold: None,
        })
    }

    fn item(state: &SyncState, pause: Pause) -> SyncItem {
        item_with(state, pause, false)
    }

    fn item_with(state: &SyncState, pause: Pause, token_expired: bool) -> SyncItem {
        sync_item(
            LoadState::Ready,
            false,
            Some(state),
            None,
            pause,
            token_expired,
            time::UtcOffset::UTC,
            NOW,
        )
    }

    #[test]
    fn a_pause_replaces_synced_and_names_its_end() {
        // FR-9 / AC-A7: "Pause › For 1 hour" → paused until +1 h.
        let until = NOW + 3_600_000;
        let paused = item(&running(Phase::Idle), Pause::Until(until));
        assert_eq!(paused.glyph, SyncGlyph::Paused);
        let expected = format!("Sync paused · until {}", clock(until, time::UtcOffset::UTC));
        assert_eq!(paused.label, expected);
        assert_eq!(paused.short, "Paused");
        assert!(paused.tooltip.ends_with("⌘R still syncs once"));
        let forever = item(&running(Phase::Idle), Pause::UntilResumed);
        assert_eq!(forever.label, "Sync paused");
        // Resume: back to "Synced 12s ago".
        assert_eq!(
            item(&running(Phase::Idle), Pause::Off).label,
            "Synced 12s ago"
        );
        // An expired pause doesn't hold anything.
        assert_eq!(
            item(&running(Phase::Idle), Pause::Until(NOW - 1)).label,
            "Synced 12s ago"
        );
    }

    #[test]
    fn syncing_offline_throttled_and_rejected_beat_paused() {
        let pause = Pause::UntilResumed;
        assert_eq!(
            item(&running(Phase::Syncing(Tier::Hot)), pause).label,
            "Syncing…"
        );
        assert!(
            item(&running(Phase::Offline), pause)
                .label
                .starts_with("Offline")
        );
        assert_eq!(
            item(&running(Phase::Unauthorized), pause).label,
            "Token rejected"
        );
        let throttled = item(
            &running(Phase::Throttled {
                until: NOW + 40_000,
                resource: None,
            }),
            pause,
        );
        assert_eq!(throttled.short, "Throttled · 40s");
    }

    #[test]
    fn a_rejected_token_past_its_expiry_reads_token_expired() {
        // Spec 0001 FR-15 / AC-20: same danger tone, the expired wording.
        let expired = item_with(&running(Phase::Unauthorized), Pause::UntilResumed, true);
        assert_eq!(expired.glyph, SyncGlyph::Dot(Tone::Error));
        assert_eq!(
            (expired.label.as_str(), expired.short.as_str()),
            ("Token expired", "Token expired")
        );
        assert!(
            expired
                .spoken
                .starts_with("Token expired. Azure DevOps refused the token")
        );
        // A known past expiry doesn't change anything until sync is rejected.
        assert_eq!(
            item_with(&running(Phase::Idle), Pause::Off, true).label,
            "Synced 12s ago"
        );
    }

    #[test]
    fn demo_reads_demo_data_and_short_forms_follow_the_table() {
        let demo = sync_item(
            LoadState::Ready,
            true,
            None,
            None,
            Pause::Off,
            false,
            time::UtcOffset::UTC,
            NOW,
        );
        assert_eq!(
            (demo.label.as_str(), demo.short.as_str()),
            ("Demo data", "Demo data")
        );
        assert!(demo.spoken.starts_with("Demo data, not synced"));
        assert_eq!(item(&running(Phase::Idle), Pause::Off).short, "12s ago");
        assert_eq!(item(&SyncState::NoToken, Pause::Off).short, "No token");
    }

    #[test]
    fn at_900_px_items_give_way_in_order() {
        let throttled = "Throttled by Azure DevOps — resuming in 40s";
        let parts = BarParts {
            sync_label: throttled,
            sync_short: "Throttled · 40s",
            org: Some("contoso-engineering-platform"),
            network_value: Some("429"),
            network_shown: true,
            network_chip: true,
            ken: true,
            demo: None,
            meter: None,
        };
        assert_eq!(
            collapse(900.0, 1.0, &parts),
            Collapse::default(),
            "fits at 1.0"
        );
        // At font scale 1.3 a narrower bar no longer fits everything: the
        // org gives way first, the sync label last.
        let tight = collapse(860.0, 1.3, &parts);
        assert!(tight.org_truncated, "org gives way first");
        assert!(
            !tight.network_value_hidden && !tight.sync_short,
            "sync label last"
        );
        let tiny = collapse(520.0, 1.3, &parts);
        assert!(tiny.org_hidden && tiny.network_value_hidden && tiny.sync_short);
    }

    #[test]
    fn the_meter_gives_way_in_its_amended_place() {
        // Spec 0023 AC-7.
        let parts = |meter| BarParts {
            sync_label: "Throttled by Azure DevOps — resuming in 40s",
            sync_short: "Throttled · 40s",
            org: Some("contoso-engineering-platform"),
            network_value: Some("429"),
            network_shown: true,
            network_chip: true,
            ken: true,
            demo: Some(("Demo mode · fake data, stays on this Mac", "Demo mode")),
            meter,
        };
        let worst = parts(Some((METER_RESERVE, METER_RESERVE_SHORT)));
        let fits = BarParts {
            demo: None,
            ..worst.clone()
        };
        assert_eq!(
            collapse(900.0, 1.0, &fits),
            Collapse::default(),
            "fits at 1.0"
        );
        // Narrowing at 1.3 (a long notice takes no room of its own): CPU
        // goes before the org truncates; the meter hides after the org and
        // before the demo pill shortens.
        let mut seen_short = false;
        let mut seen_hidden = false;
        // From roomy (nothing collapsed) down to tiny: the demo pill makes
        // the bar wider than 900 px at 1.3.
        for width in (200..=1600).rev().step_by(4) {
            let c = collapse(width as f32, 1.3, &worst);
            if c.org_truncated {
                assert!(c.meter_short, "{width}: CPU drops before the org truncates");
            }
            if c.meter_hidden {
                assert!(c.org_hidden, "{width}: the meter hides after the org");
                seen_hidden = true;
            }
            if c.demo_short {
                assert!(c.meter_hidden, "{width}: before the demo pill shortens");
            }
            seen_short |= c.meter_short && !c.org_truncated;
        }
        assert!(seen_short && seen_hidden);
        // A value change inside the reserve never moves a neighbor.
        for width in (200..=900).step_by(4) {
            let a = collapse(
                width as f32,
                1.3,
                &parts(Some(("260 MB · 1% CPU", "260 MB"))),
            );
            let b = collapse(
                width as f32,
                1.3,
                &parts(Some(("270 MB · 2% CPU", "270 MB"))),
            );
            assert_eq!(a, b, "{width}");
        }
    }
}
