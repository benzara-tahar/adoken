//! Tab pages. Each page returns its sections; a section hides when search
//! filters out all of its rows. Layout (section label, then a filled block
//! of hairline-split rows: title and description left, control right)
//! follows zeron `crates/ui/src/settings/widgets.rs` at `d1010657`.

mod about;
mod appearance;
mod backgrounds;
mod celebrations;
mod general;
mod global_shortcut;
mod notifications;
mod nudges;
mod sounds;
mod sync;

use adoken_theme::{radius, size, space, typography};
use adoken_ui::{Theme, TypeStyleExt};
use gpui::{
    AnyElement, App, Context, Entity, IntoElement, SharedString, Subscription, Window, div,
    prelude::*, px,
};

use super::model::Tab;
use super::view::SettingsView;

/// Views other modules own, created the first time their tab shows.
#[derive(Default)]
pub struct Embeds {
    pub(super) theme_picker: Option<Entity<crate::theme_picker::ThemePicker>>,
    /// The code font list, plus the page repainting on code font changes.
    pub(super) code_font: Option<(Entity<crate::code_font::CodeFontPicker>, Subscription)>,
    pub(super) ken: Option<Entity<crate::ken::KenSettingsSection>>,
    pub(super) permission: Option<Entity<crate::os_notify::PermissionRow>>,
    /// Keyboard › Global shortcut: the app's shortcuts, repainting with
    /// their state.
    pub(super) hotkey: Option<(Entity<crate::hotkey::Hotkeys>, Subscription)>,
}

impl Embeds {
    pub(super) fn ensure(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<SettingsView>) {
        match tab {
            Tab::Appearance => {
                if self.theme_picker.is_none() {
                    let picker = cx.new(|cx| crate::theme_picker::ThemePicker::new(window, cx));
                    self.theme_picker = Some(picker);
                }
                if self.code_font.is_none() {
                    let picker = cx.new(crate::code_font::CodeFontPicker::new);
                    let repaint =
                        cx.observe_global::<crate::code_font::CodeFontState>(|_, cx| cx.notify());
                    self.code_font = Some((picker, repaint));
                }
            }
            Tab::Ken if self.ken.is_none() => {
                self.ken = Some(crate::ken::settings_section(window, cx));
            }
            Tab::Notifications if self.permission.is_none() => {
                self.permission = Some(crate::os_notify::PermissionRow::new(cx));
            }
            Tab::Keyboard if self.hotkey.is_none() => {
                if let Some(hotkeys) = crate::hotkey::Hotkeys::global(cx) {
                    let repaint = cx.observe(&hotkeys, |_, _, cx| cx.notify());
                    self.hotkey = Some((hotkeys, repaint));
                }
            }
            _ => {}
        }
    }

    /// Focus the embedded control `query` lands on (`> Code font…`).
    pub(super) fn focus_for(&self, query: &str, window: &mut Window, cx: &mut App) {
        if query == crate::code_font::SETTINGS_QUERY
            && let Some((picker, _)) = &self.code_font
        {
            let handle = picker.read(cx).search_focus(cx);
            window.focus(&handle, cx);
        }
    }
}

impl SettingsView {
    pub(super) fn render_tab(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let sections = match self.tab {
            Tab::General => self.general(theme, cx),
            Tab::Appearance => self.appearance(theme, window, cx),
            Tab::Backgrounds => self.backgrounds(theme, cx),
            Tab::Sounds => self.sounds(theme, cx),
            Tab::Notifications => self.notifications(theme, cx),
            Tab::Nudges => self.nudges(theme, cx),
            Tab::Sync => self.sync(theme, cx),
            Tab::Keyboard => {
                let mut sections = vec![self.global_shortcut(theme, cx)];
                sections.extend(self.keyboard(theme));
                sections
            }
            Tab::Ken => self.ken(theme),
            Tab::Account => self.account(theme, cx),
            Tab::About => self.about(theme, cx),
        };
        sections.into_iter().flatten().collect()
    }

    /// A control callback that edits this view.
    pub(super) fn on<V: 'static>(
        &self,
        cx: &Context<Self>,
        f: impl Fn(&mut Self, V, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(V, &mut Window, &mut App) + 'static {
        let this = cx.entity().downgrade();
        move |value, window, cx| {
            this.update(cx, |this, cx| f(this, value, window, cx)).ok();
        }
    }

    /// A button callback that edits this view.
    pub(super) fn press(
        &self,
        cx: &Context<Self>,
        f: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let this = cx.entity().downgrade();
        move |window, cx| {
            this.update(cx, |this, cx| f(this, window, cx)).ok();
        }
    }
}

/// A titled block of rows; `None` when search hid every row.
pub(super) fn section(
    theme: &Theme,
    title: &str,
    description: Option<&str>,
    rows: Vec<AnyElement>,
) -> Option<AnyElement> {
    if rows.is_empty() {
        return None;
    }
    let colors = &theme.colors;
    let rows = rows.into_iter().enumerate().map(|(ix, row)| {
        div()
            .when(ix > 0, |row| {
                row.border_t_1().border_color(colors.border_subtle)
            })
            .child(row)
    });
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .child(
                div()
                    .px(px(space::S4))
                    .flex()
                    .flex_col()
                    .gap(px(space::S2))
                    .child(
                        div()
                            .text_color(colors.text_primary)
                            .type_style(typography::HEADING, theme)
                            .child(title.to_owned()),
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
                    .flex()
                    .flex_col()
                    .rounded(px(radius::MD))
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_raised)
                    .children(rows),
            )
            .into_any_element(),
    )
}

/// Title (+ muted description) on the left, `control` on the right.
pub(super) fn row(
    theme: &Theme,
    title: impl Into<SharedString>,
    description: Option<SharedString>,
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
                        .child(text)
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

/// A one-line row for long lists (keyboard shortcuts).
pub(super) fn compact_row(
    theme: &Theme,
    title: impl Into<SharedString>,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .h(px(size::CONTROL_LG))
        .px(px(space::S16))
        .flex()
        .items_center()
        .gap(px(space::S16))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.colors.text_primary)
                .type_style(typography::BODY, theme)
                .child(title.into()),
        )
        .child(control)
        .into_any_element()
}

/// A full-width row (embedded views, previews, lists).
pub(super) fn wide(child: impl IntoElement) -> AnyElement {
    div()
        .px(px(space::S16))
        .py(px(space::S12))
        .child(child)
        .into_any_element()
}

/// A muted value label beside a slider.
pub(super) fn value_label(theme: &Theme, text: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w(px(44.0))
        .flex()
        .justify_end()
        .text_color(theme.colors.text_secondary)
        .type_style(typography::SMALL, theme)
        .child(text.into())
        .into_any_element()
}

pub(super) fn percent(value: f32) -> String {
    format!("{}%", (value * 100.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds() {
        assert_eq!(percent(0.35), "35%");
        assert_eq!(percent(0.349_999), "35%");
        assert_eq!(percent(0.0), "0%");
    }
}
