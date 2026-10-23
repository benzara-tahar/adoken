//! Sync (spec 0002 FR-4a, spec 0009; user decision 2026-09-30): how often
//! each kind syncs, background behavior, pause, Sync now with the last sync
//! per kind, and a live estimate of the Azure DevOps rate-limit share.
//! Settings owns `[sync]`; edits reach the running scheduler at once.

use adoken_model::Millis;
use adoken_sync::cadence::{
    self, Background, Cadence, INDEX_BG, INDEX_FG, Kind, Lane, Pause, Prefs, STEADY_TARGET_TSTU,
};
use adoken_theme::space;
use adoken_ui::{
    Button, ButtonVariant, Chip, ChipTone, IconName, Kbd, Notice, Segmented, Select, Switch, Theme,
};
use gpui::{AnyElement, Context, div, prelude::*, px};

use super::{row, section, value_label};
use crate::activity::settings::{
    ACTIVITY_LINK, ACTIVITY_LINK_KEYWORDS, NETWORK_INDICATOR, NETWORK_INDICATOR_KEYWORDS,
};
use crate::settings::sync_prefs::{self as codec, SYNC_TABLE};
use crate::settings::view::SettingsView;

/// Pause choices, in Segmented order.
const PAUSE_CHOICES: [&str; 3] = ["Off", "For 1 hour", "Until resumed"];

fn pause_index(pause: Pause, now: Millis) -> usize {
    match pause {
        Pause::UntilResumed => 2,
        Pause::Until(_) if pause.active(now) => 1,
        _ => 0,
    }
}

/// The local UTC offset at `at` (`localtime_r`, safe with many threads).
fn local_offset(at: Millis) -> time::UtcOffset {
    use adoken_notify::Clock as _;
    time::UtcOffset::from_whole_seconds(adoken_notify::SystemClock.offset_at(at))
        .unwrap_or(time::UtcOffset::UTC)
}

impl SettingsView {
    fn sync_prefs(&self) -> Prefs {
        codec::parse(&self.doc).0
    }

    fn edit_sync(&mut self, change: impl FnOnce(&mut Prefs), cx: &mut Context<Self>) {
        let mut prefs = self.sync_prefs();
        change(&mut prefs);
        let table = codec::table(&prefs);
        self.edit_table(SYNC_TABLE, |current| *current = table, cx);
    }

    pub(super) fn sync(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let prefs = self.sync_prefs();
        let now = self.now_ms();
        let activity = self.sync_activity(cx);
        let connected = activity.is_some();
        let activity = activity.unwrap_or_default();
        let mut blocks: Vec<Option<AnyElement>> = Vec::new();

        if !connected && self.visible("Sync now", "refresh reload fetch last synced") {
            let message = if self.demo {
                "Demo data doesn't sync. These settings apply once you connect Azure DevOps."
            } else {
                "Sync isn't running (not connected, or no projects selected)."
            };
            blocks.push(Some(
                Notice::new(ChipTone::Neutral, message)
                    .icon(IconName::Info)
                    .into_any_element(),
            ));
        }

        // ── now ────────────────────────────────────────────────────────
        let mut status = Vec::new();
        if self.visible("Sync now", "refresh reload fetch last synced") {
            let last = Kind::ALL
                .into_iter()
                .filter_map(|k| activity.last_ok(k))
                .chain(activity.last_ok[Lane::Index.index()])
                .max();
            let description = match codec::pause_label(prefs.pause, now, local_offset(now)) {
                Some(paused) => format!("{paused}. Sync now still syncs once."),
                None => codec::last_sync_label(last, now),
            };
            status.push(row(
                theme,
                "Sync now",
                Some(description.into()),
                Button::new("sync-now", "Sync now")
                    .icon(IconName::RefreshCw)
                    .shortcut(Kbd::parse(crate::sync::SYNC_NOW_KEYS).expect("static binding"))
                    .disabled(!connected)
                    .on_press(|_, cx| crate::sync::sync_now(cx)),
            ));
        }
        if self.visible("Pause sync", "pause resume stop hold hour") {
            status.push(row(
                theme,
                "Pause sync",
                Some("Stops scheduled sync. Your own writes and Sync now still go out.".into()),
                Segmented::new("sync-pause", PAUSE_CHOICES, pause_index(prefs.pause, now))
                    .on_select(self.on(cx, move |this, ix: usize, _, cx| {
                        let now = this.now_ms();
                        let pause = match ix {
                            1 => Pause::Until(now + codec::PAUSE_FOR),
                            2 => Pause::UntilResumed,
                            _ => Pause::Off,
                        };
                        this.edit_sync(|p| p.pause = pause, cx);
                    })),
            ));
        }
        blocks.push(section(theme, "Status", None, status));

        // ── activity (spec 0016 FR-28) ─────────────────────────────────
        let mut activity_rows = Vec::new();
        if self.visible(ACTIVITY_LINK, ACTIVITY_LINK_KEYWORDS) {
            activity_rows.push(row(
                theme,
                ACTIVITY_LINK,
                Some(
                    "What each kind syncs, when it syncs next, and what the last passes \
                     changed."
                        .into(),
                ),
                Button::new("sync-open-activity", "Open sync activity")
                    .variant(ButtonVariant::Ghost)
                    .shortcut(Kbd::parse(crate::activity::TOGGLE_KEYS).expect("static binding"))
                    .on_press(|_, cx| {
                        crate::activity::request_open(crate::activity::Tab::Sync, cx)
                    }),
            ));
        }
        if self.visible(NETWORK_INDICATOR, NETWORK_INDICATOR_KEYWORDS) {
            let on = crate::activity::settings::parse(&self.doc, None)
                .0
                .network_indicator;
            activity_rows.push(row(
                theme,
                NETWORK_INDICATOR,
                Some(
                    "A quiet ⇅ with the last request's status that counts failures. The \
                     network log stays a ⌘⇧A away either way."
                        .into(),
                ),
                Switch::new("sync-network-indicator", on).on_toggle(self.on(
                    cx,
                    |this, on: bool, _, cx| {
                        this.edit_table(
                            crate::activity::settings::TABLE,
                            |table| {
                                table.insert("network_indicator".into(), on.into());
                            },
                            cx,
                        )
                    },
                )),
            ));
        }
        blocks.push(section(theme, "Activity", None, activity_rows));

        // ── how often ──────────────────────────────────────────────────
        let mut cadences = Vec::new();
        for kind in Kind::ALL {
            if !self.visible(codec::title(kind), codec::keywords(kind)) {
                continue;
            }
            let choices: Vec<Cadence> = Cadence::choices(kind).collect();
            let current = prefs.cadence(kind);
            let selected = choices.iter().position(|c| *c == current).unwrap_or(0);
            let last = match current {
                Cadence::Manual if activity.last_ok(kind).is_none() => {
                    "Only on Sync now".to_owned()
                }
                _ => codec::last_sync_label(activity.last_ok(kind), now),
            };
            cadences.push(row(
                theme,
                codec::title(kind),
                Some(codec::description(kind).into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S12))
                    .when(connected, |d| d.child(value_label(theme, last)))
                    .child(
                        Select::new(
                            ("sync-cadence", kind.index()),
                            choices.iter().map(|c| codec::cadence_label(*c)),
                            selected,
                        )
                        .width(140.0)
                        .on_select(self.on(
                            cx,
                            move |this, ix: usize, _, cx| {
                                let cadence = Cadence::choices(kind)
                                    .nth(ix)
                                    .unwrap_or(kind.default_cadence());
                                this.edit_sync(|p| p.set_cadence(kind, cadence), cx);
                            },
                        )),
                    ),
            ));
        }
        if self.visible(
            "Search index and projects",
            "search index projects repos title",
        ) {
            cadences.push(row(
                theme,
                "Search index and projects",
                Some(
                    format!(
                        "Every {} ({} in the background). Not adjustable: it's the cheapest \
                         kind and keeps ⌘P search complete.",
                        codec::interval_label(INDEX_FG),
                        codec::interval_label(INDEX_BG)
                    )
                    .into(),
                ),
                div().when(connected, |d| {
                    d.child(value_label(
                        theme,
                        codec::last_sync_label(activity.last_ok[Lane::Index.index()], now),
                    ))
                }),
            ));
        }
        blocks.push(section(
            theme,
            "How often",
            Some(
                "While adoken is in front. Azure DevOps can't push to a desktop app, so adoken \
                 polls; its rate governor slows down on its own long before you'd be throttled.",
            ),
            cadences,
        ));

        // ── background ─────────────────────────────────────────────────
        let mut background = Vec::new();
        if self.visible(
            "In the background",
            "background inactive unfocused slower battery",
        ) {
            let selected = Background::ALL
                .iter()
                .position(|b| *b == prefs.background)
                .unwrap_or(1);
            let effect = match prefs.background {
                Background::Same => "Same intervals as in front.".to_owned(),
                Background::Slower => match prefs.interval(Lane::MyPrs, false) {
                    Some(ms) => format!(
                        "Every interval ×{}: my pull requests every {}.",
                        cadence::SLOWER_FACTOR,
                        codec::interval_label(ms)
                    ),
                    None => format!("Every interval ×{}.", cadence::SLOWER_FACTOR),
                },
                Background::Paused => "Nothing syncs until you come back.".to_owned(),
            };
            background.push(row(
                theme,
                "In the background",
                Some(
                    format!("When adoken hasn't been the active app for 5 minutes. {effect}")
                        .into(),
                ),
                Segmented::new(
                    "sync-background",
                    Background::ALL.map(codec::background_label),
                    selected,
                )
                .on_select(self.on(cx, |this, ix: usize, _, cx| {
                    let background = Background::ALL[ix];
                    this.edit_sync(|p| p.background = background, cx);
                })),
            ));
        }
        blocks.push(section(theme, "In the background", None, background));

        // ── budget ─────────────────────────────────────────────────────
        let mut budget = Vec::new();
        if self.visible(
            "Rate limit",
            "budget tstu throttle usage azure devops estimate",
        ) {
            // Estimate for the foreground: what the user is tuning.
            let tstu = cadence::estimate_tstu(&prefs, true, &activity.pass_cost);
            let (headline, detail) = codec::budget_copy(tstu, activity.window_cost);
            let (tone, label) = if prefs.pause.active(now) {
                (ChipTone::Neutral, "Paused")
            } else if tstu > STEADY_TARGET_TSTU {
                (ChipTone::Warning, "Above target")
            } else {
                (ChipTone::Success, "Within budget")
            };
            budget.push(row(
                theme,
                headline,
                Some(detail.into()),
                Chip::new(label).tone(tone),
            ));
        }
        blocks.push(section(theme, "Rate limit", None, budget));
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_choice_follows_the_clock() {
        assert_eq!(pause_index(Pause::Off, 10), 0);
        assert_eq!(pause_index(Pause::Until(20), 10), 1);
        assert_eq!(
            pause_index(Pause::Until(20), 30),
            0,
            "expired pause shows Off"
        );
        assert_eq!(pause_index(Pause::UntilResumed, 10), 2);
    }
}
