//! The live effect state shared by the overlay and the anchoring views:
//! anchors waiting for bounds, static glyph timers, confetti (calm), sparks
//! (Ken's and arcade celebrations', one shared cap), the seal stamp and
//! the chip glow; plus their frame math and painting. Everything is lazily
//! allocated: an idle session holds empty vectors and no seal geometry.

use std::time::Instant;

use adoken_ken::celebrate::{Style, Tier};
use adoken_ken::confetti::{self, Confetti};
use adoken_ken::glyph::Poly;
use adoken_ken::seal::{self, Seal};
use adoken_ken::sparks::Sparks;
use adoken_model::Millis;
use adoken_theme::motion;
use adoken_ui::motion::{EASE_IN, EASE_OUT, SPRING_BOUNCY};
use gpui::{
    BorderStyle, Bounds, Corners, Edges, FillOptions, Hsla, PathBuilder, PathStyle, Pixels,
    SharedString, Window, point, px, size,
};

use super::Anchor;

/// ken.md §12.1: arcade Step sparks, the seal's sparks.
pub const STEP_SPARKS: usize = 12;
pub const SEAL_SPARKS: usize = 32;
/// ken.md §12.3 seal timeline (ms).
pub const SEAL_PX: f32 = 40.0;
pub const SEAL_ROTATE_DEG: f32 = -6.0;
pub const SEAL_SPARKS_AT: f32 = 120.0;
pub const SEAL_HOLD_UNTIL: f32 = 900.0;
pub const SEAL_END: f32 = SEAL_HOLD_UNTIL + motion::EXPRESSIVE_EXIT as f32;
// ken.md §12.3: the stamp's total is ≤ 1.3 s.
const _: () = assert!(SEAL_END <= 1_300.0);
/// ken.md §12.2 chip glow: 0 → 0.8 → 0 over `motion.expressive`.
pub const GLOW_MS: f32 = motion::EXPRESSIVE as f32;
const GLOW_PEAK: f32 = 0.8;
const GLOW_WIDTH: f32 = 2.0;
/// Spark/confetti step clamp: a stalled frame doesn't teleport pieces.
pub const MAX_DT_MS: f32 = 50.0;

/// One celebration waiting for its anchor's bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct Emission {
    pub anchor: Anchor,
    pub tier: Tier,
    pub style: Style,
    /// A Milestone's toast `(title, body)`, shown (and burst from) when the
    /// anchor isn't on screen. `None` for a Step (nothing extra) and once
    /// the emission already targets its toast.
    pub fallback: Option<(SharedString, SharedString)>,
}

/// Test and perf hooks (spec 0022 AC-6, AC-16): frame indices are the
/// layer's renders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hooks {
    /// Layer renders so far (the frame index).
    pub frames: u64,
    /// Animation frames the celebration requested.
    pub frames_requested: u64,
    /// Glyph and toast timers currently owned.
    pub timers: usize,
    pub effects_started: u64,
    pub first_effect_frame: Option<u64>,
    pub last_effect_frame: Option<u64>,
    /// The frame index when the latest celebration was queued: a trigger
    /// between frames N and N + 1 records N, and its effect starts on
    /// N + 1 (AC-6, AC-7).
    pub last_queued_frame: Option<u64>,
    /// The last frame anything was live (particles, seal, glow).
    pub last_live_frame: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stamp {
    x: f32,
    y: f32,
    age_ms: f32,
    sparked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Glow {
    bounds: Bounds<Pixels>,
    age_ms: f32,
}

pub struct Fx {
    anchors: Vec<(Anchor, Bounds<Pixels>)>,
    glyphs: Vec<(Anchor, Millis)>,
    pending: Vec<Emission>,
    /// Milestones whose anchor wasn't painted: the layer shows their toast
    /// and queues them again against it.
    pub(crate) unanchored: Vec<Emission>,
    pub(crate) sparks: Sparks,
    confetti: Option<Confetti>,
    seal: Option<Seal>,
    stamps: Vec<Stamp>,
    glows: Vec<Glow>,
    seed: u64,
    pub(crate) last_step: Option<Instant>,
    pub hooks: Hooks,
    /// "Now" pinned by tests and stories; the wall clock otherwise.
    pub clock: Option<Millis>,
}

impl Fx {
    pub fn new(seed: u64) -> Self {
        Self {
            anchors: Vec::new(),
            glyphs: Vec::new(),
            pending: Vec::new(),
            unanchored: Vec::new(),
            sparks: Sparks::new(seed ^ 0x004B_454E),
            confetti: None,
            seal: None,
            stamps: Vec::new(),
            glows: Vec::new(),
            seed,
            last_step: None,
            hooks: Hooks::default(),
            clock: None,
        }
    }

    /// Celebration time (glyph timers, memory, limits).
    pub fn now(&self) -> Millis {
        self.clock.unwrap_or_else(crate::palette::now_ms)
    }

    /// A celebration waits for `anchor`'s bounds.
    pub fn armed(&self, anchor: Anchor) -> bool {
        self.pending.iter().any(|e| e.anchor == anchor)
    }

    pub fn record(&mut self, anchor: Anchor, bounds: Bounds<Pixels>) {
        self.anchors.push((anchor, bounds));
    }

    pub fn queue(&mut self, emission: Emission) {
        self.hooks.last_queued_frame = Some(self.hooks.frames);
        self.pending.push(emission);
    }

    pub fn glyph_live(&self, anchor: Anchor, now: Millis) -> bool {
        self.glyphs
            .iter()
            .any(|(a, until)| *a == anchor && now < *until)
    }

    pub fn show_glyph(&mut self, anchor: Anchor, until: Millis) {
        self.glyphs.retain(|(a, _)| *a != anchor);
        self.glyphs.push((anchor, until));
    }

    /// Drop ended glyphs; whether any is still showing.
    pub fn prune_glyphs(&mut self, now: Millis) -> bool {
        self.glyphs.retain(|(_, until)| now < *until);
        !self.glyphs.is_empty()
    }

    /// Drop celebrations that haven't started and every glyph (Quiet mode,
    /// switched off). Live particles finish (≤ 1.6 s).
    pub fn clear_waiting(&mut self) {
        self.pending.clear();
        self.unanchored.clear();
        self.glyphs.clear();
    }

    fn room(&self) -> usize {
        confetti::MAX_LIVE.saturating_sub(
            self.sparks.live().len() + self.confetti.as_ref().map_or(0, |c| c.live().len()),
        )
    }

    /// Ken's own bursts (combo HUD, K.O.), inside the shared cap.
    pub fn ken_burst(&mut self, x: f32, y: f32, count: usize) {
        let room = self.room();
        self.sparks.burst_within(x, y, count, room);
    }

    /// Start every waiting emission at the bounds its anchor recorded this
    /// frame (the window's center for [`Anchor::Center`]); Milestones
    /// without one move to [`Self::unanchored`]. Returns how many started.
    pub fn resolve(&mut self, width: f32, height: f32) -> usize {
        let mut started = 0;
        for emission in std::mem::take(&mut self.pending) {
            let bounds = match emission.anchor {
                Anchor::Center => Some(Bounds::new(
                    point(px(width / 2.0 - 60.0), px(height / 2.0 - 12.0)),
                    size(px(120.0), px(24.0)),
                )),
                anchor => self
                    .anchors
                    .iter()
                    .rev()
                    .find(|(a, _)| *a == anchor)
                    .map(|(_, b)| *b),
            };
            match bounds {
                Some(bounds) => {
                    self.start(&emission, bounds);
                    started += 1;
                }
                None if emission.fallback.is_some() => self.unanchored.push(emission),
                None => {}
            }
        }
        self.anchors.clear();
        if started > 0 {
            let frame = self.hooks.frames;
            self.hooks.effects_started += started as u64;
            self.hooks.first_effect_frame.get_or_insert(frame);
            self.hooks.last_effect_frame = Some(frame);
        }
        started
    }

    /// One effect at `bounds` (window coordinates).
    pub fn start(&mut self, emission: &Emission, bounds: Bounds<Pixels>) {
        let (left, top) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let room = self.room();
        match (emission.style, emission.tier) {
            (Style::Calm, tier) => {
                let seed = self.seed;
                let confetti = self.confetti.get_or_insert_with(|| Confetti::new(seed));
                let (x, y) = (left + w / 2.0, top);
                match tier {
                    Tier::Step => confetti.puff(x, y, 0.0, room),
                    Tier::Milestone => {
                        confetti.burst(x, y, 0.0, room);
                        self.glows.push(Glow {
                            bounds,
                            age_ms: 0.0,
                        });
                    }
                }
            }
            (Style::Arcade, Tier::Step) => {
                self.sparks
                    .burst_within(left + w / 2.0, top + h / 2.0, STEP_SPARKS, room);
            }
            (Style::Arcade, Tier::Milestone) => {
                self.seal.get_or_insert_with(seal::seal);
                self.stamps.push(Stamp {
                    x: left + w,
                    y: top + h / 2.0,
                    age_ms: 0.0,
                    sparked: false,
                });
                self.glows.push(Glow {
                    bounds,
                    age_ms: 0.0,
                });
            }
        }
    }

    /// Advance everything by `dt_ms` in a `width` × `height` window.
    pub fn step(&mut self, dt_ms: f32, width: f32, height: f32) {
        let dt_ms = dt_ms.clamp(0.0, MAX_DT_MS);
        self.sparks.step(dt_ms);
        let sparks = self.sparks.live().len();
        if let Some(confetti) = &mut self.confetti {
            confetti.step(dt_ms, width, height, sparks);
        }
        let mut seal_sparks = Vec::new();
        for stamp in &mut self.stamps {
            stamp.age_ms += dt_ms;
            if !stamp.sparked && stamp.age_ms >= SEAL_SPARKS_AT {
                stamp.sparked = true;
                seal_sparks.push((stamp.x, stamp.y));
            }
        }
        for (x, y) in seal_sparks {
            let room = self.room();
            self.sparks.burst_within(x, y, SEAL_SPARKS, room);
        }
        self.stamps.retain(|s| s.age_ms < SEAL_END);
        for glow in &mut self.glows {
            glow.age_ms += dt_ms;
        }
        self.glows.retain(|g| g.age_ms < GLOW_MS);
    }

    /// Something animates (or waits to start): frames are needed.
    pub fn is_live(&self) -> bool {
        !self.pending.is_empty()
            || !self.unanchored.is_empty()
            || !self.sparks.is_empty()
            || self.confetti.as_ref().is_some_and(|c| !c.is_idle())
            || !self.stamps.is_empty()
            || !self.glows.is_empty()
    }

    /// Live particles window-wide (confetti + sparks, ≤ 64).
    pub fn live_particles(&self) -> usize {
        self.sparks.live().len() + self.confetti.as_ref().map_or(0, |c| c.live().len())
    }

    /// Heap bytes held by particles and stamps (AC-16 allocation counter).
    pub fn allocated_bytes(&self) -> usize {
        self.sparks.allocated_bytes()
            + self.confetti.as_ref().map_or(0, Confetti::allocated_bytes)
            + self.stamps.capacity() * std::mem::size_of::<Stamp>()
            + self.glows.capacity() * std::mem::size_of::<Glow>()
            + self.seal.as_ref().map_or(0, |s| {
                (s.fill.len() + s.outline.len()) * std::mem::size_of::<Poly>()
            })
    }

    /// The seal geometry was built (it never is before the first arcade
    /// Milestone, and never while celebrations are off).
    pub fn seal_built(&self) -> bool {
        self.seal.is_some()
    }
}

/// Seal stamp at `t` ms: `(scale, opacity)`, `None` outside its life.
pub fn seal_frame(t: f32) -> Option<(f32, f32)> {
    if !(0.0..SEAL_END).contains(&t) {
        return None;
    }
    let spring = SPRING_BOUNCY.position((t / 1000.0).min(SPRING_BOUNCY.duration().as_secs_f32()));
    let scale = 1.6 + (1.0 - 1.6) * spring;
    let opacity = if t < SEAL_HOLD_UNTIL {
        spring.clamp(0.0, 1.0)
    } else {
        1.0 - EASE_IN.eval(((t - SEAL_HOLD_UNTIL) / (SEAL_END - SEAL_HOLD_UNTIL)).clamp(0.0, 1.0))
    };
    Some((scale, opacity))
}

/// Chip glow opacity at `t` ms: 0 → 0.8 (`ease.out`) → 0 (`ease.in`).
pub fn glow_opacity(t: f32) -> f32 {
    let half = GLOW_MS / 2.0;
    if !(0.0..GLOW_MS).contains(&t) {
        0.0
    } else if t < half {
        GLOW_PEAK * EASE_OUT.eval(t / half)
    } else {
        GLOW_PEAK * (1.0 - EASE_IN.eval((t - half) / half))
    }
}

/// The token colors an effect paints with (FR-18).
#[derive(Clone, Copy, Debug)]
pub struct FxColors {
    /// `accent.fg`, `status.success.fg`, `status.info.fg`,
    /// `status.warning.fg`.
    pub confetti: [Hsla; 4],
    pub glow: Hsla,
    pub spark: Hsla,
    pub spark_secondary: Hsla,
    pub seal_fill: Hsla,
    pub seal_outline: Hsla,
}

impl FxColors {
    pub fn of(theme: &adoken_ui::Theme) -> Self {
        let c = &theme.colors;
        Self {
            confetti: [
                c.accent_fg,
                c.status_success_fg,
                c.status_info_fg,
                c.status_warning_fg,
            ],
            glow: c.status_success_fg,
            spark: c.ken_spark,
            spark_secondary: c.ken_secondary,
            seal_fill: c.ken_primary,
            seal_outline: c.ken_outline,
        }
    }
}

/// Paint the whole effect state (window coordinates).
pub fn paint(fx: &Fx, colors: &FxColors, window: &mut Window) {
    for glow in &fx.glows {
        let opacity = glow_opacity(glow.age_ms);
        if opacity <= 0.0 {
            continue;
        }
        let inset = px(GLOW_WIDTH);
        let bounds = Bounds::new(
            point(glow.bounds.origin.x - inset, glow.bounds.origin.y - inset),
            size(
                glow.bounds.size.width + inset * 2.0,
                glow.bounds.size.height + inset * 2.0,
            ),
        );
        window.paint_quad(gpui::quad(
            bounds,
            Corners::all(px(adoken_theme::radius::XS + GLOW_WIDTH)),
            adoken_ui::theme::hsla(adoken_theme::Color::TRANSPARENT),
            Edges::all(px(GLOW_WIDTH)),
            colors.glow.opacity(opacity * colors.glow.a),
            BorderStyle::Solid,
        ));
    }
    if let Some(confetti) = &fx.confetti {
        for piece in confetti.live() {
            let color = colors.confetti[usize::from(piece.color) % 4];
            let color = color.opacity(piece.opacity() * color.a);
            let (x, y) = piece.position();
            let (sin, cos) = piece.angle.to_radians().sin_cos();
            let corners: &[(f32, f32)] = if piece.paper {
                &[(-2.0, -4.0), (2.0, -4.0), (2.0, 4.0), (-2.0, 4.0)]
            } else {
                &DOT
            };
            let points: Vec<_> = corners
                .iter()
                .map(|(cx, cy)| point(px(x + cx * cos - cy * sin), px(y + cx * sin + cy * cos)))
                .collect();
            let mut builder = PathBuilder::fill();
            builder.add_polygon(&points, true);
            if let Ok(path) = builder.build() {
                window.paint_path(path, color);
            }
        }
    }
    if !fx.sparks.is_empty() {
        crate::ken::paint_sparks(
            fx.sparks.live(),
            colors.spark,
            colors.spark_secondary,
            Bounds::default(),
            window,
        );
    }
    if let Some(seal) = &fx.seal {
        for stamp in &fx.stamps {
            if let Some((scale, opacity)) = seal_frame(stamp.age_ms) {
                paint_seal(seal, (stamp.x, stamp.y), scale, opacity, colors, window);
            }
        }
    }
}

/// A 4 px dot as an octagon around the origin.
const DOT: [(f32, f32); 8] = [
    (2.0, 0.0),
    (1.414, 1.414),
    (0.0, 2.0),
    (-1.414, 1.414),
    (-2.0, 0.0),
    (-1.414, -1.414),
    (0.0, -2.0),
    (1.414, -1.414),
];

/// The seal centered on `center`, [`SEAL_PX`] × `scale`, rotated −6°.
pub fn paint_seal(
    seal: &Seal,
    center: (f32, f32),
    scale: f32,
    opacity: f32,
    colors: &FxColors,
    window: &mut Window,
) {
    if opacity <= 0.0 {
        return;
    }
    let unit = SEAL_PX / seal::SIZE * scale;
    let (sin, cos) = SEAL_ROTATE_DEG.to_radians().sin_cos();
    let map = |p: &adoken_ken::glyph::Pt| {
        let (x, y) = ((p.x - 50.0) * unit, (p.y - 50.0) * unit);
        point(
            px(center.0 + x * cos - y * sin),
            px(center.1 + x * sin + y * cos),
        )
    };
    for (polys, color) in [
        (&seal.outline, colors.seal_outline),
        (&seal.fill, colors.seal_fill),
    ] {
        let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()));
        for poly in polys {
            let points: Vec<_> = poly.iter().map(map).collect();
            builder.add_polygon(&points, true);
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color.opacity(opacity * color.a));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip() -> Bounds<Pixels> {
        Bounds::new(point(px(600.0), px(300.0)), size(px(90.0), px(24.0)))
    }

    fn milestone(style: Style) -> Emission {
        Emission {
            anchor: Anchor::WorkItem(42),
            tier: Tier::Milestone,
            style,
            fallback: Some(("Moved to Done".into(), "#42 Fix it".into())),
        }
    }

    #[test]
    fn idle_state_holds_nothing() {
        let mut fx = Fx::new(1);
        for _ in 0..600 {
            fx.step(16.0, 1440.0, 900.0);
        }
        assert!(!fx.is_live());
        assert_eq!(fx.allocated_bytes(), 0);
        assert!(!fx.seal_built());
    }

    #[test]
    fn an_armed_anchor_starts_the_effect_where_it_painted() {
        let mut fx = Fx::new(1);
        fx.queue(milestone(Style::Calm));
        assert!(fx.armed(Anchor::WorkItem(42)));
        assert!(!fx.armed(Anchor::WorkItem(43)));
        fx.record(Anchor::WorkItem(42), chip());
        assert_eq!(fx.resolve(1440.0, 900.0), 1);
        assert!(!fx.armed(Anchor::WorkItem(42)));
        assert_eq!(fx.live_particles(), 24, "the first wave");
        assert!(fx.is_live());
    }

    #[test]
    fn a_milestone_without_its_anchor_falls_back_to_the_toast() {
        let mut fx = Fx::new(1);
        fx.queue(milestone(Style::Calm));
        fx.queue(Emission {
            tier: Tier::Step,
            fallback: None,
            ..milestone(Style::Calm)
        });
        assert_eq!(fx.resolve(1440.0, 900.0), 0);
        assert_eq!(fx.unanchored.len(), 1, "the Step shows nothing extra");
        assert_eq!(fx.live_particles(), 0);
    }

    /// FR-12: every effect ends within 1.6 s; the last live frame is the
    /// one the last particle dies on.
    #[test]
    fn every_style_ends_within_budget() {
        for style in [Style::Calm, Style::Arcade] {
            for tier in [Tier::Step, Tier::Milestone] {
                let mut fx = Fx::new(9);
                fx.queue(Emission {
                    tier,
                    ..milestone(style)
                });
                fx.record(Anchor::WorkItem(42), chip());
                fx.resolve(1440.0, 900.0);
                let mut t = 0.0;
                while fx.is_live() {
                    fx.step(16.0, 1440.0, 900.0);
                    t += 16.0;
                    assert!(fx.live_particles() <= confetti::MAX_LIVE);
                }
                assert!(t <= confetti::MAX_EFFECT_MS, "{style:?} {tier:?}: {t} ms");
            }
        }
    }

    #[test]
    fn arcade_milestone_stamps_then_sparks_at_120_ms() {
        let mut fx = Fx::new(3);
        fx.queue(milestone(Style::Arcade));
        fx.record(Anchor::WorkItem(42), chip());
        fx.resolve(1440.0, 900.0);
        assert!(fx.seal_built());
        assert_eq!(fx.live_particles(), 0);
        fx.step(100.0_f32.min(MAX_DT_MS), 1440.0, 900.0);
        fx.step(50.0, 1440.0, 900.0);
        assert_eq!(fx.live_particles(), 0, "100 ms in, no sparks yet");
        fx.step(30.0, 1440.0, 900.0);
        assert_eq!(fx.live_particles(), SEAL_SPARKS);
    }

    #[test]
    fn seal_and_glow_timelines() {
        let (scale, opacity) = seal_frame(0.0).unwrap();
        assert!((scale - 1.6).abs() < 1e-3 && opacity < 0.05);
        let (scale, opacity) = seal_frame(850.0).unwrap();
        assert!((scale - 1.0).abs() < 0.05 && (opacity - 1.0).abs() < 0.05);
        assert!(seal_frame(SEAL_END - 1.0).unwrap().1 < 0.05);
        assert_eq!(seal_frame(SEAL_END), None);
        assert_eq!(glow_opacity(0.0), 0.0);
        assert!((glow_opacity(GLOW_MS / 2.0) - 0.8).abs() < 1e-3);
        assert_eq!(glow_opacity(GLOW_MS), 0.0);
    }

    #[test]
    fn glyphs_last_exactly_three_seconds() {
        let mut fx = Fx::new(1);
        let t = 1_000_000;
        fx.show_glyph(Anchor::PullRequest(7), t + adoken_ken::celebrate::GLYPH_MS);
        assert!(fx.glyph_live(Anchor::PullRequest(7), t));
        assert!(fx.glyph_live(Anchor::PullRequest(7), t + 2_999));
        assert!(!fx.glyph_live(Anchor::PullRequest(7), t + 3_000));
        assert!(!fx.glyph_live(Anchor::WorkItem(7), t));
        assert!(!fx.prune_glyphs(t + 3_000));
    }
}
