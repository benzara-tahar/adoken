//! [`SoundEngine`]: the one handle the app and the notification center use.
//!
//! `play` runs on the caller's thread and only gates and schedules (a mutex
//! and a few comparisons, no IO). Rendering packs and talking to the audio
//! device happen on one lazily spawned `adoken-sound` thread:
//!
//! - sounds disabled → no thread, no device, no pack memory (§1);
//! - first enable → thread starts and renders the pack (preload, §5);
//! - first play → device opens; [`crate::output::IDLE_CLOSE`] of silence
//!   closes it; disabling stops the thread and frees everything.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};

use parking_lot::{Mutex, MutexGuard};
use std::time::Instant;

use crate::event::SoundEvent;
use crate::gate::{PlayContext, Silenced, decide};
use crate::mixer::Voice;
use crate::output::{DeviceOutput, Output};
use crate::pack::Pack;
use crate::schedule::Scheduler;
use crate::settings::SoundSettings;
use crate::synth::Rng;

/// `ken.combo_step` rises one semitone per combo level, capped (sound.md §2.3).
pub const MAX_COMBO_SEMITONES: u32 = 7;

type MakeOutput = Arc<dyn Fn() -> Box<dyn Output> + Send + Sync>;

#[derive(Clone)]
pub struct SoundEngine {
    state: Arc<Mutex<State>>,
}

struct State {
    settings: SoundSettings,
    scheduler: Scheduler,
    worker: Option<Sender<Command>>,
    make_output: MakeOutput,
}

enum Command {
    Preload,
    Play {
        event: SoundEvent,
        gain: f32,
        semitones: u32,
    },
}

impl SoundEngine {
    /// Cheap: allocates nothing audio-related until sounds are enabled.
    pub fn new(settings: SoundSettings) -> Self {
        Self::with_output(settings, || Box::new(DeviceOutput::default()))
    }

    /// Use a custom output (tests, offline rendering).
    pub fn with_output(
        settings: SoundSettings,
        make_output: impl Fn() -> Box<dyn Output> + Send + Sync + 'static,
    ) -> Self {
        let engine = Self {
            state: Arc::new(Mutex::new(State {
                settings: SoundSettings::default(),
                scheduler: Scheduler::default(),
                worker: None,
                make_output: Arc::new(make_output),
            })),
        };
        engine.set_settings(settings);
        engine
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock()
    }

    pub fn settings(&self) -> SoundSettings {
        self.lock().settings.clone()
    }

    /// Apply new settings (next play uses them, no restart). Enabling
    /// preloads the pack; disabling releases the device and pack memory.
    pub fn set_settings(&self, settings: SoundSettings) {
        let mut state = self.lock();
        let enabled = settings.enabled;
        state.settings = settings;
        if enabled {
            state.send(Command::Preload);
        } else {
            // Dropping the sender ends the worker, which drops the device.
            state.worker = None;
        }
    }

    /// The audio thread is running (for tests and diagnostics).
    pub fn is_started(&self) -> bool {
        self.lock().worker.is_some()
    }

    /// Gate, schedule, and (if allowed) play `event`.
    pub fn play(&self, event: SoundEvent, context: &PlayContext) -> Result<(), Silenced> {
        self.play_pitched(event, 0, context)
    }

    /// `ken.combo_step` for combo `level` (×2 and up): one semitone higher per
    /// level, capped at +[`MAX_COMBO_SEMITONES`].
    pub fn play_combo_step(&self, level: u32, context: &PlayContext) -> Result<(), Silenced> {
        let semitones = level.saturating_sub(2).min(MAX_COMBO_SEMITONES);
        self.play_pitched(SoundEvent::KenComboStep, semitones, context)
    }

    fn play_pitched(
        &self,
        event: SoundEvent,
        semitones: u32,
        context: &PlayContext,
    ) -> Result<(), Silenced> {
        let mut state = self.lock();
        let gain = decide(&state.settings, event, context)?;
        state.scheduler.admit(event, context.now)?;
        state.send(Command::Play {
            event,
            gain,
            semitones,
        });
        Ok(())
    }
}

impl State {
    fn send(&mut self, command: Command) {
        let worker = self.worker.get_or_insert_with(|| {
            let (sender, receiver) = channel();
            let make_output = self.make_output.clone();
            let spawned = std::thread::Builder::new()
                .name("adoken-sound".into())
                .spawn(move || run(receiver, make_output()));
            if let Err(error) = spawned {
                log::warn!("sound: could not start the audio thread: {error}");
            }
            sender
        });
        if worker.send(command).is_err() {
            self.worker = None;
        }
    }
}

fn run(commands: Receiver<Command>, mut output: Box<dyn Output>) {
    let mut worker = Worker {
        pack: None,
        round_robin: HashMap::new(),
        rng: Rng::new(0x0ad0_4e11),
    };
    loop {
        let command = match output.idle_deadline() {
            Some(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                match commands.recv_timeout(wait) {
                    Ok(command) => command,
                    Err(RecvTimeoutError::Timeout) => {
                        output.close();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            None => match commands.recv() {
                Ok(command) => command,
                Err(_) => return,
            },
        };
        match command {
            Command::Preload => {
                worker.pack();
            }
            Command::Play {
                event,
                gain,
                semitones,
            } => {
                if let Some(voice) = worker.voice(event, gain, semitones) {
                    output.play(voice);
                }
            }
        }
    }
}

struct Worker {
    /// Rendered on the first preload or play; dropped with the worker.
    pack: Option<Pack>,
    round_robin: HashMap<SoundEvent, usize>,
    rng: Rng,
}

impl Worker {
    fn pack(&mut self) -> &Pack {
        self.pack.get_or_insert_with(Pack::builtin)
    }

    fn voice(&mut self, event: SoundEvent, gain: f32, semitones: u32) -> Option<Voice> {
        let turn = self.round_robin.entry(event).or_default();
        let index = *turn;
        *turn = turn.wrapping_add(1);
        let jitter = self.rng.next_f32();
        let clips = self.pack().clips(event);
        let clip = clips.get(index % clips.len().max(1))?;
        let rate = 2f32.powf(semitones as f32 / 12.0) * (1.0 + clip.pitch_jitter * jitter);
        Some(Voice {
            samples: clip.samples.clone(),
            gain: (gain * clip.gain).min(1.0),
            rate,
            priority: event.priority(),
            ducks: event.ducks_others(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;
    use time::OffsetDateTime;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-09-25 10:00 UTC);

    struct Recorder(mpsc::Sender<(Instant, Voice)>);

    impl Output for Recorder {
        fn play(&mut self, voice: Voice) {
            let _ = self.0.send((Instant::now(), voice));
        }
    }

    fn recording(
        settings: SoundSettings,
    ) -> (
        SoundEngine,
        mpsc::Receiver<(Instant, Voice)>,
        Arc<Mutex<u32>>,
    ) {
        let (sender, receiver) = mpsc::channel();
        let opened = Arc::new(Mutex::new(0));
        let counter = opened.clone();
        let engine = SoundEngine::with_output(settings, move || {
            *counter.lock() += 1;
            Box::new(Recorder(sender.clone()))
        });
        (engine, receiver, opened)
    }

    fn enabled() -> SoundSettings {
        SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        }
    }

    fn at(ms: i64) -> PlayContext {
        PlayContext::at(NOW + time::Duration::milliseconds(ms))
    }

    #[test]
    fn disabled_engine_never_starts_audio() {
        let (engine, voices, opened) = recording(SoundSettings::default());
        assert_eq!(
            engine.play(SoundEvent::NotifyMention, &at(0)),
            Err(Silenced::Disabled)
        );
        assert!(!engine.is_started());
        assert_eq!(*opened.lock(), 0);
        assert!(voices.try_recv().is_err());

        let (engine, voices, _) = recording(enabled());
        let quiet = PlayContext {
            quiet_mode: true,
            ..at(0)
        };
        assert_eq!(
            engine.play(SoundEvent::UiError, &quiet),
            Err(Silenced::QuietMode)
        );
        assert!(voices.recv_timeout(Duration::from_millis(200)).is_err());
    }

    /// AC-9 (latency half): with the pack preloaded, trigger → output < 50 ms.
    #[test]
    fn enabled_play_reaches_the_output_fast() {
        let (engine, voices, _) = recording(enabled());
        assert!(engine.is_started(), "enable preloads");
        // Let the preload finish, as it does between enabling and the first
        // real notification.
        engine.play(SoundEvent::UiActionDone, &at(0)).unwrap();
        voices
            .recv_timeout(Duration::from_secs(30))
            .expect("first voice");

        let sent = Instant::now();
        engine.play(SoundEvent::NotifyMention, &at(1_000)).unwrap();
        let (arrived, voice) = voices.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            arrived - sent < Duration::from_millis(50),
            "{:?}",
            arrived - sent
        );
        assert_eq!(voice.priority, SoundEvent::NotifyMention.priority());
        let expected = 0.6 * 0.7 * 1.0;
        assert!((voice.gain - expected).abs() < 1e-6, "{}", voice.gain);
    }

    /// AC-10 end to end.
    #[test]
    fn burst_of_five_plays_once() {
        let (engine, voices, _) = recording(enabled());
        let results: Vec<_> = [0, 200, 400, 600, 800]
            .into_iter()
            .map(|ms| engine.play(SoundEvent::NotifyPrComment, &at(ms)))
            .collect();
        assert_eq!(
            results.iter().filter(|r| r.is_ok()).count(),
            1,
            "{results:?}"
        );
        voices
            .recv_timeout(Duration::from_secs(30))
            .expect("one voice");
        assert!(voices.recv_timeout(Duration::from_millis(200)).is_err());
    }

    #[test]
    fn mention_variants_alternate_and_combo_rises() {
        let (engine, voices, _) = recording(enabled());
        engine.play(SoundEvent::NotifyMention, &at(0)).unwrap();
        engine.play(SoundEvent::NotifyMention, &at(2_000)).unwrap();
        let a = voices.recv_timeout(Duration::from_secs(30)).unwrap().1;
        let b = voices.recv_timeout(Duration::from_secs(5)).unwrap().1;
        assert!(!Arc::ptr_eq(&a.samples, &b.samples), "round-robin");

        let arcade = PlayContext {
            arcade_mode: true,
            ..at(3_000)
        };
        engine.play_combo_step(2, &arcade).unwrap();
        let base = voices.recv_timeout(Duration::from_secs(5)).unwrap().1.rate;
        let high = PlayContext {
            arcade_mode: true,
            ..at(4_000)
        };
        engine.play_combo_step(40, &high).unwrap();
        let top = voices.recv_timeout(Duration::from_secs(5)).unwrap().1.rate;
        // +7 semitones, within the ±2 % jitter of each play.
        let ratio = top / base;
        let expected = 2f32.powf(7.0 / 12.0);
        assert!((ratio / expected - 1.0).abs() < 0.05, "{ratio}");
    }

    #[test]
    fn disabling_stops_the_audio_thread() {
        let (engine, _, _) = recording(enabled());
        assert!(engine.is_started());
        engine.set_settings(SoundSettings::default());
        assert!(!engine.is_started());
    }
}
