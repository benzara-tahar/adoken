//! Loaders: the adoken fist mark, the matrix spinner (behind [`Spinner`]),
//! the 2×3 mini spinners, a determinate progress ring, and the splash
//! overlay shown while local data opens.
//!
//! Adapted from zeron `crates/ui/src/loaders.rs` + `crates/proto/src/motion.rs`
//! at `d1010657` (MIT): the gradient matrix spinner and its arrow-up phase
//! map, the mini spinner's perimeter chase, the stroked-polyline progress
//! ring, the splash overlay, and the pulse math. Changes: colors are theme
//! tokens (no fixed tints), everything runs on adoken's shared ~30 fps clock
//! ([`pulse_phase`]), and zeron's mark loader is replaced by an original
//! pixel fist (the adoken.dev mark, `public/src/art/fist.ts`).
//!
//! Reduce motion: [`pulse_phase`] schedules no frames; the matrix and mini
//! spinners draw a still frame shaded along their wave ([`still_opacity`]),
//! the fist renders fully lit, and the splash exit snaps (gpui
//! `with_animation`).
//!
//! [`Spinner`]: crate::Spinner

use std::sync::LazyLock;
use std::time::Duration;

use adoken_theme::{motion as durations, size, space, typography};
use gpui::{
    AnimationExt, App, Bounds, Hsla, IntoElement, PathBuilder, RenderOnce, SharedString, Window,
    canvas, div, fill, point, prelude::*, px,
};

use crate::motion::{EASE_IN, animation, lerp, pulse_phase};
use crate::theme::{ActiveTheme, Theme, TypeStyleExt};

// ---------------------------------------------------------------------------
// Pulse math (pure)
// ---------------------------------------------------------------------------

/// Matrix spinner wave period.
pub const MATRIX_PERIOD: Duration = Duration::from_millis(750);
/// Fist mark wave period: a charge running wrist → knuckles.
pub const FIST_PERIOD: Duration = Duration::from_millis(1600);
/// Side length of the matrix spinner.
pub const MATRIX_SIDE: usize = 3;
/// Opacity a matrix/mini cell rests at between pulses.
pub const MATRIX_DIM: f32 = 0.1;
/// Fist fill cells rest at this opacity and flare to 1 as the wave passes;
/// the outline never dims, so the mark stays legible mid-cycle.
pub const FIST_REST: f32 = 0.4;
/// Share of the fist period the wave takes to cross the mark bottom → top.
pub const FIST_SPREAD: f32 = 0.6;

/// Cosine pulse: 0 at phase 0, 1 at 0.5, back to 0 at 1.
pub fn pulse_wave(phase: f32) -> f32 {
    0.5 - 0.5 * (phase * std::f32::consts::TAU).cos()
}

/// A cell's phase given the loader phase and a per-cell offset (fraction
/// of the period), wrapped into `[0, 1)`.
pub fn staggered_phase(phase: f32, offset: f32) -> f32 {
    (phase - offset).rem_euclid(1.0)
}

/// Matrix cell opacity at local phase `t`: full at the cycle start, easing
/// to `dim` by 45 %, resting there until 92 %, then back to full.
pub fn matrix_opacity(t: f32, dim: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 0.45 {
        lerp(1.0, dim, t / 0.45)
    } else if t < 0.92 {
        dim
    } else {
        lerp(dim, 1.0, (t - 0.92) / 0.08)
    }
}

/// Phase offset of matrix cell `(row, col)`: the pulse enters at the bottom
/// edge and converges on the top-center cell, so the wave reads as rising.
pub fn matrix_cell_phase(row: usize, col: usize) -> f32 {
    let center = (MATRIX_SIDE as f32 - 1.0) / 2.0;
    let max = MATRIX_SIDE as f32 - 1.0 + center;
    let d = MATRIX_SIDE as f32 - 1.0 - row as f32 + (col as f32 - center).abs();
    d / (max + 1.0)
}

/// Clockwise ring position of each `(row, col)` of the 2×3 mini spinner,
/// top-left first: (0,0) → (0,1) → (1,1) → (2,1) → (2,0) → (1,0).
pub const MINI_RING: [[usize; 2]; 3] = [[0, 1], [5, 2], [4, 3]];
const MINI_RING_LEN: f32 = 6.0;

/// Reduce motion's still frame: a cell `offset` (its phase offset, 0..1)
/// behind the wave's origin is drawn this bright, so the resting glyph
/// shows the wave's shape instead of one lit cell.
pub fn still_opacity(offset: f32) -> f32 {
    lerp(1.0, STILL_FLOOR, offset.clamp(0.0, 1.0))
}

/// Dimmest cell of a still frame.
const STILL_FLOOR: f32 = 0.2;

/// Phase offset of fist row `y` (0 = top): the wrist leads, the knuckles
/// trail, so the light travels upward like a punch winding up.
pub fn fist_row_stagger(y: usize) -> f32 {
    let bottom = (FIST_H - 1) as f32;
    -((bottom - y as f32) / bottom) * FIST_SPREAD
}

/// Opacity of a fist fill cell in row `y` at loader phase `phase`.
pub fn fist_fill_opacity(phase: f32, y: usize) -> f32 {
    lerp(
        FIST_REST,
        1.0,
        pulse_wave(staggered_phase(phase, fist_row_stagger(y))),
    )
}

// ---------------------------------------------------------------------------
// Fist mark
// ---------------------------------------------------------------------------

/// The adoken mark, 21×20 cells (same grid as `public/src/art/fist.ts`).
/// `o` outline · `h` highlight · `s` base · `d` shade · `w` wrap ·
/// `W` wrap highlight.
const FIST_GRID: [&str; 20] = [
    ".oooo.oooo.oooo......",
    "ohhhhohhhhohhhhooooo.",
    "ohsssohsssohsssohhhho",
    "ohsssohsssohsssohssso",
    "osssdosssdosssdosssdo",
    "osssdosssdosssdosssdo",
    "oddddoddddoddddoddddo",
    "oooooooooooooodosssdo",
    "ohhhhhhhhhhhhhodsssdo",
    "ossssssssssssssosssdo",
    "osssssssssssssdosssdo",
    "odddddddddddddoddddo.",
    ".oooooooooooooossso..",
    "..osssssssssssssdo...",
    "..odsssssssssssddo...",
    "...oooooooooooooo....",
    "...owwwwwwwwwwwwo....",
    "...oWWWWWWWWWWWWo....",
    "...owwwwwwwwwwwwo....",
    "...oooooooooooooo....",
];
pub const FIST_W: usize = 21;
pub const FIST_H: usize = 20;

/// A fist cell's paint role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FistRole {
    Outline,
    Highlight,
    Base,
    Shade,
    Wrap,
    WrapHighlight,
}

impl FistRole {
    fn parse(cell: u8) -> Option<Self> {
        match cell {
            b'o' => Some(Self::Outline),
            b'h' => Some(Self::Highlight),
            b's' => Some(Self::Base),
            b'd' => Some(Self::Shade),
            b'w' => Some(Self::Wrap),
            b'W' => Some(Self::WrapHighlight),
            _ => None,
        }
    }
}

/// A horizontal run of same-role cells (≈4× fewer quads than per cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FistRun {
    pub x: usize,
    pub y: usize,
    pub len: usize,
    pub role: FistRole,
}

/// The mark as runs, parsed once.
pub fn fist_runs() -> &'static [FistRun] {
    static RUNS: LazyLock<Vec<FistRun>> = LazyLock::new(|| {
        let mut runs = Vec::new();
        for (y, row) in FIST_GRID.iter().enumerate() {
            let row = row.as_bytes();
            let mut x = 0;
            while x < row.len() {
                let mut end = x + 1;
                while end < row.len() && row[end] == row[x] {
                    end += 1;
                }
                if let Some(role) = FistRole::parse(row[x]) {
                    runs.push(FistRun {
                        x,
                        y,
                        len: end - x,
                        role,
                    });
                }
                x = end;
            }
        }
        runs
    });
    &RUNS
}

/// Fist colors from the `ken.*` tokens; highlight and shade are the base
/// mixed toward `ken.spark` / `ken.outline`, so themes only set four roles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FistPalette {
    pub outline: Hsla,
    pub highlight: Hsla,
    pub base: Hsla,
    pub shade: Hsla,
    pub wrap: Hsla,
    pub wrap_highlight: Hsla,
}

impl FistPalette {
    pub fn from_theme(theme: &Theme) -> Self {
        let c = &theme.colors;
        Self {
            outline: c.ken_outline,
            highlight: c.ken_primary.blend(c.ken_spark.opacity(0.45)),
            base: c.ken_primary,
            shade: c.ken_primary.blend(c.ken_outline.opacity(0.3)),
            wrap: c.ken_secondary,
            wrap_highlight: c.ken_secondary.blend(c.ken_spark.opacity(0.6)),
        }
    }

    fn color(&self, role: FistRole) -> Hsla {
        match role {
            FistRole::Outline => self.outline,
            FistRole::Highlight => self.highlight,
            FistRole::Base => self.base,
            FistRole::Shade => self.shade,
            FistRole::Wrap => self.wrap,
            FistRole::WrapHighlight => self.wrap_highlight,
        }
    }
}

/// The animated fist mark, `height` px tall (width follows 21:20). One
/// canvas painting ≤ 100 quads per frame; the whole mark is one element.
#[derive(IntoElement)]
pub struct FistLoader {
    height: f32,
}

pub fn fist_loader(height: f32) -> FistLoader {
    FistLoader { height }
}

impl RenderOnce for FistLoader {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let phase = pulse_phase(FIST_PERIOD, window.current_view(), cx);
        let still = cx.reduce_motion();
        let palette = FistPalette::from_theme(cx.theme());
        let cell = self.height / FIST_H as f32;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                for run in fist_runs() {
                    let mut color = palette.color(run.role);
                    if !still && run.role != FistRole::Outline {
                        color = color.opacity(fist_fill_opacity(phase, run.y));
                    }
                    let origin =
                        bounds.origin + point(px(run.x as f32 * cell), px(run.y as f32 * cell));
                    let size = gpui::size(px(run.len as f32 * cell), px(cell));
                    window.paint_quad(fill(Bounds::new(origin, size), color));
                }
            },
        )
        .flex_none()
        .w(px(cell * FIST_W as f32))
        .h(px(self.height))
    }
}

// ---------------------------------------------------------------------------
// Matrix + mini spinners
// ---------------------------------------------------------------------------

/// Default per-row matrix tints, top → bottom: a cool → warm "sunrise"
/// from the theme's info, warning and accent roles.
pub fn matrix_tints(theme: &Theme) -> [Hsla; MATRIX_SIDE] {
    let c = &theme.colors;
    [c.status_info_fg, c.status_warning_fg, c.accent_fg]
}

/// The 3×3 matrix spinner: round cells, each pulsing once per
/// [`MATRIX_PERIOD`], phased so the wave rises toward the top-center cell.
/// Total size is `4 × cell` square.
#[derive(IntoElement)]
pub struct MatrixSpinner {
    cell: f32,
    tints: Option<[Hsla; MATRIX_SIDE]>,
}

pub fn gradient_spinner(cell: f32) -> MatrixSpinner {
    MatrixSpinner { cell, tints: None }
}

impl MatrixSpinner {
    /// Per-row colors (default [`matrix_tints`]).
    pub fn tints(mut self, tints: [Hsla; MATRIX_SIDE]) -> Self {
        self.tints = Some(tints);
        self
    }

    /// One color for every cell (buttons, status slots).
    pub fn mono(self, tint: Hsla) -> Self {
        self.tints([tint; MATRIX_SIDE])
    }
}

impl RenderOnce for MatrixSpinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let phase = pulse_phase(MATRIX_PERIOD, window.current_view(), cx);
        let still = cx.reduce_motion();
        let tints = self.tints.unwrap_or_else(|| matrix_tints(cx.theme()));
        let cell = self.cell;
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(cell / 2.0))
            .children((0..MATRIX_SIDE).map(move |row| {
                div()
                    .flex()
                    .gap(px(cell / 2.0))
                    .children((0..MATRIX_SIDE).map(move |col| {
                        let offset = matrix_cell_phase(row, col);
                        div()
                            .size(px(cell))
                            .rounded_full()
                            .bg(tints[row])
                            .opacity(if still {
                                still_opacity(offset)
                            } else {
                                matrix_opacity(phase + offset, MATRIX_DIM)
                            })
                    }))
            }))
    }
}

/// A 2×3 glyph for compact status slots: brightness chases around the
/// grid's perimeter. `2.5 × cell` wide, `4 × cell` tall.
#[derive(IntoElement)]
pub struct MiniSpinner {
    cell: f32,
    tints: [Hsla; 3],
}

/// Accent-tinted mini spinner (per-row colors).
pub fn mini_glyph_spinner(cell: f32, tints: [Hsla; 3]) -> MiniSpinner {
    MiniSpinner { cell, tints }
}

/// Grayscale variant for surfaces where an accent would pull focus.
pub fn mini_mono_spinner(cell: f32, tint: Hsla) -> MiniSpinner {
    MiniSpinner {
        cell,
        tints: [tint; 3],
    }
}

impl RenderOnce for MiniSpinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let phase = pulse_phase(MATRIX_PERIOD, window.current_view(), cx);
        let still = cx.reduce_motion();
        let (cell, tints) = (self.cell, self.tints);
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(cell / 2.0))
            .children((0..3).map(move |row| {
                div()
                    .flex()
                    .gap(px(cell / 2.0))
                    .children((0..2).map(move |col| {
                        let offset = MINI_RING[row][col] as f32 / MINI_RING_LEN;
                        div()
                            .size(px(cell))
                            .rounded_full()
                            .bg(tints[row])
                            .opacity(if still {
                                still_opacity(offset)
                            } else {
                                matrix_opacity(phase + offset, MATRIX_DIM)
                            })
                    }))
            }))
    }
}

// ---------------------------------------------------------------------------
// Progress ring
// ---------------------------------------------------------------------------

const RING_STROKE: f32 = 2.5;
/// Polyline segments for a full circle: plenty for rings ≤ 48 px.
const RING_SEGMENTS: f32 = 64.0;
/// Below this diameter the percent label doesn't fit and is left out.
const RING_LABEL_MIN: f32 = 28.0;

/// Determinate progress: `border.default` track, `accent.fg` arc growing
/// clockwise from 12 o'clock, percent centered in `text.secondary`. Updates
/// its value without interpolation, so it needs no reduce-motion branch.
#[derive(IntoElement)]
pub struct ProgressRing {
    percent: u8,
    diameter: f32,
}

pub fn progress_ring(percent: u8, diameter: f32) -> ProgressRing {
    ProgressRing {
        percent: percent.min(100),
        diameter,
    }
}

/// Points of a clockwise arc from 12 o'clock covering `sweep` of a turn.
pub fn arc_points(center: (f32, f32), radius: f32, sweep: f32) -> Vec<(f32, f32)> {
    let sweep = sweep.clamp(0.0, 1.0);
    if sweep <= 0.0 {
        return Vec::new();
    }
    let steps = ((RING_SEGMENTS * sweep).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let theta = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::TAU * sweep * (i as f32 / steps as f32);
            (
                center.0 + radius * theta.cos(),
                center.1 + radius * theta.sin(),
            )
        })
        .collect()
}

impl RenderOnce for ProgressRing {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track, arc) = (theme.colors.border_default, theme.colors.accent_fg);
        let frac = f32::from(self.percent) / 100.0;
        let diameter = self.diameter;
        let ring = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let center = bounds.center();
                let radius = diameter / 2.0 - RING_STROKE;
                let origin = (f32::from(center.x), f32::from(center.y));
                for (sweep, color) in [(1.0, track), (frac, arc)] {
                    let points = arc_points(origin, radius, sweep);
                    let Some(((x0, y0), rest)) = points.split_first() else {
                        continue;
                    };
                    let mut path = PathBuilder::stroke(px(RING_STROKE));
                    path.move_to(point(px(*x0), px(*y0)));
                    for (x, y) in rest {
                        path.line_to(point(px(*x), px(*y)));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                }
            },
        )
        .absolute()
        .inset_0();
        div()
            .relative()
            .flex_none()
            .size(px(diameter))
            .flex()
            .items_center()
            .justify_center()
            .child(ring)
            .when(diameter >= RING_LABEL_MIN, |ring| {
                ring.child(
                    div()
                        .text_color(theme.colors.text_secondary)
                        .type_style(typography::LABEL, theme)
                        .child(SharedString::from(format!("{}%", self.percent))),
                )
            })
    }
}

// ---------------------------------------------------------------------------
// Splash overlay
// ---------------------------------------------------------------------------

/// Fist height on the splash.
pub const SPLASH_FIST_HEIGHT: f32 = 60.0;
/// How long the splash exit runs; hosts remove it after this.
pub const SPLASH_EXIT: Duration = Duration::from_millis(durations::SLOW_EXIT as u64);

/// Covers its (relative) parent while local data opens: the fist mark and
/// a quiet status line on `bg.app`. With [`fading`](Self::fading) it plays
/// the exit (fade + 6 px lift over [`SPLASH_EXIT`]); the host drops it after.
#[derive(IntoElement)]
pub struct SplashOverlay {
    label: SharedString,
    fading: bool,
}

pub fn splash_overlay(label: impl Into<SharedString>) -> SplashOverlay {
    SplashOverlay {
        label: label.into(),
        fading: false,
    }
}

impl SplashOverlay {
    pub fn fading(mut self, fading: bool) -> Self {
        self.fading = fading;
        self
    }
}

impl RenderOnce for SplashOverlay {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let content = div()
            .id("splash-overlay")
            .occlude()
            .absolute()
            .inset_0()
            .bg(theme.colors.bg_app)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(space::S16))
            .child(fist_loader(SPLASH_FIST_HEIGHT))
            .child(
                div()
                    .h(px(size::CONTROL_SM))
                    .text_color(theme.colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child(self.label),
            );
        if self.fading {
            content
                .with_animation(
                    "splash-out",
                    animation(durations::SLOW_EXIT, EASE_IN),
                    |el, t| el.opacity(1.0 - t).top(px(-6.0 * t)),
                )
                .into_any_element()
        } else {
            content.into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_pulse_keyframes() {
        assert!((matrix_opacity(0.0, MATRIX_DIM) - 1.0).abs() < 1e-6);
        assert!((matrix_opacity(0.45, MATRIX_DIM) - MATRIX_DIM).abs() < 1e-6);
        assert!((matrix_opacity(0.7, MATRIX_DIM) - MATRIX_DIM).abs() < 1e-6);
        assert!((matrix_opacity(1.0, MATRIX_DIM) - 1.0).abs() < 1e-6);
        for step in 0..=200 {
            let o = matrix_opacity(step as f32 / 100.0 - 0.5, MATRIX_DIM);
            assert!((MATRIX_DIM..=1.0).contains(&o), "{step} → {o}");
        }
    }

    #[test]
    fn matrix_wave_rises_to_top_center() {
        let bottom_center = matrix_cell_phase(MATRIX_SIDE - 1, 1);
        let top_center = matrix_cell_phase(0, 1);
        let top_corner = matrix_cell_phase(0, 0);
        assert_eq!(bottom_center, 0.0);
        assert!(top_center > bottom_center);
        assert!(top_corner > top_center, "corners trail the center");
        for row in 0..MATRIX_SIDE {
            for col in 0..MATRIX_SIDE {
                assert!((0.0..1.0).contains(&matrix_cell_phase(row, col)));
            }
        }
    }

    #[test]
    fn still_frames_fade_from_the_wave_origin() {
        assert_eq!(still_opacity(0.0), 1.0);
        assert!(still_opacity(0.5) < 1.0 && still_opacity(0.5) > STILL_FLOOR);
        assert!((still_opacity(3.0) - STILL_FLOOR).abs() < 1e-6, "clamped");
    }

    #[test]
    fn mini_ring_visits_every_cell_once() {
        let mut seen: Vec<usize> = MINI_RING.iter().flatten().copied().collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..6).collect::<Vec<_>>());
    }

    #[test]
    fn fist_grid_matches_the_site_mark() {
        assert!(FIST_GRID.iter().all(|row| row.len() == FIST_W));
        assert_eq!(FIST_GRID.len(), FIST_H);
        let runs = fist_runs();
        // Runs tile exactly the painted cells, without overlap.
        let painted: usize = FIST_GRID
            .iter()
            .map(|row| row.bytes().filter(|b| *b != b'.').count())
            .sum();
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), painted);
        assert!(runs.len() < painted / 2, "runs merge neighbors");
        assert!(runs.iter().all(|r| r.x + r.len <= FIST_W && r.y < FIST_H));
    }

    #[test]
    fn fist_wave_starts_at_the_wrist() {
        // The wrist row leads (offset 0); the knuckles trail by the spread.
        assert_eq!(fist_row_stagger(FIST_H - 1), 0.0);
        assert!((fist_row_stagger(0) + FIST_SPREAD).abs() < 1e-6);
        // At the wrist's crest the knuckles are still near rest.
        let crest = 0.5;
        assert!((fist_fill_opacity(crest, FIST_H - 1) - 1.0).abs() < 1e-6);
        assert!(fist_fill_opacity(crest, 0) < 0.6);
        for y in 0..FIST_H {
            for step in 0..=20 {
                let o = fist_fill_opacity(step as f32 / 20.0, y);
                assert!((FIST_REST..=1.0 + 1e-6).contains(&o));
            }
        }
    }

    #[test]
    fn arcs_start_at_twelve_and_run_clockwise() {
        let points = arc_points((10.0, 10.0), 5.0, 0.25);
        let (x0, y0) = points[0];
        assert!((x0 - 10.0).abs() < 1e-4 && (y0 - 5.0).abs() < 1e-4);
        let (x1, y1) = *points.last().expect("points");
        assert!(
            (x1 - 15.0).abs() < 1e-4 && (y1 - 10.0).abs() < 1e-4,
            "3 o'clock"
        );
        assert!(arc_points((0.0, 0.0), 5.0, 0.0).is_empty());
        assert_eq!(
            arc_points((0.0, 0.0), 5.0, 2.0).len(),
            65,
            "clamped to one turn"
        );
    }
}
