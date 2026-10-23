//! App-side sound glue (spec 0007 FR-7, spec 0025 FR-8/FR-13,
//! docs/design/sound.md): one [`SoundEngine`] per app, fed by the shell.
//!
//! - Settings come from `settings.toml [sound]` (off by default), read on the
//!   background executor after the store opens, so nothing audio-related
//!   exists before the first frame. The engine opens no thread or device
//!   while sounds are off, and the device only on the first sound.
//! - Notification sounds arrive as `adoken_notify::SoundCue`s from the inbox:
//!   notify has already applied per-priority delivery, quiet hours, delivery
//!   windows and OS Focus, so this layer maps the cue to its sound, adds
//!   focus, launch-backlog and Quiet mode state, and leaves volume and burst
//!   control to the engine. Nudges reach the inbox as `Kind::Nudge` cues.
//! - Own actions (`ui.*`) are played directly by the shell.
//! - Quiet mode lives in `[ken]` (the Ken layer owns it, with Ken on or
//!   off); the layer copies it here whenever it applies its settings
//!   ([`set_quiet_mode`]), so a play never reads the layer.

use std::path::Path;
use std::time::{Duration, Instant};

use adoken_ken::settings::QuietMode;
use adoken_model::{Millis, SyncEventKind};
use adoken_notify::{Kind, Priority, SoundCue};
use adoken_sound::{NotifyPriority, PlayContext, Silenced, SoundEngine, SoundEvent, SoundSettings};
use gpui::{App, Global};
use time::OffsetDateTime;

/// sound.md §4: notification sounds stay silent this long after launch.
pub const LAUNCH_BACKLOG: Duration = Duration::from_secs(3);

struct Sounds {
    /// `None` until settings load.
    engine: Option<SoundEngine>,
    launched: Instant,
    quiet: QuietMode,
}

impl Global for Sounds {}

/// Register the (empty) sound state; call once at launch. No IO.
pub fn init(cx: &mut App) {
    if !cx.has_global::<Sounds>() {
        cx.set_global(Sounds {
            engine: None,
            launched: Instant::now(),
            quiet: QuietMode::default(),
        });
    }
}

/// Parse `[sound]` from `settings_path` (blocking IO: background only).
/// Missing or broken files mean the defaults: sounds off.
pub fn read_settings(settings_path: &Path) -> SoundSettings {
    let text = std::fs::read_to_string(settings_path).unwrap_or_default();
    match SoundSettings::from_settings_toml(&text) {
        Ok((settings, issues)) => {
            for issue in issues {
                log::warn!("sound: {}: {issue}", settings_path.display());
            }
            settings
        }
        Err(error) => {
            log::warn!("sound: {}: {error}", settings_path.display());
            SoundSettings::default()
        }
    }
}

/// Apply loaded settings (creates the engine; cheap while sounds are off).
pub fn install(settings: SoundSettings, cx: &mut App) {
    init(cx);
    let sounds = cx.global_mut::<Sounds>();
    match &sounds.engine {
        Some(engine) => engine.set_settings(settings),
        None => sounds.engine = Some(SoundEngine::new(settings)),
    }
}

/// Quiet mode as the Ken layer last applied it: while active, nothing plays.
pub fn set_quiet_mode(quiet: QuietMode, cx: &mut App) {
    init(cx);
    cx.global_mut::<Sounds>().quiet = quiet;
}

/// Play a notification cue from the inbox.
pub fn play_cue(cue: SoundCue, window_focused: bool, cx: &App) -> Result<(), Silenced> {
    let priority = match cue {
        SoundCue::Event(_, priority) => notify_priority(priority),
        SoundCue::Digest => NotifyPriority::Normal,
    };
    match event_for(cue) {
        Some(event) => play(event, priority, window_focused, cx),
        None => Ok(()),
    }
}

/// Play one of your own actions' feedback sounds (`ui.*`).
pub fn play_ui(event: SoundEvent, window_focused: bool, cx: &App) -> Result<(), Silenced> {
    play(event, NotifyPriority::Normal, window_focused, cx)
}

/// A celebration sound (`ui.celebrate_*`, spec 0022 FR-13): a `ui` sound,
/// so it follows sound.md §4's `ui` column, and an unfocused window stays
/// silent.
pub fn play_celebration(
    event: SoundEvent,
    quiet_hours: bool,
    window_focused: bool,
    cx: &App,
) -> Result<(), Silenced> {
    let (engine, context) = ready(window_focused, NotifyPriority::Normal, cx)?;
    engine.play(
        event,
        &PlayContext {
            quiet_hours,
            ..context
        },
    )
}

/// A Ken event (`ken.*`, sound.md §2.3; Arcade chip tones); `combo_level`
/// pitches `ken.combo_step`. Ken has already dropped sounds for Quiet mode
/// and quiet hours; both reach the engine too so its matrix agrees.
pub fn play_ken(
    event: SoundEvent,
    combo_level: Option<u32>,
    quiet_hours: bool,
    window_focused: bool,
    cx: &App,
) -> Result<(), Silenced> {
    let (engine, context) = ready(window_focused, NotifyPriority::Normal, cx)?;
    let context = PlayContext {
        quiet_hours,
        arcade_mode: true,
        // Ken scores the backlog silently itself.
        backlog: false,
        ..context
    };
    match combo_level {
        Some(level) => engine.play_combo_step(level, &context),
        None => engine.play(event, &context),
    }
}

fn play(
    event: SoundEvent,
    priority: NotifyPriority,
    window_focused: bool,
    cx: &App,
) -> Result<(), Silenced> {
    let (engine, context) = ready(window_focused, priority, cx)?;
    engine.play(event, &context)
}

/// The engine and this moment's context, or `Disabled` before settings load.
fn ready(
    window_focused: bool,
    priority: NotifyPriority,
    cx: &App,
) -> Result<(&SoundEngine, PlayContext), Silenced> {
    let sounds = cx.try_global::<Sounds>().ok_or(Silenced::Disabled)?;
    let engine = sounds.engine.as_ref().ok_or(Silenced::Disabled)?;
    let now = OffsetDateTime::now_utc();
    let context = context(
        now,
        window_focused,
        sounds.launched.elapsed() < LAUNCH_BACKLOG,
        sounds.quiet.active(millis(now)),
        priority,
    );
    Ok((engine, context))
}

fn millis(at: OffsetDateTime) -> Millis {
    (at.unix_timestamp_nanos() / 1_000_000) as Millis
}

/// The engine context for a play. Quiet hours and delivery windows are
/// already applied by notify, so they're never set here.
pub fn context(
    now: OffsetDateTime,
    window_focused: bool,
    backlog: bool,
    quiet_mode: bool,
    priority: NotifyPriority,
) -> PlayContext {
    PlayContext {
        window_focused,
        backlog,
        quiet_mode,
        priority,
        ..PlayContext::at(now)
    }
}

/// Spec 0005 priorities → sound priorities (sound.md §2.2): P0 is urgent
/// (gain × 1.2, the only sound in quiet hours), P3 never plays.
pub fn notify_priority(priority: Priority) -> NotifyPriority {
    match priority {
        Priority::P0 => NotifyPriority::Urgent,
        Priority::P1 => NotifyPriority::High,
        Priority::P2 => NotifyPriority::Normal,
        Priority::P3 => NotifyPriority::Low,
    }
}

/// Which `notify.*` sound a cue plays (sound.md §2.2). Cues carry the kind
/// but not the vote or build result, so votes and pushes use the neutral PR
/// activity sound and a delivered build (notify filters "still green" runs)
/// the failure sound. Nudges and token notices never play (spec 0025
/// FR-13: the nudge sound was off by default; spec 0001 FR-14, FR-18).
pub fn event_for(cue: SoundCue) -> Option<SoundEvent> {
    use SyncEventKind::*;
    Some(match cue {
        SoundCue::Digest => SoundEvent::NotifyDigest,
        SoundCue::Event(Kind::Nudge, _) => return None,
        SoundCue::Event(Kind::Sync(kind), _) => match kind {
            ReviewRequested => SoundEvent::NotifyReviewRequested,
            MentionedInPr | MentionedInWorkItem => SoundEvent::NotifyMention,
            PrCommentAdded | PrThreadResolved | PrVoteChanged | PrPushed | PrStatusChanged => {
                SoundEvent::NotifyPrComment
            }
            PrMergeConflict => SoundEvent::NotifyPrChanges,
            WorkItemAssigned | WorkItemChanged => SoundEvent::NotifyWorkAssigned,
            BuildCompleted => SoundEvent::NotifyPipelineFailed,
            TokenRejected | TokenReplaced | TokenNotReplaced => return None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_ken::KenSettings;
    use adoken_sound::{Category, decide};
    use time::macros::datetime;

    #[test]
    fn every_cue_maps_to_a_notify_sound() {
        let kinds = [
            SyncEventKind::ReviewRequested,
            SyncEventKind::PrCommentAdded,
            SyncEventKind::PrThreadResolved,
            SyncEventKind::MentionedInPr,
            SyncEventKind::MentionedInWorkItem,
            SyncEventKind::PrVoteChanged,
            SyncEventKind::PrPushed,
            SyncEventKind::PrStatusChanged,
            SyncEventKind::PrMergeConflict,
            SyncEventKind::WorkItemAssigned,
            SyncEventKind::WorkItemChanged,
            SyncEventKind::BuildCompleted,
        ];
        for kind in kinds {
            let event = event_for(SoundCue::Event(Kind::Sync(kind), Priority::P1)).unwrap();
            assert_eq!(event.category(), Category::Notify, "{kind:?}");
        }
        for kind in [
            SyncEventKind::TokenRejected,
            SyncEventKind::TokenReplaced,
            SyncEventKind::TokenNotReplaced,
        ] {
            assert_eq!(
                event_for(SoundCue::Event(Kind::Sync(kind), Priority::P1)),
                None
            );
        }
        assert_eq!(
            event_for(SoundCue::Event(
                Kind::Sync(SyncEventKind::MentionedInWorkItem),
                Priority::P0
            )),
            Some(SoundEvent::NotifyMention)
        );
        assert_eq!(event_for(SoundCue::Event(Kind::Nudge, Priority::P2)), None);
        assert_eq!(event_for(SoundCue::Digest), Some(SoundEvent::NotifyDigest));
    }

    /// P0 plays louder, P3 never plays (sound.md §2.2), through the real gate.
    #[test]
    fn priorities_follow_the_notification_center() {
        let settings = SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        };
        let now = datetime!(2026-09-25 10:00 UTC);
        let gain = |priority| {
            let context = context(now, true, false, false, notify_priority(priority));
            decide(&settings, SoundEvent::NotifyMention, &context)
        };
        let normal = gain(Priority::P2).unwrap();
        assert!((gain(Priority::P0).unwrap() - normal * 1.2).abs() < 1e-6);
        assert_eq!(gain(Priority::P1).unwrap(), normal);
        assert_eq!(gain(Priority::P3), Err(Silenced::LowPriority));
    }

    #[test]
    fn launch_backlog_silences_notifications_not_own_actions() {
        let settings = SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        };
        let backlog = context(
            datetime!(2026-09-25 10:00 UTC),
            true,
            true,
            false,
            NotifyPriority::Urgent,
        );
        assert_eq!(
            decide(&settings, SoundEvent::NotifyMention, &backlog),
            Err(Silenced::Backlog)
        );
        assert!(decide(&settings, SoundEvent::UiActionDone, &backlog).is_ok());
    }

    /// Spec 0025 AC-7 (sound part), injected clock: "Quiet mode for 1 hour"
    /// silences every sound until the hour is over; 61 minutes later they
    /// play again.
    #[test]
    fn quiet_mode_for_an_hour_silences_sounds_until_it_runs_out() {
        let settings = SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        };
        let start = datetime!(2026-09-25 10:00 UTC);
        let mut ken = KenSettings::default();
        ken.quiet_for_hour(millis(start));
        let quiet = ken.quiet_mode();
        let at = |minutes| {
            let now = start + time::Duration::minutes(minutes);
            context(
                now,
                true,
                false,
                quiet.active(millis(now)),
                NotifyPriority::Urgent,
            )
        };
        for event in [
            SoundEvent::NotifyMention,
            SoundEvent::UiActionDone,
            SoundEvent::UiCelebrateMilestone,
            SoundEvent::OnboardingComplete,
        ] {
            assert_eq!(
                decide(&settings, event, &at(0)),
                Err(Silenced::QuietMode),
                "{event}"
            );
            assert_eq!(
                decide(&settings, event, &at(59)),
                Err(Silenced::QuietMode),
                "{event}"
            );
            assert!(decide(&settings, event, &at(61)).is_ok(), "{event}");
        }
    }
}
