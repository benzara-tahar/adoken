//! Audition the built-in pack.
//!
//! `cargo run -p adoken-sound --example audition -- [--wav <dir>]`
//!
//! Without `--wav`, plays every event through the default output device via
//! [`SoundEngine`] (the same lazy device path the app uses). With `--wav`,
//! writes one 16-bit mono WAV per clip instead and plays nothing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use adoken_sound::synth::SAMPLE_RATE;
use adoken_sound::{Pack, PlayContext, SoundEngine, SoundEvent, SoundSettings};
use time::OffsetDateTime;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let wav_dir = match args.next().as_deref() {
        Some("--wav") => args.next().map(PathBuf::from),
        _ => None,
    };

    if let Some(dir) = wav_dir {
        std::fs::create_dir_all(&dir)?;
        let pack = Pack::builtin();
        for &event in SoundEvent::ALL {
            for (index, clip) in pack.clips(event).iter().enumerate() {
                let path = dir.join(format!("{}-{index}.wav", event.id()));
                write_wav(&path, &clip.samples)?;
                println!(
                    "{} ({:.2} s)",
                    path.display(),
                    clip.duration().as_secs_f32()
                );
            }
        }
        return Ok(());
    }

    let engine = SoundEngine::new(SoundSettings {
        enabled: true,
        ..SoundSettings::default()
    });
    for &event in SoundEvent::ALL {
        let context = PlayContext {
            arcade_mode: true,
            ..PlayContext::at(OffsetDateTime::now_utc())
        };
        println!("{event}: {:?}", engine.play(event, &context));
        std::thread::sleep(event.max_duration().min(Duration::from_millis(2_100)));
    }
    Ok(())
}

fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // mono
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    std::fs::write(path, bytes)
}
