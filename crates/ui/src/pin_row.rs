//! Pin row (docs/design/pins.md §3): the kind glyph tinted by the headline
//! status with the change dot on its corner, the title (label or Azure DevOps
//! title), a trailing status mark and the unpin button that appears on hover
//! and under the keyboard cursor. Also the rail form of a pin (§4.1: glyph,
//! dot) and the dot itself (§3.4), shared with the `⌘J` rows.
//!
//! Presentational only: the app derives glyphs, tints and words from store
//! data and hands them in. Colors come from theme tokens; the dot fades in
//! over `motion.base` (reduce motion snaps it), the running glyph turns with
//! the phase the caller reads from the shared clock (0 under reduce motion).

use std::f32::consts::TAU;

use adoken_theme::{motion, opacity, radius, size, space, typography};
use gpui::{
    AnimationExt, App, ClickEvent, ElementId, Hsla, IntoElement, RenderOnce, SharedString,
    Transformation, Window, div, prelude::*, px, radians,
};

use crate::icon::{Icon, IconName};
use crate::motion::{EASE_OUT, animation};
use crate::theme::{ActiveTheme, TypeStyleExt};
use crate::tooltip::Tooltip;

/// Icons sit this far from the row's left edge in both sidebar widths, so
/// they don't move while the width animates: (rail − 2 × padding − icon) / 2.
pub const ICON_INSET: f32 = (size::RAIL - 2.0 * space::S8 - size::ICON_MD) / 2.0;
/// Group name the unpin button's hover reveal hangs off.
const ROW_GROUP: &str = "pin-row";

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// The trailing status slot (§3.3).
#[derive(Clone, Debug, PartialEq)]
pub enum PinTrailing {
    /// Active, Draft, Completed, Abandoned PRs: zero width.
    None,
    /// A 14 px status glyph in the headline tint.
    Glyph(IconName, Hsla),
    /// A work item's state word (neutral text, max 64 px).
    Word(SharedString),
    /// A running definition: the spinning `loader-circle` and elapsed time.
    /// `phase` is the `glyph-spin` phase in `[0, 1)`.
    Running {
        tint: Hsla,
        elapsed: SharedString,
        phase: f32,
    },
    /// 404 / 403 on refresh (FR-13).
    NotAvailable,
    /// The rename field's "52/60" counter (§3.9).
    Counter(SharedString),
}

/// Row states that change its background and title (§3.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PinRowState {
    /// The pinned item is in the content pane.
    pub on_screen: bool,
    /// The sidebar's keyboard cursor is on this row.
    pub cursor: bool,
    /// Mirrors a hovered or focused rail icon (the peek's linked highlight).
    pub linked: bool,
    /// The row is being dragged (origin keeps its slot, dimmed).
    pub drag_origin: bool,
}

/// The 6 px change dot (§3.4), absolutely placed on a 16 px glyph's
/// top-right corner. It fades in over `motion.base`.
pub fn change_dot(id: impl Into<ElementId>, cx: &App) -> impl IntoElement {
    let colors = &cx.theme().colors;
    div()
        .id(id)
        .absolute()
        .top(px(-space::S2))
        .right(px(-space::S2))
        .size(px(space::S6))
        .rounded(px(radius::FULL))
        .bg(colors.accent_solid)
        .with_animation("pin-dot-in", animation(motion::BASE, EASE_OUT), |dot, t| {
            dot.opacity(t)
        })
}

/// A kind glyph tinted by its headline, with the change dot when lit.
pub fn pin_glyph(
    id: impl Into<ElementId>,
    glyph: IconName,
    tint: Hsla,
    changed: bool,
    cx: &App,
) -> gpui::Div {
    div()
        .relative()
        .flex_none()
        .size(px(size::ICON_MD))
        .child(Icon::new(glyph).size(size::ICON_MD).color(tint))
        .when(changed, |glyph| glyph.child(change_dot(id, cx)))
}

#[derive(IntoElement)]
pub struct PinRow {
    id: ElementId,
    glyph: IconName,
    tint: Hsla,
    changed: bool,
    muted: bool,
    title: SharedString,
    /// Fetching row: the id shown before the title arrives ("!4821").
    fetching: Option<SharedString>,
    trailing: PinTrailing,
    state: PinRowState,
    tooltip: Option<SharedString>,
    aria_label: Option<SharedString>,
    unpin_label: SharedString,
    on_click: Option<ClickHandler>,
    on_unpin: Option<ClickHandler>,
}

impl PinRow {
    pub fn new(
        id: impl Into<ElementId>,
        glyph: IconName,
        tint: Hsla,
        title: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            glyph,
            tint,
            changed: false,
            muted: false,
            title: title.into(),
            fetching: None,
            trailing: PinTrailing::None,
            state: PinRowState::default(),
            tooltip: None,
            aria_label: None,
            unpin_label: "Unpin".into(),
            on_click: None,
            on_unpin: None,
        }
    }

    /// The change dot is lit (also bolds the title, §3.4).
    pub fn changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Fetching row (§3.5): `id` then a skeleton bar instead of the title.
    pub fn fetching(mut self, id: impl Into<SharedString>) -> Self {
        self.fetching = Some(id.into());
        self
    }

    pub fn trailing(mut self, trailing: PinTrailing) -> Self {
        self.trailing = trailing;
        self
    }

    pub fn state(mut self, state: PinRowState) -> Self {
        self.state = state;
        self
    }

    /// Shown after the standard 500 ms hover delay (§3.8).
    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    /// FR-17 spoken label.
    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// The unpin button (§3.7) and its accessible label.
    pub fn on_unpin(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.unpin_label = label.into();
        self.on_unpin = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for PinRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let state = self.state;
        let emphasized = state.on_screen || state.cursor || state.linked;
        let title_color = if self.muted {
            colors.text_tertiary
        } else if self.changed || emphasized {
            colors.text_primary
        } else {
            colors.text_secondary
        };
        let title_style = if self.changed {
            typography::BODY_STRONG
        } else {
            typography::BODY
        };
        let skeleton_phase = if self.fetching.is_some() {
            crate::motion::pulse_phase(
                crate::motion::ms(motion::GLYPH_SPIN),
                window.current_view(),
                cx,
            )
        } else {
            0.0
        };

        let glyph = pin_glyph(
            (self.id.clone(), "dot"),
            self.glyph,
            self.tint,
            self.changed && self.fetching.is_none(),
            cx,
        );
        let body = match self.fetching {
            Some(ref_id) => div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(space::S8))
                .child(
                    div()
                        .flex_none()
                        .text_color(colors.text_secondary)
                        .type_style(typography::CODE_INLINE, &theme)
                        .child(ref_id),
                )
                .child(
                    div().flex_1().min_w_0().child(
                        div()
                            .w(gpui::relative(0.6))
                            .h(px(size::PIN_SKELETON_BAR))
                            .rounded(px(radius::XS))
                            .bg(colors.bg_raised)
                            .opacity(if cx.reduce_motion() {
                                1.0
                            } else {
                                0.65 + 0.35 * (skeleton_phase * TAU).sin().abs()
                            }),
                    ),
                ),
            None => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(title_color)
                .type_style(title_style, &theme)
                .child(self.title.clone()),
        };
        let trailing = match self.trailing {
            PinTrailing::None => None,
            PinTrailing::Glyph(icon, tint) => Some(
                Icon::new(icon)
                    .size(size::ICON_SM)
                    .color(tint)
                    .into_any_element(),
            ),
            PinTrailing::Word(word) => Some(
                div()
                    .flex_none()
                    .max_w(px(size::PIN_STATE_WORD_MAX))
                    .truncate()
                    .text_color(colors.text_secondary)
                    .type_style(typography::LABEL, &theme)
                    .child(word)
                    .into_any_element(),
            ),
            PinTrailing::Running {
                tint,
                elapsed,
                phase,
            } => Some(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .child(
                        Icon::new(IconName::LoaderCircle)
                            .size(size::ICON_SM)
                            .color(tint)
                            .transform(Transformation::rotate(radians(phase * TAU))),
                    )
                    .child(
                        div()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::LABEL, &theme)
                            .child(elapsed),
                    )
                    .into_any_element(),
            ),
            PinTrailing::NotAvailable => Some(
                div()
                    .flex_none()
                    .text_color(colors.text_secondary)
                    .type_style(typography::LABEL, &theme)
                    .child("Not available")
                    .into_any_element(),
            ),
            PinTrailing::Counter(text) => Some(
                div()
                    .flex_none()
                    .text_color(colors.text_tertiary)
                    .type_style(typography::LABEL, &theme)
                    .child(text)
                    .into_any_element(),
            ),
        };
        let unpin = self.on_unpin.map(|handler| {
            div()
                .id((self.id.clone(), "unpin"))
                .role(gpui::Role::Button)
                .aria_label(self.unpin_label.clone())
                .absolute()
                .right(px(space::S2))
                .top(px((size::CONTROL_MD - size::CONTROL_SM) / 2.0))
                .flex_none()
                .size(px(size::CONTROL_SM))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::SM))
                .text_color(colors.text_tertiary)
                .hover(|s| s.bg(colors.bg_hover).text_color(colors.text_primary))
                .active(|s| s.bg(colors.bg_active))
                .tooltip(Tooltip::text("Unpin  ⌫"))
                .on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    handler(event, window, cx);
                })
                .child(Icon::new(IconName::PinOff).size(size::ICON_SM))
                .opacity(if state.cursor { 1.0 } else { 0.0 })
                .group_hover(ROW_GROUP, |button| button.opacity(1.0))
        });
        let has_unpin = unpin.is_some();

        let content = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(space::S8))
            .when(state.drag_origin, |c| c.opacity(opacity::DIMMED))
            .child(glyph)
            .child(body)
            .children(trailing);

        div()
            .id(self.id.clone())
            .group(ROW_GROUP)
            .role(gpui::Role::ListItem)
            .when_some(self.aria_label, |row, label| row.aria_label(label))
            .aria_selected(state.on_screen)
            .relative()
            .flex_none()
            .w_full()
            .h(px(size::CONTROL_MD))
            .pl(px(ICON_INSET))
            .pr(px(if state.cursor && has_unpin {
                size::CONTROL_SM + space::S4 + space::S2
            } else {
                space::S8
            }))
            .flex()
            .items_center()
            .rounded(px(radius::SM))
            .cursor_pointer()
            .when(state.on_screen, |row| row.bg(colors.bg_selected_muted))
            .when(state.cursor || state.linked, |row| row.bg(colors.bg_hover))
            .hover(move |row| {
                if has_unpin {
                    row.bg(colors.bg_hover)
                        .pr(px(size::CONTROL_SM + space::S4 + space::S2))
                } else {
                    row.bg(colors.bg_hover)
                }
            })
            .active(|row| row.bg(colors.bg_active))
            .when_some(self.tooltip, |row, text| row.tooltip(Tooltip::text(text)))
            .when_some(self.on_click, |row, handler| row.on_click(handler))
            .child(content)
            .children(unpin)
            .when(state.cursor, |row| row.child(focus_ring(&theme)))
    }
}

/// The keyboard cursor's focus ring: 2 px `border.focus`, 1 px gap outside
/// the row, radius `radius.sm` + 2 (§3.6).
pub fn focus_ring(theme: &crate::Theme) -> impl IntoElement {
    let inset = -(space::S2 + size::BORDER);
    div()
        .absolute()
        .top(px(inset))
        .bottom(px(inset))
        .left(px(inset))
        .right(px(inset))
        .border_2()
        .border_color(theme.colors.border_focus)
        .rounded(px(radius::SM + space::S2))
}

/// A pin in the collapsed rail (§4.1): the kind glyph with its dot.
#[derive(IntoElement)]
pub struct PinRailIcon {
    id: ElementId,
    glyph: IconName,
    tint: Hsla,
    changed: bool,
    state: PinRowState,
    tooltip: Option<SharedString>,
    aria_label: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl PinRailIcon {
    pub fn new(id: impl Into<ElementId>, glyph: IconName, tint: Hsla) -> Self {
        Self {
            id: id.into(),
            glyph,
            tint,
            changed: false,
            state: PinRowState::default(),
            tooltip: None,
            aria_label: None,
            on_click: None,
        }
    }

    pub fn changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    pub fn state(mut self, state: PinRowState) -> Self {
        self.state = state;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn aria_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for PinRailIcon {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let state = self.state;
        let glyph = pin_glyph(
            (self.id.clone(), "dot"),
            self.glyph,
            self.tint,
            self.changed,
            cx,
        );
        div()
            .id(self.id.clone())
            .role(gpui::Role::ListItem)
            .when_some(self.aria_label, |row, label| row.aria_label(label))
            .aria_selected(state.on_screen)
            .relative()
            .flex_none()
            .w_full()
            .h(px(size::CONTROL_MD))
            .pl(px(ICON_INSET))
            .flex()
            .items_center()
            .rounded(px(radius::SM))
            .cursor_pointer()
            .when(state.on_screen, |row| row.bg(colors.bg_selected_muted))
            .when(state.cursor || state.linked, |row| row.bg(colors.bg_hover))
            .hover(|row| row.bg(colors.bg_hover))
            .when_some(self.tooltip, |row, text| row.tooltip(Tooltip::text(text)))
            .when_some(self.on_click, |row, handler| row.on_click(handler))
            .child(glyph)
            .when(state.cursor, |row| row.child(focus_ring(&theme)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_share_the_sidebar_inset_so_they_never_move_with_the_width() {
        assert_eq!(
            space::S8 + ICON_INSET + size::ICON_MD / 2.0,
            size::RAIL / 2.0
        );
    }
}
