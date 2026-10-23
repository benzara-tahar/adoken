//! Nudges (spec 0006 §3.4): N1–N7 on/off and threshold, daily cap, working
//! days and hours. Settings owns `[nudges]`; Today re-reads it on change.

use adoken_nudge::{NudgeKind, Settings as NudgeSettings, Threshold};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{Select, Stepper, Switch, Theme, TypeStyleExt};
use gpui::{AnyElement, Context, div, prelude::*, px};

use super::{row, section};
use crate::settings::model::{self, THRESHOLD_DAYS, THRESHOLD_HOURS};
use crate::settings::view::SettingsView;

/// Half-hour steps for the working-hours selects.
const SLOTS: u16 = 48;

fn slot_label(slot: u16) -> String {
    model::clock_text(slot * 30)
}

impl SettingsView {
    fn nudge_settings(&self) -> NudgeSettings {
        model::parse_nudges(&self.doc).0
    }

    fn edit_nudges(&mut self, change: impl FnOnce(&mut NudgeSettings), cx: &mut Context<Self>) {
        let mut settings = self.nudge_settings();
        change(&mut settings);
        let table = model::nudges_table(&settings);
        self.edit_table(model::NUDGES_TABLE, |current| *current = table, cx);
    }

    pub(super) fn nudges(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let colors = &theme.colors;
        let settings = self.nudge_settings();

        let mut kinds = Vec::new();
        for kind in NudgeKind::ALL {
            let title = format!("{} · {}", kind.id(), kind.label());
            if !self.visible(&title, "nudge types thresholds hours days wait after") {
                continue;
            }
            let current = settings.kind(kind);
            let (description, stepper) = match current.threshold {
                Threshold::Immediate => ("Fires as soon as it happens.".to_owned(), None),
                Threshold::WorkingHours(hours) => (
                    "After this much working time.".to_owned(),
                    Some(
                        Stepper::new(
                            ("nudge-hours", kind as usize),
                            i64::from(hours),
                            i64::from(THRESHOLD_HOURS.0)..=i64::from(THRESHOLD_HOURS.1),
                        )
                        .suffix("h")
                        .disabled(!current.enabled)
                        .on_change(self.on(
                            cx,
                            move |this, value: i64, _, cx| {
                                this.edit_nudges(
                                    |s| {
                                        s.set_threshold(kind, Threshold::WorkingHours(value as u32))
                                    },
                                    cx,
                                )
                            },
                        )),
                    ),
                ),
                Threshold::WorkingDays(days) => (
                    "After this many working days.".to_owned(),
                    Some(
                        Stepper::new(
                            ("nudge-days", kind as usize),
                            i64::from(days),
                            i64::from(THRESHOLD_DAYS.0)..=i64::from(THRESHOLD_DAYS.1),
                        )
                        .suffix(if days == 1 { "day" } else { "days" })
                        .disabled(!current.enabled)
                        .on_change(self.on(
                            cx,
                            move |this, value: i64, _, cx| {
                                this.edit_nudges(
                                    |s| s.set_threshold(kind, Threshold::WorkingDays(value as u32)),
                                    cx,
                                )
                            },
                        )),
                    ),
                ),
            };
            kinds.push(row(
                theme,
                title,
                Some(description.into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S12))
                    .children(stepper)
                    .child(
                        Switch::new(("nudge-on", kind as usize), current.enabled).on_toggle(
                            self.on(cx, move |this, on: bool, _, cx| {
                                this.edit_nudges(|s| s.set_enabled(kind, on), cx)
                            }),
                        ),
                    ),
            ));
        }

        let mut limits = Vec::new();
        if self.visible("Daily cap", "limit per day maximum") {
            limits.push(row(
                theme,
                "Daily cap",
                Some("At most this many nudges a day; the rest wait on the Today card.".into()),
                Stepper::new(
                    "nudge-cap",
                    i64::from(settings.daily_cap()),
                    0..=i64::from(NudgeSettings::MAX_CAP),
                )
                .on_change(self.on(cx, |this, value: i64, _, cx| {
                    this.edit_nudges(|s| s.set_daily_cap(value as u8), cx)
                })),
            ));
        }
        if self.visible("Working days", "working hours work days schedule weekend") {
            let days = (0..7).map(|ix| {
                let on = settings.hours.days & (1 << ix) != 0;
                div()
                    .id(("nudge-day", ix))
                    .tab_index(0)
                    .w(px(40.0))
                    .h(px(size::CONTROL_MD))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(radius::SM))
                    .border_1()
                    .cursor_pointer()
                    .type_style(typography::BODY_STRONG, theme)
                    .when(on, |day| {
                        day.bg(colors.accent_solid)
                            .border_color(colors.accent_solid)
                            .text_color(colors.accent_on)
                    })
                    .when(!on, |day| {
                        day.bg(colors.bg_input)
                            .border_color(colors.border_control)
                            .text_color(colors.text_secondary)
                            .hover(|s| s.bg(colors.bg_hover))
                    })
                    .focus(|s| s.border_color(colors.border_focus))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_nudges(|s| s.hours.days ^= 1 << ix, cx)
                    }))
                    .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                            cx.stop_propagation();
                            this.edit_nudges(|s| s.hours.days ^= 1 << ix, cx)
                        }
                    }))
                    .child(model::weekday_label(ix))
            });
            limits.push(row(
                theme,
                "Working days",
                None,
                div().flex().gap(px(space::S4)).children(days),
            ));
        }
        if self.visible("Working hours", "work days schedule start end") {
            let start = settings.hours.start_min / 30;
            let end = settings.hours.end_min / 30;
            let slots: Vec<String> = (0..SLOTS).map(slot_label).collect();
            limits.push(row(
                theme,
                "Working hours",
                Some("Thresholds count only this time; snoozes resume at the start.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Select::new("nudge-start", slots.clone(), usize::from(start))
                            .width(96.0)
                            .on_select(self.on(cx, |this, slot: usize, _, cx| {
                                let minutes = slot as u16 * 30;
                                this.edit_nudges(
                                    |s| model::set_work_start(&mut s.hours, minutes),
                                    cx,
                                )
                            })),
                    )
                    .child(div().text_color(colors.text_tertiary).child("to"))
                    .child(
                        Select::new("nudge-end", slots, usize::from(end))
                            .width(96.0)
                            .on_select(self.on(cx, |this, slot: usize, _, cx| {
                                let minutes = slot as u16 * 30;
                                this.edit_nudges(|s| model::set_work_end(&mut s.hours, minutes), cx)
                            })),
                    ),
            ));
        }
        vec![
            section(
                theme,
                "Nudge types",
                Some(
                    "Nudges are computed on this Mac from synced data; adoken never messages anyone.",
                ),
                kinds,
            ),
            section(theme, "Limits and working time", None, limits),
        ]
    }
}
