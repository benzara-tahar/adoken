//! Relation menu of "Add link" (spec 0015 FR-21; design §6.2): seven work
//! item relations in the state-menu frame.

use adoken_theme::{size, typography};
use gpui::{
    AnyElement, App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, Role,
    SharedString, Window, div, prelude::*,
};

use super::{
    Choose, Close, Placement, ReturnFocus, SelectNext, SelectPrevious, card, group_label, overlay,
    row_frame,
};
use crate::icon::{Icon, IconName};
use crate::theme::{ActiveTheme, TypeStyleExt};

/// A work item relation, in FR-21's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkType {
    Parent,
    Child,
    Related,
    Predecessor,
    Successor,
    Duplicate,
    DuplicateOf,
}

impl LinkType {
    pub const ALL: [LinkType; 7] = [
        Self::Parent,
        Self::Child,
        Self::Related,
        Self::Predecessor,
        Self::Successor,
        Self::Duplicate,
        Self::DuplicateOf,
    ];

    /// Row text; "Change parent…" when the item already has a parent.
    pub fn label(self, has_parent: bool) -> &'static str {
        match self {
            Self::Parent if has_parent => "Change parent…",
            Self::Parent => "Parent…",
            Self::Child => "Child…",
            Self::Related => "Related…",
            Self::Predecessor => "Predecessor…",
            Self::Successor => "Successor…",
            Self::Duplicate => "Duplicate…",
            Self::DuplicateOf => "Duplicate of…",
        }
    }

    /// The `rel` sent in `add /relations/-` (FR-21).
    pub fn rel(self) -> &'static str {
        match self {
            Self::Parent => "System.LinkTypes.Hierarchy-Reverse",
            Self::Child => "System.LinkTypes.Hierarchy-Forward",
            Self::Related => "System.LinkTypes.Related",
            Self::Predecessor => "System.LinkTypes.Dependency-Reverse",
            Self::Successor => "System.LinkTypes.Dependency-Forward",
            Self::Duplicate => "System.LinkTypes.Duplicate-Forward",
            Self::DuplicateOf => "System.LinkTypes.Duplicate-Reverse",
        }
    }

    /// The relation a stored `rel` names, if it's one of these.
    pub fn from_rel(rel: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.rel() == rel)
    }

    /// Design §6.2 glyphs (`text.secondary`).
    pub fn icon(self) -> IconName {
        match self {
            Self::Parent => IconName::ArrowUpFromLine,
            Self::Child => IconName::ArrowDownToLine,
            Self::Related => IconName::Link,
            Self::Predecessor => IconName::ArrowLeft,
            Self::Successor => IconName::ArrowRight,
            Self::Duplicate | Self::DuplicateOf => IconName::Copy,
        }
    }

    /// Its row in the menu, FR-21's order.
    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }
}

/// The menu's selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkTypeModel {
    selected: usize,
}

impl LinkTypeModel {
    pub fn selected(&self) -> LinkType {
        LinkType::ALL[self.selected]
    }

    /// `↑`/`↓`, wrapping.
    pub fn step(&mut self, delta: isize) {
        let len = LinkType::ALL.len() as isize;
        self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
    }

    pub fn highlight(&mut self, ty: LinkType) {
        self.selected = ty.index();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkTypeEvent {
    Chosen(LinkType),
}

pub struct LinkTypeMenu {
    has_parent: bool,
    model: LinkTypeModel,
    focus: FocusHandle,
    placement: Placement,
    return_focus: ReturnFocus,
}

impl EventEmitter<LinkTypeEvent> for LinkTypeMenu {}
impl EventEmitter<DismissEvent> for LinkTypeMenu {}

impl Focusable for LinkTypeMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl LinkTypeMenu {
    /// Opens with the keyboard; `has_parent` turns "Parent…" into
    /// "Change parent…".
    pub fn new(
        has_parent: bool,
        placement: Placement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        let return_focus = ReturnFocus::capture(window, cx);
        window.focus(&focus, cx);
        Self {
            has_parent,
            model: LinkTypeModel::default(),
            focus,
            placement,
            return_focus,
        }
    }

    pub fn model(&self) -> LinkTypeModel {
        self.model
    }

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus.restore(window, cx);
        cx.emit(DismissEvent);
    }

    fn choose(&mut self, ty: LinkType, window: &mut Window, cx: &mut Context<Self>) {
        self.return_focus.restore(window, cx);
        cx.emit(LinkTypeEvent::Chosen(ty));
        cx.emit(DismissEvent);
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.model.step(delta);
        cx.notify();
    }

    fn render_row(&self, ty: LinkType, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let selected = self.model.selected() == ty;
        let label = ty.label(self.has_parent);
        row_frame(("link-type-row", ty.index()), selected, theme)
            .role(Role::MenuItem)
            .aria_label(label.trim_end_matches('…').to_owned())
            .aria_selected(selected)
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.model.selected() != ty {
                    this.model.highlight(ty);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| this.choose(ty, window, cx)))
            .child(
                Icon::new(ty.icon())
                    .size(size::ICON_SM)
                    .color(colors.text_secondary),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(colors.text_primary)
                    .child(SharedString::from(label)),
            )
            .into_any_element()
    }
}

impl Render for LinkTypeMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<AnyElement> = LinkType::ALL
            .into_iter()
            .map(|ty| self.render_row(ty, cx))
            .collect();
        let theme = cx.theme().clone();
        let title = "Add link as…";
        let card = card("link-type-menu", self.placement.width, &theme)
            .track_focus(&self.focus)
            .key_context(super::KEY_CONTEXT)
            .role(Role::Menu)
            .aria_label(title.trim_end_matches('…'))
            .type_style(typography::BODY, &theme)
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.dismiss(window, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Choose, window, cx| {
                let ty = this.model.selected();
                this.choose(ty, window, cx)
            }))
            .on_action(cx.listener(|this, _: &Close, window, cx| this.dismiss(window, cx)))
            .child(group_label(title, &theme).items_center().pb_0())
            .children(rows);
        overlay("link-type-menu-in", card, self.placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_reads_change_parent_when_one_exists() {
        assert_eq!(LinkType::Parent.label(false), "Parent…");
        assert_eq!(LinkType::Parent.label(true), "Change parent…");
        assert_eq!(LinkType::Child.label(true), "Child…");
    }

    #[test]
    fn relations_round_trip_through_their_rel() {
        for ty in LinkType::ALL {
            assert_eq!(LinkType::from_rel(ty.rel()), Some(ty));
        }
        assert_eq!(
            LinkType::DuplicateOf.rel(),
            "System.LinkTypes.Duplicate-Reverse"
        );
    }

    /// Spec 0025 AC-3: digits choose no relation (and ⌃N/⌃P are gone from
    /// pickers, FR-4); ↑/↓/↩ still drive the menu.
    #[test]
    fn ac3_digits_and_ctrl_n_p_are_unbound_in_the_menu() {
        use gpui::{Action as _, KeyContext, Keymap, Keystroke};
        let keymap = Keymap::new(super::super::bindings());
        let stack = [KeyContext::parse(super::super::KEY_CONTEXT).expect("context")];
        let resolve = |keys: &str| {
            let typed = [Keystroke::parse(keys).expect("keystroke")];
            let (bindings, _) = keymap.bindings_for_input(&typed, &stack);
            bindings.first().map(|binding| binding.action().name())
        };
        for keys in ["1", "2", "3", "4", "5", "6", "7", "ctrl-n", "ctrl-p"] {
            assert_eq!(resolve(keys), None, "{keys}");
        }
        assert_eq!(resolve("down"), Some(SelectNext.name()));
        assert_eq!(resolve("up"), Some(SelectPrevious.name()));
        assert_eq!(resolve("enter"), Some(Choose.name()));
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut model = LinkTypeModel::default();
        model.step(-1);
        assert_eq!(model.selected(), LinkType::DuplicateOf);
        model.step(1);
        assert_eq!(model.selected(), LinkType::Parent);
    }
}
