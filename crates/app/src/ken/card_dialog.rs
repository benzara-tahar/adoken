//! The weekly fight card dialog (ken.md §5): week header + 拳 emblem, four
//! stat tiles, the secondary row, the week's K.O.s, and the share image:
//! [Export image…] (native save dialog, NSSavePanel on macOS) and [Copy
//! image]. Export options all default off, so the image shows numbers only.
//!
//! Keys (context [`CONTEXT`]): ← / → previous / next week, ⌘S export, ⌘C
//! copy, Esc close. Loading and rendering run on the background executor.

use std::path::PathBuf;
use std::sync::Arc;

use adoken_ken::card::{self, ExportOptions, Palette};
use adoken_ken::{FightCard, Naming, Week};
use adoken_notify::SystemClock;
use adoken_store::Store;
use adoken_theme::background::{BackgroundSettings, BackgroundSource};
use adoken_theme::{radius, space, typography};
use adoken_ui::{ActiveTheme, Button, ButtonVariant, Checkbox, Kbd, Theme, TypeStyleExt};
use gpui::{
    App, ClipboardItem, Context, EventEmitter, FocusHandle, Focusable, Image, ImageFormat,
    IntoElement, KeyBinding, Render, SharedString, Task, Window, actions, div, prelude::*, px,
};

use super::glyphs::{ArcadeText, GlyphColors};
use crate::settings;

/// Key context of the dialog.
pub const CONTEXT: &str = "KenFightCard";
const WIDTH: f32 = 640.0;
const TILE_NUMERAL: f32 = 34.0;
const EXPORT_KEYS: &str = "secondary-s";
const COPY_KEYS: &str = "secondary-c";

actions!(
    ken_card,
    [
        /// Close the fight card.
        CloseFightCard,
        /// Fight card: export the image.
        ExportImage,
        /// Fight card: copy the image.
        CopyImage,
        /// Fight card: previous week.
        PreviousWeek,
        /// Fight card: next week.
        NextWeek,
    ]
);

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("escape", CloseFightCard, Some(CONTEXT)),
        KeyBinding::new(EXPORT_KEYS, ExportImage, Some(CONTEXT)),
        KeyBinding::new(COPY_KEYS, CopyImage, Some(CONTEXT)),
        KeyBinding::new("left", PreviousWeek, Some(CONTEXT)),
        KeyBinding::new("right", NextWeek, Some(CONTEXT)),
    ]
}

pub enum DialogEvent {
    Close,
    Toast(SharedString),
}

pub struct FightCardDialog {
    store: Option<Store>,
    week: Week,
    naming: Naming,
    streak_days: u32,
    shields: u8,
    card: Option<FightCard>,
    error: Option<String>,
    options: ExportOptions,
    /// The user's background image, when they have one to include.
    background: Option<PathBuf>,
    include_background: bool,
    busy: bool,
    status: Option<SharedString>,
    focus: FocusHandle,
    /// Export as soon as the card loads ("Ken: Export fight card…").
    export_when_loaded: bool,
    /// Focus before the dialog opened, restored on close.
    restore: Option<Option<FocusHandle>>,
    _tasks: Vec<Task<()>>,
}

impl EventEmitter<DialogEvent> for FightCardDialog {}

impl Focusable for FightCardDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FightCardDialog {
    pub fn new(
        store: Store,
        week: Week,
        naming: Naming,
        streak_days: u32,
        shields: u8,
        export: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            store: Some(store),
            week,
            naming,
            streak_days,
            shields,
            card: None,
            error: None,
            options: ExportOptions::default(),
            background: None,
            include_background: false,
            busy: false,
            status: None,
            focus: cx.focus_handle(),
            export_when_loaded: export,
            restore: None,
            _tasks: Vec::new(),
        };
        this.load(cx);
        this.find_background(cx);
        this
    }

    /// A dialog over a ready card (gallery stories).
    pub fn with_card(card: FightCard, naming: Naming, cx: &mut Context<Self>) -> Self {
        Self {
            store: None,
            week: card.week,
            naming,
            streak_days: card.streak_days,
            shields: card.shields,
            card: Some(card),
            error: None,
            options: ExportOptions::default(),
            background: None,
            include_background: false,
            busy: false,
            status: None,
            focus: cx.focus_handle(),
            export_when_loaded: false,
            restore: None,
            _tasks: Vec::new(),
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let (week, streak_days, shields) = (self.week, self.streak_days, self.shields);
        let job = cx.background_executor().spawn(async move {
            adoken_ken::persist::migrate(&store)?;
            FightCard::load(&store, week, streak_days, shields, &SystemClock)
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(card) => {
                        this.card = Some(card);
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.to_string()),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// "Include my background" is offered only for an image background
    /// whose file is readable (procedural ones have nothing to include).
    fn find_background(&mut self, cx: &mut Context<Self>) {
        let path = settings::path(cx);
        let dark = cx.theme().appearance;
        let job = cx.background_executor().spawn(async move {
            let text = std::fs::read_to_string(path?).ok()?;
            let (settings, _) = BackgroundSettings::from_settings_toml(&text).ok()?;
            match settings.resolve(dark, None).source? {
                BackgroundSource::Image(path) => {
                    let path = match path.strip_prefix("~/") {
                        Some(rest) => dirs::home_dir()?.join(rest),
                        None => PathBuf::from(path),
                    };
                    path.is_file().then_some(path)
                }
                BackgroundSource::Builtin(_) => None,
            }
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let background = job.await;
            this.update(cx, |this, cx| {
                this.background = background;
                cx.notify();
            })
            .ok();
        }));
    }

    fn step_week(&mut self, weeks: i64, cx: &mut Context<Self>) {
        let clock = SystemClock;
        let this_week = Week::of(crate::palette::now_ms(), &clock);
        let next = Week::starting(self.week.monday + 7 * weeks, &clock);
        if next.monday > this_week.monday || self.store.is_none() {
            return;
        }
        self.week = next;
        self.card = None;
        self.status = None;
        self.load(cx);
        cx.notify();
    }

    fn export_options(&self) -> ExportOptions {
        ExportOptions {
            background: self.background.clone().filter(|_| self.include_background),
            ..self.options.clone()
        }
    }

    /// Render the PNG off the UI thread.
    fn render_png(&self, cx: &mut Context<Self>) -> Option<Task<Result<Vec<u8>, String>>> {
        let card = self.card.clone()?;
        let palette = Palette::from_tokens(&cx.theme().raw);
        let (naming, options) = (self.naming, self.export_options());
        Some(cx.background_executor().spawn(async move {
            let svg = card::svg(&card, &palette, naming, &options);
            card::png(&svg, &card::options()).map_err(|e| e.to_string())
        }))
    }

    fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.card.as_ref() else {
            self.export_when_loaded = true;
            return;
        };
        if self.busy {
            return;
        }
        let name = card::file_name(card);
        let directory = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let chosen = cx.prompt_for_new_path(&directory, Some(&name));
        let Some(png) = self.render_png(cx) else {
            return;
        };
        self.busy = true;
        cx.notify();
        let focus = self.focus.clone();
        self._tasks.push(cx.spawn_in(window, async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => Some(path),
                Ok(Err(error)) => {
                    log::warn!("ken: save dialog failed: {error}");
                    None
                }
                _ => None,
            };
            let result = match path {
                Some(path) => {
                    let bytes = png.await;
                    let saved = cx
                        .background_executor()
                        .spawn(async move {
                            let bytes = bytes?;
                            card::save(&bytes, &path).map_err(|e| e.to_string())?;
                            Ok::<_, String>(path)
                        })
                        .await;
                    Some(saved)
                }
                None => None,
            };
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Some(Ok(path)) => {
                        let text: SharedString =
                            format!("Fight card saved to {}", path.display()).into();
                        this.status = Some(text.clone());
                        cx.emit(DialogEvent::Toast(text));
                    }
                    Some(Err(error)) => {
                        this.status = Some(format!("Couldn't save the image: {error}").into());
                    }
                    None => {}
                }
                window.focus(&focus, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let Some(png) = self.render_png(cx) else {
            return;
        };
        self.busy = true;
        cx.notify();
        self._tasks.push(cx.spawn(async move |this, cx| {
            let bytes = png.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match bytes {
                    Ok(bytes) => {
                        let image = Image::from_bytes(ImageFormat::Png, bytes);
                        cx.write_to_clipboard(ClipboardItem::new_image(&image));
                        this.status = Some("Image copied".into());
                    }
                    Err(error) => {
                        this.status = Some(format!("Couldn't render the image: {error}").into())
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Some(previous)) = self.restore.take() {
            window.focus(&previous, cx);
        }
        cx.emit(DialogEvent::Close);
    }

    #[allow(clippy::too_many_arguments)]
    fn option_row(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        enabled: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
        toggle: fn(&mut Self),
    ) -> impl IntoElement {
        let colors = &theme.colors;
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(space::S6))
            .text_color(if enabled {
                colors.text_primary
            } else {
                colors.text_disabled
            })
            .type_style(typography::SMALL, theme)
            .when(enabled, |row| {
                row.cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(this);
                        cx.notify();
                    }))
            })
            .child(Checkbox::new(checked))
            .child(label)
    }
}

/// One stat tile: label, arcade numerals, caption.
fn tile(label: &str, value: String, caption: Option<String>, theme: &Theme) -> impl IntoElement {
    let colors = &theme.colors;
    div()
        .flex_1()
        .min_w_0()
        .h(px(118.0))
        .p(px(space::S12))
        .flex()
        .flex_col()
        .gap(px(space::S4))
        .rounded(px(radius::MD))
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_raised)
        .child(
            div()
                .text_color(colors.text_secondary)
                .type_style(typography::LABEL, theme)
                .child(label.to_owned()),
        )
        .child(ArcadeText::new(
            value,
            TILE_NUMERAL,
            GlyphColors::ken(theme),
        ))
        .children(caption.map(|caption| {
            div()
                .text_color(colors.ken_secondary)
                .type_style(typography::SMALL, theme)
                .child(caption)
        }))
}

impl Render for FightCardDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.restore.is_none() && self.store.is_some() {
            // Keyboard-first: the dialog takes focus when it opens.
            self.restore = Some(window.focused(cx));
            window.focus(&self.focus, cx);
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let body = match (&self.card, &self.error) {
            (Some(card), _) => {
                if self.export_when_loaded {
                    self.export_when_loaded = false;
                    cx.defer_in(window, |this, window, cx| this.export(window, cx));
                }
                self.render_card(card, &theme).into_any_element()
            }
            (None, Some(error)) => div()
                .text_color(colors.status_danger_fg)
                .child(format!("Couldn't read arcade stats: {error}"))
                .into_any_element(),
            (None, None) => div()
                .h(px(260.0))
                .text_color(colors.text_tertiary)
                .child("Loading…")
                .into_any_element(),
        };
        let has_background = self.background.is_some();
        let options = div()
            .flex()
            .flex_wrap()
            .gap(px(space::S16))
            .child(self.option_row(
                "ken-card-repos",
                "Show repository names",
                self.options.show_repos,
                true,
                &theme,
                cx,
                |this| this.options.show_repos = !this.options.show_repos,
            ))
            .child(self.option_row(
                "ken-card-titles",
                "Show PR titles",
                self.options.show_titles,
                true,
                &theme,
                cx,
                |this| this.options.show_titles = !this.options.show_titles,
            ))
            .child(self.option_row(
                "ken-card-background",
                "Include my background",
                self.include_background && has_background,
                has_background,
                &theme,
                cx,
                |this| this.include_background = !this.include_background,
            ));
        let ready = self.card.is_some() && !self.busy;
        let this = cx.entity().downgrade();
        let copy_target = this.clone();
        let close_target = this.clone();
        let actions = div()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .children(self.status.clone().map(|status| {
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, &theme)
                    .child(status)
            }))
            .when(self.status.is_none(), |row| row.child(div().flex_1()))
            .child(
                Button::new("ken-card-close", "Close")
                    .variant(ButtonVariant::Ghost)
                    .shortcut(Kbd::parse("escape").expect("static binding"))
                    .on_press(move |window, cx| {
                        close_target
                            .update(cx, |this, cx| this.close(window, cx))
                            .ok();
                    }),
            )
            .child(
                Button::new("ken-card-copy", "Copy image")
                    .variant(ButtonVariant::Secondary)
                    .disabled(!ready)
                    .shortcut(Kbd::parse(COPY_KEYS).expect("static binding"))
                    .on_press(move |_, cx| {
                        copy_target.update(cx, |this, cx| this.copy(cx)).ok();
                    }),
            )
            .child(
                Button::new("ken-card-export", "Export image…")
                    .variant(ButtonVariant::Primary)
                    .disabled(!ready)
                    .loading(self.busy)
                    .shortcut(Kbd::parse(EXPORT_KEYS).expect("static binding"))
                    .on_press(move |window, cx| {
                        this.update(cx, |this, cx| this.export(window, cx)).ok();
                    }),
            );
        // Scrim + centered dialog; the scrim takes clicks (closes).
        div()
            .id("ken-fight-card-scrim")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.bg_scrim)
            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
            .child(
                div()
                    .id("ken-fight-card")
                    .key_context(CONTEXT)
                    .track_focus(&self.focus)
                    .on_action(
                        cx.listener(|this, _: &CloseFightCard, window, cx| this.close(window, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &ExportImage, window, cx| this.export(window, cx)),
                    )
                    .on_action(cx.listener(|this, _: &CopyImage, _, cx| this.copy(cx)))
                    .on_action(cx.listener(|this, _: &PreviousWeek, _, cx| this.step_week(-1, cx)))
                    .on_action(cx.listener(|this, _: &NextWeek, _, cx| this.step_week(1, cx)))
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .occlude()
                    .w(px(WIDTH))
                    .p(px(space::S24))
                    .flex()
                    .flex_col()
                    .gap(px(space::S16))
                    .rounded(px(radius::LG))
                    .border_1()
                    .border_color(colors.border_default)
                    .bg(colors.bg_overlay)
                    .shadow(theme.shadows(adoken_theme::elevation::LEVEL_3))
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY, &theme)
                    .child(body)
                    .child(options)
                    .child(actions),
            )
    }
}

impl FightCardDialog {
    fn render_card(&self, card: &FightCard, theme: &Theme) -> impl IntoElement {
        let colors = &theme.colors;
        let best = card
            .best_combo
            .map(|(hits, rank)| (format!("×{hits}"), rank.name().text(self.naming)));
        let shields = match card.shields {
            0 => String::new(),
            1 => " · 1 shield".to_owned(),
            n => format!(" · {n} shields"),
        };
        let days = if card.streak_days == 1 { "day" } else { "days" };
        let kos = if card.kos.is_empty() {
            vec![
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child("No K.O.s this week"),
            ]
        } else {
            card.kos
                .iter()
                .map(|ko| {
                    div()
                        .flex()
                        .gap(px(space::S8))
                        .type_style(typography::SMALL, theme)
                        .child(div().text_color(colors.ken_primary).child(if ko.double {
                            "Double K.O."
                        } else {
                            "K.O."
                        }))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(colors.text_primary)
                                .child(format!("!{} {}", ko.pr, ko.title)),
                        )
                        .child(
                            div()
                                .text_color(colors.text_secondary)
                                .child(ko.when.clone()),
                        )
                })
                .collect()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(space::S12))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .type_style(typography::TITLE, theme)
                                    .child(card.week.label()),
                            )
                            .child(
                                div()
                                    .text_color(colors.text_tertiary)
                                    .type_style(typography::SMALL, theme)
                                    .child("Private: stays on this Mac unless you export it. ← → change week."),
                            ),
                    )
                    .child(
                        div()
                            .size(px(40.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(radius::FULL))
                            .border_2()
                            .border_color(colors.ken_primary)
                            .text_color(colors.ken_primary)
                            .type_style(typography::HEADING, theme)
                            .child("拳"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(space::S12))
                    .child(tile("Hits", card.hits.to_string(), None, theme))
                    .child(tile(
                        "Best combo",
                        best.as_ref().map_or("0".to_owned(), |(n, _)| n.clone()),
                        Some(best.map_or_else(|| "—".to_owned(), |(_, rank)| rank)),
                        theme,
                    ))
                    .child(tile("K.O.s", card.kos.len().to_string(), None, theme))
                    .child(tile(
                        "Streak",
                        card.streak_days.to_string(),
                        Some(format!("{days}{shields}")),
                        theme,
                    )),
            )
            .child(
                div()
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child(format!(
                        "First strikes {} · Comebacks {} · Assists {} · PRs reviewed {}",
                        card.first_strikes, card.comebacks, card.assists, card.prs_reviewed
                    )),
            )
            .child(div().flex().flex_col().gap(px(space::S4)).children(kos))
    }
}

/// A rendered share image for previews (stories); blocking.
pub fn preview_png(card: &FightCard, palette: &Palette, naming: Naming) -> Option<Arc<Image>> {
    let svg = card::svg(card, palette, naming, &ExportOptions::default());
    let bytes = card::png(&svg, &card::options()).ok()?;
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
}
