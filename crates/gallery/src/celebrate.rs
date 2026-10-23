//! Celebration stories (spec 0022 FR-18, ken.md §12.7): each effect at
//! t = 0, 200 and 600 ms from a seeded generator, through the same anchor
//! → resolve → step → paint path the overlay runs; the Reduce Motion glyph
//! and Milestone toast; Settings › General › Celebrations.

use std::cell::RefCell;
use std::rc::Rc;

use adoken_app::celebrate::{Anchor, CelebrationToast, Emission, Fx, FxColors, paint, sparkles};
use adoken_app::settings::{SettingsView, Tab};
use adoken_ken::celebrate::{Style, Tier};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::{ActiveTheme, ChipTone, TypeStyleExt};
use gpui::{AnyElement, App, IntoElement, Window, canvas, div, prelude::*, px};

/// Fixed seed: snapshots never drift.
const SEED: u64 = 0x0022;
const TIMES: [f32; 3] = [0.0, 200.0, 600.0];
const STAGE: (f32, f32) = (300.0, 240.0);
const FRAME_MS: f32 = 16.0;

fn caption(text: String, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .text_color(theme.colors.text_tertiary)
        .type_style(typography::LABEL, theme)
        .child(text)
}

/// A state chip like the work item header's (or the review's status
/// chip): tone colors, label, the anchor tracker and the optional glyph.
fn chip(
    label: &'static str,
    tone: ChipTone,
    fx: Option<Rc<RefCell<Fx>>>,
    glyph: bool,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let (fg, bg) = tone.colors(&theme.colors);
    div()
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(space::S4))
        .h(px(size::CONTROL_SM))
        .px(px(space::S8))
        .rounded(px(radius::XS))
        .bg(bg)
        .text_color(fg)
        .type_style(typography::LABEL, theme)
        .children(fx.map(|fx| {
            canvas(
                move |bounds, _, _| fx.borrow_mut().record(Anchor::WorkItem(1), bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        }))
        .child(label)
        .when(glyph, |chip| chip.child(sparkles(cx)))
}

fn stage(cx: &App) -> gpui::Div {
    let colors = &cx.theme().colors;
    div()
        .relative()
        .overflow_hidden()
        .w(px(STAGE.0))
        .h(px(STAGE.1))
        .rounded(px(radius::MD))
        .border_1()
        .border_color(colors.border_subtle)
        .bg(colors.bg_surface)
}

/// One frame of an effect at `t` ms: the chip records its bounds, the
/// overlay (last child) resolves, steps to `t` and paints.
fn frame(
    t: f32,
    tier: Tier,
    style: Style,
    label: &'static str,
    tone: ChipTone,
    title: Option<&'static str>,
    cx: &App,
) -> AnyElement {
    let fx = Rc::new(RefCell::new(Fx::new(SEED)));
    fx.borrow_mut().queue(Emission {
        anchor: Anchor::WorkItem(1),
        tier,
        style,
        fallback: None,
    });
    let colors = FxColors::of(cx.theme());
    let (run, draw) = (fx.clone(), fx.clone());
    let theme = cx.theme();
    // The title sits above the chip so the chip, and the burst from it,
    // stays centered in the stage.
    let row = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(px(48.0))
        .px(px(space::S16))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(space::S8))
        .children(title.map(|title| {
            div()
                .text_color(theme.colors.text_primary)
                .type_style(typography::BODY_STRONG, theme)
                .child(title)
        }))
        .child(chip(label, tone, Some(fx), false, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(space::S6))
        .child(caption(format!("t = {t:.0} ms"), cx))
        .child(
            stage(cx).child(row).child(
                canvas(
                    move |_, window, _| {
                        let viewport = window.viewport_size();
                        let (w, h) = (f32::from(viewport.width), f32::from(viewport.height));
                        let mut fx = run.borrow_mut();
                        fx.resolve(w, h);
                        let mut at = 0.0;
                        while at < t {
                            let dt = FRAME_MS.min(t - at);
                            fx.step(dt, w, h);
                            at += dt;
                        }
                    },
                    move |_, _, window, _| paint(&draw.borrow(), &colors, window),
                )
                .absolute()
                .inset_0()
                .size_full(),
            ),
        )
        .into_any_element()
}

fn timeline(
    tier: Tier,
    style: Style,
    label: &'static str,
    tone: ChipTone,
    title: Option<&'static str>,
    cx: &App,
) -> AnyElement {
    div()
        .flex()
        .gap(px(space::S16))
        .children(TIMES.map(|t| frame(t, tier, style, label, tone, title, cx)))
        .into_any_element()
}

/// Step, calm: a confetti puff from a work item's "Code Review" chip.
pub fn step_story(_: &mut Window, cx: &mut App) -> AnyElement {
    timeline(
        Tier::Step,
        Style::Calm,
        "Code Review",
        ChipTone::Accent,
        None,
        cx,
    )
}

/// Milestone, calm: the two-wave burst and chip glow from "Done".
pub fn milestone_story(_: &mut Window, cx: &mut App) -> AnyElement {
    timeline(
        Tier::Milestone,
        Style::Calm,
        "Done",
        ChipTone::Success,
        None,
        cx,
    )
}

/// A PR I completed, confirmed merged: the burst from the review header's
/// status chip.
pub fn pr_story(_: &mut Window, cx: &mut App) -> AnyElement {
    timeline(
        Tier::Milestone,
        Style::Calm,
        "Completed",
        ChipTone::Success,
        Some("!4821 Retry idempotency keys"),
        cx,
    )
}

/// Milestone, arcade (Ken on at arcade intensity): the 済 seal stamp,
/// its 32 sparks at 120 ms, and the chip glow.
pub fn arcade_milestone_story(_: &mut Window, cx: &mut App) -> AnyElement {
    timeline(
        Tier::Milestone,
        Style::Arcade,
        "Done",
        ChipTone::Success,
        None,
        cx,
    )
}

/// Reduce Motion (and rate-limited) fallback: the `sparkles` glyph beside
/// the new state for 3 s, a Milestone's toast, a Step's glyph only, and
/// the chip once the glyph is gone.
pub fn reduced_story(_: &mut Window, cx: &mut App) -> AnyElement {
    let column = |label: String, body: AnyElement| {
        div()
            .flex()
            .flex_col()
            .gap(px(space::S6))
            .child(caption(label, cx))
            .child(
                stage(cx)
                    .h(px(120.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(body),
            )
    };
    div()
        .flex()
        .flex_col()
        .gap(px(space::S16))
        .child(
            div()
                .flex()
                .gap(px(space::S16))
                .child(column(
                    "Milestone, t = 0 ms".into(),
                    chip("Done", ChipTone::Success, None, true, cx).into_any_element(),
                ))
                .child(column(
                    "Step, t = 0 ms".into(),
                    chip("Code Review", ChipTone::Accent, None, true, cx).into_any_element(),
                ))
                .child(column(
                    "t = 3 s".into(),
                    chip("Done", ChipTone::Success, None, false, cx).into_any_element(),
                )),
        )
        .child(CelebrationToast {
            id: 0,
            title: "Moved to Done".into(),
            body: "#4821 Fix timezone bug in daily settlement job".into(),
            stripe: false,
        })
        .into_any_element()
}

/// `[celebrations]` with the master on and work item celebrations off.
const SETTINGS: &str = r#"
[celebrations]
enabled = true
pull_requests = true
work_items = false
step_states = ["Code Review", "In Review", "Ready for Review", "Review"]
milestone_states = ["Committed", "Done"]
"#;

/// Settings › General with the Celebrations section.
pub fn settings_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let view = window.use_keyed_state("settings-celebrations", cx, |window, cx| {
        SettingsView::story(Tab::General, SETTINGS, true, window, cx)
    });
    div().size_full().child(view).into_any_element()
}
