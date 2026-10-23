//! The K.O. overlay (ken.md §4.2): a pure timeline (`t` ms since trigger →
//! every element's state) and a pointer-transparent, focus-free element
//! that paints one frame of it. The layer re-renders it each animation
//! frame for 1.72 s; stories paint fixed frames.

use adoken_theme::{radius, space, typography};
use adoken_ui::motion::{EASE_IN, EASE_OUT, SPRING_BOUNCY};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{App, FontWeight, IntoElement, RenderOnce, Window, div, prelude::*, px};

use super::glyphs::{ArcadeText, GlyphColors};

/// Letters: "K", ".", "O", "." (staggered 90 ms each).
pub const LETTERS: [&str; 4] = ["K", ".", "O", "."];
const LETTER_STAGGER: f32 = 90.0;
const LETTER_HEIGHT: f32 = 160.0;
const VIGNETTE_MS: f32 = 140.0;
const VIGNETTE_MAX: f32 = 0.35;
const IMPACT_AT: f32 = 180.0;
const FLASH_MS: f32 = 120.0;
/// Impact flash peaks: the spec's white quad is `text.primary` (near white)
/// on dark themes; light themes flash `ken.outline` at a lower peak.
const FLASH_PEAK_DARK: f32 = 0.14;
const FLASH_PEAK_LIGHT: f32 = 0.08;
const SHAKE_MS: f32 = 280.0;
const SHAKE_AMPLITUDE: f32 = 6.0;
const SHAKE_OSCILLATIONS: f32 = 4.0;
const CAPTION_AT: f32 = 320.0;
const CAPTION_FADE: f32 = 200.0;
pub const EXIT_AT: f32 = 1_400.0;
const EXIT_MS: f32 = 320.0;
/// Dismissed by input: exit this fast.
pub const DISMISS_MS: f32 = 90.0;
/// Whole timeline.
pub const DURATION_MS: f32 = EXIT_AT + EXIT_MS;
/// Sparks burst at the impact.
pub const IMPACT_SPARKS: usize = 24;

/// One letter's state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letter {
    pub scale: f32,
    pub rotate_deg: f32,
    pub opacity: f32,
    pub translate_y: f32,
}

/// Every element at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub vignette: f32,
    pub letters: [Letter; 4],
    /// Impact flash opacity (fraction of the peak, 0..1).
    pub flash: f32,
    /// Content offset (px) for the screen shake.
    pub shake: (f32, f32),
    pub caption: f32,
    /// Everything's opacity during the exit.
    pub exit: f32,
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// The frame at `t` ms. `dismissed_at`: input arrived then; the exit runs
/// over [`DISMISS_MS`] from there instead of waiting for [`EXIT_AT`].
pub fn frame(t: f32, dismissed_at: Option<f32>) -> Frame {
    let (exit_start, exit_len) = match dismissed_at {
        Some(d) if d < EXIT_AT => (d, DISMISS_MS),
        _ => (EXIT_AT, EXIT_MS),
    };
    let exit_p = clamp01((t - exit_start) / exit_len);
    let exit = 1.0 - EASE_IN.eval(exit_p);
    let letters = std::array::from_fn(|i| {
        let local = (t - i as f32 * LETTER_STAGGER).max(0.0) / 1000.0;
        let started = t >= i as f32 * LETTER_STAGGER;
        let s = if started {
            SPRING_BOUNCY.position(local.min(SPRING_BOUNCY.duration().as_secs_f32()))
        } else {
            0.0
        };
        Letter {
            scale: 1.8 + (1.0 - 1.8) * s,
            rotate_deg: -8.0 * (1.0 - s),
            opacity: if started { clamp01(s * 1.5) } else { 0.0 },
            translate_y: -12.0 * EASE_IN.eval(exit_p),
        }
    });
    let impact = t - IMPACT_AT;
    let flash = if (0.0..FLASH_MS).contains(&impact) {
        let p = impact / FLASH_MS;
        if p < 0.5 { p * 2.0 } else { (1.0 - p) * 2.0 }
    } else {
        0.0
    };
    let shake = if (0.0..SHAKE_MS).contains(&impact) {
        let u = impact / SHAKE_MS;
        let amplitude = SHAKE_AMPLITUDE * (-4.0 * u).exp();
        let wave = (std::f32::consts::TAU * SHAKE_OSCILLATIONS * u).sin();
        (amplitude * wave, amplitude * wave * 0.5)
    } else {
        (0.0, 0.0)
    };
    Frame {
        vignette: VIGNETTE_MAX * EASE_OUT.eval(clamp01(t / VIGNETTE_MS)),
        letters,
        flash,
        shake,
        caption: EASE_OUT.eval(clamp01((t - CAPTION_AT) / CAPTION_FADE)),
        exit,
    }
}

/// Whether the timeline is over.
pub fn finished(t: f32, dismissed_at: Option<f32>) -> bool {
    match dismissed_at {
        Some(d) if d < EXIT_AT => t >= d + DISMISS_MS,
        _ => t >= DURATION_MS,
    }
}

/// Toast copy when K.O. can't be an overlay (subtle, reduce motion, quiet
/// hours): "K.O. · !4821 merged into main".
pub fn toast_text(prs: &[(i64, String)], double: bool) -> String {
    let ids = prs
        .iter()
        .map(|(id, _)| format!("!{id}"))
        .collect::<Vec<_>>()
        .join(", ");
    if double {
        format!("Double K.O. · {ids} merged")
    } else {
        format!("K.O. · {ids} merged")
    }
}

/// One painted frame of the overlay.
#[derive(IntoElement)]
pub struct KoOverlay {
    pub prs: Vec<(i64, String)>,
    pub double: bool,
    pub frame: Frame,
    /// Apply [`Frame::shake`] to the letters (`screen_shake` on).
    pub shake: bool,
    /// Second caption line ("merged into main · 3 reviewers").
    pub subtitle: String,
}

impl RenderOnce for KoOverlay {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let f = self.frame;
        let glyphs = GlyphColors::ken(&theme);
        let (dx, dy) = if self.shake { f.shake } else { (0.0, 0.0) };
        let flash_color = if theme.appearance.is_dark() {
            colors.text_primary.opacity(FLASH_PEAK_DARK * f.flash)
        } else {
            colors.ken_outline.opacity(FLASH_PEAK_LIGHT * f.flash)
        };
        let title = match self.prs.as_slice() {
            [(id, title)] => format!("!{id} {title}"),
            prs => prs
                .iter()
                .map(|(id, _)| format!("!{id}"))
                .collect::<Vec<_>>()
                .join(" · "),
        };
        let letters = LETTERS.iter().zip(f.letters).map(|(text, l)| {
            div().relative().top(px(l.translate_y)).child(
                ArcadeText::new(*text, LETTER_HEIGHT, glyphs)
                    .scale(l.scale)
                    .rotate(l.rotate_deg)
                    .opacity(l.opacity * f.exit),
            )
        });
        div()
            .absolute()
            .inset_0()
            .size_full()
            .opacity(f.exit)
            // Vignette: `ken.outline` fading in from the four edges (gpui
            // has no radial gradient; four linear ones read the same).
            .children(vignette(colors.ken_outline.opacity(f.vignette)))
            .child(div().absolute().inset_0().bg(flash_color))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(space::S16))
                    .left(px(dx))
                    .top(px(dy))
                    .when(self.double, |overlay| {
                        overlay.child(
                            div()
                                .opacity(f.caption)
                                .text_color(colors.ken_secondary)
                                .type_style(typography::TITLE, &theme)
                                .font_weight(FontWeight(800.0))
                                .child("DOUBLE"),
                        )
                    })
                    .child(div().flex().items_end().children(letters))
                    .child(
                        div()
                            .opacity(f.caption)
                            .px(px(space::S16))
                            .py(px(space::S8))
                            .rounded(px(radius::FULL))
                            .bg(colors.ken_outline.opacity(0.7))
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(
                                div()
                                    .text_color(colors.text_primary)
                                    .type_style(typography::TITLE, &theme)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_color(colors.text_primary)
                                    .type_style(typography::SMALL, &theme)
                                    .child(self.subtitle),
                            ),
                    ),
            )
    }
}

/// Four edge strips, `color` at the edge fading to clear inward.
fn vignette(color: gpui::Hsla) -> [gpui::Div; 4] {
    let clear = color.opacity(0.0);
    let strip = |angle: f32| {
        gpui::linear_gradient(
            angle,
            gpui::linear_color_stop(color, 0.0),
            gpui::linear_color_stop(clear, 1.0),
        )
    };
    [
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(gpui::relative(0.3))
            .bg(strip(180.0)),
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(gpui::relative(0.3))
            .bg(strip(0.0)),
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(gpui::relative(0.25))
            .bg(strip(90.0)),
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .right_0()
            .w(gpui::relative(0.25))
            .bg(strip(270.0)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_land_in_order_and_settle() {
        let start = frame(0.0, None);
        assert_eq!(start.letters[1].opacity, 0.0, "'.' waits 90 ms");
        assert!(start.letters[0].scale > 1.7, "K starts large");
        assert!((start.letters[0].rotate_deg + 8.0).abs() < 0.01);
        let settled = frame(1_300.0, None);
        for letter in settled.letters {
            assert!((letter.scale - 1.0).abs() < 0.01);
            assert!(letter.rotate_deg.abs() < 0.1);
            assert_eq!(letter.opacity, 1.0);
        }
    }

    #[test]
    fn impact_flash_and_shake_are_brief() {
        assert_eq!(frame(170.0, None).flash, 0.0);
        assert!(
            (frame(240.0, None).flash - 1.0).abs() < 0.01,
            "peak mid-flash"
        );
        assert_eq!(frame(301.0, None).flash, 0.0, "one flash, 120 ms");
        assert_eq!(frame(179.0, None).shake, (0.0, 0.0));
        assert!(frame(200.0, None).shake.0.abs() > 0.5);
        assert!(frame(200.0, None).shake.0.abs() <= SHAKE_AMPLITUDE);
        assert_eq!(frame(460.0, None).shake, (0.0, 0.0));
    }

    #[test]
    fn caption_fades_in_then_everything_exits() {
        assert_eq!(frame(300.0, None).caption, 0.0);
        assert!((frame(520.0, None).caption - 1.0).abs() < 0.01);
        assert_eq!(frame(1_399.0, None).exit, 1.0);
        assert!(frame(1_600.0, None).exit < 0.6);
        assert!(frame(1_600.0, None).letters[0].translate_y < -3.0);
        assert!(!finished(1_700.0, None));
        assert!(finished(DURATION_MS, None));
    }

    #[test]
    fn input_dismisses_in_90_ms() {
        assert!(frame(545.0, Some(500.0)).exit < 0.7);
        assert!(!finished(580.0, Some(500.0)));
        assert!(finished(590.0, Some(500.0)));
        // Input during the natural exit doesn't prolong it.
        assert!(finished(DURATION_MS, Some(1_500.0)));
    }

    #[test]
    fn toast_copy() {
        assert_eq!(
            toast_text(&[(4821, "Retry".into())], false),
            "K.O. · !4821 merged"
        );
        assert_eq!(
            toast_text(&[(1, String::new()), (2, String::new())], true),
            "Double K.O. · !1, !2 merged"
        );
    }
}
