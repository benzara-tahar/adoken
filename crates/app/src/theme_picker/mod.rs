//! Theme picker (spec 0007 §3.1, tokens.md §9): every built-in and user
//! theme as a card with a live miniature and search. Click or `Enter`
//! applies and saves; hovering and arrowing only move the highlight (no
//! preview, no light/dark pairing: user decision 2026-09-30).
//!
//! App-wide state (registry, selection, hot reload) lives in [`library`];
//! the palette gets `> Theme: Choose…` from [`commands`] (no per-theme
//! rows, spec 0025 FR-10).

pub mod library;
pub mod model;
pub mod preview;

use adoken_theme::{Appearance, ColorTokens, ThemeRegistry, radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Chip, ContainScroll, Icon, IconName, Kbd, TextInput, TextInputEvent, TypeStyleExt,
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, Hsla, KeyBinding, ScrollHandle, SharedString,
    Subscription, Window, actions, div, prelude::*, px,
};

pub use library::{ThemeLibrary, commit, init, load};
use model::{COLUMNS, Entry, Move};
use preview::{MINIATURE_HEIGHT, Miniature, paint_colors};

use crate::palette::PaletteCommand;

actions!(
    theme_picker,
    [
        /// Theme picker: highlight the card to the left.
        MoveLeft,
        /// Theme picker: highlight the card to the right.
        MoveRight,
        /// Theme picker: highlight the card above.
        MoveUp,
        /// Theme picker: highlight the card below.
        MoveDown,
        /// Theme picker: apply the highlighted theme.
        Apply,
        /// Open Settings › Appearance on the theme picker.
        OpenThemePicker,
    ]
);

const KEY_CONTEXT: &str = "ThemePicker";

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("left", MoveLeft, cx),
        KeyBinding::new("right", MoveRight, cx),
        KeyBinding::new("up", MoveUp, cx),
        KeyBinding::new("down", MoveDown, cx),
        KeyBinding::new("enter", Apply, cx),
    ]
}

/// Register the app-wide theme actions (call once, after [`init`]).
pub fn register_actions(cx: &mut App) {
    cx.on_action(|_: &OpenThemePicker, cx| {
        crate::settings::open(crate::settings::Tab::Appearance, cx);
    });
}

/// `> Theme: Choose…`: the one theme command; the picker lists every theme.
pub fn commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: "Theme: Choose…",
        keys: None,
        action: Box::new(OpenThemePicker),
    }]
}

pub struct ThemePicker {
    /// The grid's focus: arrows move the highlight, Enter applies.
    focus: FocusHandle,
    search: Entity<TextInput>,
    entries: Vec<Entry>,
    /// Paint colors per entry, converted once per registry change.
    palettes: Vec<ColorTokens<Hsla>>,
    /// Indices into `entries`, in grid order.
    visible: Vec<usize>,
    /// Position in `visible` of the highlighted card.
    highlighted: Option<usize>,
    revision: Option<u64>,
    grid: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl ThemePicker {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        if cx.try_global::<ThemeLibrary>().is_none() {
            // Stories and tests: a library around whatever theme is installed.
            library::install_for_story(Default::default(), Vec::new(), Vec::new(), cx);
        }
        let search = cx.new(|cx| TextInput::new("Search themes", cx));
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, event, cx| {
                if *event == TextInputEvent::Changed {
                    this.refilter(cx);
                }
            }),
            cx.observe_global::<ThemeLibrary>(|this, cx| this.sync(cx)),
        ];
        let mut picker = Self {
            focus: cx.focus_handle(),
            search,
            entries: Vec::new(),
            palettes: Vec::new(),
            visible: Vec::new(),
            highlighted: None,
            revision: None,
            grid: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        picker.sync(cx);
        picker
    }

    /// Rebuild entries when the library's registry changed.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let library = cx.global::<ThemeLibrary>();
        if self.revision == Some(library.revision()) {
            cx.notify();
            return;
        }
        self.revision = Some(library.revision());
        let highlighted = self.highlighted_id().map(str::to_owned);
        self.entries = model::entries(library.registry());
        self.palettes = library
            .registry()
            .variants()
            .map(|variant| paint_colors(&variant.colors))
            .collect();
        self.refilter(cx);
        if let Some(id) = highlighted {
            self.highlighted = self.position_of(&id);
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().to_owned();
        let highlighted = self.highlighted_id().map(str::to_owned);
        self.visible = model::visible(&self.entries, &query);
        self.highlighted = highlighted.and_then(|id| self.position_of(&id));
        cx.notify();
    }

    fn position_of(&self, id: &str) -> Option<usize> {
        self.visible
            .iter()
            .position(|&index| self.entries[index].variant_id == id)
    }

    fn highlighted_id(&self) -> Option<&str> {
        self.highlighted
            .and_then(|position| self.visible.get(position))
            .map(|&index| self.entries[index].variant_id.as_str())
    }

    /// Search text (stories, the Settings search box handing off a query).
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_text(query, cx));
        self.refilter(cx);
    }

    /// Highlight the card at grid `position` (arrows; nothing is applied).
    pub fn highlight(&mut self, position: usize, cx: &mut Context<Self>) {
        if position >= self.visible.len() {
            return;
        }
        self.highlighted = Some(position);
        self.grid.scroll_to_item(position / COLUMNS);
        cx.notify();
    }

    /// Arrow keys: from the search box they hand focus to the grid, so
    /// left/right move cards instead of the caret from then on.
    fn step(&mut self, step: Move, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        if let Some(position) = model::step(self.highlighted, self.visible.len(), step) {
            self.highlight(position, cx);
        }
    }

    /// Apply the card at `position` (click / Enter): paint it and save it.
    pub fn apply(&mut self, position: usize, cx: &mut Context<Self>) {
        let Some(&index) = self.visible.get(position) else {
            return;
        };
        let id = self.entries[index].variant_id.clone();
        self.highlighted = Some(position);
        commit(&id, cx);
        cx.notify();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        div().flex().items_center().gap(px(space::S8)).child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(size::CONTROL_MD))
                .flex()
                .items_center()
                .gap(px(space::S6))
                .px(px(space::S8))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(theme.colors.border_control)
                .bg(theme.colors.bg_input)
                .child(
                    Icon::new(IconName::Search)
                        .size(size::ICON_SM)
                        .color(theme.colors.text_tertiary),
                )
                .child(div().flex_1().min_w_0().child(self.search.clone())),
        )
    }

    fn render_card(&self, position: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let index = self.visible[position];
        let entry = &self.entries[index];
        let theme = cx.theme();
        let colors = &theme.colors;
        let library = cx.global::<ThemeLibrary>();
        let in_use = library.active().id == entry.variant_id;
        let highlighted = self.highlighted == Some(position);
        let file_problem = entry.user
            && library.files().iter().any(|file| {
                file.family_id.as_deref() == Some(entry.family_id.as_str())
                    && (file.error.is_some() || !file.issues.is_empty())
            });
        let byline = match &entry.author {
            Some(author) if *author != entry.family_name => {
                format!("{} · {author}", entry.family_name)
            }
            _ => entry.family_name.clone(),
        };
        let border = if highlighted {
            colors.border_focus
        } else if in_use {
            colors.accent_solid
        } else {
            colors.border_default
        };
        div()
            .id(SharedString::from(format!(
                "theme-card-{}",
                entry.variant_id
            )))
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .rounded(px(radius::MD))
            .overflow_hidden()
            .border_1()
            .border_color(border)
            .when(highlighted || in_use, |card| {
                card.shadow(theme.shadows(adoken_theme::elevation::LEVEL_1))
            })
            .bg(colors.bg_raised)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.focus, cx);
                this.apply(position, cx);
            }))
            .child(Miniature::new(self.palettes[index]))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .px(px(space::S8))
                    .py(px(space::S6))
                    .border_t_1()
                    .border_color(colors.border_subtle)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::S4))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .type_style(typography::BODY_STRONG, theme)
                                    .text_color(colors.text_primary)
                                    .child(entry.name.clone()),
                            )
                            .when(file_problem, |row| {
                                row.child(
                                    Icon::new(IconName::CircleAlert)
                                        .size(size::ICON_SM)
                                        .color(colors.status_warning_fg),
                                )
                            })
                            .when(in_use, |row| {
                                row.child(
                                    Icon::new(IconName::Check)
                                        .size(size::ICON_SM)
                                        .color(colors.accent_fg),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::S4))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .type_style(typography::SMALL, theme)
                                    .text_color(colors.text_tertiary)
                                    .child(byline),
                            )
                            .when(entry.user, |row| row.child(Chip::new("Custom")))
                            .child(Chip::new(match entry.appearance {
                                Appearance::Dark => "Dark",
                                Appearance::Light => "Light",
                            })),
                    ),
            )
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut rows = Vec::new();
        for row in 0..self.visible.len().div_ceil(COLUMNS) {
            let start = row * COLUMNS;
            let end = (start + COLUMNS).min(self.visible.len());
            let cards: Vec<_> = (start..end)
                .map(|position| self.render_card(position, cx))
                .collect();
            rows.push(
                div()
                    .flex()
                    .gap(px(space::S12))
                    .pb(px(space::S12))
                    .children(cards)
                    // Keep card widths equal in a short last row.
                    .children((end..start + COLUMNS).map(|_| div().flex_1())),
            );
        }
        let theme = cx.theme();
        let empty = self.visible.is_empty().then(|| {
            div()
                .py(px(space::S24))
                .flex()
                .justify_center()
                .type_style(typography::BODY, theme)
                .text_color(theme.colors.text_tertiary)
                .child("No themes match. Try another name, family or author.")
        });
        div()
            .id("theme-grid")
            .flex_1()
            .min_h(px(MINIATURE_HEIGHT * 2.0))
            // Bounded even inside a scrolling host (Settings), so arrowing
            // scrolls the grid to the highlighted card.
            .max_h(px(MINIATURE_HEIGHT * 6.0))
            .overflow_y_scroll()
            .track_scroll(&self.grid)
            .contain_scroll()
            .children(rows)
            .children(empty)
    }

    fn render_issues(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let theme = cx.theme();
        let library = cx.global::<ThemeLibrary>();
        let mut lines: Vec<String> = library.settings_issues().to_vec();
        for file in library.files() {
            let name = file
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(error) = &file.error {
                let kept = if file.family_id.is_some() {
                    " (showing the last version that loaded)"
                } else {
                    ""
                };
                lines.push(format!("{name}: {error}{kept}"));
            }
            lines.extend(file.issues.iter().map(|issue| format!("{name}: {issue}")));
        }
        if lines.is_empty() {
            return None;
        }
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(space::S4))
                .child(
                    div()
                        .type_style(typography::LABEL, theme)
                        .text_color(theme.colors.text_secondary)
                        .child(format!("Theme issues ({})", lines.len())),
                )
                .children(lines.into_iter().take(12).map(|line| {
                    div()
                        .type_style(typography::SMALL, theme)
                        .text_color(theme.colors.text_tertiary)
                        .child(line)
                })),
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let hint = |keys: &'static str, label: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .child(Kbd::from_label(keys))
                .child(label)
        };
        let folder = cx
            .global::<ThemeLibrary>()
            .themes_dir()
            .map(|dir| format!("Theme files: {}", dir.display()));
        div()
            .flex()
            .items_center()
            .gap(px(space::S12))
            .type_style(typography::SMALL, theme)
            .text_color(theme.colors.text_tertiary)
            .child(hint("↑↓←→", "move"))
            .child(hint("⏎", "apply"))
            .child(div().flex_1())
            .children(folder.map(|folder| div().truncate().child(folder)))
    }
}

impl Focusable for ThemePicker {
    /// The grid: arrows move the highlight, Enter applies. The search box
    /// takes focus on click (or Tab); arrows from it return to the grid.
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ThemePicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let grid = self.render_grid(cx);
        let issues = self.render_issues(cx);
        let footer = self.render_footer(cx);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(space::S12))
            .on_action(
                cx.listener(|this, _: &MoveLeft, window, cx| this.step(Move::Left, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveRight, window, cx| this.step(Move::Right, window, cx)),
            )
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| this.step(Move::Up, window, cx)))
            .on_action(
                cx.listener(|this, _: &MoveDown, window, cx| this.step(Move::Down, window, cx)),
            )
            .on_action(cx.listener(|this, _: &Apply, _, cx| {
                if let Some(position) = this.highlighted {
                    this.apply(position, cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(toolbar)
            .child(grid)
            .children(issues)
            .child(footer)
    }
}

/// Every built-in variant's miniature in one grid, painted in each variant's
/// own tokens: the gallery's visual coverage for the ported themes.
pub fn theme_strip(cx: &App) -> impl IntoElement {
    const STRIP_COLUMNS: usize = 6;
    let theme = cx.theme();
    let variants: Vec<_> = ThemeRegistry::builtin().variants().collect();
    div()
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .p(px(space::S12))
        .children(variants.chunks(STRIP_COLUMNS).map(|row| {
            div()
                .flex()
                .gap(px(space::S8))
                .children(row.iter().map(|variant| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(space::S2))
                        .child(
                            div()
                                .rounded(px(radius::SM))
                                .overflow_hidden()
                                .border_1()
                                .border_color(theme.colors.border_default)
                                .child(Miniature::new(paint_colors(&variant.colors))),
                        )
                        .child(
                            div()
                                .truncate()
                                .type_style(typography::LABEL, theme)
                                .text_color(theme.colors.text_secondary)
                                .child(variant.name.clone()),
                        )
                }))
                .children((row.len()..STRIP_COLUMNS).map(|_| div().flex_1()))
        }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_palette_lists_one_theme_command() {
        // Spec 0025 AC-8 (theme part): no "Theme: <name>" rows (FR-10) and
        // no VS Code import (FR-11); "Theme: Choose…" opens the picker.
        for demo in [false, true] {
            let themes: Vec<&str> = crate::palette_commands(demo)
                .iter()
                .map(|command| command.title)
                .filter(|title| title.starts_with("Theme:"))
                .collect();
            assert_eq!(themes, ["Theme: Choose…"], "demo: {demo}");
        }
    }
}
