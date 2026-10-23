//! Segmented control: one choice out of a few (access level, token
//! lifetime). Click a segment, or drive it from actions; the selected
//! segment is a raised plate.

use std::rc::Rc;

use adoken_theme::{elevation, radius, size, space, typography};
use gpui::{App, ElementId, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use crate::theme::{ActiveTheme, TypeStyleExt};

type SelectHandler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<SharedString>,
    selected: usize,
    on_select: Option<SelectHandler>,
}

impl Segmented {
    pub fn new(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            on_select: None,
        }
    }

    pub fn on_select(mut self, handler: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let segment_height = size::CONTROL_MD - 2.0 * space::S2 - 2.0 * adoken_theme::size::BORDER;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(space::S2))
            .h(px(size::CONTROL_MD))
            .p(px(space::S2))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(colors.border_control)
            .bg(colors.bg_input)
            .type_style(typography::BODY_STRONG, theme)
            .children(self.options.into_iter().enumerate().map(|(ix, label)| {
                let selected = ix == self.selected;
                let handler = self.on_select.clone();
                div()
                    .id(ix)
                    .h(px(segment_height))
                    .px(px(space::S12))
                    .flex()
                    .items_center()
                    .rounded(px(radius::XS))
                    .when(selected, |segment| {
                        segment
                            .bg(colors.bg_raised)
                            .border_1()
                            .border_color(colors.border_default)
                            .shadow(theme.shadows(elevation::LEVEL_1))
                            .text_color(colors.text_primary)
                    })
                    .when(!selected, |segment| {
                        segment
                            .text_color(colors.text_secondary)
                            .hover(|style| style.bg(colors.bg_hover))
                    })
                    .when_some(handler, |segment, handler| {
                        segment
                            .cursor_pointer()
                            .on_click(move |_, window, cx| handler(ix, window, cx))
                    })
                    .child(label)
            }))
    }
}
