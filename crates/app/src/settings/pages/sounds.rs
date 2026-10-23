//! Sounds (spec 0007 §3.4, spec 0025 FR-13, docs/design/sound.md §3): on/off
//! (off by default) and the master volume, nothing else. An edit rewrites
//! `[sound]` from the parsed settings, so keys spec 0025 removed never go
//! back to the file (FR-14), then installs it in the running engine.

use adoken_sound::{SETTINGS_TABLE, SoundSettings};
use adoken_theme::space;
use adoken_ui::{Slider, Switch, Theme};
use gpui::{AnyElement, Context, div, prelude::*, px};

use super::{percent, row, section, value_label};
use crate::settings::model;
use crate::settings::view::SettingsView;

impl SettingsView {
    fn edit_sound(&mut self, change: impl FnOnce(&mut SoundSettings), cx: &mut Context<Self>) {
        self.edit_table(
            SETTINGS_TABLE,
            |table| {
                let (mut settings, _) = SoundSettings::from_table(std::mem::take(table));
                change(&mut settings);
                *table = settings.to_table();
            },
            cx,
        );
    }

    pub(super) fn sounds(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let table = model::table_of(&self.doc, SETTINGS_TABLE);
        let (settings, _) = SoundSettings::from_table(table);
        let enabled = settings.enabled;

        let mut main = Vec::new();
        if self.visible("Sounds", "audio enable on off mute") {
            main.push(row(
                theme,
                "Sounds",
                Some("Off by default. Quiet hours and delivery settings still apply.".into()),
                Switch::new("sound-enabled", enabled).on_toggle(
                    self.on(cx, |this, on: bool, _, cx| {
                        this.edit_sound(|s| s.enabled = on, cx)
                    }),
                ),
            ));
        }
        if self.visible("Volume", "master loud quiet level") {
            main.push(row(
                theme,
                "Volume",
                None,
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Slider::new("sound-master", settings.master, 0.0..=1.0)
                            .step(0.05)
                            .disabled(!enabled)
                            .on_change(self.on(cx, |this, value: f32, _, cx| {
                                this.edit_sound(|s| s.master = value, cx)
                            })),
                    )
                    .child(value_label(theme, percent(settings.master))),
            ));
        }
        vec![section(theme, "Playback", None, main)]
    }
}
