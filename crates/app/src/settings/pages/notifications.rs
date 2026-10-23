//! Notifications (spec 0005 flow step 5): what used to be the inbox's ⌘⇧,
//! panel, plus the OS banner permission row. Settings are `[notify]`;
//! mutes live in the inbox engine (local store) and are reached through
//! `crate::inbox`'s link.

use adoken_notify::{NotifySettings, Priority, settings::SETTINGS_TABLE};
use adoken_ui::{Button, ButtonVariant, Segmented, Select, Switch, Theme};
use gpui::{AnyElement, Context, div, prelude::*};

use super::{row, section, wide};
use crate::settings::notify_rules::{self as rules, Line, Setting, Value};
use crate::settings::view::SettingsView;

/// Choices with more (or longer) options than this become a dropdown.
const SEGMENTED_MAX: usize = 4;

/// Any priority now shows a banner that didn't before.
pub fn banners_added(before: &NotifySettings, after: &NotifySettings) -> bool {
    Priority::ALL
        .into_iter()
        .any(|p| !before.delivery.get(p).banner() && after.delivery.get(p).banner())
}

impl SettingsView {
    pub(crate) fn notify_settings(&self) -> NotifySettings {
        self.doc
            .get(SETTINGS_TABLE)
            .cloned()
            .and_then(|table| table.try_into().ok())
            .unwrap_or_default()
    }

    fn edit_notify(
        &mut self,
        change: impl FnOnce(&mut NotifySettings) -> bool,
        cx: &mut Context<Self>,
    ) {
        let before = self.notify_settings();
        let mut after = before.clone();
        if !change(&mut after) || after == before {
            return;
        }
        let Ok(table) = after.to_table() else {
            return;
        };
        self.edit_table(SETTINGS_TABLE, |current| *current = table, cx);
        // Turning banners on is the moment to ask macOS (no-op once decided).
        if banners_added(&before, &after) {
            crate::os_notify::request_permission(cx);
        }
    }

    pub(super) fn notifications(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let settings = self.notify_settings();
        let mutes = crate::inbox::mutes(cx);
        let mut sections = Vec::new();

        if self.visible("Banners", "permission macos system alerts")
            && let Some(permission) = self.embeds.permission.clone()
        {
            sections.push(section(
                theme,
                "Banners",
                Some("macOS decides whether adoken may show banners."),
                vec![wide(div().child(permission))],
            ));
        }

        let mut title: &'static str = "";
        let mut hint: &'static str = "";
        let mut rows = Vec::new();
        let flush = |sections: &mut Vec<Option<AnyElement>>,
                     title: &str,
                     hint: &str,
                     rows: &mut Vec<AnyElement>| {
            if !title.is_empty() {
                sections.push(section(theme, title, Some(hint), std::mem::take(rows)));
            }
        };
        for line in rules::lines(&settings, &mutes) {
            match line {
                Line::Header(next_title, next_hint) => {
                    flush(&mut sections, title, hint, &mut rows);
                    (title, hint) = (next_title, next_hint);
                }
                Line::Setting(setting) => {
                    let label = rules::label(&setting, &settings, &mutes);
                    if !self.visible(&label, &format!("{title} {hint}")) {
                        continue;
                    }
                    let control = self.notify_control(&setting, &settings, cx);
                    rows.push(row(theme, label, None, control));
                }
            }
        }
        flush(&mut sections, title, hint, &mut rows);
        sections
    }

    fn notify_control(
        &self,
        setting: &Setting,
        settings: &NotifySettings,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("notify-{setting:?}");
        match rules::value(setting, settings) {
            Value::Choice { options, selected } => {
                let pick = {
                    let setting = setting.clone();
                    self.on(cx, move |this, ix: usize, _, cx| {
                        let setting = setting.clone();
                        this.edit_notify(
                            move |s| {
                                rules::choose(&setting, ix, s);
                                true
                            },
                            cx,
                        )
                    })
                };
                let short = options.len() <= SEGMENTED_MAX && options.iter().all(|o| o.len() <= 16);
                if short {
                    Segmented::new(gpui::SharedString::from(id), options, selected)
                        .on_select(pick)
                        .into_any_element()
                } else {
                    Select::new(gpui::SharedString::from(id), options, selected)
                        .width(220.0)
                        .on_select(pick)
                        .into_any_element()
                }
            }
            Value::Toggle(on) => {
                let setting = setting.clone();
                Switch::new(gpui::SharedString::from(id), on)
                    .on_toggle(self.on(cx, move |this, _: bool, _, cx| {
                        let setting = setting.clone();
                        this.edit_notify(move |s| rules::toggle(&setting, s), cx)
                    }))
                    .into_any_element()
            }
            Value::Action(label) => {
                let Setting::Mute(ix) = *setting else {
                    return div().into_any_element();
                };
                Button::new(gpui::SharedString::from(id), label)
                    .variant(ButtonVariant::Secondary)
                    .on_press(move |_, cx| crate::inbox::unmute(ix, cx))
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_notify::DeliveryMode;

    #[test]
    fn banners_added_only_when_a_priority_gains_a_banner() {
        let before = NotifySettings::default();
        let mut after = before.clone();
        after.delivery.set(Priority::P1, DeliveryMode::BannerSound);
        assert!(!banners_added(&before, &after), "P1 already had a banner");
        after.delivery.set(Priority::P3, DeliveryMode::Banner);
        assert!(banners_added(&before, &after));
        assert!(
            !banners_added(&after, &before),
            "removing banners never asks"
        );
    }
}
