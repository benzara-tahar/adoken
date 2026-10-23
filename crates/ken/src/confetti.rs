//! Calm confetti (ken.md §12.2): paper rectangles (70 %) and dots, colors
//! cycled over four theme tokens, a Step **puff** and a Milestone **burst**
//! in two waves, an apex clamp so pieces stay in the window, and a
//! post-apex flutter. Window-wide, confetti and Ken's sparks share
//! [`MAX_LIVE`]; the caller passes the room left. Deterministic for a seed;
//! allocates nothing until the first burst.

/// Live particles window-wide, confetti and sparks together (spec 0022
/// FR-12). Ken's sparks alone stay ≤ [`crate::sparks::MAX_LIVE`].
pub const MAX_LIVE: usize = 64;
/// Every celebration ends within this (FR-12).
pub const MAX_EFFECT_MS: f32 = 1_600.0;

const GRAVITY: f32 = 900.0;
const PAPER_SHARE: f32 = 0.7;
/// Pieces whose apex would come closer than this to the window top are
/// launched slower (and wider).
const TOP_MARGIN: f32 = 12.0;
const SWAY_PX: f32 = 6.0;
const SWAY_HZ: f32 = 3.0;

/// One emission recipe.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Recipe {
    /// Degrees; −90 is straight up (y grows down).
    cone: (f32, f32),
    speed: (f32, f32),
    drag: f32,
    spin: (f32, f32),
    life_ms: (f32, f32),
    /// Opacity starts falling at this share of life.
    fade_from: f32,
    flutter: bool,
}

const PUFF: Recipe = Recipe {
    cone: (-140.0, -40.0),
    speed: (160.0, 300.0),
    drag: 3.0,
    spin: (360.0, 720.0),
    life_ms: (550.0, 750.0),
    fade_from: 0.65,
    flutter: false,
};

const BURST: Recipe = Recipe {
    cone: (-160.0, -20.0),
    speed: (320.0, 620.0),
    drag: 2.2,
    spin: (360.0, 900.0),
    life_ms: (1_100.0, 1_500.0),
    fade_from: 0.70,
    flutter: true,
};

/// Step puff: 14 pieces.
pub const PUFF_PIECES: usize = 14;
/// Milestone burst: 24 now, 16 at +90 ms.
pub const BURST_WAVES: [(f32, usize); 2] = [(0.0, 24), (90.0, 16)];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    age_ms: f32,
    life_ms: f32,
    fade_from: f32,
    /// Rotation in degrees.
    pub angle: f32,
    spin: f32,
    /// Index into the four confetti colors (`accent.fg`,
    /// `status.success.fg`, `status.info.fg`, `status.warning.fg`).
    pub color: u8,
    /// A 4 × 8 paper rectangle, else a 4 px dot.
    pub paper: bool,
    flutter: bool,
    /// Age at the apex, once past it (flutter phase).
    apex_ms: Option<f32>,
    drag: f32,
}

impl Piece {
    pub fn opacity(&self) -> f32 {
        let p = (self.age_ms / self.life_ms).clamp(0.0, 1.0);
        if p <= self.fade_from {
            0.9
        } else {
            0.9 * (1.0 - (p - self.fade_from) / (1.0 - self.fade_from))
        }
    }

    /// Paint position: the simulated one plus the post-apex sway.
    pub fn position(&self) -> (f32, f32) {
        let sway = match self.apex_ms {
            Some(apex) if self.flutter && self.paper => {
                let t = (self.age_ms - apex) / 1000.0;
                SWAY_PX * (std::f32::consts::TAU * SWAY_HZ * t).sin()
            }
            _ => 0.0,
        };
        (self.x + sway, self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pending {
    in_ms: f32,
    x: f32,
    y: f32,
    top: f32,
    count: usize,
    recipe: Recipe,
}

#[derive(Clone, Debug)]
pub struct Confetti {
    pieces: Vec<Piece>,
    pending: Vec<Pending>,
    next_color: u8,
    state: u64,
}

impl Confetti {
    pub fn new(seed: u64) -> Self {
        Self {
            pieces: Vec::new(),
            pending: Vec::new(),
            next_color: 0,
            state: seed | 1,
        }
    }

    fn next(&mut self) -> f32 {
        // xorshift64* (same generator as the sparks).
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let bits = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
        bits as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, (lo, hi): (f32, f32)) -> f32 {
        lo + (hi - lo) * self.next()
    }

    /// Step: a puff of [`PUFF_PIECES`] from `(x, y)` (the anchor's
    /// top-center); `top` is the window's top edge for the apex clamp;
    /// `room` the live particles still allowed window-wide.
    pub fn puff(&mut self, x: f32, y: f32, top: f32, room: usize) {
        self.emit(x, y, top, PUFF_PIECES.min(room), PUFF);
    }

    /// Milestone: the two-wave burst; the second wave is trimmed to the
    /// room left when it fires (see [`Self::step`]).
    pub fn burst(&mut self, x: f32, y: f32, top: f32, room: usize) {
        let [(_, now), (later_ms, later)] = BURST_WAVES;
        self.emit(x, y, top, now.min(room), BURST);
        self.pending.push(Pending {
            in_ms: later_ms,
            x,
            y,
            top,
            count: later,
            recipe: BURST,
        });
    }

    fn emit(&mut self, x: f32, y: f32, top: f32, count: usize, recipe: Recipe) {
        let max_rise = (y - top - TOP_MARGIN).max(24.0);
        for _ in 0..count {
            let angle = self.between(recipe.cone).to_radians();
            let speed = self.between(recipe.speed);
            let (mut vx, mut vy) = (angle.cos() * speed, angle.sin() * speed);
            // Apex clamp: the drag-free rise vy²/2g bounds the real one.
            let rise = vy * vy / (2.0 * GRAVITY);
            if vy < 0.0 && rise > max_rise {
                let scale = (max_rise / rise).sqrt();
                vy *= scale;
                vx *= (1.0 / scale).min(1.6);
            }
            let spin = self.between(recipe.spin) * if self.next() < 0.5 { -1.0 } else { 1.0 };
            let piece = Piece {
                x,
                y,
                vx,
                vy,
                age_ms: 0.0,
                life_ms: self.between(recipe.life_ms),
                fade_from: recipe.fade_from,
                angle: self.next() * 360.0,
                spin,
                color: self.next_color,
                paper: self.next() < PAPER_SHARE,
                flutter: recipe.flutter,
                apex_ms: None,
                drag: recipe.drag,
            };
            self.next_color = (self.next_color + 1) % 4;
            self.pieces.push(piece);
        }
    }

    /// Advance by `dt_ms` in a `width` × `height` window: fire due waves
    /// (trimmed to `room` minus what's live) and drop dead pieces and
    /// pieces that left the window.
    pub fn step(&mut self, dt_ms: f32, width: f32, height: f32, other_live: usize) {
        let dt = dt_ms / 1000.0;
        for piece in &mut self.pieces {
            let damping = (-piece.drag * dt).exp();
            piece.vx *= damping;
            let was_rising = piece.vy < 0.0;
            piece.vy = piece.vy * damping + GRAVITY * dt;
            piece.x += piece.vx * dt;
            piece.y += piece.vy * dt;
            piece.angle += piece.spin * dt;
            piece.age_ms += dt_ms;
            if was_rising && piece.vy >= 0.0 && piece.apex_ms.is_none() {
                piece.apex_ms = Some(piece.age_ms);
            }
        }
        self.pieces.retain(|p| {
            p.age_ms < p.life_ms && p.x > -16.0 && p.x < width + 16.0 && p.y < height + 16.0
        });
        let mut due = Vec::new();
        self.pending.retain_mut(|wave| {
            wave.in_ms -= dt_ms;
            if wave.in_ms <= 0.0 {
                due.push(*wave);
                false
            } else {
                true
            }
        });
        for wave in due {
            let room = MAX_LIVE.saturating_sub(other_live + self.pieces.len());
            self.emit(wave.x, wave.y, wave.top, wave.count.min(room), wave.recipe);
        }
    }

    pub fn live(&self) -> &[Piece] {
        &self.pieces
    }

    /// Nothing live and nothing waiting: no frame is needed.
    pub fn is_idle(&self) -> bool {
        self.pieces.is_empty() && self.pending.is_empty()
    }

    /// Heap bytes held (the allocation hook of spec 0022 AC-16).
    pub fn allocated_bytes(&self) -> usize {
        self.pieces.capacity() * std::mem::size_of::<Piece>()
            + self.pending.capacity() * std::mem::size_of::<Pending>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f32 = 1440.0;
    const H: f32 = 900.0;

    fn run(confetti: &mut Confetti, ms: f32) {
        let mut t = 0.0;
        while t < ms {
            confetti.step(16.0, W, H, 0);
            t += 16.0;
        }
    }

    #[test]
    fn nothing_is_allocated_before_the_first_burst() {
        let mut confetti = Confetti::new(1);
        assert_eq!(confetti.allocated_bytes(), 0);
        confetti.step(16.0, W, H, 0);
        assert!(confetti.is_idle());
        assert_eq!(confetti.allocated_bytes(), 0);
    }

    #[test]
    fn puff_rises_and_is_gone_within_its_life() {
        let mut confetti = Confetti::new(7);
        confetti.puff(700.0, 600.0, 0.0, MAX_LIVE);
        assert_eq!(confetti.live().len(), PUFF_PIECES);
        confetti.step(100.0, W, H, 0);
        assert!(
            confetti.live().iter().all(|p| p.y < 600.0),
            "the cone is upward"
        );
        run(&mut confetti, 700.0);
        assert!(confetti.is_idle(), "every puff life ≤ 750 ms");
    }

    #[test]
    fn burst_fires_its_second_wave_and_ends_within_budget() {
        let mut confetti = Confetti::new(9);
        confetti.burst(700.0, 600.0, 0.0, MAX_LIVE);
        assert_eq!(confetti.live().len(), 24);
        confetti.step(100.0, W, H, 0);
        assert_eq!(confetti.live().len(), 40);
        run(&mut confetti, MAX_EFFECT_MS);
        assert!(confetti.is_idle());
    }

    #[test]
    fn shared_cap_trims_both_waves() {
        let mut confetti = Confetti::new(3);
        confetti.burst(700.0, 600.0, 0.0, 10);
        assert_eq!(confetti.live().len(), 10);
        // 48 sparks are live when the second wave fires: 64 − 48 − 10 = 6.
        confetti.step(100.0, W, H, 48);
        assert_eq!(confetti.live().len(), 16);
    }

    #[test]
    fn apex_stays_inside_the_window_near_the_top() {
        let mut confetti = Confetti::new(5);
        confetti.burst(700.0, 60.0, 0.0, MAX_LIVE);
        let mut highest = f32::MAX;
        for _ in 0..60 {
            confetti.step(16.0, W, H, 0);
            highest = confetti.live().iter().map(|p| p.y).fold(highest, f32::min);
        }
        assert!(highest >= 0.0, "apex left the window: {highest}");
    }

    #[test]
    fn colors_cycle_and_pieces_fade() {
        let mut confetti = Confetti::new(11);
        confetti.puff(500.0, 500.0, 0.0, MAX_LIVE);
        let colors: Vec<u8> = confetti.live().iter().map(|p| p.color).collect();
        assert_eq!(&colors[..5], &[0, 1, 2, 3, 0]);
        assert!(
            confetti
                .live()
                .iter()
                .all(|p| (p.opacity() - 0.9).abs() < 1e-6)
        );
        confetti.step(600.0, W, H, 0);
        assert!(confetti.live().iter().all(|p| p.opacity() < 0.9));
    }

    #[test]
    fn deterministic_for_a_seed() {
        let mut a = Confetti::new(42);
        let mut b = Confetti::new(42);
        a.burst(10.0, 400.0, 0.0, MAX_LIVE);
        b.burst(10.0, 400.0, 0.0, MAX_LIVE);
        a.step(120.0, W, H, 0);
        b.step(120.0, W, H, 0);
        assert_eq!(a.live(), b.live());
    }
}
