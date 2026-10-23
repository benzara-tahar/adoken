//! Settings › Ken (ken.md §1): the opt-in switch, intensity, naming, each
//! surface toggle with a live preview, the streak calendar, Quiet mode,
//! streak pause, the fight card, and [Reset stats] behind a confirm.
//! Embedded by the Settings window (`crate::ken::settings_section`).

use adoken_ken::settings::Intensity;
use adoken_ken::{KenSettings, Naming, Rank};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Checkbox, Segmented, Switch, Theme, TypeStyleExt,
};
use gpui::{
    AnyElement, Context, Entity, IntoElement, Render, SharedString, Subscription, Task, Window,
    div, prelude::*, px,
};

use super::hud::{ComboHud, StatusChip};
use super::layer::{KenLayer, quiet_label};
use crate::settings;

const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
/// The live preview's box (ken.md §1: 200 × 64).
const PREVIEW_WIDTH: f32 = 200.0;
const PREVIEW_HEIGHT: f32 = 64.0;

pub struct KenSettingsSection {
    layer: Option<Entity<KenLayer>>,
    settings: KenSettings,
    confirm_reset: bool,
    _subscriptions: Vec<Subscription>,
    _load: Option<Task<()>>,
}

impl KenSettingsSection {
    pub fn new(layer: Option<Entity<KenLayer>>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![settings::observe(cx, |this, cx| this.reload(cx))];
        if let Some(layer) = &layer {
            subscriptions.push(cx.observe(layer, |this, layer, cx| {
                this.settings = layer.read(cx).settings().clone();
                cx.notify();
            }));
        }
        let mut this = Self {
            settings: layer
                .as_ref()
                .map(|layer| layer.read(cx).settings().clone())
                .unwrap_or_default(),
            layer,
            confirm_reset: false,
            _subscriptions: subscriptions,
            _load: None,
        };
        this.reload(cx);
        this
    }

    /// A section over fixed settings (gallery stories).
    pub fn with_settings(settings: KenSettings) -> Self {
        Self {
            layer: None,
            settings,
            confirm_reset: false,
            _subscriptions: Vec::new(),
            _load: None,
        }
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        if self.layer.is_some() {
            return; // The layer re-reads and we observe it.
        }
        let Some(path) = settings::path(cx) else {
            return;
        };
        let job = cx.background_executor().spawn(async move {
            std::fs::read_to_string(path)
                .map(|text| KenSettings::from_settings_toml(&text))
                .unwrap_or_default()
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let settings = job.await;
            this.update(cx, |this, cx| {
                this.settings = settings;
                cx.notify();
            })
            .ok();
        }));
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut KenSettings)) {
        f(&mut self.settings);
        let settings = self.settings.clone();
        match &self.layer {
            Some(layer) => layer.update(cx, |layer, cx| {
                layer.update_settings(|current| *current = settings, cx)
            }),
            None => settings::write_table(
                cx,
                adoken_ken::settings::SETTINGS_TABLE,
                settings.to_table(),
            ),
        }
        cx.notify();
    }

    fn with_layer(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut KenLayer, &mut Context<KenLayer>),
    ) {
        if let Some(layer) = &self.layer {
            layer.update(cx, f);
        }
    }

    fn switch(
        &self,
        id: &'static str,
        on: bool,
        disabled: bool,
        cx: &mut Context<Self>,
        set: fn(&mut KenSettings, bool),
    ) -> impl IntoElement {
        let this = cx.entity().downgrade();
        Switch::new(id, on)
            .disabled(disabled)
            .on_toggle(move |on, _, cx| {
                this.update(cx, |this, cx| this.change(cx, |s| set(s, on)))
                    .ok();
            })
    }
}

fn section(theme: &Theme, title: &str, rows: Vec<AnyElement>) -> AnyElement {
    let colors = &theme.colors;
    let rows = rows.into_iter().enumerate().map(|(ix, row)| {
        div()
            .when(ix > 0, |row| {
                row.border_t_1().border_color(colors.border_subtle)
            })
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .child(
            div()
                .px(px(space::S4))
                .text_color(colors.text_primary)
                .type_style(typography::HEADING, theme)
                .child(title.to_owned()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(radius::MD))
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_raised)
                .children(rows),
        )
        .into_any_element()
}

fn row(
    theme: &Theme,
    title: impl Into<SharedString>,
    description: Option<&str>,
    control: impl IntoElement,
) -> AnyElement {
    let colors = &theme.colors;
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
                        .child(title.into()),
                )
                .children(description.map(|text| {
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, theme)
                        .child(text.to_owned())
                })),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(control),
        )
        .into_any_element()
}

impl Render for KenSettingsSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let s = self.settings.clone();
        let off = !s.enabled;
        let reduce_motion = cx.reduce_motion();
        let arcade = s.intensity == Intensity::Arcade;

        // Live preview: the HUD (arcade) or the statusbar chip (subtle).
        let preview = div()
            .w(px(PREVIEW_WIDTH))
            .min_h(px(PREVIEW_HEIGHT))
            .p(px(space::S8))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_subtle)
            .bg(colors.bg_app)
            .child(if arcade && s.combo {
                ComboHud {
                    count: 5,
                    rank: Rank::Gale,
                    naming: s.naming,
                    remaining_ms: 6 * 60_000,
                    window_ms: 10 * 60_000,
                    motion: !reduce_motion,
                    appear_ms: f32::MAX,
                    step_ms: f32::MAX,
                    rank_up_ms: None,
                    last_move: None,
                    finish: None,
                }
                .into_any_element()
            } else {
                StatusChip {
                    combo: s.combo.then_some((5, Rank::Gale, 6 * 60_000)),
                    naming: s.naming,
                    streak_days: if s.streak { 12 } else { 0 },
                    shields: 1,
                    paused: false,
                    quiet: None,
                }
                .into_any_element()
            });

        let this = cx.entity().downgrade();
        let intensity = {
            let this = this.clone();
            Segmented::new(
                "ken-intensity",
                [Intensity::Subtle.label(), Intensity::Arcade.label()],
                usize::from(arcade),
            )
            .on_select(move |ix, _, cx| {
                this.update(cx, |this, cx| {
                    this.change(cx, |s| {
                        s.intensity = if ix == 1 {
                            Intensity::Arcade
                        } else {
                            Intensity::Subtle
                        }
                    })
                })
                .ok();
            })
        };
        let naming = {
            let this = this.clone();
            Segmented::new(
                "ken-naming",
                Naming::ALL.map(Naming::label),
                Naming::ALL.iter().position(|n| *n == s.naming).unwrap_or(1),
            )
            .on_select(move |ix, _, cx| {
                this.update(cx, |this, cx| {
                    this.change(cx, |s| s.naming = Naming::ALL[ix])
                })
                .ok();
            })
        };
        let days = div()
            .flex()
            .gap(px(space::S8))
            .children((0..7u8).map(|day| {
                let on = s.working_days.contains(day);
                let this = this.clone();
                div()
                    .id(("ken-day", day as usize))
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .cursor_pointer()
                    .type_style(typography::SMALL, &theme)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |this, cx| {
                            this.change(cx, |s| s.working_days = s.working_days.with(day, !on))
                        })
                        .ok();
                    })
                    .child(Checkbox::new(on))
                    .child(WEEKDAYS[day as usize])
            }));

        let mode = section(
            &theme,
            "Mode",
            vec![
                row(
                    &theme,
                    "Arcade mode",
                    Some(
                        "Combos for review runs, a K.O. when your PR merges, daily streaks. Computed on this Mac only; nothing is sent anywhere.",
                    ),
                    self.switch("ken-enabled", s.enabled, false, cx, |s, on| s.enabled = on),
                ),
                row(
                    &theme,
                    "Intensity",
                    Some("Subtle: toasts and statusbar only. Arcade: HUD, overlays, sparks."),
                    intensity,
                ),
                row(
                    &theme,
                    "Names",
                    Some("How combo ranks and moves are captioned."),
                    naming,
                ),
                row(&theme, "Preview", None, preview),
            ],
        );
        let shake_note = if reduce_motion {
            Some("Off while Reduce Motion is on.")
        } else {
            Some("Arcade only.")
        };
        let surfaces = section(
            &theme,
            "Moments",
            vec![
                row(
                    &theme,
                    "Combo counter",
                    Some("×2 and up while you review within 10 minutes."),
                    self.switch("ken-combo", s.combo, off, cx, |s, on| s.combo = on),
                ),
                row(
                    &theme,
                    "K.O. when your PR merges",
                    None,
                    self.switch("ken-ko", s.ko, off, cx, |s, on| s.ko = on),
                ),
                row(
                    &theme,
                    "Daily streak",
                    Some(
                        "Working days with at least one review action. Shields cover a missed day.",
                    ),
                    self.switch("ken-streak", s.streak, off, cx, |s, on| s.streak = on),
                ),
                row(
                    &theme,
                    "Badges",
                    Some("First strike, clean sweep, comeback, assist."),
                    self.switch("ken-badges", s.badges, off, cx, |s, on| s.badges = on),
                ),
                row(
                    &theme,
                    "Screen shake",
                    shake_note,
                    self.switch(
                        "ken-shake",
                        s.screen_shake && !reduce_motion,
                        off || reduce_motion || !arcade,
                        cx,
                        |s, on| s.screen_shake = on,
                    ),
                ),
                row(
                    &theme,
                    "Sparks",
                    shake_note,
                    self.switch(
                        "ken-sparks",
                        s.particles && !reduce_motion,
                        off || reduce_motion || !arcade,
                        cx,
                        |s, on| s.particles = on,
                    ),
                ),
                row(
                    &theme,
                    "Working days",
                    Some("Days that count for the streak."),
                    days,
                ),
            ],
        );

        let now = crate::palette::now_ms();
        let quiet_text = quiet_label(&s, now).unwrap_or_else(|| {
            "Silences sounds and hides celebrations and Ken for demos and screen shares; stats keep counting.".into()
        });
        let quiet_buttons = {
            let (hour, toggle) = (this.clone(), this.clone());
            div()
                .flex()
                .gap(px(space::S8))
                .child(
                    Button::new("ken-quiet-hour", "1 hour").on_press(move |_, cx| {
                        hour.update(cx, |this, cx| {
                            this.with_layer(cx, |l, cx| l.quiet_for_hour(cx))
                        })
                        .ok();
                    }),
                )
                .child(
                    Button::new(
                        "ken-quiet-toggle",
                        if s.quiet_active(now) {
                            "Turn off"
                        } else {
                            "Turn on"
                        },
                    )
                    .on_press(move |_, cx| {
                        toggle
                            .update(cx, |this, cx| {
                                this.with_layer(cx, |l, cx| l.toggle_quiet(cx))
                            })
                            .ok();
                    }),
                )
        };
        let streak_buttons =
            {
                let (pause, resume) = (this.clone(), this.clone());
                div()
                    .flex()
                    .gap(px(space::S8))
                    .child(Button::new("ken-pause", "Pause").disabled(off).on_press(
                        move |_, cx| {
                            pause
                                .update(cx, |this, cx| {
                                    this.with_layer(cx, |l, cx| l.set_streak_paused(true, cx))
                                })
                                .ok();
                        },
                    ))
                    .child(Button::new("ken-resume", "Resume").disabled(off).on_press(
                        move |_, cx| {
                            resume
                                .update(cx, |this, cx| {
                                    this.with_layer(cx, |l, cx| l.set_streak_paused(false, cx))
                                })
                                .ok();
                        },
                    ))
            };
        let card_button = {
            let this = this.clone();
            Button::new("ken-card", "Show fight card").on_press(move |_, cx| {
                this.update(cx, |this, cx| {
                    this.with_layer(cx, |l, cx| l.show_fight_card(None, false, cx))
                })
                .ok();
            })
        };
        let reset = if self.confirm_reset {
            let (yes, no) = (this.clone(), this.clone());
            div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::SMALL, &theme)
                        .child("Reset all arcade stats? Combos, streaks, and K.O. history on this Mac will be cleared."),
                )
                .child(Button::new("ken-reset-yes", "Reset").variant(ButtonVariant::Primary).on_press(move |_, cx| {
                    yes.update(cx, |this, cx| {
                        this.confirm_reset = false;
                        this.with_layer(cx, |l, cx| l.reset_stats(cx));
                        cx.notify();
                    })
                    .ok();
                }))
                .child(Button::new("ken-reset-no", "Cancel").on_press(move |_, cx| {
                    no.update(cx, |this, cx| {
                        this.confirm_reset = false;
                        cx.notify();
                    })
                    .ok();
                }))
                .into_any_element()
        } else {
            Button::new("ken-reset", "Reset stats…")
                .on_press(move |_, cx| {
                    this.update(cx, |this, cx| {
                        this.confirm_reset = true;
                        cx.notify();
                    })
                    .ok();
                })
                .into_any_element()
        };
        let stats = section(
            &theme,
            "Stats",
            vec![
                row(&theme, "Quiet mode", Some(&quiet_text), quiet_buttons),
                row(
                    &theme,
                    "Streak",
                    Some("Pause for vacations; the count freezes until you resume."),
                    streak_buttons,
                ),
                row(
                    &theme,
                    "Weekly fight card",
                    Some("Your week in numbers. Private unless you export it."),
                    card_button,
                ),
                row(&theme, "Reset", None, reset),
            ],
        );
        div()
            .flex()
            .flex_col()
            .gap(px(space::S24))
            .child(mode)
            .child(surfaces)
            .child(stats)
    }
}
