//! Backgrounds (spec 0007 §3.2, tokens.md §10): source, dim, blur, per-view
//! overrides, and a live preview. Edits go into the raw `[background]`
//! table and are parsed by `adoken_theme::background`, the same parser the
//! app loads with, so what Settings shows is what the app paints.

use std::path::PathBuf;

use adoken_theme::background::{
    BackgroundSettings, BackgroundSource, BuiltinBackground, SETTINGS_TABLE, ViewId,
};
use adoken_theme::{radius, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, ChipTone, IconName, Notice, Segmented, Select, Slider,
    Switch, Theme, TypeStyleExt,
};
use gpui::{AnyElement, Context, PathPromptOptions, div, prelude::*, px};
use toml::Table;

use super::{percent, row, section, value_label, wide};
use crate::background::{self, Status};
use crate::settings::model;
use crate::settings::view::{SettingsView, view_label};

const PREVIEW_HEIGHT: f32 = 180.0;
const SOURCES: [&str; 5] = ["None", "Dusk", "Lantern", "Ink", "Image"];

impl SettingsView {
    /// The `[background]` table as the app sees it: demo mode without one
    /// shows the demo illustration, so start from that.
    fn background_table(&self) -> Table {
        if let Some(table) = self.doc.get(SETTINGS_TABLE).and_then(toml::Value::as_table) {
            return table.clone();
        }
        match (&self.dir, self.demo) {
            (Some(dir), true) => model::demo_background_table(
                &dir.join(background::DEMO_ILLUSTRATION)
                    .display()
                    .to_string(),
            ),
            _ => Table::new(),
        }
    }

    fn edit_background(&mut self, change: impl FnOnce(&mut Table), cx: &mut Context<Self>) {
        let seed = self.background_table();
        self.edit_table(
            SETTINGS_TABLE,
            move |table| {
                if table.is_empty() {
                    *table = seed;
                }
                change(table);
            },
            cx,
        );
    }

    fn scope_view(&self) -> Option<ViewId> {
        self.bg_scope
            .checked_sub(1)
            .and_then(|ix| ViewId::ALL.get(ix).copied())
    }

    fn set_source(&mut self, source: Option<String>, cx: &mut Context<Self>) {
        self.bg_error = None;
        self.edit_background(
            |table| {
                model::set_value(table, &["source"], source.unwrap_or_default());
                model::set_value(table, &["enabled"], true);
                // A per-appearance source would hide the choice.
                model::remove_value(table, &["dark", "source"]);
                model::remove_value(table, &["light", "source"]);
            },
            cx,
        );
    }

    fn choose_image(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as Background".into()),
        });
        self._tasks.push(cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| this.use_image(path, cx)).ok();
        }));
    }

    pub(crate) fn use_image(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !model::image_supported(&path) {
            self.bg_error = Some(format!(
                "{} isn't a supported image. Use PNG, JPEG, WebP or GIF.",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ));
            cx.notify();
            return;
        }
        self.set_source(Some(path.display().to_string()), cx);
    }

    /// Write `key` in the current scope (`[background]` or its view).
    fn set_scoped(&mut self, key: &'static str, value: toml::Value, cx: &mut Context<Self>) {
        let view = self.scope_view();
        self.edit_background(
            move |table| match view {
                Some(view) => model::set_value(table, &["views", view.id(), key], value),
                None => model::set_value(table, &[key], value),
            },
            cx,
        );
    }

    pub(super) fn backgrounds(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        let colors = &theme.colors;
        let table = self.background_table();
        let (settings, issues) = BackgroundSettings::from_table(table.clone());
        let appearance = cx.theme().appearance;
        let view = self.scope_view();
        let base = settings.resolve(appearance, None);
        let scoped = settings.resolve(appearance, view);

        let mut source_rows = Vec::new();
        if self.visible(
            "Background",
            "wallpaper image picture builtin dusk lantern ink",
        ) || self.visible("Choose image…", "file picker upload photo wallpaper")
        {
            let selected = match (&base.source, base.enabled) {
                (None, _) | (_, false) => 0,
                (Some(BackgroundSource::Builtin(BuiltinBackground::Dusk)), _) => 1,
                (Some(BackgroundSource::Builtin(BuiltinBackground::Lantern)), _) => 2,
                (Some(BackgroundSource::Builtin(BuiltinBackground::Ink)), _) => 3,
                (Some(BackgroundSource::Image(_)), _) => 4,
            };
            let detail = match &base.source {
                Some(BackgroundSource::Image(path)) if base.enabled => std::path::Path::new(path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone()),
                Some(BackgroundSource::Builtin(builtin)) if base.enabled => {
                    format!("{}, generated on this Mac", builtin.name())
                }
                _ => "The theme's own background.".into(),
            };
            source_rows.push(row(
                theme,
                "Background",
                Some(detail.into()),
                Segmented::new("bg-source", SOURCES, selected).on_select(self.on(
                    cx,
                    |this, ix: usize, _, cx| match ix {
                        0 => this.set_source(None, cx),
                        1..=3 => {
                            let builtin = BuiltinBackground::ALL[ix - 1];
                            this.set_source(
                                Some(BackgroundSource::Builtin(builtin).to_string()),
                                cx,
                            );
                        }
                        _ => this.choose_image(cx),
                    },
                )),
            ));
            source_rows.push(row(
                theme,
                "Choose image…",
                Some(
                    "PNG, JPEG, WebP or GIF. adoken keeps its own copy, so the original can move."
                        .into(),
                ),
                Button::new("bg-choose", "Choose image…")
                    .icon(IconName::ImagePlus)
                    .on_press(self.press(cx, |this, _, cx| this.choose_image(cx))),
            ));
        }
        let mut notices: Vec<AnyElement> = Vec::new();
        if let Some(error) = &self.bg_error {
            notices.push(
                Notice::new(ChipTone::Danger, error.clone())
                    .icon(IconName::CircleAlert)
                    .into_any_element(),
            );
        }
        match background::status(cx) {
            Status::Processing => notices.push(
                Notice::new(ChipTone::Accent, "Processing image…")
                    .icon(IconName::LoaderCircle)
                    .into_any_element(),
            ),
            Status::Missing => notices.push(
                Notice::new(ChipTone::Warning, "Background image not found")
                    .icon(IconName::CircleAlert)
                    .detail("Choose another image, or pick None.")
                    .into_any_element(),
            ),
            Status::Failed(error) => notices.push(
                Notice::new(ChipTone::Danger, "Couldn't use that image")
                    .icon(IconName::CircleAlert)
                    .detail(error)
                    .into_any_element(),
            ),
            Status::Off | Status::Ready => {}
        }
        for issue in issues.iter().take(3) {
            notices.push(Notice::new(ChipTone::Warning, issue.to_string()).into_any_element());
        }
        source_rows.extend(notices.into_iter().map(wide));

        let mut look_rows = Vec::new();
        if self.visible("Per-view override", "view inbox review hide show scope") {
            let options = std::iter::once("Every view")
                .chain(ViewId::ALL.map(view_label))
                .collect::<Vec<_>>();
            let has_override = view.is_some_and(|view| {
                table
                    .get("views")
                    .and_then(|views| views.get(view.id()))
                    .is_some()
            });
            look_rows.push(row(
                theme,
                "Applies to",
                Some(match view {
                    Some(view) => format!(
                        "Changes below override the background in {} only.",
                        view_label(view)
                    )
                    .into(),
                    None => "Changes below apply everywhere a view has no override.".into(),
                }),
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .when(has_override, |this| {
                        this.child(
                            Button::new("bg-reset-view", "Reset view")
                                .variant(ButtonVariant::Ghost)
                                .on_press(self.press(cx, move |this, _, cx| {
                                    if let Some(view) = this.scope_view() {
                                        this.edit_background(
                                            |table| {
                                                model::remove_value(table, &["views", view.id()])
                                            },
                                            cx,
                                        );
                                    }
                                })),
                        )
                    })
                    .child(
                        Select::new("bg-scope", options, self.bg_scope)
                            .width(160.0)
                            .on_select(self.on(cx, |this, ix: usize, _, cx| {
                                this.bg_scope = ix;
                                cx.notify();
                            })),
                    ),
            ));
            if let Some(view) = view {
                look_rows.push(row(
                    theme,
                    format!("Show in {}", view_label(view)),
                    None,
                    Switch::new("bg-view-enabled", scoped.enabled).on_toggle(
                        self.on(cx, |this, on: bool, _, cx| {
                            this.set_scoped("enabled", on.into(), cx)
                        }),
                    ),
                ));
            }
        }
        let active = scoped.is_active();
        if self.visible("Dim", "darken scrim readability contrast") {
            look_rows.push(row(
                theme,
                "Dim",
                Some("Darkens (light themes: lightens) the image under the panes.".into()),
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Slider::new("bg-dim", scoped.dim, 0.0..=0.9)
                            .step(0.05)
                            .disabled(!active)
                            .on_change(self.on(cx, |this, value: f32, _, cx| {
                                this.set_scoped("dim", model::tidy(value), cx)
                            })),
                    )
                    .child(value_label(theme, percent(scoped.dim))),
            ));
        }
        if self.visible("Blur", "frost soften") {
            look_rows.push(row(
                theme,
                "Blur",
                None,
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .child(
                        Slider::new("bg-blur", f32::from(scoped.blur), 0.0..=64.0)
                            .step(1.0)
                            .disabled(!active)
                            .on_change(self.on(cx, |this, value: f32, _, cx| {
                                this.set_scoped("blur", i64::from(value.round() as u8).into(), cx)
                            })),
                    )
                    .child(value_label(theme, format!("{} px", scoped.blur))),
            ));
        }
        if !look_rows.is_empty() {
            let frame = background::preview(scoped.clone(), cx);
            let note = frame.as_ref().and_then(background::readability_note);
            let mock = div()
                .size_full()
                .flex()
                .gap(px(space::S8))
                .p(px(space::S8))
                .child(
                    div()
                        .w(px(56.0))
                        .h_full()
                        .rounded(px(radius::SM))
                        .bg(colors.bg_sidebar),
                )
                .child(
                    div()
                        .flex_1()
                        .h_full()
                        .rounded(px(radius::SM))
                        .bg(colors.bg_surface)
                        .p(px(space::S12))
                        .flex()
                        .flex_col()
                        .gap(px(space::S4))
                        .text_color(colors.text_primary)
                        .type_style(typography::BODY_STRONG, theme)
                        .child("!912 Add login throttle")
                        .child(
                            div()
                                .text_color(colors.text_secondary)
                                .type_style(typography::SMALL, theme)
                                .child("Review requested · 2 h ago"),
                        ),
                );
            let preview = div()
                .id("bg-preview")
                .w_full()
                .h(px(PREVIEW_HEIGHT))
                .rounded(px(radius::MD))
                .overflow_hidden()
                .border_1()
                .border_color(colors.border_subtle)
                .bg(colors.bg_app)
                .child(background::paint(frame.as_ref(), mock.into_any_element()));
            look_rows.push(wide(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S6))
                    .child(preview)
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(match (&note, active) {
                                (Some(note), _) => note.clone(),
                                (None, true) => {
                                    "Live preview. The window behind updates as you drag.".into()
                                }
                                (None, false) => "No background in this scope.".into(),
                            }),
                    ),
            ));
        }
        vec![
            section(theme, "Source", None, source_rows),
            section(
                theme,
                "Look",
                Some(
                    "Readability is guarded: dim rises on its own if text would get hard to read.",
                ),
                look_rows,
            ),
        ]
    }
}
