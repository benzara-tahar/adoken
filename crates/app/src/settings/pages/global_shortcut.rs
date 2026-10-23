//! Keyboard › Global shortcut (spec 0021 §3.2, FR-8, FR-11, FR-12; one
//! shortcut since 0025 FR-12): its recorder, Record / Clear / Reset and a
//! status line, then the Hyper tip. The state lives in
//! [`crate::hotkey::Hotkeys`].

use adoken_theme::{radius, size, space, typography};
use adoken_ui::{Button, ButtonVariant, Kbd, Theme, TypeStyleExt};
use gpui::{AnyElement, Context, Entity, div, prelude::*, px};

use super::section;
use crate::hotkey::model::{HotkeySettings, LABEL};
use crate::hotkey::{self, Hotkeys};
use crate::settings::view::SettingsView;

impl SettingsView {
    pub(super) fn global_shortcut(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.visible(hotkey::SECTION, hotkey::SECTION_KEYWORDS) {
            return None;
        }
        let hotkeys = self.embeds.hotkey.as_ref()?.0.clone();
        let row = shortcut_row(&hotkeys, theme, cx);
        section(theme, hotkey::SECTION, Some(hotkey::TIP), vec![row])
    }
}

fn shortcut_row(
    hotkeys: &Entity<Hotkeys>,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let colors = &theme.colors;
    let state = hotkeys.read(cx);
    let recording = state.recording();
    let chord = state.settings().summon.clone();
    let status = state.status_line();
    let failed = state.failed();
    let focus = state.recorder_focus.clone();
    let at_default = HotkeySettings::default().summon == chord;
    let recorder = {
        let label: AnyElement = if recording {
            div()
                .text_color(colors.text_secondary)
                .child("Press keys…")
                .into_any_element()
        } else {
            match &chord {
                Some(chord) => Kbd::new(&[chord.keystroke()]).into_any_element(),
                None => div()
                    .text_color(colors.text_tertiary)
                    .child("Not set")
                    .into_any_element(),
            }
        };
        let entity = hotkeys.downgrade();
        div()
            .id("hotkey-recorder")
            .min_w(px(112.0))
            .h(px(size::CONTROL_MD))
            .px(px(space::S8))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::SM))
            .border_1()
            .border_color(if recording {
                colors.border_focus
            } else {
                colors.border_default
            })
            .bg(colors.bg_surface)
            .type_style(typography::SMALL, theme)
            .role(gpui::Role::Button)
            .aria_label(format!(
                "{LABEL}: {}",
                chord.as_ref().map_or("not set".into(), |c| c.label())
            ))
            .when(recording, |d| {
                let entity = entity.clone();
                d.track_focus(&focus)
                    .on_modifiers_changed(move |event, _, cx| {
                        entity
                            .update(cx, |this, cx| this.modifiers_changed(event.modifiers, cx))
                            .ok();
                    })
            })
            .cursor_pointer()
            .on_click(move |_, window, cx| {
                entity
                    .update(cx, |this, cx| this.start_recording(window, cx))
                    .ok();
            })
            .child(label)
    };
    let record = {
        let entity = hotkeys.downgrade();
        let (label, recording_now) = if recording {
            ("Cancel", true)
        } else {
            ("Record", false)
        };
        Button::new("hotkey-record", label).on_press(move |window, cx| {
            entity
                .update(cx, |this, cx| {
                    if recording_now {
                        this.stop_recording(cx)
                    } else {
                        this.start_recording(window, cx)
                    }
                })
                .ok();
        })
    };
    let clear = {
        let entity = hotkeys.downgrade();
        Button::new("hotkey-clear", "Clear")
            .variant(ButtonVariant::Ghost)
            .disabled(chord.is_none() || recording)
            .on_press(move |_, cx| {
                entity.update(cx, |this, cx| this.clear(cx)).ok();
            })
    };
    let reset = {
        let entity = hotkeys.downgrade();
        Button::new("hotkey-reset", "Reset")
            .variant(ButtonVariant::Ghost)
            .disabled(at_default || recording)
            .on_press(move |_, cx| {
                entity.update(cx, |this, cx| this.reset(cx)).ok();
            })
    };
    div()
        .min_h(px(size::CONTROL_LG + space::S16))
        .px(px(space::S16))
        .py(px(space::S8))
        .flex()
        .items_center()
        .gap(px(space::S16))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(space::S2))
                .child(
                    div()
                        .text_color(colors.text_primary)
                        .type_style(typography::BODY, theme)
                        .child(LABEL),
                )
                .child(
                    div()
                        .id("hotkey-status")
                        .role(gpui::Role::Status)
                        .flex()
                        .items_start()
                        .gap(px(space::S4))
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        // Hue lives in the glyph, never the text (activity.md §9.3).
                        .when(failed, |d| {
                            // One line tall, so the glyph centers on the
                            // first line when the text wraps.
                            d.child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .h(px(theme.resolve(typography::SMALL).line_height))
                                    .child(
                                        adoken_ui::Icon::new(adoken_ui::IconName::CircleAlert)
                                            .size(size::ICON_XS)
                                            .color(colors.status_danger_fg),
                                    ),
                            )
                        })
                        // Wraps under its label instead of running beneath
                        // the controls.
                        .child(div().flex_1().min_w_0().child(status)),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(recorder)
                .child(record)
                .child(clear)
                .child(reset),
        )
        .into_any_element()
}
