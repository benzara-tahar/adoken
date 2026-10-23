//! Settings › Appearance › Code font: a searchable list of the bundled
//! JetBrains Mono, the system monospace face, and every installed monospace
//! family, each previewed in its own face. `↑`/`↓` move the highlight from
//! the search box, `Enter` or a click applies and saves.

use adoken_theme::{CodeFont, radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, ContainScroll, Icon, IconName, ListRow, Spinner, TextInput, TextInputEvent,
    TypeStyleExt,
};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, KeyBinding, ScrollHandle, SharedString,
    Subscription, Window, actions, div, prelude::*, px,
};

use super::{CodeFontState, Families, choices, load_families, update};

actions!(
    code_font_picker,
    [
        /// Code font list: highlight the previous font.
        Previous,
        /// Code font list: highlight the next font.
        Next,
        /// Code font list: use the highlighted font.
        Confirm,
    ]
);

const KEY_CONTEXT: &str = "CodeFontPicker";
/// Rows visible before the list scrolls.
const VISIBLE_ROWS: f32 = 6.0;
/// What each row previews in its own face (ligature pairs included, so the
/// ligatures switch shows).
const SAMPLE: &str = "fn diff(a: &str) -> bool { a != \"0Oo1lI\" }";

pub fn bindings() -> Vec<KeyBinding> {
    let cx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("up", Previous, cx),
        KeyBinding::new("down", Next, cx),
        KeyBinding::new("enter", Confirm, cx),
    ]
}

pub struct CodeFontPicker {
    focus: FocusHandle,
    search: Entity<TextInput>,
    visible: Vec<CodeFont>,
    highlighted: Option<usize>,
    list: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl CodeFontPicker {
    /// Starts the installed-family scan the first time a picker shows.
    pub fn new(cx: &mut Context<Self>) -> Self {
        load_families(cx);
        let search = cx.new(|cx| TextInput::new("Search fonts", cx));
        let subscriptions = vec![
            cx.subscribe(&search, |this, _, event, cx| {
                if *event == TextInputEvent::Changed {
                    this.refilter(cx);
                }
            }),
            cx.observe_global::<CodeFontState>(|this, cx| this.refilter(cx)),
        ];
        let mut picker = Self {
            focus: cx.focus_handle(),
            search,
            visible: Vec::new(),
            highlighted: None,
            list: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        picker.refilter(cx);
        picker
    }

    /// The search box's focus (`> Code font…` lands there).
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }

    /// Search text (stories).
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_text(query, cx));
        self.refilter(cx);
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().to_owned();
        let highlighted = self
            .highlighted
            .and_then(|ix| self.visible.get(ix).cloned());
        let installed = match cx.try_global::<CodeFontState>().map(|s| s.families()) {
            Some(Families::Ready(families)) => families.clone(),
            _ => Default::default(),
        };
        self.visible = choices(&installed, &query);
        self.highlighted = highlighted
            .and_then(|font| self.visible.iter().position(|f| *f == font))
            .or_else(|| (!query.is_empty() && !self.visible.is_empty()).then_some(0));
        cx.notify();
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(last) = self.visible.len().checked_sub(1) else {
            return;
        };
        let next = match self.highlighted {
            None if delta > 0 => 0,
            None => last,
            Some(ix) => ix.saturating_add_signed(delta).min(last),
        };
        self.highlighted = Some(next);
        self.list.scroll_to_item(next);
        cx.notify();
    }

    /// Use the font at `ix` (click / Enter): paint it and save it.
    pub fn choose(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(font) = self.visible.get(ix).cloned() else {
            return;
        };
        self.highlighted = Some(ix);
        update(cx, |settings| settings.font = font);
        cx.notify();
    }

    fn render_row(&self, ix: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let font = &self.visible[ix];
        let theme = cx.theme();
        let configured = cx
            .try_global::<CodeFontState>()
            .map(|state| state.settings().font == *font)
            .unwrap_or(false);
        let this = cx.entity().downgrade();
        let sample = div()
            .max_w(px(260.0))
            .truncate()
            .text_color(theme.colors.text_secondary)
            .font_family(SharedString::from(font.family().to_owned()))
            .font_features(theme.code_font.features.clone())
            .text_size(px(typography::CODE.size))
            .child(SAMPLE);
        ListRow::new(("code-font", ix), font.label())
            .height(size::PALETTE_ROW_COMPACT)
            .selected(self.highlighted == Some(ix))
            .leading(div().children(configured.then(|| {
                Icon::new(IconName::Check)
                    .size(size::ICON_SM)
                    .color(theme.colors.accent_fg)
            })))
            .trailing(sample)
            .on_click(move |_, _, cx| {
                this.update(cx, |this, cx| this.choose(ix, cx)).ok();
            })
    }
}

impl Focusable for CodeFontPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CodeFontPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = (0..self.visible.len())
            .map(|ix| self.render_row(ix, cx))
            .collect();
        let theme = cx.theme();
        let colors = &theme.colors;
        let loading = matches!(
            cx.try_global::<CodeFontState>().map(|s| s.families()),
            Some(Families::Loading)
        );
        let status = if loading {
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S6))
                    .child(Spinner::new())
                    .child("Finding installed monospace fonts…")
                    .into_any_element(),
            )
        } else if self.visible.is_empty() {
            Some(div().child("No monospace fonts match.").into_any_element())
        } else {
            None
        };
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .w_full()
            .flex()
            .flex_col()
            .gap(px(space::S8))
            .on_action(cx.listener(|this, _: &Previous, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Next, _, cx| this.step(1, cx)))
            .on_action(
                cx.listener(|this, _: &Confirm, _, cx| match this.highlighted {
                    Some(ix) => this.choose(ix, cx),
                    None => cx.propagate(),
                }),
            )
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
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("code-font-list")
                    .max_h(px(size::PALETTE_ROW_COMPACT * VISIBLE_ROWS))
                    .overflow_y_scroll()
                    .track_scroll(&self.list)
                    .contain_scroll()
                    .children(rows),
            )
            .children(status.map(|status| {
                div()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child(status)
            }))
    }
}
