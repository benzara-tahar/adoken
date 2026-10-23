//! Keyboard shortcut cheat sheet (docs/design/command-palette.md §7: `?`
//! when no text field is focused, and ⌘/). Generated on open from the live
//! keymap plus the palette's commands ([`sheet`]), so it can't drift from
//! the real bindings; grouped by screen (Everywhere, Today, Inbox, … then
//! Command palette) with widget-internal keys hidden (spec 0025 FR-7);
//! searchable by title, screen, key, or action.

pub mod sheet;

use adoken_theme::{elevation, radius, size, space, typography, z};
use adoken_ui::{
    ActiveTheme, Icon, IconName, Kbd, KeyPlatform, TextInput, TextInputEvent, Theme, TypeStyleExt,
    format_keystroke,
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, MouseButton, Subscription, Window,
    actions, anchored, deferred, div, point, prelude::*, px,
};

use crate::palette::PaletteCommand;
use crate::{FocusNext, FocusPrevious};
use sheet::{BindingInfo, CommandInfo, Group};

actions!(
    help,
    [
        /// Show or hide the keyboard shortcut cheat sheet.
        ToggleShortcuts,
        /// Clear the cheat sheet's search, or close it when already empty.
        DismissShortcuts,
    ]
);

/// ⌘/ everywhere; `?` wherever typing isn't going into a text field.
pub const TOGGLE_KEYS: &str = "secondary-/";
pub const QUESTION_KEYS: &str = "?";
const KEY_CONTEXT: &str = "CheatSheet";
/// `?` only in the shell, and never while a text field has focus.
const QUESTION_CONTEXT: &str = "Workspace && !TextInput";
const WIDTH: f32 = 640.0;
const PLACEHOLDER: &str = "Search shortcuts…";

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(TOGGLE_KEYS, ToggleShortcuts, None),
        KeyBinding::new(QUESTION_KEYS, ToggleShortcuts, Some(QUESTION_CONTEXT)),
        KeyBinding::new("escape", DismissShortcuts, Some(KEY_CONTEXT)),
    ]
}

pub fn commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: "Keyboard shortcuts",
        keys: Some(TOGGLE_KEYS),
        action: Box::new(ToggleShortcuts),
    }]
}

/// Palette commands as the sheet sees them (title per action).
pub fn command_infos(commands: &[PaletteCommand]) -> Vec<CommandInfo> {
    commands
        .iter()
        .map(|command| CommandInfo {
            title: command.title,
            action: command.action.name(),
        })
        .collect()
}

/// Flattens gpui bindings for [`sheet::build`], formatting keys for this
/// platform.
pub fn binding_infos<'a>(bindings: impl IntoIterator<Item = &'a KeyBinding>) -> Vec<BindingInfo> {
    let platform = KeyPlatform::current();
    bindings
        .into_iter()
        .map(|binding| BindingInfo {
            action: binding.action().name(),
            keys: binding
                .keystrokes()
                .iter()
                .map(|keystroke| format_keystroke(keystroke.inner(), platform))
                .collect::<Vec<_>>()
                .join(" "),
            context: binding.predicate().map(|predicate| predicate.to_string()),
        })
        .collect()
}

/// The sheet over the live keymap and action docs, with the global summon
/// shortcuts (spec 0021 FR-13) first in the Everywhere group.
pub fn generate(commands: &[CommandInfo], cx: &App) -> Vec<Group> {
    let keymap = cx.key_bindings();
    let bindings = binding_infos(keymap.borrow().bindings());
    let docs = cx.action_documentation();
    let mut groups = sheet::build(&bindings, commands, |action| docs.get(action).copied());
    let global = crate::hotkey::Hotkeys::global(cx)
        .map(|hotkeys| hotkeys.read(cx).cheat_sheet_rows())
        .unwrap_or_default();
    if !global.is_empty() {
        let entries = global.into_iter().map(|(title, keys)| sheet::Entry {
            title: title.to_owned(),
            action: "hotkey::Summon",
            keys: vec![keys],
        });
        let everywhere = sheet::Screen::Everywhere.name();
        match groups.iter_mut().find(|g| g.name == everywhere) {
            Some(group) => {
                group.entries.splice(0..0, entries);
            }
            None => groups.insert(
                0,
                Group {
                    name: everywhere,
                    entries: entries.collect(),
                },
            ),
        }
    }
    groups
}

pub struct CheatSheet {
    visible: bool,
    input: Entity<TextInput>,
    commands: Vec<CommandInfo>,
    groups: Vec<Group>,
    shown: Vec<Group>,
    previous_focus: Option<FocusHandle>,
    /// Render in place instead of as a window overlay (gallery stories).
    inline: bool,
    _input_events: Subscription,
}

impl Focusable for CheatSheet {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl CheatSheet {
    pub fn new(commands: Vec<CommandInfo>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(PLACEHOLDER, cx));
        let input_events = cx.subscribe(&input, |this, input, event, cx| {
            if let TextInputEvent::Changed = event {
                let query = input.read(cx).text().to_owned();
                this.shown = sheet::filter(&this.groups, &query);
                cx.notify();
            }
        });
        Self {
            visible: false,
            input,
            commands,
            groups: Vec::new(),
            shown: Vec::new(),
            previous_focus: None,
            inline: false,
            _input_events: input_events,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn set_inline(&mut self, inline: bool, cx: &mut Context<Self>) {
        self.inline = inline;
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            self.close(window, cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Regenerates from the current keymap (cheap: a few hundred bindings).
    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visible {
            return;
        }
        self.visible = true;
        self.groups = generate(&self.commands, cx);
        self.shown = self.groups.clone();
        self.previous_focus = window.focused(cx);
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }

    /// Opens with `query` typed (gallery stories).
    pub fn open_with(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.open(window, cx);
        self.input.update(cx, |input, cx| input.set_text(query, cx));
    }

    pub fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        self.visible = false;
        self.input.update(cx, |input, cx| input.set_text("", cx));
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).text().is_empty() {
            self.close(window, cx);
        } else {
            self.input.update(cx, |input, cx| input.set_text("", cx));
        }
    }

    fn render_group(group: &Group, theme: &Theme) -> gpui::Div {
        let colors = &theme.colors;
        div()
            .flex()
            .flex_col()
            .pb(px(space::S12))
            .child(
                div()
                    .h(px(size::CONTROL_SM))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::LABEL, theme)
                    .child(group.name),
            )
            .children(group.entries.iter().map(|entry| {
                let keys = entry.keys.iter().map(|keys| Kbd::from_label(keys.clone()));
                div()
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .rounded(px(radius::SM))
                    .hover(|row| row.bg(colors.bg_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .child(entry.title.clone()),
                    )
                    .children(keys)
                    .when(entry.keys.is_empty(), |row| {
                        row.child(
                            div()
                                .text_color(colors.text_tertiary)
                                .type_style(typography::SMALL, theme)
                                .child("in ⌘P"),
                        )
                    })
            }))
    }
}

impl Render for CheatSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let viewport = window.viewport_size();
        let width = WIDTH.min(f32::from(viewport.width) - 2.0 * space::S16);
        let max_height = (f32::from(viewport.height) - 2.0 * (size::TITLEBAR + space::S24))
            .max(size::CONTROL_LG * 4.0);

        let header = div()
            .flex_none()
            .h(px(size::CONTROL_LG + space::S16))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                Icon::new(IconName::Keyboard)
                    .size(size::ICON_MD)
                    .color(colors.text_tertiary),
            )
            .child(
                div()
                    .flex_none()
                    .text_color(colors.text_primary)
                    .type_style(typography::HEADING, &theme)
                    .child("Keyboard shortcuts"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h(px(size::CONTROL_MD))
                    .px(px(space::S8))
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(colors.border_control)
                    .bg(colors.bg_input)
                    .child(
                        Icon::new(IconName::Search)
                            .size(size::ICON_SM)
                            .color(colors.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().child(self.input.clone())),
            );

        let body = if self.shown.is_empty() {
            div()
                .p(px(space::S24))
                .flex()
                .justify_center()
                .text_color(colors.text_secondary)
                .child("No shortcuts match.")
                .into_any_element()
        } else {
            div()
                .id("cheat-sheet-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p(px(space::S8))
                .children(
                    self.shown
                        .iter()
                        .map(|group| Self::render_group(group, &theme)),
                )
                .into_any_element()
        };

        let footer = div()
            .flex_none()
            .h(px(size::CONTROL_LG))
            .px(px(space::S16))
            .flex()
            .items_center()
            .gap(px(space::S6))
            .border_t_1()
            .border_color(colors.border_subtle)
            .text_color(colors.text_tertiary)
            .type_style(typography::SMALL, &theme)
            .child(Kbd::parse("escape").expect("static binding"))
            .child("close")
            .child(div().flex_1())
            .child("Every action also runs from")
            .child(Kbd::parse(crate::palette::TOGGLE_KEYS).expect("static binding"));

        let card = div()
            .id("cheat-sheet")
            .key_context(KEY_CONTEXT)
            .occlude()
            .w(px(width))
            .max_h(px(max_height))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(radius::LG))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_3))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .on_action(
                cx.listener(|this, _: &DismissShortcuts, window, cx| this.dismiss(window, cx)),
            )
            // Modal focus trap: Tab must not leave the search field.
            .on_action(|_: &FocusNext, _, _| {})
            .on_action(|_: &FocusPrevious, _, _| {})
            .child(header)
            .child(body)
            .child(footer);

        let scrim = div()
            .id("cheat-sheet-scrim")
            .occlude()
            .bg(colors.bg_scrim)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close(window, cx)),
            )
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(crate::background::frosted(radius::LG, card));
        if self.inline {
            return scrim.size_full().into_any_element();
        }
        deferred(
            anchored()
                .position(point(px(0.0), px(0.0)))
                .child(scrim.w(viewport.width).h(viewport.height)),
        )
        .with_priority(usize::from(z::DIALOG))
        .into_any_element()
    }
}
