//! General: motion override (spec 0007 §3.6) and the sidebar style.

use adoken_ui::{Kbd, Segmented, Theme};
use gpui::{AnyElement, Context, prelude::*};

use super::{row, section};
use crate::settings::model::{self, Motion};
use crate::settings::view::SettingsView;

impl SettingsView {
    pub(super) fn general(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let mut rows = Vec::new();
        if self.visible("Motion", "animation reduce motion accessibility") {
            let (motion, _) = Motion::from_document(&self.doc);
            let selected = Motion::ALL.iter().position(|m| *m == motion).unwrap_or(0);
            let now = if cx.reduce_motion() {
                "reduced"
            } else {
                "full"
            };
            rows.push(row(
                theme,
                "Motion",
                Some(
                    format!("System follows macOS Reduce Motion. Animations are {now} right now.")
                        .into(),
                ),
                Segmented::new("motion", Motion::ALL.map(Motion::label), selected).on_select(
                    self.on(cx, |this, ix: usize, _, cx| {
                        let motion = Motion::ALL[ix];
                        this.edit_table(
                            model::GENERAL_TABLE,
                            |general| {
                                model::set_value(general, &["motion"], motion.id());
                            },
                            cx,
                        );
                    }),
                ),
            ));
        }
        if self.visible("Sidebar", "collapse icons rail compact") {
            let collapsed = model::sidebar_collapsed(&self.doc);
            rows.push(row(
                theme,
                "Sidebar",
                Some("Icons only keeps the sidebar to a narrow rail.".into()),
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(adoken_theme::space::S8))
                    .child(Kbd::parse(crate::shell::TOGGLE_SIDEBAR_KEYS).expect("static binding"))
                    .child(
                        Segmented::new("sidebar", ["Labels", "Icons only"], usize::from(collapsed))
                            .on_select(self.on(cx, |this, ix: usize, _, cx| {
                                this.edit_table(
                                    model::SIDEBAR_TABLE,
                                    |sidebar| model::set_value(sidebar, &["collapsed"], ix == 1),
                                    cx,
                                );
                            })),
                    ),
            ));
        }
        // Spec 0023 FR-11 (hidden where there is no source, FR-4).
        if crate::resource::source::SUPPORTED
            && self.visible(crate::resource::SETTING, crate::resource::SETTING_KEYWORDS)
        {
            let on = crate::activity::settings::parse(&self.doc, None)
                .0
                .resource_meter;
            rows.push(row(
                theme,
                crate::resource::SETTING,
                Some(
                    "adoken's own memory, as Activity Monitor counts it, and CPU, at the right \
                     of the statusbar."
                        .into(),
                ),
                adoken_ui::Switch::new("general-resource-meter", on).on_toggle(self.on(
                    cx,
                    |this, on: bool, _, cx| {
                        this.edit_table(
                            crate::activity::settings::TABLE,
                            |table| {
                                table.insert("resource_meter".into(), on.into());
                            },
                            cx,
                        )
                    },
                )),
            ));
        }
        // Spec 0020 FR-15: `[tours] auto_start`, and clearing `seen`.
        if self.visible(crate::tours::SETTING_TITLE, crate::tours::SETTING_KEYWORDS) {
            let tours = crate::tours::settings::TourSettings::parse(&self.doc).0;
            rows.push(row(
                theme,
                crate::tours::SETTING_TITLE,
                Some(
                    "A few steps pointing at the parts of a view and their keys. Replay any \
                     tour from ⌘P."
                        .into(),
                ),
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(adoken_theme::space::S8))
                    .child(
                        adoken_ui::Button::new("general-tours-reset", crate::tours::RESET_TITLE)
                            .variant(adoken_ui::ButtonVariant::Ghost)
                            .disabled(tours.seen.is_empty())
                            .on_press(self.press(cx, |this, _, cx| {
                                this.edit_table(
                                    crate::tours::settings::TABLE,
                                    |table| {
                                        table.insert("seen".into(), toml::Value::Array(Vec::new()));
                                    },
                                    cx,
                                );
                                adoken_ui::ToastStack::show(
                                    adoken_ui::Toast::new(
                                        "tours-show-again",
                                        adoken_ui::IconName::Info,
                                        crate::tours::SHOW_AGAIN_TOAST,
                                    ),
                                    cx,
                                );
                            })),
                    )
                    .child(
                        adoken_ui::Switch::new("general-tours", tours.auto_start).on_toggle(
                            self.on(cx, |this, on: bool, _, cx| {
                                this.edit_table(
                                    crate::tours::settings::TABLE,
                                    |table| {
                                        table.insert("auto_start".into(), on.into());
                                    },
                                    cx,
                                )
                            }),
                        ),
                    ),
            ));
        }
        // Spec 0019 FR-14 (contract C1): `[review] show_group_reviewers`.
        let mut pull_requests = Vec::new();
        if self.visible(
            "Show group reviewers",
            "group team policy reviewers required pull requests",
        ) {
            let on = crate::review::groups::from_document(&self.doc);
            pull_requests.push(row(
                theme,
                "Show group reviewers",
                Some(
                    "Teams and groups added by branch policies, such as \u{201c}[Project]\\React PR \
                     reviewers\u{201d}. Hidden groups still count toward approvals."
                        .into(),
                ),
                adoken_ui::Switch::new("general-group-reviewers", on).on_toggle(self.on(
                    cx,
                    |this, on: bool, _, cx| {
                        crate::review::groups::apply(on, cx);
                        this.edit_table(
                            crate::review::groups::SETTINGS_TABLE,
                            |table| {
                                table.insert(
                                    crate::review::groups::SHOW_GROUP_REVIEWERS_KEY.into(),
                                    on.into(),
                                );
                            },
                            cx,
                        )
                    },
                )),
            ));
        }
        vec![
            section(theme, "General", None, rows),
            section(theme, "Pull requests", None, pull_requests),
            self.celebrations(theme, cx),
        ]
    }
}
