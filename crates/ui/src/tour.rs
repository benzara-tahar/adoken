//! Guided tours' presentation (spec 0020, docs/design/tours.md): anchor
//! recording and FR-8 visibility, §3 placement, the §2.2 spotlight and the
//! §2.1 coach mark, painted by one [`layer`]. Which tour shows which step,
//! and when, is the app's business (`adoken_app::tours`); this module only
//! paints what it is told.
//!
//! Anchors cost nothing while nobody records (FR-11): [`anchor`] checks one
//! flag and returns `None`. While a [`Recording`] lives, each anchor is an
//! invisible canvas filling its parent that records the parent's bounds and
//! clip during prepaint; the [`layer`] (deferred, prepainted after every
//! anchor) commits that frame's records and places the mark in the same
//! frame.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adoken_theme::{elevation, motion, radius, size, space, typography, z};
use gpui::{
    AnimationExt, AnyElement, App, AvailableSpace, BorderStyle, Bounds, DispatchPhase, Element,
    ElementId, Global, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId,
    MouseButton, MouseDownEvent, PathBuilder, Pixels, Point, RenderOnce, Role, ScrollWheelEvent,
    SharedString, Size, Style, Window, WindowId, canvas, deferred, div, point, prelude::*, px,
    quad,
};

use crate::Kbd;
use crate::button::ButtonVariant;
use crate::motion::{EASE_IN, EASE_OUT, animation};
use crate::theme::{ActiveTheme, TypeStyleExt, hsla};

/// Gap between the spotlight outline and the coach mark (§3 rule 1).
pub const GAP: f32 = space::S8;
/// The mark stays this far inside the content area (§3 rule 1).
pub const MARGIN: f32 = space::S8;
/// The spotlight is drawn this far outside the anchor (§2.2).
pub const OUTLINE_OFFSET: f32 = space::S4;
/// Spotlight stroke width (§2.2).
pub const OUTLINE_WIDTH: f32 = 2.0;
/// Inside placement inset from the anchor's top-right corner (§3 rule 4).
pub const INSIDE_INSET: f32 = space::S16;
/// FR-8: an anchor smaller than this (either side) isn't visible.
pub const MIN_VISIBLE: f32 = 24.0;
/// FR-8: at least this share of the anchor's area must show.
pub const MIN_VISIBLE_SHARE: f32 = 0.5;
/// §2.1 arrow: 12 × 6, at least 16 px from the mark's corners.
const ARROW_WIDTH: f32 = 12.0;
const ARROW_DEPTH: f32 = 6.0;
const ARROW_CORNER: f32 = 16.0;
/// `menu-in` travel toward the anchor (§4).
const ENTRANCE_TRAVEL: f32 = space::S4;
/// Records kept per window and frame; a runaway list never grows memory.
const MAX_RECORDS: usize = 256;

// ── Anchors (FR-8, FR-11) ───────────────────────────────────────────────

/// Where an anchor painted: its bounds and the clip (scroll viewport and
/// every clipping ancestor) it painted under, both in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorRect {
    pub bounds: Bounds<Pixels>,
    pub clip: Bounds<Pixels>,
}

impl AnchorRect {
    /// The part of the anchor that shows inside `content` (the window's
    /// content area), when FR-8 counts it as visible: at least half its area
    /// and at least 24 × 24 px inside both its clip and `content`.
    pub fn visible_in(&self, content: Bounds<Pixels>) -> Option<Bounds<Pixels>> {
        let area = |b: &Bounds<Pixels>| f32::from(b.size.width) * f32::from(b.size.height);
        let total = area(&self.bounds);
        if total <= 0.0 {
            return None;
        }
        let shown = self.bounds.intersect(&self.clip).intersect(&content);
        let (w, h) = (f32::from(shown.size.width), f32::from(shown.size.height));
        (w >= MIN_VISIBLE && h >= MIN_VISIBLE && area(&shown) >= total * MIN_VISIBLE_SHARE)
            .then_some(shown)
    }
}

/// One frame's anchors in one window, in paint order.
pub type FrameAnchors = Rc<[(&'static str, AnchorRect)]>;

#[derive(Default)]
struct Frames {
    pending: HashMap<WindowId, Vec<(&'static str, AnchorRect)>>,
    current: HashMap<WindowId, FrameAnchors>,
    /// Windows where a view reported, this frame, that content its anchors
    /// cover is still on its way (a search in flight).
    unsettled: HashSet<WindowId>,
    /// Whether each window's last committed frame was settled.
    settled: HashMap<WindowId, bool>,
}

#[derive(Default)]
struct Registry {
    recorders: Rc<Cell<usize>>,
    frames: Rc<RefCell<Frames>>,
}

impl Global for Registry {}

/// While held, anchors record their bounds every frame. Dropping the last
/// one stops recording (anchors go back to costing one flag check).
#[must_use = "anchors record only while the guard lives"]
pub struct Recording(Rc<Cell<usize>>);

impl Drop for Recording {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Start recording anchors (a tour shows or is about to).
pub fn record(cx: &mut App) -> Recording {
    let recorders = cx.default_global::<Registry>().recorders.clone();
    recorders.set(recorders.get() + 1);
    Recording(recorders)
}

/// Whether anchors record this frame.
pub fn is_recording(cx: &App) -> bool {
    cx.try_global::<Registry>()
        .is_some_and(|registry| registry.recorders.get() > 0)
}

/// An invisible child filling its parent that records the parent as the
/// tour anchor `id` (`<view>.<region>`, spec FR-10). `None` (no element,
/// no work) unless a [`Recording`] lives.
pub fn anchor(id: &'static str, cx: &App) -> Option<AnyElement> {
    let registry = cx.try_global::<Registry>()?;
    if registry.recorders.get() == 0 {
        return None;
    }
    let frames = registry.frames.clone();
    Some(
        canvas(
            move |bounds, window, _| {
                let clip = window.content_mask().bounds;
                let window_id = window.window_handle().window_id();
                let mut frames = frames.borrow_mut();
                let pending = frames.pending.entry(window_id).or_default();
                if pending.len() < MAX_RECORDS {
                    pending.push((id, AnchorRect { bounds, clip }));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element(),
    )
}

/// The view rendering now is still loading content its anchors cover (a
/// search in flight), so this frame's anchors aren't final: a tour waits
/// for a settled frame before it starts. One flag check unless a
/// [`Recording`] lives (FR-11).
pub fn unsettled(window: &Window, cx: &App) {
    let Some(registry) = cx.try_global::<Registry>() else {
        return;
    };
    if registry.recorders.get() == 0 {
        return;
    }
    let window_id = window.window_handle().window_id();
    registry.frames.borrow_mut().unsettled.insert(window_id);
}

/// This frame's records for `window` become its current anchors (the
/// layer calls it once per frame, after every anchor prepainted).
fn commit(window: &Window, cx: &App) -> FrameAnchors {
    let Some(registry) = cx.try_global::<Registry>() else {
        return Rc::from([]);
    };
    let window_id = window.window_handle().window_id();
    let mut frames = registry.frames.borrow_mut();
    let pending = frames.pending.remove(&window_id).unwrap_or_default();
    let anchors: FrameAnchors = Rc::from(pending);
    frames.current.insert(window_id, anchors.clone());
    let settled = !frames.unsettled.remove(&window_id);
    frames.settled.insert(window_id, settled);
    anchors
}

/// The anchors `window` painted in its last committed frame.
pub fn anchors(window: WindowId, cx: &App) -> FrameAnchors {
    cx.try_global::<Registry>()
        .and_then(|registry| registry.frames.borrow().current.get(&window).cloned())
        .unwrap_or_else(|| Rc::from([]))
}

/// Whether `window`'s last committed frame had no view still loading
/// ([`unsettled`]); `false` before any frame was committed.
pub fn settled(window: WindowId, cx: &App) -> bool {
    cx.try_global::<Registry>().is_some_and(|registry| {
        registry
            .frames
            .borrow()
            .settled
            .get(&window)
            .copied()
            .unwrap_or(false)
    })
}

/// The visible part of the first visible record of `id` in `anchors`.
pub fn visible(
    anchors: &[(&'static str, AnchorRect)],
    id: &str,
    content: Bounds<Pixels>,
) -> Option<Bounds<Pixels>> {
    anchors
        .iter()
        .filter(|(anchor, _)| *anchor == id)
        .find_map(|(_, rect)| rect.visible_in(content))
}

// ── Placement (§3) ──────────────────────────────────────────────────────

/// Where a coach mark sits relative to its anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Above,
    Below,
    Left,
    Right,
    /// Inside the anchor's top-right corner (pane-sized anchors).
    Inside,
}

impl Side {
    fn opposite(self) -> Self {
        match self {
            Side::Above => Side::Below,
            Side::Below => Side::Above,
            Side::Left => Side::Right,
            Side::Right => Side::Left,
            Side::Inside => Side::Inside,
        }
    }

    fn perpendicular(self) -> [Self; 2] {
        match self {
            Side::Above | Side::Below => [Side::Right, Side::Left],
            Side::Left | Side::Right => [Side::Below, Side::Above],
            Side::Inside => [Side::Inside, Side::Inside],
        }
    }

    /// Unit direction from the anchor toward the mark (the entrance starts
    /// [`ENTRANCE_TRAVEL`] further out and moves back toward the anchor).
    fn outward(self) -> (f32, f32) {
        match self {
            Side::Above => (0.0, -1.0),
            Side::Below => (0.0, 1.0),
            Side::Left => (-1.0, 0.0),
            Side::Right => (1.0, 0.0),
            Side::Inside => (0.0, -1.0),
        }
    }
}

/// A placed coach mark: its bounds, the side it took, and the arrow's
/// center along the edge facing the anchor (from the mark's left or top
/// edge); no arrow inside the anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub bounds: Bounds<Pixels>,
    pub side: Side,
    pub arrow: Option<f32>,
}

/// The spotlight outline for an anchor's visible part: `space.4` outside
/// it, clamped `space.4` inside the content area (§2.2).
pub fn spotlight(anchor: Bounds<Pixels>, content: Bounds<Pixels>) -> Bounds<Pixels> {
    anchor
        .dilate(px(OUTLINE_OFFSET))
        .intersect(&content.inset(px(OUTLINE_OFFSET)))
}

/// Place a `mark`-sized coach mark beside `target` (the spotlight) inside
/// `content` (§3): the preferred side, its opposite, then the two
/// perpendicular sides; centered on the target, slid to fit; inside the
/// target's top-right corner when no side fits or `preferred` is
/// [`Side::Inside`].
pub fn place(
    target: Bounds<Pixels>,
    mark: Size<Pixels>,
    content: Bounds<Pixels>,
    preferred: Side,
) -> Placement {
    let (w, h) = (f32::from(mark.width), f32::from(mark.height));
    let t = Rect::of(target);
    let area = Rect::of(content).inset(MARGIN);
    let clamp = |value: f32, min: f32, max: f32| value.max(min).min(max.max(min));
    let along_x = clamp(t.cx() - w / 2.0, area.left, area.right - w);
    let along_y = clamp(t.cy() - h / 2.0, area.top, area.bottom - h);
    let candidate = |side: Side| -> Option<(f32, f32)> {
        let fits_w = w <= area.width();
        let fits_h = h <= area.height();
        match side {
            Side::Below => {
                let y = t.bottom + GAP;
                (fits_w && y + h <= area.bottom).then_some((along_x, y))
            }
            Side::Above => {
                let y = t.top - GAP - h;
                (fits_w && y >= area.top).then_some((along_x, y))
            }
            Side::Right => {
                let x = t.right + GAP;
                (fits_h && x + w <= area.right).then_some((x, along_y))
            }
            Side::Left => {
                let x = t.left - GAP - w;
                (fits_h && x >= area.left).then_some((x, along_y))
            }
            Side::Inside => None,
        }
    };
    let order = if preferred == Side::Inside {
        Vec::new()
    } else {
        let [a, b] = preferred.perpendicular();
        vec![preferred, preferred.opposite(), a, b]
    };
    let found = order
        .into_iter()
        .find_map(|side| candidate(side).map(|origin| (side, origin)));
    let (side, (x, y)) = found.unwrap_or_else(|| {
        let x = clamp(t.right - INSIDE_INSET - w, area.left, area.right - w);
        let y = clamp(t.top + INSIDE_INSET, area.top, area.bottom - h);
        (Side::Inside, (x, y))
    });
    let arrow = match side {
        Side::Above | Side::Below => Some(clamp(t.cx() - x, ARROW_CORNER, w - ARROW_CORNER)),
        Side::Left | Side::Right => Some(clamp(t.cy() - y, ARROW_CORNER, h - ARROW_CORNER)),
        Side::Inside => None,
    };
    Placement {
        bounds: Bounds::new(point(px(x), px(y)), mark),
        side,
        arrow,
    }
}

/// Plain f32 edges for the placement math.
#[derive(Clone, Copy)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    fn of(bounds: Bounds<Pixels>) -> Self {
        Self {
            left: f32::from(bounds.left()),
            top: f32::from(bounds.top()),
            right: f32::from(bounds.right()),
            bottom: f32::from(bounds.bottom()),
        }
    }

    fn inset(self, by: f32) -> Self {
        Self {
            left: self.left + by,
            top: self.top + by,
            right: self.right - by,
            bottom: self.bottom - by,
        }
    }

    fn width(self) -> f32 {
        self.right - self.left
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }

    fn cx(self) -> f32 {
        (self.left + self.right) / 2.0
    }

    fn cy(self) -> f32 {
        (self.top + self.bottom) / 2.0
    }
}

// ── Coach mark (§2.1) ───────────────────────────────────────────────────

/// One piece of a step's body.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    /// Prose; whitespace separates words (wrapping points).
    Text(SharedString),
    /// A keycap: its label (`⇧V`) and spoken form ("Shift V").
    Key {
        label: SharedString,
        spoken: SharedString,
    },
    /// Literal query text (`!`, `author:@me`), in `type.code_inline`.
    Code(SharedString),
}

impl Segment {
    fn spoken(&self) -> &str {
        match self {
            Segment::Text(text) | Segment::Code(text) => text,
            Segment::Key { spoken, .. } => spoken,
        }
    }
}

/// The body as a screen reader reads it (keys spelled out).
pub fn spoken(body: &[Segment]) -> String {
    body.iter().map(Segment::spoken).collect()
}

/// The coach mark's three buttons, in Tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoachButton {
    Back,
    Next,
    Skip,
}

impl CoachButton {
    /// `Tab` (`forward`) or `⇧Tab` from `self`, wrapping, skipping Back on
    /// the first step (disabled, not a Tab stop).
    pub fn cycle(self, forward: bool, first_step: bool) -> Self {
        const ORDER: [CoachButton; 3] = [CoachButton::Back, CoachButton::Next, CoachButton::Skip];
        let stops: Vec<CoachButton> = ORDER
            .into_iter()
            .filter(|button| !(first_step && *button == CoachButton::Back))
            .collect();
        let at = stops.iter().position(|b| *b == self).unwrap_or(0);
        let len = stops.len();
        let next = if forward {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        };
        stops[next]
    }
}

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
type PointerHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

/// The coach mark card: title, "n of N", body with inline keycaps, footer
/// with the shortcuts hint and Skip tour · Back · Next/Done. Keyboard focus
/// stays in the view (FR-6: non-tour keys reach it untouched); `focused`
/// is the button `↩`/`Space` activate, drawn with the focus ring.
#[derive(IntoElement)]
pub struct CoachMark {
    id: ElementId,
    tour: SharedString,
    title: SharedString,
    step: usize,
    steps: usize,
    body: Vec<Segment>,
    focused: CoachButton,
    shortcuts: Option<SharedString>,
    width: f32,
    entrance: Option<(ElementId, Rc<Cell<f32>>)>,
    exiting: bool,
    on_back: Option<Handler>,
    on_next: Option<Handler>,
    on_skip: Option<Handler>,
}

impl CoachMark {
    /// Step `step` (1-based) of `steps` of the tour named `tour`.
    pub fn new(
        id: impl Into<ElementId>,
        tour: impl Into<SharedString>,
        title: impl Into<SharedString>,
        step: usize,
        steps: usize,
    ) -> Self {
        Self {
            id: id.into(),
            tour: tour.into(),
            title: title.into(),
            step,
            steps,
            body: Vec::new(),
            focused: CoachButton::Next,
            shortcuts: None,
            width: size::COACH_MARK,
            entrance: None,
            exiting: false,
            on_back: None,
            on_next: None,
            on_skip: None,
        }
    }

    pub fn body(mut self, body: Vec<Segment>) -> Self {
        self.body = body;
        self
    }

    pub fn focused(mut self, button: CoachButton) -> Self {
        self.focused = button;
        self
    }

    /// The "All shortcuts" footer hint with this keycap label (`?`).
    pub fn shortcuts(mut self, label: impl Into<SharedString>) -> Self {
        self.shortcuts = Some(label.into());
        self
    }

    /// Width in px ([`width_for`]).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Run `menu-in` keyed by `key` (a new key per step); `progress`
    /// receives the eased progress so the [`layer`] can move the mark
    /// toward its anchor in step. gpui snaps it under reduce motion.
    pub fn entrance(mut self, key: impl Into<ElementId>, progress: Rc<Cell<f32>>) -> Self {
        self.entrance = Some((key.into(), progress));
        self
    }

    /// `menu-out`: fades out and takes no input (the tour already ended).
    pub fn exiting(mut self, exiting: bool) -> Self {
        self.exiting = exiting;
        self
    }

    pub fn on_back(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_back = Some(Rc::new(handler));
        self
    }

    pub fn on_next(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_next = Some(Rc::new(handler));
        self
    }

    pub fn on_skip(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_skip = Some(Rc::new(handler));
        self
    }

    /// "Pull request review tour, step 2 of 5: Files" (FR-18).
    pub fn label(&self) -> String {
        format!(
            "{} tour, step {} of {}: {}",
            self.tour, self.step, self.steps, self.title
        )
    }

    pub fn is_last(&self) -> bool {
        self.step >= self.steps
    }
}

/// The mark's width for a window: `size.coach_mark` × `font_scale`, at most
/// the window width − 16 (§9).
pub fn width_for(viewport_width: f32, rem_size: f32) -> f32 {
    (size::COACH_MARK * rem_size / 16.0).min(viewport_width - 2.0 * MARGIN)
}

/// Body pieces glued into words: a keycap followed by "," stays with it.
enum Piece {
    Word(SharedString),
    Key(SharedString),
    Code(SharedString),
}

fn push(words: &mut Vec<Vec<Piece>>, fresh: &mut bool, piece: Piece) {
    match words.last_mut() {
        Some(word) if !*fresh => word.push(piece),
        _ => words.push(vec![piece]),
    }
    *fresh = false;
}

fn words(body: &[Segment]) -> Vec<Vec<Piece>> {
    let mut words: Vec<Vec<Piece>> = Vec::new();
    // Whether the next piece starts a new word.
    let mut fresh = true;
    for segment in body {
        match segment {
            Segment::Text(text) => {
                let mut rest: &str = text;
                while !rest.is_empty() {
                    let gap = rest.len() - rest.trim_start().len();
                    if gap > 0 {
                        fresh = true;
                        rest = &rest[gap..];
                        continue;
                    }
                    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                    push(
                        &mut words,
                        &mut fresh,
                        Piece::Word(rest[..end].to_owned().into()),
                    );
                    rest = &rest[end..];
                }
            }
            Segment::Key { label, .. } => push(&mut words, &mut fresh, Piece::Key(label.clone())),
            Segment::Code(code) => push(&mut words, &mut fresh, Piece::Code(code.clone())),
        }
    }
    words
}

impl RenderOnce for CoachMark {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        let label = self.label();
        let description = spoken(&self.body);
        let last = self.is_last();
        let first = self.step <= 1;
        let interactive = !self.exiting;
        let body = words(&self.body).into_iter().map(|word| {
            div()
                .flex()
                .items_center()
                .children(word.into_iter().map(|piece| {
                    match piece {
                        Piece::Word(text) => div().child(text).into_any_element(),
                        Piece::Key(label) => Kbd::from_label(label).into_any_element(),
                        Piece::Code(code) => div()
                            .text_color(colors.text_primary)
                            .type_style(typography::CODE_INLINE, theme)
                            .child(code)
                            .into_any_element(),
                    }
                }))
        });
        let button = |which: CoachButton,
                      text: &'static str,
                      variant: ButtonVariant,
                      disabled: bool,
                      handler: Option<Handler>| {
            let palette = variant.palette(&theme.raw, theme.raw.bg_overlay, disabled);
            let focused = interactive && !disabled && self.focused == which;
            let hover = hsla(palette.background_hover);
            let id: &'static str = match which {
                CoachButton::Back => "coach-back",
                CoachButton::Next => "coach-next",
                CoachButton::Skip => "coach-skip",
            };
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(text)
                .flex()
                .flex_none()
                .items_center()
                .h(px(size::CONTROL_SM))
                .px(px(space::S8))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(if focused {
                    colors.border_focus
                } else {
                    hsla(palette.border)
                })
                .bg(hsla(palette.background))
                .text_color(match (which, disabled) {
                    (CoachButton::Skip, false) => colors.text_tertiary,
                    _ => hsla(palette.foreground),
                })
                .type_style(typography::SMALL, theme)
                .child(text)
                .when(interactive && !disabled, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| style.bg(hover))
                        .when_some(handler, |button, handler| {
                            button.on_click(move |_, window, cx| handler(window, cx))
                        })
                })
        };
        let footer = div()
            .mt(px(space::S12))
            .flex()
            .items_center()
            .gap(px(space::S4))
            .children(self.shortcuts.clone().map(|keys| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S4))
                    .text_color(colors.text_tertiary)
                    .type_style(typography::SMALL, theme)
                    .child("All shortcuts")
                    .child(Kbd::from_label(keys))
            }))
            .child(div().flex_1())
            .child(button(
                CoachButton::Skip,
                "Skip tour",
                ButtonVariant::Ghost,
                false,
                self.on_skip.clone(),
            ))
            .child(button(
                CoachButton::Back,
                "Back",
                ButtonVariant::Ghost,
                first,
                self.on_back.clone(),
            ))
            .child(button(
                CoachButton::Next,
                if last { "Done" } else { "Next" },
                ButtonVariant::Primary,
                false,
                self.on_next.clone(),
            ));
        let card = div()
            .id(self.id)
            .role(Role::Dialog)
            .aria_label(SharedString::from(label.clone()))
            .aria_description(SharedString::from(description))
            .w(px(self.width))
            .p(px(space::S16))
            .flex()
            .flex_col()
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_2))
            .when(interactive, |card| {
                card.occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(space::S8))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(colors.text_primary)
                            .type_style(typography::BODY_STRONG, theme)
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(colors.text_tertiary)
                            .type_style(typography::SMALL, theme)
                            .child(format!("{} of {}", self.step, self.steps)),
                    ),
            )
            .child(
                div()
                    .mt(px(space::S6))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_x(px(space::S4))
                    .text_color(colors.text_secondary)
                    .type_style(typography::BODY, theme)
                    .children(body),
            )
            .child(footer)
            // Announced on show (FR-18): a polite live region with the label.
            .child(
                div()
                    .id("coach-announcement")
                    .absolute()
                    .size_0()
                    .overflow_hidden()
                    .role(Role::Status)
                    .when(interactive, |d| d.aria_label(SharedString::from(label))),
            );
        match (self.exiting, self.entrance) {
            (true, _) => card
                .with_animation(
                    "coach-mark-out",
                    animation(motion::QUICK_EXIT, EASE_IN),
                    |card, t| card.opacity(1.0 - t),
                )
                .into_any_element(),
            (false, Some((key, progress))) => card
                .with_animation(key, animation(motion::QUICK, EASE_OUT), move |card, t| {
                    progress.set(t);
                    card.opacity(t)
                })
                .into_any_element(),
            (false, None) => card.into_any_element(),
        }
    }
}

// ── The layer ───────────────────────────────────────────────────────────

/// What the layer saw this frame: the committed anchors and where the
/// mark went (`None` when the current anchor isn't visible).
pub struct LayerFrame {
    pub anchors: FrameAnchors,
    pub placed: Option<Placement>,
}

type FrameReport = Box<dyn FnOnce(LayerFrame, &mut Window, &mut App)>;

/// The coach mark and spotlight over the window (`z.dialog`). Always
/// render it while recording: it commits each frame's anchors.
pub struct TourLayer {
    content: Bounds<Pixels>,
    anchor: Option<&'static str>,
    side: Side,
    mark: Option<AnyElement>,
    progress: Option<Rc<Cell<f32>>>,
    spotlight: Option<Hsla>,
    arrow: Option<(Hsla, Hsla)>,
    report: Option<FrameReport>,
    on_pointer: Option<PointerHandler>,
}

/// A layer that only commits anchors (nothing shows yet).
pub fn layer(content: Bounds<Pixels>) -> TourLayer {
    TourLayer {
        content,
        anchor: None,
        side: Side::Below,
        mark: None,
        progress: None,
        spotlight: None,
        arrow: None,
        report: None,
        on_pointer: None,
    }
}

impl TourLayer {
    /// Show `mark` beside anchor `id`, preferring `side`, with the
    /// spotlight in `outline` (`border.focus`, faded on exit) and the arrow
    /// in `(fill, edge)` (`bg.overlay`, `border.default`).
    pub fn show(
        mut self,
        id: &'static str,
        side: Side,
        mark: impl IntoElement,
        outline: Hsla,
        arrow: (Hsla, Hsla),
    ) -> Self {
        self.anchor = Some(id);
        self.side = side;
        self.mark = Some(mark.into_any_element());
        self.spotlight = Some(outline);
        self.arrow = Some(arrow);
        self
    }

    /// The entrance progress the mark's `menu-in` reports.
    pub fn progress(mut self, progress: Rc<Cell<f32>>) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Called once this frame's anchors are known and the mark is placed.
    pub fn on_frame(
        mut self,
        report: impl FnOnce(LayerFrame, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.report = Some(Box::new(report));
        self
    }

    /// Any pointer down or scroll in the window, with whether it landed
    /// outside the shown mark (FR-6: outside input ends the tour and is
    /// still delivered normally; it also counts as input for FR-2 item 6).
    pub fn on_pointer(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_pointer = Some(Rc::new(handler));
        self
    }

    /// Deferred at `z.dialog`, after every anchor in the frame.
    pub fn into_deferred(self) -> impl IntoElement {
        deferred(self).with_priority(usize::from(z::DIALOG))
    }
}

pub struct LayerPrepaint {
    spotlight: Bounds<Pixels>,
    placement: Placement,
}

impl IntoElement for TourLayer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TourLayer {
    type RequestLayoutState = ();
    type PrepaintState = Option<LayerPrepaint>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<LayerPrepaint> {
        let anchors = commit(window, cx);
        let shown = self
            .anchor
            .and_then(|id| visible(&anchors, id, self.content));
        let mut prepaint = None;
        if let (Some(shown), Some(mark)) = (shown, self.mark.as_mut()) {
            let target = spotlight(shown, self.content);
            let measured = mark.layout_as_root(AvailableSpace::min_size(), window, cx);
            let placement = place(target, measured, self.content, self.side);
            let travel = self
                .progress
                .as_ref()
                .map_or(0.0, |p| (1.0 - p.get().clamp(0.0, 1.0)) * ENTRANCE_TRAVEL);
            let (dx, dy) = placement.side.outward();
            let origin = placement.bounds.origin + point(px(dx * travel), px(dy * travel));
            mark.prepaint_at(origin, window, cx);
            prepaint = Some(LayerPrepaint {
                spotlight: target,
                placement: Placement {
                    bounds: Bounds::new(origin, placement.bounds.size),
                    ..placement
                },
            });
        }
        if let Some(report) = self.report.take() {
            report(
                LayerFrame {
                    anchors,
                    placed: prepaint.as_ref().map(|p| p.placement),
                },
                window,
                cx,
            );
        }
        prepaint
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut Option<LayerPrepaint>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mark_bounds = prepaint.as_ref().map(|p| p.placement.bounds);
        if let Some(on_pointer) = self.on_pointer.clone() {
            let outside = move |position: &Point<Pixels>| {
                !mark_bounds.is_some_and(|mark| mark.contains(position))
            };
            let on_scroll = on_pointer.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    on_pointer(outside(&event.position), window, cx);
                }
            });
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    on_scroll(outside(&event.position), window, cx);
                }
            });
        }
        let Some(prepaint) = prepaint.as_ref() else {
            return;
        };
        if let Some(color) = self.spotlight {
            window.paint_quad(quad(
                prepaint.spotlight,
                px(radius::MD),
                crate::theme::hsla(adoken_theme::Color::TRANSPARENT),
                px(OUTLINE_WIDTH),
                color,
                BorderStyle::Solid,
            ));
        }
        if let Some(mark) = self.mark.as_mut() {
            mark.paint(window, cx);
        }
        if let (Some((fill, edge)), Some(offset)) = (self.arrow, prepaint.placement.arrow) {
            paint_arrow(&prepaint.placement, offset, fill, edge, window);
        }
    }
}

/// The 12 × 6 arrow on the mark's edge facing the anchor: `fill` with an
/// `edge` stroke on its two outer sides.
fn paint_arrow(placement: &Placement, offset: f32, fill: Hsla, edge: Hsla, window: &mut Window) {
    let b = placement.bounds;
    let half = ARROW_WIDTH / 2.0;
    let (base_a, base_b, tip): (Point<Pixels>, Point<Pixels>, Point<Pixels>) = match placement.side
    {
        Side::Below => {
            let x = f32::from(b.left()) + offset;
            let y = f32::from(b.top()) + 1.0;
            (
                point(px(x - half), px(y)),
                point(px(x + half), px(y)),
                point(px(x), px(y - ARROW_DEPTH - 1.0)),
            )
        }
        Side::Above => {
            let x = f32::from(b.left()) + offset;
            let y = f32::from(b.bottom()) - 1.0;
            (
                point(px(x - half), px(y)),
                point(px(x + half), px(y)),
                point(px(x), px(y + ARROW_DEPTH + 1.0)),
            )
        }
        Side::Right => {
            let x = f32::from(b.left()) + 1.0;
            let y = f32::from(b.top()) + offset;
            (
                point(px(x), px(y - half)),
                point(px(x), px(y + half)),
                point(px(x - ARROW_DEPTH - 1.0), px(y)),
            )
        }
        Side::Left => {
            let x = f32::from(b.right()) - 1.0;
            let y = f32::from(b.top()) + offset;
            (
                point(px(x), px(y - half)),
                point(px(x), px(y + half)),
                point(px(x + ARROW_DEPTH + 1.0), px(y)),
            )
        }
        Side::Inside => return,
    };
    let mut body = PathBuilder::fill();
    body.move_to(base_a);
    body.line_to(tip);
    body.line_to(base_b);
    body.close();
    if let Ok(path) = body.build() {
        window.paint_path(path, fill);
    }
    let mut outline = PathBuilder::stroke(px(1.0));
    outline.move_to(base_a);
    outline.line_to(tip);
    outline.line_to(base_b);
    if let Ok(path) = outline.build() {
        window.paint_path(path, edge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size as gsize;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), gsize(px(w), px(h)))
    }

    const CONTENT: (f32, f32, f32, f32) = (0.0, 38.0, 1440.0, 798.0);

    fn content() -> Bounds<Pixels> {
        rect(CONTENT.0, CONTENT.1, CONTENT.2, CONTENT.3)
    }

    fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>, margin: f32) -> bool {
        inner.left() >= outer.left() + px(margin)
            && inner.top() >= outer.top() + px(margin)
            && inner.right() <= outer.right() - px(margin)
            && inner.bottom() <= outer.bottom() - px(margin)
    }

    #[test]
    fn visibility_needs_half_the_area_and_24_px_inside_clip_and_content() {
        let whole = rect(0.0, 0.0, 10_000.0, 10_000.0);
        let full = AnchorRect {
            bounds: rect(100.0, 100.0, 200.0, 100.0),
            clip: whole,
        };
        assert_eq!(full.visible_in(content()), Some(full.bounds));
        // Scrolled so only 40% shows in its viewport.
        let scrolled = AnchorRect {
            clip: rect(0.0, 160.0, 10_000.0, 10_000.0),
            ..full
        };
        assert_eq!(scrolled.visible_in(content()), None);
        // Exactly half shows: still visible.
        let half = AnchorRect {
            clip: rect(0.0, 150.0, 10_000.0, 10_000.0),
            ..full
        };
        assert!(half.visible_in(content()).is_some());
        // Under the titlebar (outside the content area).
        let under_titlebar = AnchorRect {
            bounds: rect(100.0, 0.0, 200.0, 60.0),
            clip: whole,
        };
        assert_eq!(under_titlebar.visible_in(content()), None);
        // A 20 px tall chip is never visible, whatever its share.
        let tiny = AnchorRect {
            bounds: rect(100.0, 100.0, 200.0, 20.0),
            clip: whole,
        };
        assert_eq!(tiny.visible_in(content()), None);
        // Collapsed (never laid out).
        let empty = AnchorRect {
            bounds: rect(100.0, 100.0, 0.0, 0.0),
            clip: whole,
        };
        assert_eq!(empty.visible_in(content()), None);
    }

    #[test]
    fn first_visible_record_wins() {
        let whole = rect(0.0, 0.0, 10_000.0, 10_000.0);
        let anchors = [
            (
                "review.thread",
                AnchorRect {
                    // Scrolled up under the titlebar: 20 of 80 px show.
                    bounds: rect(400.0, -22.0, 300.0, 80.0),
                    clip: whole,
                },
            ),
            (
                "review.thread",
                AnchorRect {
                    bounds: rect(400.0, 300.0, 300.0, 80.0),
                    clip: whole,
                },
            ),
        ];
        assert_eq!(
            visible(&anchors, "review.thread", content()),
            Some(rect(400.0, 300.0, 300.0, 80.0))
        );
        assert_eq!(visible(&anchors, "review.diff", content()), None);
    }

    #[test]
    fn placement_prefers_its_side_then_flips_and_never_overlaps() {
        let mark = gsize(px(320.0), px(140.0));
        // File tree on the left of a wide window: right of it.
        let tree = spotlight(rect(0.0, 100.0, 300.0, 600.0), content());
        let placed = place(tree, mark, content(), Side::Right);
        assert_eq!(placed.side, Side::Right);
        assert!(!placed.bounds.intersects(&tree));
        assert!(inside(placed.bounds, content(), MARGIN));
        assert_eq!(placed.bounds.left(), tree.right() + px(GAP));
        // Against the right edge: flips left.
        let right_pane = rect(1200.0, 100.0, 240.0, 300.0);
        let placed = place(right_pane, mark, content(), Side::Right);
        assert_eq!(placed.side, Side::Left);
        assert_eq!(placed.bounds.right(), right_pane.left() - px(GAP));
        // A header at the very top asked to go above: below instead.
        let header = rect(100.0, 40.0, 600.0, 40.0);
        let placed = place(header, mark, content(), Side::Above);
        assert_eq!(placed.side, Side::Below);
        assert!(inside(placed.bounds, content(), MARGIN));
    }

    #[test]
    fn placement_centers_then_slides_to_fit() {
        let mark = gsize(px(320.0), px(140.0));
        let button = rect(1360.0, 50.0, 70.0, 28.0);
        let placed = place(button, mark, content(), Side::Below);
        assert_eq!(placed.side, Side::Below);
        // Slid left so its right edge keeps the 8 px margin.
        assert_eq!(placed.bounds.right(), content().right() - px(MARGIN));
        // The arrow still points at the button's center, clamped off the corner.
        let arrow = placed.arrow.expect("an arrow beside the anchor");
        assert!((ARROW_CORNER..=320.0 - ARROW_CORNER).contains(&arrow));
        let tip_x = f32::from(placed.bounds.left()) + arrow;
        assert!((tip_x - (1360.0 + 35.0)).abs() <= 16.0);
        let centered = place(
            rect(500.0, 300.0, 100.0, 40.0),
            mark,
            content(),
            Side::Below,
        );
        assert_eq!(
            f32::from(centered.bounds.center().x),
            550.0,
            "centered on the anchor"
        );
    }

    #[test]
    fn pane_sized_anchors_take_the_inside_corner_without_an_arrow() {
        let mark = gsize(px(320.0), px(140.0));
        let diff = spotlight(rect(300.0, 80.0, 1140.0, 700.0), content());
        let placed = place(diff, mark, content(), Side::Right);
        assert_eq!(placed.side, Side::Inside);
        assert_eq!(placed.arrow, None);
        assert_eq!(
            placed.bounds.right(),
            diff.right() - px(INSIDE_INSET),
            "top-right corner, inset 16"
        );
        assert_eq!(placed.bounds.top(), diff.top() + px(INSIDE_INSET));
        // `Inside` as the preferred side goes straight there.
        let list = rect(300.0, 100.0, 500.0, 300.0);
        assert_eq!(
            place(list, mark, content(), Side::Inside).side,
            Side::Inside
        );
    }

    #[test]
    fn narrow_window_still_places_inside_the_content_area() {
        // 900 × 600 at font_scale 1.3: the mark is 416 wide.
        let content = rect(0.0, 38.0, 900.0, 538.0);
        let mark = gsize(px(width_for(900.0, 16.0 * 1.3)), px(190.0));
        let tree = spotlight(rect(0.0, 90.0, 260.0, 480.0), content);
        let placed = place(tree, mark, content, Side::Right);
        assert_eq!(placed.side, Side::Right);
        assert!(inside(placed.bounds, content, MARGIN));
        assert!(!placed.bounds.intersects(&tree));
    }

    #[test]
    fn spotlight_sits_outside_the_anchor_but_inside_the_content_area() {
        let anchor = rect(200.0, 200.0, 100.0, 50.0);
        assert_eq!(
            spotlight(anchor, content()),
            rect(196.0, 196.0, 108.0, 58.0)
        );
        let pane = rect(0.0, 38.0, 1440.0, 798.0);
        assert_eq!(
            spotlight(pane, content()),
            content().inset(px(OUTLINE_OFFSET))
        );
    }

    #[test]
    fn tab_cycles_the_buttons_and_skips_a_disabled_back() {
        use CoachButton::*;
        assert_eq!(Back.cycle(true, false), Next);
        assert_eq!(Next.cycle(true, false), Skip);
        assert_eq!(Skip.cycle(true, false), Back);
        assert_eq!(Back.cycle(false, false), Skip);
        assert_eq!(Next.cycle(true, true), Skip);
        assert_eq!(Skip.cycle(true, true), Next);
        assert_eq!(Next.cycle(false, true), Skip);
    }

    #[test]
    fn keycaps_glue_to_the_punctuation_after_them() {
        let body = vec![
            Segment::Key {
                label: "⇧V".into(),
                spoken: "Shift V".into(),
            },
            Segment::Text(" votes, ".into()),
            Segment::Key {
                label: "⇧Y".into(),
                spoken: "Shift Y".into(),
            },
            Segment::Text(", then".into()),
        ];
        let shape: Vec<usize> = words(&body).iter().map(Vec::len).collect();
        // [⇧V] · votes, · [⇧Y], · then
        assert_eq!(shape, vec![1, 1, 2, 1]);
        assert_eq!(spoken(&body), "Shift V votes, Shift Y, then");
    }

    #[test]
    fn width_scales_with_font_scale_and_fits_the_window() {
        assert_eq!(width_for(1440.0, 16.0), 320.0);
        assert_eq!(width_for(1440.0, 16.0 * 1.3), 416.0);
        assert_eq!(width_for(300.0, 16.0), 284.0);
    }
}
