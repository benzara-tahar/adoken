//! App-level toast stack (docs/design/principles.md §4, pins.md §6.3):
//! bottom-right, at most three visible, 360 wide on `bg.overlay` with
//! `elevation.2` and `radius.md`, a leading 16 px glyph, an optional action
//! button with its keycap, and a polite live region (`Role::Status`). Each
//! toast lives `motion.toast_life` (5 s) unless the caller sets its own
//! life; hovering the stack pauses every timer. Showing a toast whose key is
//! already visible restarts that toast instead of stacking a second one.
//!
//! The window's shell hosts one [`ToastStack`] (installed as a global) and
//! paints it last before the palette; features push into it with
//! [`ToastStack::show`]. [`Queue`] is the pure timing model.

use std::rc::Rc;
use std::time::{Duration, Instant};

use adoken_theme::{elevation, motion, radius, size, space, typography};
use gpui::{
    AnimationExt, App, Context, Entity, EventEmitter, Global, Hsla, IntoElement, Render,
    SharedString, Task, Window, div, prelude::*, px,
};

use crate::button::{Button, ButtonVariant};
use crate::icon::{Icon, IconName};
use crate::kbd::Kbd;
use crate::motion::{EASE_OUT, animation, lerp, ms};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};

/// At most this many toasts are visible; a fourth dismisses the oldest.
pub const MAX_VISIBLE: usize = 3;

/// The leading glyph's tint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastTone {
    /// `text.tertiary`: confirmations and hints.
    #[default]
    Neutral,
    /// `status.warning.fg`: a refused action.
    Warning,
}

impl ToastTone {
    fn color(self, theme: &Theme) -> Hsla {
        match self {
            ToastTone::Neutral => theme.colors.text_tertiary,
            ToastTone::Warning => theme.colors.status_warning_fg,
        }
    }
}

type PressHandler = Rc<dyn Fn(&mut Window, &mut App) + 'static>;

/// The toast's one button ("Undo ⌘Z").
#[derive(Clone)]
pub struct ToastAction {
    pub label: SharedString,
    /// The binding that does the same, shown as a keycap.
    pub keys: Option<&'static str>,
    /// Spoken form of the binding for the announcement ("Command Z").
    pub spoken_keys: Option<SharedString>,
    pub on_press: PressHandler,
    dismiss_on_press: bool,
}

impl ToastAction {
    pub fn new(
        label: impl Into<SharedString>,
        on_press: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            keys: None,
            spoken_keys: None,
            on_press: Rc::new(on_press),
            dismiss_on_press: true,
        }
    }

    pub fn keys(mut self, keys: &'static str, spoken: impl Into<SharedString>) -> Self {
        self.keys = Some(keys);
        self.spoken_keys = Some(spoken.into());
        self
    }

    /// The handler decides whether to dismiss (for actions that can be refused).
    pub fn keep_open(mut self) -> Self {
        self.dismiss_on_press = false;
        self
    }
}

#[derive(Clone)]
pub struct Toast {
    /// Identity: a toast with the same key restarts instead of stacking.
    pub key: SharedString,
    pub icon: IconName,
    pub tone: ToastTone,
    pub message: SharedString,
    pub action: Option<ToastAction>,
    pub life: Duration,
}

impl Toast {
    pub fn new(
        key: impl Into<SharedString>,
        icon: IconName,
        message: impl Into<SharedString>,
    ) -> Self {
        Self {
            key: key.into(),
            icon,
            tone: ToastTone::Neutral,
            message: message.into(),
            action: None,
            life: ms(motion::TOAST_LIFE),
        }
    }

    pub fn tone(mut self, tone: ToastTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn action(mut self, action: ToastAction) -> Self {
        self.action = Some(action);
        self
    }

    pub fn life(mut self, life: Duration) -> Self {
        self.life = life;
        self
    }

    /// The polite announcement: "Unpinned !4821, Undo, Command Z".
    pub fn spoken(&self) -> String {
        let mut parts = vec![self.message.to_string()];
        if let Some(action) = &self.action {
            parts.push(action.label.to_string());
            parts.extend(action.spoken_keys.as_ref().map(|k| k.to_string()));
        }
        parts.join(", ")
    }
}

/// A toast left the stack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastEvent {
    Dismissed(SharedString),
}

/// One visible toast and when it expires.
#[derive(Clone)]
pub struct Shown<T> {
    pub key: SharedString,
    pub item: T,
    pub deadline: Instant,
    /// Bumped on every (re)start: keys the entrance animation.
    pub generation: u64,
}

/// The stack's timing model: order, cap, restart on repeat, pause.
pub struct Queue<T> {
    shown: Vec<Shown<T>>,
    paused_at: Option<Instant>,
    generation: u64,
}

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self {
            shown: Vec::new(),
            paused_at: None,
            generation: 0,
        }
    }
}

impl<T> Queue<T> {
    /// Oldest first.
    pub fn shown(&self) -> &[Shown<T>] {
        &self.shown
    }

    /// Shows `item`; a visible toast with the same key restarts in place
    /// (newest content, full life). Returns the keys pushed out by the cap.
    pub fn push(
        &mut self,
        key: SharedString,
        item: T,
        life: Duration,
        now: Instant,
    ) -> Vec<SharedString> {
        self.generation += 1;
        let deadline = self.paused_at.unwrap_or(now) + life;
        if let Some(shown) = self.shown.iter_mut().find(|s| s.key == key) {
            shown.item = item;
            shown.deadline = deadline;
            shown.generation = self.generation;
            return Vec::new();
        }
        self.shown.push(Shown {
            key,
            item,
            deadline,
            generation: self.generation,
        });
        let overflow = self.shown.len().saturating_sub(MAX_VISIBLE);
        self.shown.drain(..overflow).map(|s| s.key).collect()
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.shown.len();
        self.shown.retain(|s| s.key != key);
        self.shown.len() != before
    }

    /// Removes and returns the toasts whose time is up (none while paused).
    pub fn expire(&mut self, now: Instant) -> Vec<SharedString> {
        if self.paused_at.is_some() {
            return Vec::new();
        }
        let mut gone = Vec::new();
        self.shown.retain(|s| {
            let keep = s.deadline > now;
            if !keep {
                gone.push(s.key.clone());
            }
            keep
        });
        gone
    }

    /// Hover (or focus) holds every timer.
    pub fn pause(&mut self, now: Instant) {
        self.paused_at.get_or_insert(now);
    }

    /// Timers resume with the time they had left.
    pub fn resume(&mut self, now: Instant) {
        if let Some(at) = self.paused_at.take() {
            let held = now.saturating_duration_since(at);
            for shown in &mut self.shown {
                shown.deadline += held;
            }
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// When the next toast expires; `None` while paused or empty.
    pub fn next_deadline(&self) -> Option<Instant> {
        if self.paused_at.is_some() {
            return None;
        }
        self.shown.iter().map(|s| s.deadline).min()
    }
}

struct Installed(Entity<ToastStack>);

impl Global for Installed {}

#[derive(Default)]
struct Holds {
    hovered: bool,
    focused: bool,
}

impl Holds {
    fn apply<T>(&self, queue: &mut Queue<T>, now: Instant) {
        if self.hovered || self.focused {
            queue.pause(now);
        } else {
            queue.resume(now);
        }
    }
}

/// The window's toast stack.
pub struct ToastStack {
    queue: Queue<Toast>,
    timer: Option<Task<()>>,
    holds: Holds,
    focus: Option<gpui::FocusHandle>,
    focus_events: Vec<gpui::Subscription>,
    /// Space kept free under the stack (the statusbar).
    bottom_inset: f32,
}

impl EventEmitter<ToastEvent> for ToastStack {}

impl ToastStack {
    pub fn new(bottom_inset: f32) -> Self {
        Self {
            queue: Queue::default(),
            timer: None,
            holds: Holds::default(),
            focus: None,
            focus_events: Vec::new(),
            bottom_inset,
        }
    }

    /// Makes `stack` the one [`ToastStack::show`] pushes into.
    pub fn install(stack: Entity<ToastStack>, cx: &mut App) {
        cx.set_global(Installed(stack));
    }

    pub fn global(cx: &App) -> Option<Entity<ToastStack>> {
        cx.try_global::<Installed>().map(|g| g.0.clone())
    }

    /// Shows `toast` on the installed stack (no-op without one: stories).
    pub fn show(toast: Toast, cx: &mut App) {
        if let Some(stack) = Self::global(cx) {
            stack.update(cx, |stack, cx| stack.push(toast, cx));
        }
    }

    /// Dismisses the toast with `key` on the installed stack.
    pub fn dismiss_global(key: &str, cx: &mut App) {
        if let Some(stack) = Self::global(cx) {
            stack.update(cx, |stack, cx| stack.dismiss(key, cx));
        }
    }

    pub fn push(&mut self, toast: Toast, cx: &mut Context<Self>) {
        let key = toast.key.clone();
        let life = toast.life;
        for gone in self.queue.push(key, toast, life, Instant::now()) {
            cx.emit(ToastEvent::Dismissed(gone));
        }
        self.schedule(cx);
        cx.notify();
    }

    pub fn dismiss(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.queue.remove(key) {
            cx.emit(ToastEvent::Dismissed(key.to_owned().into()));
            self.schedule(cx);
            cx.notify();
        }
    }

    pub fn is_visible(&self, key: &str) -> bool {
        self.queue.shown().iter().any(|s| s.key == key)
    }

    /// The toast region's focus handle while toasts show (`F6` cycles to
    /// it, principles §4); `None` with nothing to focus.
    pub fn region_focus(&self) -> Option<gpui::FocusHandle> {
        if self.queue.shown().is_empty() {
            None
        } else {
            self.focus.clone()
        }
    }

    pub fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        self.holds.hovered = hovered;
        self.holds.apply(&mut self.queue, Instant::now());
        self.schedule(cx);
    }

    fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        self.holds.focused = focused;
        self.holds.apply(&mut self.queue, Instant::now());
        self.schedule(cx);
    }

    fn install_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_some() {
            return;
        }
        let focus = cx.focus_handle();
        self.focus_events
            .push(cx.on_focus_in(&focus, window, |stack, _, cx| stack.set_focused(true, cx)));
        self.focus_events
            .push(cx.on_focus_out(&focus, window, |stack, _, _, cx| {
                stack.set_focused(false, cx)
            }));
        self.focus = Some(focus);
    }

    fn schedule(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.queue.next_deadline() else {
            self.timer = None;
            return;
        };
        self.timer = Some(cx.spawn(async move |this, cx| {
            let wait = deadline.saturating_duration_since(Instant::now());
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                for gone in this.queue.expire(Instant::now()) {
                    cx.emit(ToastEvent::Dismissed(gone));
                }
                this.schedule(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    fn render_toast(
        shown: &Shown<Toast>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let colors = &theme.colors;
        let toast = &shown.item;
        let key = toast.key.clone();
        let action = toast.action.clone().map(|action| {
            let press = action.on_press.clone();
            let dismiss = action.dismiss_on_press;
            let key = key.clone();
            let stack = cx.entity().downgrade();
            let button = Button::new(("toast-action", shown.generation as usize), action.label)
                .variant(ButtonVariant::Ghost)
                .on_press(move |window, cx| {
                    press(window, cx);
                    if dismiss {
                        stack.update(cx, |stack, cx| stack.dismiss(&key, cx)).ok();
                    }
                });
            match action.keys.and_then(|keys| Kbd::parse(keys).ok()) {
                Some(kbd) => button.shortcut(kbd),
                None => button,
            }
        });
        div()
            .id(("toast", shown.generation as usize))
            .role(gpui::Role::Status)
            .aria_label(toast.spoken())
            .w(px(size::TOAST))
            .min_h(px(size::CONTROL_LG))
            .px(px(space::S12))
            .py(px(space::S8))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(elevation::LEVEL_2))
            .child(
                Icon::new(toast.icon)
                    .size(size::ICON_MD)
                    .color(toast.tone.color(theme)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(colors.text_primary)
                    .type_style(typography::BODY, theme)
                    .child(toast.message.clone()),
            )
            .children(action)
            .with_animation(
                ("toast-in", shown.generation as usize),
                animation(motion::BASE, EASE_OUT),
                |toast, t| toast.opacity(t).mt(px(lerp(space::S8, 0.0, t))),
            )
    }
}

impl Render for ToastStack {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.install_focus(window, cx);
        if self.queue.shown().is_empty() {
            self.holds = Holds::default();
            self.queue.resume(Instant::now());
            return div().into_any_element();
        }
        let theme = cx.theme().clone();
        let toasts: Vec<_> = self
            .queue
            .shown()
            .iter()
            .map(|shown| Self::render_toast(shown, &theme, cx).into_any_element())
            .collect();
        div()
            .id("toast-stack")
            .track_focus(self.focus.as_ref().expect("installed toast focus"))
            .absolute()
            .right(px(space::S16))
            .bottom(px(self.bottom_inset + space::S12))
            .flex()
            .flex_col()
            .items_end()
            .gap(px(space::S8))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(*hovered, cx)))
            .children(toasts)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> SharedString {
        s.to_owned().into()
    }

    const LIFE: Duration = Duration::from_secs(5);

    #[test]
    fn keeps_three_and_drops_the_oldest() {
        let now = Instant::now();
        let mut q = Queue::default();
        for k in ["a", "b", "c"] {
            assert!(q.push(key(k), (), LIFE, now).is_empty());
        }
        assert_eq!(q.push(key("d"), (), LIFE, now), vec![key("a")]);
        let keys: Vec<_> = q.shown().iter().map(|s| s.key.as_ref()).collect();
        assert_eq!(keys, ["b", "c", "d"]);
    }

    #[test]
    fn a_repeat_restarts_the_visible_toast_instead_of_stacking() {
        let now = Instant::now();
        let mut q = Queue::default();
        q.push(key("limit"), 1, LIFE, now);
        q.push(key("other"), 2, LIFE, now);
        let later = now + Duration::from_secs(4);
        assert!(q.push(key("limit"), 3, LIFE, later).is_empty());
        assert_eq!(q.shown().len(), 2);
        let limit = &q.shown()[0];
        assert_eq!((limit.item, limit.deadline), (3, later + LIFE));
        // The other one still expires on its own clock.
        assert_eq!(q.expire(now + LIFE), vec![key("other")]);
        assert_eq!(q.shown().len(), 1);
    }

    #[test]
    fn hover_holds_every_timer_and_resumes_with_the_time_left() {
        let now = Instant::now();
        let mut q = Queue::default();
        q.push(key("a"), (), LIFE, now);
        q.pause(now + Duration::from_secs(1));
        assert_eq!(q.next_deadline(), None);
        assert!(q.expire(now + Duration::from_secs(60)).is_empty());
        q.resume(now + Duration::from_secs(11));
        // Paused for 10 s: four seconds were left.
        assert_eq!(q.next_deadline(), Some(now + Duration::from_secs(15)));
        assert!(q.expire(now + Duration::from_secs(14)).is_empty());
        assert_eq!(q.expire(now + Duration::from_secs(15)), vec![key("a")]);
    }

    #[test]
    fn focus_and_hover_independently_hold_timer_until_both_leave() {
        let now = Instant::now();
        let mut queue = Queue::default();
        queue.push(key("undo"), (), LIFE, now);
        let mut holds = Holds {
            focused: true,
            hovered: false,
        };
        holds.apply(&mut queue, now + Duration::from_secs(1));
        assert!(queue.expire(now + Duration::from_secs(60)).is_empty());
        holds.hovered = true;
        holds.apply(&mut queue, now + Duration::from_secs(61));
        holds.focused = false;
        holds.apply(&mut queue, now + Duration::from_secs(62));
        assert!(queue.expire(now + Duration::from_secs(100)).is_empty());
        holds.hovered = false;
        holds.apply(&mut queue, now + Duration::from_secs(101));
        assert!(queue.expire(now + Duration::from_secs(104)).is_empty());
        assert_eq!(
            queue.expire(now + Duration::from_secs(105)),
            vec![key("undo")]
        );
    }
}
