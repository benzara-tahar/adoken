//! The Pull requests list's repository filter (spec 0004 §3.1, user
//! decision 2026-09-30): a searchable picker over the repositories that
//! have pull requests in the current tab, a chip in the list header while a
//! repository is chosen, and the filtered-empty state. The choice is saved
//! in the org's store (`review_pref`, beside the diff layout).

use adoken_theme::{elevation, radius, size, space, typography, z};
use adoken_ui::{
    ActiveTheme, Button, ButtonVariant, Chip, ChipTone, EmptyState, Icon, IconName, Kbd, TextInput,
    TextInputEvent, TypeStyleExt,
};
use gpui::{
    AnyElement, Context, Entity, Focusable, KeyBinding, ScrollStrategy, Subscription,
    UniformListScrollHandle, Window, actions, deferred, div, prelude::*, px, uniform_list,
};

use super::data::{self, ALL_REPOS, RepoOption};
use super::{LIST_CONTEXT, ReviewView};
// The picker keeps the former list pane's width (`size.list_pane`) under
// the full-width list's Repository button.
use crate::browse::{LIST_WIDTH, press};
use crate::palette::step_selection;

actions!(
    pull_requests,
    [
        /// Filter pull requests by repository…
        FilterPrsByRepo,
        /// Show pull requests from every repository.
        ClearRepoFilter,
        /// Repository picker: next repository.
        RepoPickerNext,
        /// Repository picker: previous repository.
        RepoPickerPrevious,
        /// Repository picker: filter by the highlighted repository.
        RepoPickerConfirm,
        /// Close the repository picker.
        CloseRepoPicker,
    ]
);

/// `r` in the list (the review's `r` replies; different context).
pub const OPEN_KEYS: &str = "r";
/// ⌘⌫ clears filters in the inbox and pipelines lists too.
pub const CLEAR_KEYS: &str = "secondary-backspace";
const PICKER_CONTEXT: &str = "RepoPicker";
const ROW_HEIGHT: f32 = size::CONTROL_MD;
/// Visible rows before the picker scrolls; the half row hints there is more.
const MAX_VISIBLE_ROWS: f32 = 8.5;

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(LIST_CONTEXT);
    let picker = Some(PICKER_CONTEXT);
    vec![
        KeyBinding::new(OPEN_KEYS, FilterPrsByRepo, list),
        KeyBinding::new(CLEAR_KEYS, ClearRepoFilter, list),
        KeyBinding::new("down", RepoPickerNext, picker),
        KeyBinding::new("up", RepoPickerPrevious, picker),
        KeyBinding::new("enter", RepoPickerConfirm, picker),
        KeyBinding::new("escape", CloseRepoPicker, picker),
    ]
}

pub struct RepoPicker {
    input: Entity<TextInput>,
    /// Repositories with PRs in the current tab, most first.
    options: Vec<RepoOption>,
    /// What the query shows: `None` = "All repositories", else an index
    /// into `options` ([`data::picker_rows`]).
    rows: Vec<Option<usize>>,
    /// Index into `rows`.
    selected: usize,
    scroll: UniformListScrollHandle,
    _input_events: Subscription,
}

impl RepoPicker {
    /// Re-match after the query or the options changed; the highlight lands
    /// on the active repository when it's shown, else the first row.
    fn requery(&mut self, query: &str, active: Option<&str>) {
        self.rows = data::picker_rows(&self.options, query);
        self.selected = self
            .rows
            .iter()
            .position(|row| row.map(|ix| self.options[ix].id.as_str()) == active)
            .filter(|_| query.trim().is_empty())
            .unwrap_or(0);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
    }
}

impl ReviewView {
    /// Name of the filtered repository (its id when the store has no name).
    pub(super) fn repo_name(&self) -> Option<&str> {
        let repo = self.repo.as_deref()?;
        let list = self.list.as_ref()?;
        Some(list.repos.get(repo).map_or(repo, String::as_str))
    }

    fn repo_options(&self) -> Vec<RepoOption> {
        let Some(list) = &self.list else {
            return Vec::new();
        };
        data::repo_options(
            &list.prs,
            self.tab,
            &list.me,
            &list.repos,
            self.repo.as_deref(),
        )
    }

    /// Keeps an open picker's options in step with the tab (counts).
    pub(super) fn refresh_repo_picker(&mut self, cx: &gpui::App) {
        let options = self.repo_options();
        let active = self.repo.clone();
        if let Some(picker) = &mut self.picker {
            picker.options = options;
            let query = picker.input.read(cx).text().to_owned();
            picker.requery(&query, active.as_deref());
        }
    }

    /// Opens the repository picker, its query field focused.
    pub fn open_repo_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.list.is_none() {
            return;
        }
        // The picker lives in the list's title row: the list shows first.
        self.back(window, cx);
        let input = cx.new(|cx| TextInput::new("Filter repositories", cx));
        let events = cx.subscribe(&input, |this, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let query = input.read(cx).text().to_owned();
                let active = this.repo.clone();
                if let Some(picker) = &mut this.picker {
                    picker.requery(&query, active.as_deref());
                    cx.notify();
                }
            }
        });
        window.focus(&input.focus_handle(cx), cx);
        let mut picker = RepoPicker {
            input,
            options: self.repo_options(),
            rows: Vec::new(),
            selected: 0,
            scroll: UniformListScrollHandle::new(),
            _input_events: events,
        };
        picker.requery("", self.repo.as_deref());
        self.picker = Some(picker);
        cx.notify();
    }

    fn close_repo_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn step_repo_picker(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(picker) = &mut self.picker {
            picker.selected = step_selection(picker.selected, picker.rows.len(), delta);
            picker
                .scroll
                .scroll_to_item(picker.selected, ScrollStrategy::Nearest);
            cx.notify();
        }
    }

    fn choose_repo_row(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = &self.picker else {
            return;
        };
        let Some(choice) = picker.rows.get(row) else {
            return;
        };
        let repo = choice.map(|ix| picker.options[ix].id.clone());
        self.close_repo_picker(window, cx);
        self.set_repo(repo, cx);
    }

    /// Filters the list by `repo` (`None` = all repositories) and saves the
    /// choice for this org.
    pub fn set_repo(&mut self, repo: Option<String>, cx: &mut Context<Self>) {
        if self.repo == repo {
            return;
        }
        self.repo = repo.clone();
        self.refilter(cx);
        if let Some(selected) = self.selected {
            self.list_scroll
                .scroll_to_item(selected, ScrollStrategy::Nearest);
        }
        let store = self.store.clone();
        cx.background_spawn(async move {
            if let Err(error) = data::set_repo_filter_preference(&store, repo.as_deref()) {
                log::warn!("review: couldn't save the repository filter: {error}");
            }
        })
        .detach();
        cx.notify();
    }

    /// The list title row: "Pull requests", the Repository button, and the
    /// picker dropping down under it.
    pub(super) fn render_list_title(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let colors = &theme.colors;
        div()
            .relative()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .child(
                div()
                    .flex_1()
                    .text_color(colors.text_primary)
                    .type_style(typography::HEADING, theme)
                    .child("Pull requests"),
            )
            .child(
                Button::new("pr-repo-filter", "Repository")
                    .variant(ButtonVariant::Ghost)
                    .icon(IconName::Folder)
                    .shortcut(Kbd::parse(OPEN_KEYS).expect("static binding"))
                    .disabled(self.list.is_none())
                    .on_press(press(cx, |this: &mut Self, window, cx| {
                        this.open_repo_picker(window, cx)
                    })),
            )
            .children(self.render_repo_picker(cx))
    }

    /// The active filter under the tabs: the repository chip and Clear.
    pub(super) fn render_repo_chip(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let name = self.repo_name()?.to_owned();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .child(Chip::new(format!("Repository: {name}")).tone(ChipTone::Accent)),
                )
                .child(div().flex_1())
                .child(
                    Button::new("pr-repo-clear", "Clear")
                        .variant(ButtonVariant::Ghost)
                        .shortcut(Kbd::parse(CLEAR_KEYS).expect("static binding"))
                        .on_press(press(cx, |this: &mut Self, _, cx| this.set_repo(None, cx))),
                ),
        )
    }

    /// The filter hides every PR of this tab.
    pub(super) fn render_repo_empty(&self, cx: &mut Context<Self>) -> Option<EmptyState> {
        let name = self.repo_name()?;
        Some(
            EmptyState::new(IconName::Folder, format!("No pull requests in {name}"))
                .body(format!(
                    "Nothing under {} here. Clear the filter to see every repository.",
                    self.tab.label()
                ))
                .action(
                    Button::new("pr-repo-empty-clear", ALL_REPOS)
                        .shortcut(Kbd::parse(CLEAR_KEYS).expect("static binding"))
                        .on_press(press(cx, |this: &mut Self, _, cx| this.set_repo(None, cx))),
                ),
        )
    }

    fn render_repo_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let picker = self.picker.as_ref()?;
        let theme = cx.theme();
        let colors = &theme.colors;
        let visible = (picker.rows.len() as f32).min(MAX_VISIBLE_ROWS);
        let rows = if picker.rows.is_empty() {
            div()
                .px(px(space::S8))
                .h(px(ROW_HEIGHT))
                .flex()
                .items_center()
                .text_color(colors.text_tertiary)
                .child("No repositories match")
                .into_any_element()
        } else {
            uniform_list(
                "repo-picker-rows",
                picker.rows.len(),
                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range
                        .map(|row| this.render_repo_row(row, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&picker.scroll)
            .h(px(visible * ROW_HEIGHT))
            .into_any_element()
        };
        Some(
            deferred(
                div()
                    .id("repo-picker")
                    .key_context(PICKER_CONTEXT)
                    .absolute()
                    .top(px(size::CONTROL_MD + space::S4))
                    .right_0()
                    .w(px(LIST_WIDTH))
                    .p(px(space::S4))
                    .flex()
                    .flex_col()
                    .gap(px(space::S4))
                    .rounded(px(radius::MD))
                    .border_1()
                    .border_color(colors.border_default)
                    .bg(colors.bg_overlay)
                    .shadow(theme.shadows(elevation::LEVEL_2))
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY, theme)
                    .occlude()
                    .role(gpui::Role::Menu)
                    .aria_label("Filter by repository")
                    .on_mouse_down_out(
                        cx.listener(|this, _, window, cx| this.close_repo_picker(window, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &RepoPickerNext, _, cx| this.step_repo_picker(1, cx)),
                    )
                    .on_action(cx.listener(|this, _: &RepoPickerPrevious, _, cx| {
                        this.step_repo_picker(-1, cx)
                    }))
                    .on_action(cx.listener(|this, _: &RepoPickerConfirm, window, cx| {
                        let row = this.picker.as_ref().map_or(0, |p| p.selected);
                        this.choose_repo_row(row, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &CloseRepoPicker, window, cx| {
                        this.close_repo_picker(window, cx)
                    }))
                    .child(
                        div()
                            .h(px(size::CONTROL_MD))
                            .flex()
                            .items_center()
                            .gap(px(space::S6))
                            .px(px(space::S8))
                            .rounded(px(radius::SM))
                            .border_1()
                            .border_color(colors.border_control)
                            .bg(colors.bg_input)
                            .child(
                                Icon::new(IconName::Search)
                                    .size(size::ICON_SM)
                                    .color(colors.text_tertiary),
                            )
                            .child(div().flex_1().min_w_0().child(picker.input.clone())),
                    )
                    .child(rows),
            )
            .with_priority(usize::from(z::DROPDOWN))
            .into_any_element(),
        )
    }

    fn render_repo_row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let picker = self.picker.as_ref().expect("rows render while open");
        let theme = cx.theme();
        let colors = &theme.colors;
        let (label, count, active) = match picker.rows[row] {
            None => (
                ALL_REPOS.to_owned(),
                picker.options.iter().map(|o| o.count).sum::<usize>(),
                self.repo.is_none(),
            ),
            Some(ix) => {
                let option = &picker.options[ix];
                (
                    option.name.clone(),
                    option.count,
                    self.repo.as_deref() == Some(option.id.as_str()),
                )
            }
        };
        let highlighted = row == picker.selected;
        div()
            .id(("repo-picker-row", row))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::XS))
            .cursor_pointer()
            .when(highlighted, |d| d.bg(colors.bg_selected))
            .when(!highlighted, |d| d.hover(|s| s.bg(colors.bg_hover)))
            .role(gpui::Role::MenuItem)
            .aria_label(label.clone())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_primary)
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(count.to_string()),
            )
            .child(div().flex_none().size(px(size::ICON_SM)).when(active, |d| {
                d.child(
                    Icon::new(IconName::Check)
                        .size(size::ICON_SM)
                        .color(colors.accent_fg),
                )
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.choose_repo_row(row, window, cx)))
            .into_any_element()
    }
}
