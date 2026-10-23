//! The combo HUD (ken.md §4.1), the statusbar chip (§4.3) and Ken toasts
//! (§2 badge toasts with the 3 px `ken.primary` stripe). Pure components:
//! every animation input is an elapsed time, so stories paint exact frames.

use adoken_ken::{HitKind, Naming, Rank};
use adoken_theme::{radius, size, space, typography};
use adoken_ui::motion::{EASE_IN, EASE_OUT, SPRING_BOUNCY};
use adoken_ui::{ActiveTheme, Icon, IconName, Tooltip, TypeStyleExt};
use gpui::{AnyElement, App, IntoElement, RenderOnce, SharedString, Window, div, prelude::*, px};

use super::glyphs::{ArcadeText, GlyphColors};

pub const HUD_WIDTH: f32 = 132.0;
pub const HUD_HEIGHT: f32 = 60.0;
const NUMERAL_HEIGHT: f32 = 40.0;
/// Appear: opacity + translate Y −8 → 0 over `motion.quick`.
const APPEAR_MS: f32 = 140.0;
/// Rank caption crossfade.
const RANK_FADE_MS: f32 = 140.0;
/// Move name flash under the caption.
pub const MOVE_FLASH_MS: f32 = 1_200.0;
const MOVE_FADE_OUT_MS: f32 = 210.0;
/// Finish: numerals 1.0 → 1.15 then fade over 320 ms (×3+); plain fade 210.
pub const FINISH_MS: f32 = 320.0;
pub const FADE_MS: f32 = 210.0;
/// Timer bar turns from `ken.secondary` to `ken.primary` in the last minute.
const HOT_MS: i64 = 60_000;
const TIMER_HEIGHT: f32 = 2.0;
/// Toast size.
pub const TOAST_WIDTH: f32 = 340.0;

/// "6 min", "<1 min".
pub fn minutes_left(remaining_ms: i64) -> String {
    let minutes = (remaining_ms.max(0) + 59_999) / 60_000;
    if remaining_ms < 60_000 {
        "<1 min".to_owned()
    } else {
        format!("{minutes} min")
    }
}

/// Numeral scale for the increment pop: 1.35 → 1.0 on `spring.bouncy`.
pub fn pop_scale(since_step_ms: f32) -> f32 {
    let s = SPRING_BOUNCY
        .position((since_step_ms / 1000.0).min(SPRING_BOUNCY.duration().as_secs_f32()));
    1.35 + (1.0 - 1.35) * s
}

/// Finish: `(scale, opacity)` at `t` ms; the flourish grows to 1.15.
pub fn finish_state(t: f32, flourish: bool) -> (f32, f32) {
    if flourish {
        let p = (t / FINISH_MS).clamp(0.0, 1.0);
        (1.0 + 0.15 * EASE_OUT.eval(p), 1.0 - EASE_IN.eval(p))
    } else {
        (1.0, 1.0 - (t / FADE_MS).clamp(0.0, 1.0))
    }
}

/// One frame of the combo HUD.
#[derive(IntoElement, Clone, Debug)]
pub struct ComboHud {
    pub count: u32,
    pub rank: Rank,
    pub naming: Naming,
    /// Time left in the combo window.
    pub remaining_ms: i64,
    pub window_ms: i64,
    /// Pops, crossfades and the timer bar (off: numbers update in place and
    /// the timer reads "6 min").
    pub motion: bool,
    /// Since the combo appeared (×2).
    pub appear_ms: f32,
    /// Since the last increment.
    pub step_ms: f32,
    /// Since the rank last changed, if it just did.
    pub rank_up_ms: Option<f32>,
    /// The latest move, flashed under the caption for 1.2 s.
    pub last_move: Option<(HitKind, f32)>,
    /// Finishing: `(ms since the window expired, flourish)`.
    pub finish: Option<(f32, bool)>,
}

impl RenderOnce for ComboHud {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let motion = self.motion;
        let (appear, lift) = if motion {
            let p = EASE_OUT.eval((self.appear_ms / APPEAR_MS).clamp(0.0, 1.0));
            (p, -8.0 * (1.0 - p))
        } else {
            (1.0, 0.0)
        };
        let (finish_scale, finish_opacity) = match self.finish {
            Some((t, flourish)) if motion => finish_state(t, flourish),
            Some(_) => (1.0, 0.0),
            None => (1.0, 1.0),
        };
        let scale = if motion {
            pop_scale(self.step_ms) * finish_scale
        } else {
            1.0
        };
        let caption_opacity = match self.rank_up_ms {
            Some(t) if motion => (t / RANK_FADE_MS).clamp(0.0, 1.0),
            _ => 1.0,
        };
        let (primary, kanji) = self.rank.name().caption(self.naming);
        let fraction = (self.remaining_ms as f32 / self.window_ms.max(1) as f32).clamp(0.0, 1.0);
        let bar_color = if self.remaining_ms <= HOT_MS {
            colors.ken_primary
        } else {
            colors.ken_secondary
        };
        let move_flash = self.last_move.and_then(|(kind, t)| {
            (t < MOVE_FLASH_MS).then(|| {
                let fade = ((MOVE_FLASH_MS - t) / MOVE_FADE_OUT_MS).clamp(0.0, 1.0);
                (
                    kind.move_name().text(self.naming),
                    if motion { fade } else { 1.0 },
                )
            })
        });
        div()
            .w(px(HUD_WIDTH))
            .min_h(px(HUD_HEIGHT))
            .relative()
            .top(px(lift))
            .opacity(appear * finish_opacity)
            .flex()
            .flex_col()
            .items_start()
            .gap(px(space::S2))
            .child(
                ArcadeText::new(
                    format!("×{}", self.count),
                    NUMERAL_HEIGHT,
                    GlyphColors::ken(&theme),
                )
                .scale(scale),
            )
            .child(
                div()
                    .opacity(caption_opacity)
                    .px(px(space::S6))
                    .rounded(px(radius::FULL))
                    .bg(colors.ken_outline.opacity(0.55))
                    .flex()
                    .items_baseline()
                    .gap(px(space::S4))
                    .child(
                        div()
                            .text_color(colors.ken_secondary)
                            .type_style(typography::BODY_STRONG, &theme)
                            .child(primary),
                    )
                    .children(kanji.map(|kanji| {
                        div()
                            .text_color(colors.ken_secondary)
                            .type_style(typography::SMALL, &theme)
                            .child(kanji)
                    })),
            )
            .children(move_flash.map(|(name, opacity)| {
                div()
                    .opacity(opacity)
                    .px(px(space::S6))
                    .rounded(px(radius::FULL))
                    .bg(colors.ken_outline.opacity(0.55))
                    .text_color(colors.text_primary)
                    .type_style(typography::SMALL, &theme)
                    .child(name)
            }))
            .child(if motion {
                div()
                    .w(px(HUD_WIDTH))
                    .h(px(TIMER_HEIGHT))
                    .rounded(px(radius::FULL))
                    .bg(colors.ken_outline.opacity(0.55))
                    .child(
                        div()
                            .h_full()
                            .w(px(HUD_WIDTH * fraction))
                            .rounded(px(radius::FULL))
                            .bg(bar_color),
                    )
                    .into_any_element()
            } else {
                div()
                    .px(px(space::S6))
                    .rounded(px(radius::FULL))
                    .bg(colors.ken_outline.opacity(0.55))
                    .text_color(colors.text_primary)
                    .type_style(typography::LABEL, &theme)
                    .child(minutes_left(self.remaining_ms))
                    .into_any_element()
            })
    }
}

/// The statusbar chip's inputs.
#[derive(IntoElement, Clone, Debug, Default)]
pub struct StatusChip {
    /// Live combo (×2+) and its time left.
    pub combo: Option<(u32, Rank, i64)>,
    pub naming: Naming,
    pub streak_days: u32,
    pub shields: u8,
    pub paused: bool,
    /// "Quiet mode until 15:40" / "Quiet mode on" while Quiet mode is on
    /// (everything else hidden).
    pub quiet: Option<String>,
}

impl StatusChip {
    /// Tooltip and accessibility label (ken.md §4.3, §10).
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some((count, rank, remaining)) = self.combo {
            parts.push(format!(
                "Combo ×{count} · {} · {} left",
                rank.name().english,
                minutes_left(remaining)
            ));
        }
        if self.streak_days > 0 || self.paused {
            let shields = match self.shields {
                0 => String::new(),
                1 => " (1 shield)".to_owned(),
                n => format!(" ({n} shields)"),
            };
            let paused = if self.paused { ", paused" } else { "" };
            let days = if self.streak_days == 1 { "day" } else { "days" };
            parts.push(format!(
                "Streak {} {days}{shields}{paused}",
                self.streak_days
            ));
        }
        parts.join(" · ")
    }
}

impl RenderOnce for StatusChip {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        if let Some(label) = self.quiet.clone() {
            return div()
                .id("ken-chip")
                .flex()
                .items_center()
                .role(gpui::Role::Status)
                .aria_label(label.clone())
                .tooltip(Tooltip::text(label))
                .child(
                    Icon::new(IconName::Moon)
                        .size(size::ICON_XS)
                        .color(colors.text_secondary),
                )
                .into_any_element();
        }
        let describe = self.describe();
        if describe.is_empty() {
            return div().into_any_element();
        }
        let combo = self.combo.map(|(count, rank, _)| {
            let (primary, kanji) = rank.name().caption(self.naming);
            div()
                .flex()
                .items_center()
                .gap(px(space::S4))
                .child(
                    div()
                        .text_color(colors.ken_primary)
                        .type_style(typography::LABEL, &theme)
                        .child(format!("×{count}")),
                )
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::LABEL, &theme)
                        .child(SharedString::from(kanji.unwrap_or(primary))),
                )
        });
        let streak = (self.streak_days > 0 || self.paused).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(space::S2))
                .child(
                    Icon::new(if self.paused {
                        IconName::Pause
                    } else {
                        IconName::Flame
                    })
                    .size(size::ICON_XS)
                    .color(colors.ken_secondary),
                )
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .type_style(typography::LABEL, &theme)
                        .child(self.streak_days.to_string()),
                )
        });
        div()
            .id("ken-chip")
            .flex()
            .items_center()
            .gap(px(space::S8))
            .role(gpui::Role::Status)
            .aria_label(describe.clone())
            .tooltip(Tooltip::text(describe))
            .children(combo)
            .children(streak)
            .into_any_element()
    }
}

/// A Ken toast: standard toast with the ken stripe; K.O. toasts carry a
/// small K.O. glyph badge.
#[derive(IntoElement, Clone, Debug)]
pub struct KenToast {
    pub text: SharedString,
    pub ko_badge: bool,
    /// Optional action label (e.g. "Open" for the weekly card), shown as a
    /// key hint; the action itself runs from the palette.
    pub hint: Option<SharedString>,
}

impl RenderOnce for KenToast {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        div()
            .id(gpui::ElementId::Name(
                format!("ken-toast-{}", self.text).into(),
            ))
            .w(px(TOAST_WIDTH))
            .flex()
            .items_center()
            .gap(px(space::S8))
            .relative()
            .overflow_hidden()
            .pl(px(space::S12 + 3.0))
            .pr(px(space::S12))
            .py(px(space::S8))
            .rounded(px(radius::MD))
            .border_1()
            .border_color(colors.border_default)
            .bg(colors.bg_overlay)
            .shadow(theme.shadows(adoken_theme::elevation::LEVEL_2))
            .text_color(colors.text_primary)
            .type_style(typography::BODY, &theme)
            .role(gpui::Role::Status)
            .aria_label(self.text.clone())
            // The ken accent stripe (3 px `ken.primary` left edge).
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .w(px(3.0))
                    .bg(colors.ken_primary),
            )
            .when(self.ko_badge, |toast| {
                toast.child(ArcadeText::new("K.O.", 18.0, GlyphColors::ken(&theme)))
            })
            .child(div().flex_1().min_w_0().child(self.text))
            .children(self.hint.map(|hint| {
                div()
                    .text_color(colors.text_secondary)
                    .type_style(typography::SMALL, &theme)
                    .child(hint)
            }))
    }
}

/// A column of toasts (Ken's and celebrations'), newest last.
pub fn toast_stack<T: IntoElement>(toasts: impl IntoIterator<Item = T>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_end()
        .gap(px(space::S8))
        .children(toasts)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_round_up_and_floor_at_one() {
        assert_eq!(minutes_left(6 * 60_000), "6 min");
        assert_eq!(minutes_left(5 * 60_000 + 1), "6 min");
        assert_eq!(minutes_left(59_000), "<1 min");
        assert_eq!(minutes_left(-5), "<1 min");
    }

    #[test]
    fn pop_settles_from_1_35_to_1() {
        assert!((pop_scale(0.0) - 1.35).abs() < 1e-3);
        assert!((pop_scale(900.0) - 1.0).abs() < 0.01);
        // spring.bouncy overshoots below 1 on the way.
        assert!((0..60).any(|step| pop_scale(step as f32 * 10.0) < 1.0));
    }

    #[test]
    fn finish_flourish_grows_then_fades() {
        assert_eq!(finish_state(0.0, true), (1.0, 1.0));
        let (scale, opacity) = finish_state(FINISH_MS, true);
        assert!((scale - 1.15).abs() < 1e-3 && opacity.abs() < 1e-3);
        assert_eq!(finish_state(FADE_MS, false), (1.0, 0.0));
    }

    #[test]
    fn chip_description_reads_naturally() {
        let chip = StatusChip {
            combo: Some((7, Rank::Gale, 6 * 60_000)),
            streak_days: 12,
            shields: 1,
            ..StatusChip::default()
        };
        assert_eq!(
            chip.describe(),
            "Combo ×7 · Gale · 6 min left · Streak 12 days (1 shield)"
        );
        let paused = StatusChip {
            streak_days: 1,
            paused: true,
            ..StatusChip::default()
        };
        assert_eq!(paused.describe(), "Streak 1 day, paused");
        assert_eq!(StatusChip::default().describe(), "");
    }
}
