//! Motion tokens as code (docs/design/tokens.md §7): easing curves, springs,
//! the shared repeat clock for spinners, and entrance helpers.
//!
//! Reduce motion: gpui snaps every `with_animation` element to its end state
//! when `App::reduce_motion` is set, and [`pulse_phase`] returns a static 0
//! without scheduling frames, so components built on these helpers need no
//! extra branches.
//!
//! Adapted from zeron `crates/ui/src/motion.rs` at `d1010657` (MIT):
//! [`CubicBezier`] (Newton solve with bisection fallback, clamped output) and
//! the leased ~30 fps repeat clock behind [`pulse_phase`].

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{Animation, App, EntityId, Global};

/// Milliseconds (`adoken_theme::motion::*`) as a `Duration`.
pub fn ms(millis: u32) -> Duration {
    Duration::from_millis(u64::from(millis))
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing function.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl CubicBezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self { x1, y1, x2, y2 }
    }

    fn coefficients(a: f32, b: f32) -> (f32, f32, f32) {
        let c = 3.0 * a;
        let bb = 3.0 * (b - a) - c;
        (1.0 - c - bb, bb, c)
    }

    fn sample(a: f32, b: f32, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(a, b);
        ((a * t + b) * t + c) * t
    }

    fn sample_x_derivative(&self, t: f32) -> f32 {
        let (a, b, c) = Self::coefficients(self.x1, self.x2);
        (3.0 * a * t + 2.0 * b) * t + c
    }

    fn solve_t_for_x(&self, x: f32) -> f32 {
        let mut t = x;
        for _ in 0..8 {
            let err = Self::sample(self.x1, self.x2, t) - x;
            if err.abs() < 1e-6 {
                return t;
            }
            let d = self.sample_x_derivative(t);
            if d.abs() < 1e-6 {
                break;
            }
            t -= err / d;
        }
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            let mid = (lo + hi) / 2.0;
            if Self::sample(self.x1, self.x2, mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (lo + hi) / 2.0
    }

    /// Eased output for progress `x`, both in `[0, 1]` (input clamped, and
    /// output clamped because gpui asserts animation deltas stay in range).
    pub fn eval(&self, x: f32) -> f32 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        Self::sample(self.y1, self.y2, self.solve_t_for_x(x)).clamp(0.0, 1.0)
    }

    pub fn easing(self) -> impl Fn(f32) -> f32 + 'static {
        move |x| self.eval(x)
    }
}

/// `ease.out`: entrances.
pub const EASE_OUT: CubicBezier = CubicBezier::new(0.16, 1.0, 0.3, 1.0);
/// `ease.in`: exits.
pub const EASE_IN: CubicBezier = CubicBezier::new(0.4, 0.0, 1.0, 1.0);
/// `ease.in_out`: moves, resizes, crossfades.
pub const EASE_IN_OUT: CubicBezier = CubicBezier::new(0.65, 0.0, 0.35, 1.0);

/// A damped spring (tokens.md §7.2). GPUI has no spring primitive, so a
/// spring runs as a linear animation over its settle time and the animator
/// maps progress through [`Spring::at`], which may overshoot 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
    /// [`Spring::settling_time`], precomputed so per-frame [`Spring::at`]
    /// calls don't rescan the curve (a unit test keeps it in sync).
    settle_ms: u32,
}

/// Settling threshold: |1 − x| stays below this after the settle time.
const SPRING_EPSILON: f32 = 0.001;

impl Spring {
    const fn new(stiffness: f32, damping: f32, mass: f32, settle_ms: u32) -> Self {
        Self {
            stiffness,
            damping,
            mass,
            settle_ms,
        }
    }

    /// Position at `t` seconds, from 0 at rest toward 1 (underdamped only,
    /// which both tokens are).
    pub fn position(&self, t: f32) -> f32 {
        let omega = (self.stiffness / self.mass).sqrt();
        let zeta = self.damping / (2.0 * (self.stiffness * self.mass).sqrt());
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin())
    }

    /// First time after which the spring stays within [`SPRING_EPSILON`] of
    /// rest (tokens.md §7.2 definition), at 1 ms resolution.
    pub fn settling_time(&self) -> Duration {
        let settled_from = (0..5_000u32)
            .rev()
            .find(|step| (1.0 - self.position(*step as f32 / 1000.0)).abs() >= SPRING_EPSILON)
            .map_or(0, |step| step + 1);
        Duration::from_millis(u64::from(settled_from))
    }

    pub fn duration(&self) -> Duration {
        ms(self.settle_ms)
    }

    /// Value at animation progress `p ∈ [0, 1]` over [`Spring::duration`].
    pub fn at(&self, p: f32) -> f32 {
        if p >= 1.0 {
            return 1.0;
        }
        self.position(p.max(0.0) * self.duration().as_secs_f32())
    }

    /// A linear gpui animation over the settle time; map its delta with
    /// [`Spring::at`].
    pub fn animation(&self) -> Animation {
        Animation::new(self.duration())
    }
}

/// `spring.snappy` (k 520, c 36, m 1): checkmarks, chip insertion, toggle knob.
pub const SPRING_SNAPPY: Spring = Spring::new(520.0, 36.0, 1.0, 368);
/// `spring.bouncy` (k 300, c 14, m 1): Ken moments.
pub const SPRING_BOUNCY: Spring = Spring::new(300.0, 14.0, 1.0, 889);

/// A `duration` animation eased by `curve`.
pub fn animation(duration_ms: u32, curve: CubicBezier) -> Animation {
    Animation::new(ms(duration_ms)).with_easing(curve.easing())
}

/// `[delay, animation]` for `with_animations`: index 0 holds the start
/// state for `delay_ms`, index 1 runs the motion. A zero delay still gets a
/// 1 ms hold so callers can match on the index uniformly.
pub fn delayed(delay_ms: u32, animation: Animation) -> Vec<Animation> {
    vec![Animation::new(ms(delay_ms.max(1))), animation]
}

pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Repeat tick for spinners and shimmer (tokens.md §7.3: repeating motion
/// runs on one shared ~30 fps clock, never at display refresh rate).
const PULSE_TICK: Duration = Duration::from_millis(33);
/// A view stays on the clock this long after its last repeating paint, so
/// an unmounted spinner lets the clock park.
const PULSE_LEASE: Duration = Duration::from_millis(300);

struct PulseClock {
    epoch: Instant,
    leases: HashMap<EntityId, Instant>,
    running: bool,
}

impl Global for PulseClock {}

impl Default for PulseClock {
    fn default() -> Self {
        Self {
            epoch: Instant::now(),
            leases: HashMap::new(),
            running: false,
        }
    }
}

/// Phase in `[0, 1)` of a repeating motion with the given period, shared by
/// every caller so multiple spinners stay in step. Keeps `view` re-rendering
/// at ~30 fps while it keeps painting. Reduce motion: 0, nothing scheduled.
pub fn pulse_phase(period: Duration, view: EntityId, cx: &mut App) -> f32 {
    if cx.reduce_motion() {
        return 0.0;
    }
    let clock = cx.default_global::<PulseClock>();
    clock.leases.insert(view, Instant::now() + PULSE_LEASE);
    let phase = (clock.epoch.elapsed().as_secs_f32() / period.as_secs_f32()).fract();
    if !clock.running {
        clock.running = true;
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(PULSE_TICK).await;
                let parked = cx.update(|cx| {
                    let clock = cx.default_global::<PulseClock>();
                    let now = Instant::now();
                    clock.leases.retain(|_, until| *until > now);
                    if clock.leases.is_empty() {
                        clock.running = false;
                        return true;
                    }
                    let views: Vec<EntityId> = clock.leases.keys().copied().collect();
                    for view in views {
                        cx.notify(view);
                    }
                    false
                });
                if parked {
                    break;
                }
            }
        })
        .detach();
    }
    phase
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32, tolerance: f32) -> bool {
        (actual - expected).abs() <= tolerance
    }

    #[test]
    fn bezier_endpoints_clamp_and_linear_is_identity() {
        for curve in [EASE_OUT, EASE_IN, EASE_IN_OUT] {
            assert_eq!(curve.eval(-1.0), 0.0);
            assert_eq!(curve.eval(0.0), 0.0);
            assert_eq!(curve.eval(1.0), 1.0);
            assert_eq!(curve.eval(2.0), 1.0);
        }
        let linear = CubicBezier::new(0.0, 0.0, 1.0, 1.0);
        for x in [0.1, 0.33, 0.5, 0.9] {
            assert!(close(linear.eval(x), x, 1e-4));
        }
    }

    #[test]
    fn curves_have_their_documented_shape() {
        // ease.out front-loads, ease.in back-loads, ease.in_out is symmetric.
        assert!(EASE_OUT.eval(0.25) > 0.75);
        assert!(EASE_IN.eval(0.25) < 0.1);
        assert!(close(EASE_IN_OUT.eval(0.5), 0.5, 1e-3));
        assert!(close(
            EASE_IN_OUT.eval(0.2) + EASE_IN_OUT.eval(0.8),
            1.0,
            1e-3
        ));
    }

    #[test]
    fn precomputed_spring_durations_match_the_settling_definition() {
        // tokens.md §7.2: the first t after which |1 − x| < 0.001
        // permanently (≈ 368 ms snappy, ≈ 889 ms bouncy).
        for spring in [SPRING_SNAPPY, SPRING_BOUNCY] {
            let computed = spring.settling_time().as_millis() as i64;
            let stored = spring.duration().as_millis() as i64;
            assert!((computed - stored).abs() <= 1, "{computed} vs {stored}");
        }
    }

    #[test]
    fn springs_start_at_rest_settle_at_one_and_bouncy_overshoots() {
        for spring in [SPRING_SNAPPY, SPRING_BOUNCY] {
            assert_eq!(spring.at(0.0), 0.0);
            assert_eq!(spring.at(1.0), 1.0);
            assert!(close(spring.at(0.999), 1.0, SPRING_EPSILON * 2.0));
        }
        let peak = (0..100)
            .map(|i| SPRING_BOUNCY.at(i as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.1, "bouncy peak {peak}");
    }
}
