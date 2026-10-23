//! The built-in sound pack (docs/design/sound.md §7) and pack validation
//! (§6).
//!
//! One pack, Dojo (spec 0025 FR-13): wood, paper and soft bells for every
//! event, except Ken's (`ken.*`), which keep the Arcade chip tones; those
//! only play while Ken's arcade mode is on (see [`crate::decide`]). The
//! pack is synthesized from the recipes below at load time, off the UI
//! thread (the engine's audio thread renders it on first enable). No
//! samples, no third-party audio, no voices; nothing imitates any game.
//! sound.md §7 planned a `tools/soundgen` step baking OGG files at build
//! time; rendering at load keeps the recipes the only source and costs a few
//! milliseconds once. `examples/audition.rs` writes WAVs for auditioning.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use crate::event::{Category, SoundEvent};
use crate::synth::{
    BELL_PARTIALS, Biquad, Buf, SAMPLE_RATE, Wave, bell, db, noise, pink_noise, samples, sine, tone,
};

/// sound.md §5: decoded pack budget.
pub const MAX_PACK_BYTES: usize = 16 * 1024 * 1024;
/// sound.md §6: files per pack.
pub const MAX_CLIPS: usize = 64;
/// sound.md §6 warning threshold.
pub const MAX_PEAK_DBFS: f32 = -1.0;
/// No silence padding at the start (latency): audible within this.
pub const MAX_LEADING_SILENCE: Duration = Duration::from_millis(5);
const SILENCE_THRESHOLD_DB: f32 = -40.0;

/// One playable sound: mono f32 at [`SAMPLE_RATE`].
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub samples: Arc<[f32]>,
    /// Per-file gain (the last factor of the final gain, §1).
    pub gain: f32,
    /// ± fraction of playback rate applied per play, 0.0–0.1.
    pub pitch_jitter: f32,
}

impl Clip {
    pub fn new(samples: Vec<f32>, pitch_jitter: f32) -> Self {
        Self {
            samples: samples.into(),
            gain: 1.0,
            pitch_jitter,
        }
    }

    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples.len() as f64 / SAMPLE_RATE as f64)
    }

    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
    }
}

#[derive(Debug, Clone)]
pub struct Pack {
    /// SPDX id; required for every pack (§6).
    pub license: &'static str,
    clips: HashMap<SoundEvent, Vec<Clip>>,
}

impl Pack {
    /// Synthesize every clip of the built-in pack. Deterministic; takes a
    /// few ms in release, so call it off the UI thread.
    pub fn builtin() -> Self {
        let clips = SoundEvent::ALL
            .iter()
            .map(|&event| {
                let rendered = recipe(event)
                    .into_iter()
                    .map(|buf| Clip::new(master(event, buf), jitter(event)))
                    .collect();
                (event, rendered)
            })
            .collect();
        Self {
            license: "MIT",
            clips,
        }
    }

    /// Variants for `event` (played round-robin); empty if the pack lacks it.
    pub fn clips(&self, event: SoundEvent) -> &[Clip] {
        self.clips.get(&event).map_or(&[], Vec::as_slice)
    }

    pub fn decoded_bytes(&self) -> usize {
        self.clips
            .values()
            .flatten()
            .map(|clip| clip.samples.len() * size_of::<f32>())
            .sum()
    }

    #[cfg(test)]
    fn clips_mut(&mut self) -> &mut HashMap<SoundEvent, Vec<Clip>> {
        &mut self.clips
    }

    /// §6 validation rules that apply to rendered audio.
    pub fn validate(&self) -> Vec<PackIssue> {
        let mut issues = Vec::new();
        let mut push = |event, severity, problem| {
            issues.push(PackIssue {
                event,
                severity,
                problem,
            })
        };
        if self.license.trim().is_empty() {
            push(None, Severity::Error, Problem::MissingLicense);
        }
        let count: usize = self.clips.values().map(Vec::len).sum();
        if count > MAX_CLIPS {
            push(None, Severity::Error, Problem::TooManyClips(count));
        }
        let bytes = self.decoded_bytes();
        if bytes > MAX_PACK_BYTES {
            push(None, Severity::Error, Problem::TooLarge(bytes));
        }
        let silence = db(SILENCE_THRESHOLD_DB);
        let max_lead = samples(MAX_LEADING_SILENCE.as_secs_f32() * 1000.0);
        for &event in SoundEvent::ALL {
            let clips = self.clips(event);
            if clips.is_empty() {
                push(Some(event), Severity::Error, Problem::Missing);
            }
            for clip in clips {
                if clip.samples.is_empty() || clip.samples.iter().any(|s| !s.is_finite()) {
                    push(Some(event), Severity::Error, Problem::Unplayable);
                    continue;
                }
                if clip.duration() > event.max_duration() {
                    push(
                        Some(event),
                        Severity::Error,
                        Problem::TooLong(clip.duration()),
                    );
                }
                let peak = clip.peak() * clip.gain;
                if peak > db(MAX_PEAK_DBFS) {
                    push(
                        Some(event),
                        Severity::Warning,
                        Problem::Hot(20.0 * peak.log10()),
                    );
                }
                let lead = clip.samples.iter().position(|s| s.abs() >= silence);
                match lead {
                    Some(lead) if lead <= max_lead => {}
                    _ => push(Some(event), Severity::Warning, Problem::LeadingSilence),
                }
            }
        }
        issues
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    /// The event falls back (or, pack-wide, the pack is refused).
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackIssue {
    /// `None` for pack-wide problems.
    pub event: Option<SoundEvent>,
    pub severity: Severity,
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    MissingLicense,
    TooManyClips(usize),
    TooLarge(usize),
    Missing,
    Unplayable,
    TooLong(Duration),
    /// Peak in dBFS; the output limiter catches it.
    Hot(f32),
    LeadingSilence,
}

impl fmt::Display for PackIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(event) = self.event {
            write!(f, "{event}: ")?;
        }
        match &self.problem {
            Problem::MissingLicense => f.write_str("pack has no license (SPDX id required)"),
            Problem::TooManyClips(count) => write!(f, "{count} files (max {MAX_CLIPS})"),
            Problem::TooLarge(bytes) => write!(
                f,
                "{:.1} MB decoded (max {} MB)",
                *bytes as f64 / 1_048_576.0,
                MAX_PACK_BYTES / 1_048_576
            ),
            Problem::Missing => f.write_str("no sound"),
            Problem::Unplayable => f.write_str("empty or invalid audio"),
            Problem::TooLong(duration) => write!(
                f,
                "{:.2} s is longer than {:.1} s",
                duration.as_secs_f32(),
                self.event.map_or(0.0, |e| e.max_duration().as_secs_f32())
            ),
            Problem::Hot(peak) => write!(f, "peaks at {peak:.1} dBFS (limit {MAX_PEAK_DBFS} dBFS)"),
            Problem::LeadingSilence => f.write_str("starts with silence (adds latency)"),
        }
    }
}

/// Loudness proxy per category (sound.md §6 authoring guidance: −20 LUFS for
/// notify, −24 for ui), peaks ≤ −3 dBFS, click-free edges.
fn master(event: SoundEvent, buf: Buf) -> Vec<f32> {
    let rms_db = match event.category() {
        Category::Ui => -24.0,
        Category::Notify | Category::Onboarding => -20.0,
        Category::Ken => -19.0,
    };
    let max = event.max_duration().as_secs_f32() * 1000.0;
    let mut buf = buf;
    buf.0.truncate(samples(max));
    buf.fades(2.0, 12.0).master(rms_db, -3.0).0
}

fn jitter(event: SoundEvent) -> f32 {
    use SoundEvent::*;
    match event {
        UiActionDone | KenComboStep => 0.02,
        _ => 0.0,
    }
}

// Note frequencies (equal temperament, A4 = 440 Hz).
const C5: f32 = 523.25;
const E5: f32 = 659.26;
const G5: f32 = 783.99;
const F5: f32 = 698.46;
const A5: f32 = 880.0;
const C6: f32 = 1046.5;
const E6: f32 = 1318.51;
const G6: f32 = 1567.98;

/// Band-passed noise burst: a wood/paper tap.
fn tap(seed: u32, hz: f32, q: f32, decay_ms: f32) -> Buf {
    noise(seed, decay_ms * 5.0, decay_ms).filter(Biquad::band_pass(hz, q))
}

/// Soft marimba: sine plus a 4× partial.
fn marimba(hz: f32, decay_ms: f32) -> Buf {
    let mut out = sine(hz, decay_ms * 5.0, decay_ms);
    out.mix(0.0, &sine(hz * 4.0, decay_ms * 2.0, decay_ms / 3.0), -18.0);
    out
}

fn small_bell(hz: f32, decay_ms: f32) -> Buf {
    bell(hz, &BELL_PARTIALS, (decay_ms * 4.0).min(2_000.0), decay_ms)
}

/// Swelling noise band (paper slide, broom).
fn swish(seed: u32, from_hz: f32, to_hz: f32, ms: f32) -> Buf {
    let len = ms / 1000.0;
    pink_noise(seed, ms)
        .sweep_band(from_hz, to_hz, 1.2)
        .shape(move |t| 0.2 + 0.8 * (std::f32::consts::PI * t / len).sin().powi(2))
}

/// Punchy "tock" (ken.combo_step): pitch drop 1.5× → 1× in 25 ms + click.
fn tock(wave: Wave, seed: u32) -> Buf {
    let mut out = tone(wave, 780.0, 520.0, 25.0, 180.0, 45.0);
    out.mix(0.0, &noise(seed, 8.0, 3.0), -6.0);
    out
}

/// Sparse crackle: random impulses over `ms`.
fn crackle(seed: u32, ms: f32) -> Buf {
    let mut rng = crate::synth::Rng::new(seed);
    let mut out = Buf::silence(ms);
    for slot in &mut out.0 {
        if rng.next_f32() > 0.995 {
            *slot = rng.next_f32();
        }
    }
    out.filter(Biquad::high_pass(1_500.0, 0.7))
}

/// Chip note: pulse (or triangle) with a short decay, low-passed so the
/// harmonics stay friendly.
fn chip(wave: Wave, hz: f32, ms: f32) -> Buf {
    tone(wave, hz, hz, 1.0, ms, ms * 0.8).filter(Biquad::low_pass(7_000.0, 0.7))
}

fn chip_glide(wave: Wave, from: f32, to: f32, ms: f32) -> Buf {
    tone(wave, from, to, ms, ms * 1.2, ms).filter(Biquad::low_pass(7_000.0, 0.7))
}

/// Notes in sequence, `step_ms` apart.
fn arpeggio(notes: &[f32], step_ms: f32, voice: impl Fn(f32) -> Buf) -> Buf {
    let mut out = Buf::default();
    for (i, &note) in notes.iter().enumerate() {
        out.mix(i as f32 * step_ms, &voice(note), 0.0);
    }
    out
}

const PULSE_8: Wave = Wave::Pulse(0.125);
const PULSE_4: Wave = Wave::Pulse(0.25);

/// sound.md §7.1 and §7.3 (Dojo); `ken.*` keep §7.2's Arcade chip tones.
fn recipe(event: SoundEvent) -> Vec<Buf> {
    use SoundEvent::*;
    let one = |buf: Buf| vec![buf];
    match event {
        UiActionDone => {
            let mut out = tap(11, 1_800.0, 6.0, 60.0);
            out.mix(0.0, &sine(1_200.0, 400.0, 80.0), -12.0);
            one(out)
        }
        // sound.md §7.1: two rising wood taps and a small sparkle.
        UiCelebrateStep => {
            let mut out = tap(41, 1_800.0, 6.0, 60.0);
            out.mix(70.0, &tap(42, 2_400.0, 6.0, 60.0), 0.0);
            out.mix(70.0, &sine(2_400.0, 250.0, 150.0), -12.0);
            one(out)
        }
        // Marimba C5-E5-G5-C6, the last note ringing with bell partials,
        // over a soft paper flutter.
        UiCelebrateMilestone => {
            let mut out = arpeggio(&[C5, E5, G5, C6], 60.0, |hz| marimba(hz, 300.0));
            out.mix(
                180.0,
                &bell(C6, &[(2.76, -16.0), (5.40, -24.0)], 1_000.0, 900.0 / 2.5),
                0.0,
            );
            out.mix(0.0, &swish(43, 4_000.0, 4_000.0, 300.0), -24.0);
            one(out)
        }
        UiError => {
            let mut out = tap(12, 700.0, 6.0, 90.0);
            out.mix(90.0, &tap(13, 560.0, 6.0, 90.0), 0.0);
            one(out)
        }
        NotifyMention => [E6, G6]
            .into_iter()
            .map(|second| {
                let mut out = small_bell(C6, 600.0);
                out.mix(110.0, &small_bell(second, 600.0), -3.0);
                out
            })
            .collect(),
        NotifyReviewRequested => one(small_bell(A5, 700.0)),
        NotifyPrComment => one(marimba(G5, 250.0)),
        NotifyPrApproved => one(arpeggio(&[E5, G5], 90.0, |hz| marimba(hz, 250.0))),
        NotifyPrChanges => one(arpeggio(&[G5, E5], 90.0, |hz| marimba(hz, 250.0))),
        NotifyPipelineFailed => {
            let mut out = tap(17, 440.0, 5.0, 110.0);
            out.mix(140.0, &tap(18, 440.0, 5.0, 110.0), -4.0);
            one(out)
        }
        NotifyWorkAssigned => {
            let mut out = pink_noise(19, 150.0).filter(Biquad::band_pass(1_200.0, 1.5));
            let len = 0.150;
            out = out.shape(move |t| 0.2 + 0.8 * (t / len).min(1.0));
            out.mix(150.0, &marimba(F5, 250.0), 0.0);
            one(out)
        }
        NotifyDigest => one(arpeggio(&[C6, E6, G6], 70.0, |hz| small_bell(hz, 500.0))),
        OnboardingComplete => {
            let chord = arpeggio(&[C5, G5, E6], 60.0, |hz| {
                bell(hz, &BELL_PARTIALS, 3_600.0, 2_400.0 / 2.5)
            });
            one(chord.filter(Biquad::low_pass(6_000.0, 0.7)))
        }
        // Arcade chip tones (sound.md §7.2), Ken's events only.
        KenComboStep => one(tock(PULSE_8, 30).filter(Biquad::low_pass(7_000.0, 0.7))),
        KenComboFinish => {
            let mut out = chip_glide(PULSE_4, 400.0, 1_600.0, 60.0);
            out.mix(60.0, &chip(PULSE_4, G6, 600.0), 0.0);
            one(out)
        }
        KenKo => one(ko()),
        KenStreak => {
            let mut out = crackle(31, 250.0).gain(-6.0);
            out.mix(0.0, &chip(PULSE_4, 440.0, 150.0), 0.0);
            out.mix(140.0, &chip(PULSE_4, 660.0, 400.0), 0.0);
            one(out)
        }
        KenFirstStrike => {
            let mut out = pink_noise(32, 80.0).sweep_band(600.0, 3_000.0, 2.0);
            out.mix(60.0, &tock(PULSE_8, 33), 0.0);
            one(out)
        }
        KenCleanSweep => {
            let mut out = swish(34, 2_000.0, 2_000.0, 200.0);
            out.mix(150.0, &chip(PULSE_4, E6, 300.0), 0.0);
            one(out)
        }
    }
}

/// K.O.: low boom + taiko-style hit, then a chip gong after 180 ms.
fn ko() -> Buf {
    let mut out = tone(Wave::Sine, 60.0, 40.0, 400.0, 600.0, 250.0).soft_clip(2.0);
    out.mix(
        0.0,
        &noise(25, 400.0, 150.0).filter(Biquad::low_pass(300.0, 0.7)),
        -2.0,
    );
    let gong = [(330.0, 0.0), (891.0, -6.0), (1_782.0, -12.0)];
    let mut ring = Buf::default();
    for (hz, gain) in gong {
        ring.mix(
            0.0,
            &tone(PULSE_4, hz, hz, 1.0, 2_600.0, 1_800.0 / 2.5),
            gain,
        );
    }
    out.mix(180.0, &ring.filter(Biquad::low_pass(5_000.0, 0.7)), -4.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec 0007 FR-11: the built-in pack covers every event and passes
    /// every §6 rule.
    #[test]
    fn builtin_pack_validates_clean() {
        let pack = Pack::builtin();
        let issues = pack.validate();
        let text: Vec<String> = issues.iter().map(ToString::to_string).collect();
        assert!(issues.is_empty(), "{text:#?}");
        assert!(pack.decoded_bytes() <= MAX_PACK_BYTES);
        for &event in SoundEvent::ALL {
            assert!(!pack.clips(event).is_empty(), "{event}");
        }
    }

    /// Spec 0022 AC-19: both celebration sounds exist, stay within 2 s, and
    /// differ from the plain `ui.action_done` they replace.
    #[test]
    fn celebration_sounds_fit_their_budget() {
        let pack = Pack::builtin();
        let done = pack.clips(SoundEvent::UiActionDone);
        for event in [
            SoundEvent::UiCelebrateStep,
            SoundEvent::UiCelebrateMilestone,
        ] {
            let clips = pack.clips(event);
            assert_eq!(clips.len(), 1, "{event}");
            let seconds = clips[0].samples.len() as f32 / SAMPLE_RATE as f32;
            assert!(seconds <= 2.0, "{event} lasts {seconds} s");
            assert_ne!(clips, done, "{event}");
        }
        let step = pack.clips(SoundEvent::UiCelebrateStep)[0].samples.len();
        let milestone = pack.clips(SoundEvent::UiCelebrateMilestone)[0]
            .samples
            .len();
        assert!(milestone > step, "the Milestone rings longer");
    }

    #[test]
    fn pack_is_deterministic() {
        let a = Pack::builtin();
        let b = Pack::builtin();
        for &event in SoundEvent::ALL {
            assert_eq!(a.clips(event), b.clips(event), "{event}");
        }
        // Mention has two variants for round-robin (§7.1).
        assert_eq!(a.clips(SoundEvent::NotifyMention).len(), 2);
    }

    #[test]
    fn validation_catches_broken_clips() {
        let mut pack = Pack::builtin();
        let clips = pack.clips_mut();
        clips.remove(&SoundEvent::NotifyDigest);
        // 2.5 s of full-scale tone: too long for a ui event and too hot.
        clips.insert(
            SoundEvent::UiError,
            vec![Clip::new(vec![1.0; 120_000], 0.0)],
        );
        // 50 ms of silence before the sound.
        let mut padded = vec![0.0; samples(50.0)];
        padded.extend(std::iter::repeat_n(0.2, 1_000));
        clips.insert(SoundEvent::NotifyPrComment, vec![Clip::new(padded, 0.0)]);
        clips.insert(
            SoundEvent::NotifyReviewRequested,
            vec![Clip::new(vec![f32::NAN; 10], 0.0)],
        );

        let issues = pack.validate();
        let find = |event| {
            issues
                .iter()
                .filter(move |issue| issue.event == Some(event))
                .map(|issue| (issue.severity, issue.problem.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            find(SoundEvent::NotifyDigest),
            [(Severity::Error, Problem::Missing)]
        );
        let error = find(SoundEvent::UiError);
        assert!(
            matches!(error[0], (Severity::Error, Problem::TooLong(_))),
            "{error:?}"
        );
        assert!(matches!(error[1], (Severity::Warning, Problem::Hot(peak)) if peak.abs() < 0.01));
        assert_eq!(
            find(SoundEvent::NotifyPrComment),
            [(Severity::Warning, Problem::LeadingSilence)]
        );
        assert_eq!(
            find(SoundEvent::NotifyReviewRequested),
            [(Severity::Error, Problem::Unplayable)]
        );
        assert_eq!(issues.len(), 5, "{issues:#?}");
    }

    #[test]
    fn pack_wide_limits() {
        let mut pack = Pack::builtin();
        pack.license = " ";
        let clips = pack.clips_mut();
        let long = Clip::new(vec![0.1; 48_000], 0.0);
        clips.insert(SoundEvent::UiActionDone, vec![long; 90]);
        let problems: Vec<_> = pack
            .validate()
            .into_iter()
            .filter(|issue| issue.event.is_none())
            .map(|issue| issue.problem)
            .collect();
        assert!(problems.contains(&Problem::MissingLicense));
        assert!(
            problems
                .iter()
                .any(|p| matches!(p, Problem::TooManyClips(_)))
        );
        assert!(problems.iter().any(|p| matches!(p, Problem::TooLarge(_))));
    }
}
