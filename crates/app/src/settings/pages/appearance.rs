//! Appearance: the theme picker (spec 0007 §3.1), owned by `theme_picker`,
//! which persists `[appearance]` itself; and the code font (spec 0009
//! §3 Code font), owned by `code_font`, which writes its keys through the same
//! table.

use adoken_diff::LineKind;
use adoken_diff::lex::Lang;
use adoken_theme::code_font::{MAX_CODE_FONT_SIZE, MIN_CODE_FONT_SIZE};
use adoken_theme::{radius, space};
use adoken_ui::diff::UnifiedLine;
use adoken_ui::{ChipTone, Notice, Stepper, Switch, Theme};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

use super::{row, section, wide};
use crate::code_font;
use crate::settings::model::{
    CODE_FONT_KEYWORDS, CODE_FONT_SIZE_KEYWORDS, CODE_LIGATURES_KEYWORDS,
};
use crate::settings::view::SettingsView;

impl SettingsView {
    pub(super) fn appearance(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Option<AnyElement>> {
        self.embeds.ensure(self.tab, window, cx);
        let mut sections = Vec::new();
        if self.visible("Theme", "color dark light") {
            let picker = self
                .embeds
                .theme_picker
                .clone()
                .map(|picker| wide(div().child(picker)));
            sections.push(section(
                theme,
                "Theme",
                Some("Click a theme or press Enter to apply it."),
                picker.into_iter().collect(),
            ));
        }
        sections.push(self.code_font_section(theme, cx));
        sections
    }

    fn code_font_section(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let settings = code_font::current(cx);
        let missing = cx
            .try_global::<code_font::CodeFontState>()
            .and_then(|state| state.missing().map(str::to_owned));
        let mut rows = Vec::new();
        if self.visible("Code font", CODE_FONT_KEYWORDS) {
            if let Some(name) = missing {
                rows.push(wide(
                    Notice::new(ChipTone::Warning, format!("“{name}” isn't available.")).detail(
                        format!(
                            "Code shows in {} until it is. Pick another font below.",
                            code_font::display_family(&theme.code_font.family)
                        ),
                    ),
                ));
            }
            if let Some((picker, _)) = &self.embeds.code_font {
                rows.push(wide(div().child(picker.clone())));
            }
        }
        if self.visible("Code font size", CODE_FONT_SIZE_KEYWORDS) {
            rows.push(row(
                theme,
                "Code font size",
                Some("Diffs and code blocks.".into()),
                Stepper::new(
                    "code-font-size",
                    i64::from(settings.size),
                    i64::from(MIN_CODE_FONT_SIZE)..=i64::from(MAX_CODE_FONT_SIZE),
                )
                .suffix(" px")
                .on_change(|size, _, cx| {
                    code_font::update(cx, |settings| settings.size = size as u8)
                }),
            ));
        }
        if self.visible("Ligatures", CODE_LIGATURES_KEYWORDS) {
            rows.push(row(
                theme,
                "Ligatures",
                Some("Draw != and -> as single glyphs. Off shows every character as typed.".into()),
                Switch::new("code-ligatures", settings.ligatures).on_toggle(|on, _, cx| {
                    code_font::update(cx, |settings| settings.ligatures = on)
                }),
            ));
        }
        if !rows.is_empty() {
            rows.push(wide(preview(theme)));
        }
        section(
            theme,
            "Code font",
            Some("Diffs, code blocks and inline code. JetBrains Mono ships with adoken."),
            rows,
        )
    }
}

/// Three diff rows in the code font as configured, painted live.
fn preview(theme: &Theme) -> impl IntoElement {
    let lines = [
        (
            LineKind::Context,
            Some(41),
            Some(41),
            "fn retry(budget: u32) -> Result<(), Error> {",
        ),
        (
            LineKind::Del,
            Some(42),
            None,
            "    if budget == 0 { return Err(Error::Spent); }",
        ),
        (
            LineKind::Add,
            None,
            Some(42),
            "    if budget == 0 || self.state != State::Open {",
        ),
    ];
    div()
        .rounded(px(radius::SM))
        .overflow_hidden()
        .border_1()
        .border_color(theme.colors.border_subtle)
        .py(px(space::S4))
        .bg(theme.colors.bg_code)
        .children(
            lines
                .into_iter()
                .enumerate()
                .map(|(ix, (kind, old, new, text))| {
                    UnifiedLine::new(("code-font-preview", ix), kind, old, new, text, Lang::Rust)
                }),
        )
}
