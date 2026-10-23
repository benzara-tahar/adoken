//! Interruption decisions (FR-6..FR-8): whether an inbox item also gets an OS
//! banner and/or a sound now, later (held by quiet hours or a delivery
//! window), or never. Pure: the OS and audio are behind sinks.

use adoken_model::Millis;

use crate::classify::{Kind, Priority, Source};
use crate::clock::Clock;
use crate::settings::NotifySettings;

/// macOS notification authorization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OsPermission {
    /// Not asked yet: banners are attempted (the sink asks).
    #[default]
    Unknown,
    Granted,
    Denied,
}

/// Runtime facts that aren't settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeliveryEnv {
    pub permission: OsPermission,
    /// The OS's own Focus / Do Not Disturb is on (FR-7).
    pub os_focus: bool,
    /// Global sound switch (spec 0007); off by default.
    pub sounds_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interrupt {
    /// Inbox only.
    Silent,
    Now {
        banner: bool,
        sound: bool,
    },
    /// Gated by quiet hours or the source's window; released at `until`.
    Held {
        until: Millis,
    },
}

/// First instant ≥ `now` at which `source`/`priority` may interrupt: inside
/// the source's delivery window and outside quiet hours. `None` = never (an
/// empty window). "Let P0 through" opens both gates for P0.
pub fn release_time(
    now: Millis,
    source: Source,
    priority: Priority,
    settings: &NotifySettings,
    clock: &dyn Clock,
) -> Option<Millis> {
    let quiet = &settings.quiet_hours;
    if priority == Priority::P0 && quiet.let_p0_through {
        return Some(now);
    }
    let window = settings.windows.get(&source);
    let mut at = now;
    // Alternate the two gates until both are open; 16 hops covers any weekly
    // combination (each hop moves to a distinct boundary within 8 days).
    for _ in 0..16 {
        let mut moved = false;
        if let Some(window) = window {
            let opens = window.next_start(at, clock)?;
            moved |= opens != at;
            at = opens;
        }
        if quiet.enabled {
            let ends = quiet.schedule.next_end(at, clock)?;
            moved |= ends != at;
            at = ends;
        }
        if !moved {
            return Some(at);
        }
    }
    None
}

/// FR-6..FR-8 for one item.
pub fn decide(
    priority: Priority,
    source: Source,
    skip_banner: bool,
    settings: &NotifySettings,
    env: &DeliveryEnv,
    now: Millis,
    clock: &dyn Clock,
) -> Interrupt {
    let mode = settings.delivery.get(priority);
    if skip_banner || !mode.banner() {
        return Interrupt::Silent;
    }
    match release_time(now, source, priority, settings, clock) {
        None => Interrupt::Silent,
        Some(at) if at > now => Interrupt::Held { until: at },
        Some(_) => immediate(mode.sound(), env),
    }
}

/// What may actually fire right now, given the OS state.
pub(crate) fn immediate(wants_sound: bool, env: &DeliveryEnv) -> Interrupt {
    if env.os_focus {
        return Interrupt::Silent;
    }
    let banner = env.permission != OsPermission::Denied;
    let sound = wants_sound && env.sounds_enabled;
    if banner || sound {
        Interrupt::Now { banner, sound }
    } else {
        Interrupt::Silent
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerAction {
    Open,
    SnoozeOneHour,
}

/// One OS notification. `thread_id` groups banners per subject (FR-7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub title: String,
    pub body: String,
    pub thread_id: String,
    /// Notification to focus on click; `None` for multi-item summaries.
    pub notification: Option<i64>,
    pub actions: Vec<BannerAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundCue {
    /// `notify.*` event sound (sound.md §3).
    Event(Kind, Priority),
    /// `notify.digest`: a held summary was released.
    Digest,
}

/// Where banners go: the macOS notification center in the app, a recorder
/// in tests.
pub trait BannerSink {
    fn post(&mut self, banner: &Banner);
}

pub trait SoundSink {
    fn play(&mut self, cue: SoundCue);
}

/// Interruptions produced by one engine call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub banners: Vec<Banner>,
    pub sounds: Vec<SoundCue>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.banners.is_empty() && self.sounds.is_empty()
    }

    pub fn extend(&mut self, other: Effects) {
        self.banners.extend(other.banners);
        self.sounds.extend(other.sounds);
    }

    pub fn dispatch(&self, banners: &mut dyn BannerSink, sounds: &mut dyn SoundSink) {
        for banner in &self.banners {
            banners.post(banner);
        }
        for cue in &self.sounds {
            sounds.play(*cue);
        }
    }
}

/// Fake OS-notification + sound sink (spec 0005 §6 harness; demo mode).
#[derive(Clone, Debug, Default)]
pub struct Recorder {
    pub banners: Vec<Banner>,
    pub sounds: Vec<SoundCue>,
}

impl BannerSink for Recorder {
    fn post(&mut self, banner: &Banner) {
        self.banners.push(banner.clone());
    }
}

impl SoundSink for Recorder {
    fn play(&mut self, cue: SoundCue) {
        self.sounds.push(cue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FakeClock;
    use crate::schedule::tests::at;
    use crate::schedule::{Schedule, Span, TimeOfDay, Weekdays};

    fn quiet_18_to_08() -> NotifySettings {
        let mut settings = NotifySettings::default();
        settings.quiet_hours.enabled = true;
        settings.quiet_hours.schedule = Schedule::new(vec![Span::new(
            Weekdays::ALL,
            TimeOfDay::hm(18, 0),
            TimeOfDay::hm(8, 0),
        )]);
        settings
    }

    #[test]
    fn inbox_only_priorities_never_interrupt() {
        let clock = FakeClock::new(0, 0);
        let settings = NotifySettings::default();
        let env = DeliveryEnv::default();
        let decide = |p| {
            decide(
                p,
                Source::PullRequests,
                false,
                &settings,
                &env,
                at(0, 12, 0),
                &clock,
            )
        };
        assert_eq!(decide(Priority::P3), Interrupt::Silent);
        assert_eq!(decide(Priority::P2), Interrupt::Silent);
        assert_eq!(
            decide(Priority::P1),
            Interrupt::Now {
                banner: true,
                sound: false
            }
        );
    }

    #[test]
    fn sound_needs_the_global_switch() {
        let clock = FakeClock::new(0, 0);
        let settings = NotifySettings::default();
        let mut env = DeliveryEnv::default();
        let now = at(0, 12, 0);
        let p0 = |env: &DeliveryEnv| {
            decide(
                Priority::P0,
                Source::Mentions,
                false,
                &settings,
                env,
                now,
                &clock,
            )
        };
        assert_eq!(
            p0(&env),
            Interrupt::Now {
                banner: true,
                sound: false
            }
        );
        env.sounds_enabled = true;
        assert_eq!(
            p0(&env),
            Interrupt::Now {
                banner: true,
                sound: true
            }
        );
        env.permission = OsPermission::Denied;
        assert_eq!(
            p0(&env),
            Interrupt::Now {
                banner: false,
                sound: true
            }
        );
        env.os_focus = true;
        assert_eq!(p0(&env), Interrupt::Silent);
    }

    #[test]
    fn quiet_hours_hold_until_they_end() {
        let clock = FakeClock::new(0, 0);
        let settings = quiet_18_to_08();
        let env = DeliveryEnv::default();
        assert_eq!(
            decide(
                Priority::P1,
                Source::Mentions,
                false,
                &settings,
                &env,
                at(0, 22, 0),
                &clock
            ),
            Interrupt::Held { until: at(1, 8, 0) }
        );
        // AC-5: P0 goes through by default…
        assert!(matches!(
            decide(
                Priority::P0,
                Source::Mentions,
                false,
                &settings,
                &env,
                at(0, 22, 0),
                &clock
            ),
            Interrupt::Now { banner: true, .. }
        ));
        // …and is held once the user turns that off.
        let mut strict = settings.clone();
        strict.quiet_hours.let_p0_through = false;
        assert_eq!(
            decide(
                Priority::P0,
                Source::Mentions,
                false,
                &strict,
                &env,
                at(0, 22, 0),
                &clock
            ),
            Interrupt::Held { until: at(1, 8, 0) }
        );
    }

    #[test]
    fn window_and_quiet_hours_combine() {
        let clock = FakeClock::new(0, 0);
        let mut settings = quiet_18_to_08();
        // Pipelines only 07:00–10:00: at 22:00 the window opens 07:00 but
        // quiet hours last until 08:00.
        settings.windows.insert(
            Source::Pipelines,
            Schedule::new(vec![Span::new(
                Weekdays::ALL,
                TimeOfDay::hm(7, 0),
                TimeOfDay::hm(10, 0),
            )]),
        );
        assert_eq!(
            release_time(
                at(0, 22, 0),
                Source::Pipelines,
                Priority::P1,
                &settings,
                &clock
            ),
            Some(at(1, 8, 0))
        );
        // Other sources ignore the pipelines window.
        assert_eq!(
            release_time(
                at(1, 12, 0),
                Source::Mentions,
                Priority::P1,
                &settings,
                &clock
            ),
            Some(at(1, 12, 0))
        );
        settings
            .windows
            .insert(Source::Pipelines, Schedule::default());
        assert_eq!(
            release_time(
                at(1, 12, 0),
                Source::Pipelines,
                Priority::P1,
                &settings,
                &clock
            ),
            None
        );
    }

    #[test]
    fn skip_banner_rule_keeps_it_in_the_inbox() {
        let clock = FakeClock::new(0, 0);
        let settings = NotifySettings::default();
        assert_eq!(
            decide(
                Priority::P0,
                Source::Mentions,
                true,
                &settings,
                &DeliveryEnv::default(),
                0,
                &clock
            ),
            Interrupt::Silent
        );
    }
}
