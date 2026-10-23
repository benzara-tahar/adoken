//! Settings › General › Celebrations (spec 0022 FR-14, ken.md §12.5): the
//! master switch, the two surfaces (disabled while the master is off) and
//! a read-only summary of the state lists, edited in the file in v0.1.

use adoken_ken::celebrate::{CelebrationSettings, SETTINGS_TABLE};
use adoken_ui::{Button, ButtonVariant, Switch, Theme};
use gpui::{AnyElement, Context};

use super::{row, section};
use crate::settings::model;
use crate::settings::view::SettingsView;

const KEYWORDS: &str = "celebrations confetti effects animation";

impl SettingsView {
    pub(super) fn celebrations(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (settings, _) = CelebrationSettings::parse(&self.doc);
        let mut rows = Vec::new();
        let switch =
            |view: &Self, id: &'static str, key: &'static str, on: bool, cx: &Context<Self>| {
                Switch::new(id, on).on_toggle(view.on(cx, move |this, on: bool, _, cx| {
                    this.edit_table(SETTINGS_TABLE, |t| model::set_value(t, &[key], on), cx)
                }))
            };
        if self.visible("Celebrate finished work", KEYWORDS) {
            rows.push(row(
                theme,
                "Celebrate finished work",
                Some(
                    "A short burst when your pull request merges or a work item moves to review or done."
                        .into(),
                ),
                switch(self, "celebrations-enabled", "enabled", settings.enabled, cx),
            ));
        }
        for (title, id, key, on) in [
            (
                "Pull requests I complete",
                "celebrations-prs",
                "pull_requests",
                settings.pull_requests,
            ),
            (
                "Work item state changes",
                "celebrations-work-items",
                "work_items",
                settings.work_items,
            ),
        ] {
            if self.visible(title, KEYWORDS) {
                rows.push(row(
                    theme,
                    title,
                    None,
                    switch(self, id, key, on, cx).disabled(!settings.enabled),
                ));
            }
        }
        if self.visible("States that celebrate", KEYWORDS) {
            let path = crate::settings::path(cx);
            rows.push(row(
                theme,
                "States that celebrate",
                Some(settings.summary().into()),
                Button::new("celebrations-edit-file", "Edit in settings file")
                    .variant(ButtonVariant::Ghost)
                    .disabled(path.is_none())
                    .on_press(move |_, cx| {
                        if let Some(path) = &path {
                            cx.open_with_system(path);
                        }
                    }),
            ));
        }
        section(theme, "Celebrations", None, rows)
    }
}
