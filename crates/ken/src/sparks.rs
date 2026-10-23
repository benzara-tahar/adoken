//! Spark particles (ken.md §4.4): random angle, 180–420 px/s, drag 3.0/s,
//! gravity 600 px/s², 450–650 ms life, fading over the last 40 %, scale
//! 1 → 0.6; at most [`MAX_LIVE`] window-wide. Stepped on the shared 30 fps
//! clock; the painter interpolates. Deterministic for a seed.

/// Live particles allowed window-wide.
pub const MAX_LIVE: usize = 48;
const SPEED: (f32, f32) = (180.0, 420.0);
const DRAG: f32 = 3.0;
const GRAVITY: f32 = 600.0;
const LIFE_MS: (f32, f32) = (450.0, 650.0);
/// Share of particles in `ken.spark` (the rest `ken.secondary`).
const SPARK_SHARE: f32 = 0.7;
/// Share drawn as 4-point stars (the rest 3 px dots).
const STAR_SHARE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    age_ms: f32,
    life_ms: f32,
    /// Star (8 px) or dot (3 px).
    pub star: bool,
    /// `ken.spark` or `ken.secondary`.
    pub spark_color: bool,
}

impl Particle {
    fn progress(&self) -> f32 {
        (self.age_ms / self.life_ms).clamp(0.0, 1.0)
    }

    /// 1 until 60 % of life, then linearly to 0.
    pub fn opacity(&self) -> f32 {
        let p = self.progress();
        if p <= 0.6 { 1.0 } else { (1.0 - p) / 0.4 }
    }

    pub fn scale(&self) -> f32 {
        1.0 - 0.4 * self.progress()
    }
}

#[derive(Clone, Debug)]
pub struct Sparks {
    particles: Vec<Particle>,
    state: u64,
}

impl Sparks {
    /// Allocates nothing until the first burst (spec 0022 FR-12).
    pub fn new(seed: u64) -> Self {
        Self {
            particles: Vec::new(),
            state: seed | 1,
        }
    }

    fn next(&mut self) -> f32 {
        // xorshift64*
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let bits = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        bits as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.next()
    }

    /// Emit up to `count` particles at `(x, y)`; the budget caps the rest.
    pub fn burst(&mut self, x: f32, y: f32, count: usize) {
        self.burst_within(x, y, count, usize::MAX);
    }

    /// [`Self::burst`] further trimmed to `room`, the particles still
    /// allowed window-wide when confetti shares the cap
    /// ([`crate::confetti::MAX_LIVE`]).
    pub fn burst_within(&mut self, x: f32, y: f32, count: usize, room: usize) {
        let room = (MAX_LIVE - self.particles.len()).min(room);
        for _ in 0..count.min(room) {
            let angle = self.next() * std::f32::consts::TAU;
            let speed = self.between(SPEED);
            let life_ms = self.between(LIFE_MS);
            let star = self.next() < STAR_SHARE;
            let spark_color = self.next() < SPARK_SHARE;
            self.particles.push(Particle {
                x,
                y,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed,
                age_ms: 0.0,
                life_ms,
                star,
                spark_color,
            });
        }
    }

    /// Advance by `dt_ms` and drop dead particles.
    pub fn step(&mut self, dt_ms: f32) {
        let dt = dt_ms / 1000.0;
        let damping = (-DRAG * dt).exp();
        for p in &mut self.particles {
            p.vx *= damping;
            p.vy = p.vy * damping + GRAVITY * dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.age_ms += dt_ms;
        }
        self.particles.retain(|p| p.age_ms < p.life_ms);
    }

    pub fn live(&self) -> &[Particle] {
        &self.particles
    }

    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Heap bytes held (the allocation hook of spec 0022 AC-16).
    pub fn allocated_bytes(&self) -> usize {
        self.particles.capacity() * std::mem::size_of::<Particle>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_caps_live_particles() {
        let mut sparks = Sparks::new(7);
        sparks.burst(0.0, 0.0, 24);
        sparks.burst(0.0, 0.0, 40);
        assert_eq!(sparks.live().len(), MAX_LIVE);
    }

    #[test]
    fn particles_fly_fall_fade_and_die_within_their_life() {
        let mut sparks = Sparks::new(42);
        sparks.burst(100.0, 100.0, 12);
        let start: Vec<_> = sparks.live().to_vec();
        sparks.step(33.0);
        for (before, after) in start.iter().zip(sparks.live()) {
            assert!(after.x != before.x || after.y != before.y);
            assert_eq!(after.opacity(), 1.0);
        }
        sparks.step(33.0 * 12.0); // ~430 ms: still alive, fading soon
        assert!(!sparks.is_empty());
        for p in sparks.live() {
            assert!(p.scale() < 1.0 && p.scale() >= 0.6);
        }
        sparks.step(250.0);
        assert!(sparks.is_empty(), "every life ≤ 650 ms");
    }

    #[test]
    fn deterministic_for_a_seed() {
        let mut a = Sparks::new(3);
        let mut b = Sparks::new(3);
        a.burst(0.0, 0.0, 8);
        b.burst(0.0, 0.0, 8);
        a.step(33.0);
        b.step(33.0);
        assert_eq!(a.live(), b.live());
    }

    #[test]
    fn color_and_shape_mix_roughly_matches_the_spec() {
        let mut sparks = Sparks::new(11);
        let mut spark_color = 0;
        for _ in 0..20 {
            sparks.burst(0.0, 0.0, MAX_LIVE);
            spark_color += sparks.live().iter().filter(|p| p.spark_color).count();
            sparks.step(1_000.0);
        }
        let share = spark_color as f32 / (20 * MAX_LIVE) as f32;
        assert!((0.6..0.8).contains(&share), "{share}");
    }
}
