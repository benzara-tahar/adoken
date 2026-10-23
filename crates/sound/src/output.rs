//! The real audio device (rodio/cpal). Opened on the first play, closed
//! after [`IDLE_CLOSE`] without sound (sound.md §5), never opened while
//! sounds are disabled (§1). Device failures are logged once per
//! [`RETRY_AFTER`] and otherwise skipped silently (spec 0007 §3 states).

use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Source};

use crate::mixer::{Mixer, Voice};
use crate::synth::SAMPLE_RATE;

pub const IDLE_CLOSE: Duration = Duration::from_secs(60);
const RETRY_AFTER: Duration = Duration::from_secs(30);
/// New voices are picked up every this many samples (~1.3 ms).
const POLL_EVERY: u32 = 64;

/// Where voices go. The engine's worker thread owns it.
pub trait Output: Send {
    fn play(&mut self, voice: Voice);
    /// Called when nothing played for a while; release the device.
    fn close(&mut self) {}
    /// When the device may be released, if it is open.
    fn idle_deadline(&self) -> Option<Instant> {
        None
    }
}

/// Default device output through rodio.
#[derive(Default)]
pub struct DeviceOutput {
    open: Option<OpenDevice>,
    failed_at: Option<Instant>,
}

struct OpenDevice {
    // Dropping the sink stops the cpal stream.
    _sink: MixerDeviceSink,
    voices: Sender<Voice>,
    /// Set by the callback when its mixer has nothing left to play.
    idle: Arc<AtomicBool>,
    last_play: Instant,
    longest_end: Instant,
}

impl DeviceOutput {
    fn open(&mut self) -> Option<&mut OpenDevice> {
        if self.open.is_none() {
            if self.failed_at.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
                return None;
            }
            match DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    let (voices, receiver) = channel();
                    let idle = Arc::new(AtomicBool::new(true));
                    sink.mixer().add(MixSource {
                        mixer: Mixer::new(),
                        receiver,
                        idle: idle.clone(),
                        countdown: 0,
                    });
                    let now = Instant::now();
                    self.open = Some(OpenDevice {
                        _sink: sink,
                        voices,
                        idle,
                        last_play: now,
                        longest_end: now,
                    });
                    self.failed_at = None;
                }
                Err(error) => {
                    if self.failed_at.is_none() {
                        log::warn!("sound: audio output unavailable, skipping sounds: {error}");
                    }
                    self.failed_at = Some(Instant::now());
                    return None;
                }
            }
        }
        self.open.as_mut()
    }
}

impl Output for DeviceOutput {
    fn play(&mut self, voice: Voice) {
        let length = Duration::from_secs_f64(
            voice.samples.len() as f64 / (SAMPLE_RATE as f64 * voice.rate.max(0.01) as f64),
        );
        let Some(device) = self.open() else {
            return;
        };
        let now = Instant::now();
        device.last_play = now;
        device.longest_end = device.longest_end.max(now + length);
        device.idle.store(false, Ordering::Relaxed);
        if device.voices.send(voice).is_err() {
            // The stream died (device unplugged): reopen on the next play.
            self.open = None;
        }
    }

    fn close(&mut self) {
        if let Some(device) = &self.open
            && device.idle.load(Ordering::Relaxed)
        {
            self.open = None;
        }
    }

    fn idle_deadline(&self) -> Option<Instant> {
        self.open
            .as_ref()
            .map(|device| device.longest_end.max(device.last_play) + IDLE_CLOSE)
    }
}

/// The endless mono source rodio pulls from its callback.
struct MixSource {
    mixer: Mixer,
    receiver: Receiver<Voice>,
    idle: Arc<AtomicBool>,
    countdown: u32,
}

impl Iterator for MixSource {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.countdown == 0 {
            self.countdown = POLL_EVERY;
            loop {
                match self.receiver.try_recv() {
                    Ok(voice) => {
                        self.mixer.start(voice);
                    }
                    Err(TryRecvError::Empty) => break,
                    // The output was closed: end the source.
                    Err(TryRecvError::Disconnected) => return None,
                }
            }
            self.idle.store(self.mixer.is_idle(), Ordering::Relaxed);
        }
        self.countdown -= 1;
        Some(self.mixer.next_sample() as rodio::Sample)
    }
}

impl Source for MixSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(1).expect("1 is non-zero")
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(SAMPLE_RATE).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
