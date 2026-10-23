//! The mix bus (sound.md §5): at most [`MAX_VOICES`] voices, priority voice
//! stealing, `ken.ko` ducking with 20 ms ramps, and a peak limiter keeping
//! output ≤ −1 dBFS. Pure sample math, driven by the audio callback through
//! [`crate::output`]; no locks, no allocation per sample.

use std::sync::Arc;

use crate::synth::{SAMPLE_RATE, db};

pub const MAX_VOICES: usize = 4;
const DUCK_DB: f32 = -6.0;
const RAMP_SECONDS: f32 = 0.020;
/// −1 dBFS.
const CEILING: f32 = 0.891;
/// The limiter starts bending the curve here (−3 dBFS).
const KNEE: f32 = 0.708;

/// A sound ready to mix.
#[derive(Debug, Clone)]
pub struct Voice {
    pub samples: Arc<[f32]>,
    /// Final linear gain (settings × clip).
    pub gain: f32,
    /// Playback rate (pitch jitter, combo steps); 1.0 = as rendered.
    pub rate: f32,
    pub priority: u8,
    /// Attenuates the other voices while it plays (`ken.ko`).
    pub ducks: bool,
}

#[derive(Debug)]
struct Active {
    voice: Voice,
    /// Fractional read position in samples.
    position: f64,
    /// Arrival order, for "steal the oldest".
    serial: u64,
}

#[derive(Debug, Default)]
pub struct Mixer {
    active: Vec<Active>,
    serial: u64,
    /// Current gain of non-ducking voices (ramps toward 1.0 or −6 dB).
    duck: f32,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            active: Vec::with_capacity(MAX_VOICES),
            serial: 0,
            duck: 1.0,
        }
    }

    /// Start `voice`. When all voices are busy, steal the oldest voice of the
    /// lowest priority not above the new one; otherwise drop the new sound.
    pub fn start(&mut self, voice: Voice) -> bool {
        if self.active.len() >= MAX_VOICES {
            let victim = self
                .active
                .iter()
                .enumerate()
                .filter(|(_, active)| active.voice.priority <= voice.priority)
                .min_by_key(|(_, active)| (active.voice.priority, active.serial))
                .map(|(index, _)| index);
            match victim {
                Some(index) => {
                    self.active.swap_remove(index);
                }
                None => return false,
            }
        }
        self.serial += 1;
        self.active.push(Active {
            voice,
            position: 0.0,
            serial: self.serial,
        });
        true
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_empty()
    }

    /// Priorities of the voices now playing, oldest first.
    pub fn priorities(&self) -> Vec<u8> {
        let mut active: Vec<_> = self.active.iter().collect();
        active.sort_by_key(|active| active.serial);
        active.iter().map(|active| active.voice.priority).collect()
    }

    /// One output sample.
    pub fn next_sample(&mut self) -> f32 {
        if self.active.is_empty() {
            self.duck = 1.0;
            return 0.0;
        }
        let ducking = self.active.iter().any(|active| active.voice.ducks);
        let target = if ducking { db(DUCK_DB) } else { 1.0 };
        let step = 1.0 / (RAMP_SECONDS * SAMPLE_RATE as f32);
        self.duck = if self.duck < target {
            (self.duck + step).min(target)
        } else {
            (self.duck - step).max(target)
        };

        let mut sum = 0.0;
        let duck = self.duck;
        self.active.retain_mut(|active| {
            let samples = &active.voice.samples;
            let index = active.position as usize;
            if index + 1 >= samples.len() {
                return false;
            }
            let frac = (active.position - index as f64) as f32;
            let value = samples[index] + (samples[index + 1] - samples[index]) * frac;
            let gain = if active.voice.ducks { 1.0 } else { duck };
            sum += value * active.voice.gain * gain;
            active.position += active.voice.rate as f64;
            true
        });
        limit(sum)
    }
}

/// Transparent below the knee, then a tanh curve that never exceeds the
/// ceiling.
fn limit(x: f32) -> f32 {
    let magnitude = x.abs();
    if magnitude <= KNEE {
        return x;
    }
    let range = CEILING - KNEE;
    let bent = KNEE + range * ((magnitude - KNEE) / range).tanh();
    bent.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(priority: u8, value: f32, len: usize) -> Voice {
        Voice {
            samples: vec![value; len].into(),
            gain: 1.0,
            rate: 1.0,
            priority,
            ducks: false,
        }
    }

    #[test]
    fn full_mixer_steals_oldest_lowest_priority_never_higher() {
        let mut mixer = Mixer::new();
        for priority in [2, 1, 3, 1] {
            assert!(mixer.start(voice(priority, 0.0, 100)));
        }
        // The older of the two priority-1 voices goes.
        assert!(mixer.start(voice(2, 0.0, 100)));
        assert_eq!(mixer.priorities(), [2, 3, 1, 2]);
        // A priority-0 sound can't steal anything: dropped.
        assert!(!mixer.start(voice(0, 0.0, 100)));
        assert_eq!(mixer.priorities(), [2, 3, 1, 2]);
    }

    #[test]
    fn voices_end_and_the_bus_goes_idle() {
        let mut mixer = Mixer::new();
        mixer.start(voice(1, 0.1, 10));
        let out: Vec<f32> = (0..20).map(|_| mixer.next_sample()).collect();
        assert!(out[..9].iter().all(|s| (*s - 0.1).abs() < 1e-6));
        assert!(mixer.is_idle());
        assert_eq!(out[19], 0.0);
    }

    #[test]
    fn ko_ducks_others_by_6_db_with_a_ramp() {
        let mut mixer = Mixer::new();
        mixer.start(voice(2, 0.1, 10_000));
        mixer.start(Voice {
            ducks: true,
            ..voice(3, 0.0, 10_000)
        });
        let first = mixer.next_sample();
        assert!(first > 0.099, "ramps, doesn't jump: {first}");
        let ramp = (RAMP_SECONDS * SAMPLE_RATE as f32) as usize;
        let mut last = 0.0;
        for _ in 0..ramp + 10 {
            last = mixer.next_sample();
        }
        assert!((last - 0.1 * db(-6.0)).abs() < 1e-4, "{last}");
    }

    #[test]
    fn limiter_keeps_peaks_under_minus_one_dbfs() {
        let mut mixer = Mixer::new();
        for _ in 0..4 {
            mixer.start(voice(1, 0.9, 100));
        }
        let peak = (0..50)
            .map(|_| mixer.next_sample().abs())
            .fold(0.0, f32::max);
        assert!(peak <= CEILING, "{peak}");
        assert_eq!(limit(0.5), 0.5);
        assert_eq!(limit(-0.5), -0.5);
    }

    #[test]
    fn rate_changes_pitch_by_resampling() {
        let ramp: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let mut mixer = Mixer::new();
        mixer.start(Voice {
            rate: 2.0,
            ..voice(1, 0.0, 0)
        });
        mixer.active[0].voice.samples = ramp.into();
        let out: Vec<f32> = (0..3).map(|_| mixer.next_sample()).collect();
        assert_eq!(out, [0.0, 0.02, 0.04]);
    }
}
