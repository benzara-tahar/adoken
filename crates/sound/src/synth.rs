//! Tiny deterministic synthesizer for the built-in pack: oscillators,
//! noise, RBJ biquads, envelopes. Mono f32 at [`SAMPLE_RATE`]. Everything is
//! computed from recipes (sound.md §7), so the pack ships no audio files.

use std::f32::consts::TAU;

pub const SAMPLE_RATE: u32 = 48_000;
const SR: f32 = SAMPLE_RATE as f32;

pub fn samples(ms: f32) -> usize {
    (ms * SR / 1000.0).round().max(0.0) as usize
}

pub fn db(gain_db: f32) -> f32 {
    10f32.powf(gain_db / 20.0)
}

/// A mono buffer being built.
#[derive(Debug, Clone, Default)]
pub struct Buf(pub Vec<f32>);

impl Buf {
    pub fn silence(ms: f32) -> Self {
        Self(vec![0.0; samples(ms)])
    }

    /// Mix `other` in at `offset_ms` with `gain_db`, growing as needed.
    pub fn mix(&mut self, offset_ms: f32, other: &Buf, gain_db: f32) -> &mut Self {
        let offset = samples(offset_ms);
        let gain = db(gain_db);
        if self.0.len() < offset + other.0.len() {
            self.0.resize(offset + other.0.len(), 0.0);
        }
        for (slot, sample) in self.0[offset..].iter_mut().zip(&other.0) {
            *slot += sample * gain;
        }
        self
    }

    pub fn gain(mut self, gain_db: f32) -> Self {
        let gain = db(gain_db);
        self.0.iter_mut().for_each(|s| *s *= gain);
        self
    }

    /// Multiply by `env(t_seconds)`.
    pub fn shape(mut self, env: impl Fn(f32) -> f32) -> Self {
        for (i, s) in self.0.iter_mut().enumerate() {
            *s *= env(i as f32 / SR);
        }
        self
    }

    /// Linear fade-in (click-free onset) and fade-out.
    pub fn fades(self, in_ms: f32, out_ms: f32) -> Self {
        let len = self.0.len() as f32 / SR;
        let (fin, fout) = (in_ms / 1000.0, out_ms / 1000.0);
        self.shape(move |t| {
            let a = if fin > 0.0 { (t / fin).min(1.0) } else { 1.0 };
            let b = if fout > 0.0 {
                ((len - t) / fout).clamp(0.0, 1.0)
            } else {
                1.0
            };
            a * b
        })
    }

    pub fn filter(mut self, mut filter: Biquad) -> Self {
        for s in &mut self.0 {
            *s = filter.process(*s);
        }
        self
    }

    /// Band-pass whose center moves `from_hz` → `to_hz` (exponentially)
    /// across the buffer.
    pub fn sweep_band(mut self, from_hz: f32, to_hz: f32, q: f32) -> Self {
        let len = self.0.len().max(1) as f32;
        let mut filter = Biquad::band_pass(from_hz, q);
        for (i, s) in self.0.iter_mut().enumerate() {
            if i % 32 == 0 {
                let p = i as f32 / len;
                filter.retune_band_pass(from_hz * (to_hz / from_hz).powf(p), q);
            }
            *s = filter.process(*s);
        }
        self
    }

    /// Gentle tanh saturation.
    pub fn soft_clip(mut self, drive: f32) -> Self {
        let norm = drive.tanh();
        self.0
            .iter_mut()
            .for_each(|s| *s = (*s * drive).tanh() / norm);
        self
    }

    pub fn peak(&self) -> f32 {
        self.0.iter().fold(0.0, |peak, s| peak.max(s.abs()))
    }

    pub fn rms(&self) -> f32 {
        if self.0.is_empty() {
            return 0.0;
        }
        (self.0.iter().map(|s| s * s).sum::<f32>() / self.0.len() as f32).sqrt()
    }

    /// Scale toward `rms_db` (a loudness proxy for sound.md's LUFS targets),
    /// but never let the peak exceed `peak_db`.
    pub fn master(self, rms_db: f32, peak_db: f32) -> Self {
        let (rms, peak) = (self.rms(), self.peak());
        if rms <= 0.0 || peak <= 0.0 {
            return self;
        }
        let gain = (db(rms_db) / rms).min(db(peak_db) / peak);
        Self(self.0.into_iter().map(|s| s * gain).collect())
    }
}

/// Deterministic xorshift noise (seeded per recipe so the pack is identical
/// on every machine).
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Oscillator shapes.
#[derive(Debug, Clone, Copy)]
pub enum Wave {
    Sine,
    /// Pulse with the given duty cycle (0.125, 0.25).
    Pulse(f32),
}

impl Wave {
    fn at(self, phase: f32) -> f32 {
        match self {
            Self::Sine => (phase * TAU).sin(),
            Self::Pulse(duty) => {
                if phase < duty {
                    1.0
                } else {
                    -1.0
                }
            }
        }
    }
}

/// A tone gliding from `from_hz` to `to_hz` (exponential) over
/// `glide_ms`, lasting `ms`, with exponential decay time-constant
/// `decay_ms` and a 1 ms attack.
pub fn tone(wave: Wave, from_hz: f32, to_hz: f32, glide_ms: f32, ms: f32, decay_ms: f32) -> Buf {
    let n = samples(ms);
    let glide = (glide_ms / 1000.0).max(1e-6);
    let tau = decay_ms / 1000.0;
    let mut phase = 0.0f32;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / SR;
        let p = (t / glide).min(1.0);
        let freq = from_hz * (to_hz / from_hz).powf(p);
        phase = (phase + freq / SR).fract();
        let attack = (t / 0.001).min(1.0);
        let decay = if tau > 0.0 { (-t / tau).exp() } else { 1.0 };
        out.push(wave.at(phase) * attack * decay);
    }
    Buf(out)
}

pub fn sine(hz: f32, ms: f32, decay_ms: f32) -> Buf {
    tone(Wave::Sine, hz, hz, 1.0, ms, decay_ms)
}

/// Bell from inharmonic partials `(ratio, gain_db)`.
pub fn bell(hz: f32, partials: &[(f32, f32)], ms: f32, decay_ms: f32) -> Buf {
    let mut out = Buf::silence(ms);
    for &(ratio, gain_db) in partials {
        // Higher partials die faster, as in a struck bell.
        let partial = sine(hz * ratio, ms, decay_ms / ratio.sqrt());
        out.mix(0.0, &partial, gain_db);
    }
    out
}

/// sound.md §7.1 small-bell ratios.
pub const BELL_PARTIALS: [(f32, f32); 3] = [(1.0, 0.0), (2.76, -14.0), (5.40, -22.0)];

/// White noise with exponential decay (`decay_ms` time constant).
pub fn noise(seed: u32, ms: f32, decay_ms: f32) -> Buf {
    let mut rng = Rng::new(seed);
    let tau = decay_ms / 1000.0;
    Buf((0..samples(ms))
        .map(|i| {
            let t = i as f32 / SR;
            let attack = (t / 0.001).min(1.0);
            rng.next_f32() * attack * if tau > 0.0 { (-t / tau).exp() } else { 1.0 }
        })
        .collect())
}

/// Pink-ish noise (Paul Kellet's economy filter), unit-ish level.
pub fn pink_noise(seed: u32, ms: f32) -> Buf {
    let mut rng = Rng::new(seed);
    let (mut b0, mut b1, mut b2) = (0.0f32, 0.0f32, 0.0f32);
    Buf((0..samples(ms))
        .map(|_| {
            let white = rng.next_f32();
            b0 = 0.99765 * b0 + white * 0.0990460;
            b1 = 0.96300 * b1 + white * 0.2965164;
            b2 = 0.57000 * b2 + white * 1.0526913;
            (b0 + b1 + b2 + white * 0.1848) * 0.25
        })
        .collect())
}

/// RBJ cookbook biquad (direct form I).
#[derive(Debug, Clone, Copy)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn from_coefficients(b: [f32; 3], a: [f32; 3]) -> Self {
        Self {
            b0: b[0] / a[0],
            b1: b[1] / a[0],
            b2: b[2] / a[0],
            a1: a[1] / a[0],
            a2: a[2] / a[0],
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn omega(hz: f32, q: f32) -> (f32, f32) {
        let w = TAU * (hz / SR).min(0.49);
        (w.cos(), w.sin() / (2.0 * q))
    }

    /// Constant 0 dB peak gain band-pass.
    pub fn band_pass(hz: f32, q: f32) -> Self {
        let (cos, alpha) = Self::omega(hz, q);
        Self::from_coefficients([alpha, 0.0, -alpha], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    }

    fn retune_band_pass(&mut self, hz: f32, q: f32) {
        let state = (self.x1, self.x2, self.y1, self.y2);
        *self = Self::band_pass(hz, q);
        (self.x1, self.x2, self.y1, self.y2) = state;
    }

    pub fn low_pass(hz: f32, q: f32) -> Self {
        let (cos, alpha) = Self::omega(hz, q);
        let b = (1.0 - cos) / 2.0;
        Self::from_coefficients([b, 1.0 - cos, b], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    }

    pub fn high_pass(hz: f32, q: f32) -> Self {
        let (cos, alpha) = Self::omega(hz, q);
        let b = (1.0 + cos) / 2.0;
        Self::from_coefficients([b, -(1.0 + cos), b], [1.0 + alpha, -2.0 * cos, 1.0 - alpha])
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        (self.x2, self.x1, self.y2, self.y1) = (self.x1, x, self.y1, y);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_deterministic_and_bounded() {
        let a = noise(7, 50.0, 20.0);
        let b = noise(7, 50.0, 20.0);
        assert_eq!(a.0, b.0);
        assert_ne!(a.0, noise(8, 50.0, 20.0).0);
        assert!(a.peak() <= 1.0);
    }

    #[test]
    fn band_pass_keeps_its_band_and_rejects_far_tones() {
        let pass = sine(1_800.0, 200.0, 0.0).filter(Biquad::band_pass(1_800.0, 6.0));
        let reject = sine(200.0, 200.0, 0.0).filter(Biquad::band_pass(1_800.0, 6.0));
        assert!(pass.rms() > 0.5, "{}", pass.rms());
        assert!(reject.rms() < 0.05, "{}", reject.rms());
    }

    #[test]
    fn master_hits_the_rms_target_without_exceeding_the_peak() {
        let loud = sine(440.0, 200.0, 0.0).gain(-30.0).master(-20.0, -3.0);
        assert!((20.0 * loud.rms().log10() + 20.0).abs() < 0.1);
        let spiky = noise(1, 200.0, 5.0).master(-6.0, -3.0);
        assert!(spiky.peak() <= db(-3.0) + 1e-4);
    }

    #[test]
    fn mix_grows_and_offsets() {
        let mut out = Buf::silence(10.0);
        out.mix(20.0, &sine(1_000.0, 10.0, 0.0), 0.0);
        assert_eq!(out.0.len(), samples(30.0));
        assert!(out.0[..samples(20.0)].iter().all(|s| *s == 0.0));
    }
}
