//! Ken, the opt-in arcade layer (docs/design/ken.md) in the app: combo HUD,
//! K.O. overlay, badge toasts, statusbar chip, the weekly fight card dialog
//! and export, and the Settings › Ken section. Logic, tables and the share
//! image live in `adoken_ken`; this module renders, plays sounds, and runs
//! the ingest loop on the background executor.
//!
//! Shell contract:
//! - [`mount`] once the store is open → [`KenHandles`]: render `layer`
//!   last inside the shell (full-window, pointer-transparent overlay) and
//!   `chip` in the statusbar's right side.
//! - [`commands`] in the palette catalog (`> Ken: …`, and the two Quiet
//!   mode commands), [`bindings`] in `key_bindings()`.
//! - [`inbox_unread`] with the inbox's unread count (Clean Sweep).
//!
//! Off by default: until `[ken] enabled = true` nothing is read beyond the
//! settings table and no loop runs. Nothing before the first frame.

mod card_dialog;
mod glyphs;
mod hud;
mod ko;
mod layer;
mod settings_section;

use gpui::{App, AppContext as _, Entity, Global, KeyBinding, WeakEntity, Window, actions};

use adoken_store::Store;

use crate::palette::PaletteCommand;

pub use card_dialog::{FightCardDialog, preview_png};
pub(crate) use glyphs::paint_sparks;
pub use glyphs::{ArcadeText, GlyphColors};
pub use hud::{ComboHud, KenToast, StatusChip, toast_stack};
pub use ko::KoOverlay;
pub use layer::{KenChip, KenLayer};
pub use settings_section::KenSettingsSection;

/// Story/test access to pure frame math.
pub mod frames {
    pub use super::hud::{
        FADE_MS, FINISH_MS, MOVE_FLASH_MS, finish_state, minutes_left, pop_scale,
    };
    pub use super::ko::{DURATION_MS, EXIT_AT, frame as ko_frame, toast_text as ko_toast_text};
}

actions!(
    ken,
    [
        /// Turn the arcade layer on or off (ken.md §1; off by default).
        ToggleArcadeMode,
        /// Show this week's fight card (ken.md §5).
        ShowFightCard,
        /// Export the fight card as a 1200 × 675 PNG (save dialog).
        ExportFightCard,
        /// Freeze the review streak (vacation), or resume it.
        ToggleStreakPause,
        /// Silence sounds and hide celebrations and every Ken surface until
        /// turned off, or turn that off (spec 0025 FR-8). Works with Ken off.
        ToggleQuietMode,
        /// Quiet mode for an hour (demos, screen shares).
        QuietModeForHour,
    ]
);

/// Palette commands (keyboard-first: every Ken action is here). The Quiet
/// mode commands carry no "Ken:" prefix: they work with Ken off and also
/// silence sounds and celebrations (spec 0025 FR-8, ken.md §7).
pub fn commands() -> Vec<PaletteCommand> {
    [
        (
            "Ken: Toggle arcade mode",
            Box::new(ToggleArcadeMode) as Box<dyn gpui::Action>,
        ),
        ("Ken: Show fight card", Box::new(ShowFightCard)),
        ("Ken: Export fight card…", Box::new(ExportFightCard)),
        (STREAK_TOGGLE, Box::new(ToggleStreakPause)),
        (QUIET_TOGGLE, Box::new(ToggleQuietMode)),
        (QUIET_HOUR, Box::new(QuietModeForHour)),
    ]
    .into_iter()
    .map(|(title, action)| PaletteCommand {
        title,
        keys: None,
        action,
    })
    .collect()
}

pub const STREAK_TOGGLE: &str = "Ken: Pause or resume streak";
pub const QUIET_TOGGLE: &str = "Quiet mode: turn on or off";
pub const QUIET_HOUR: &str = "Quiet mode for 1 hour";

/// The fight card dialog's keys (context [`card_dialog::CONTEXT`]).
pub fn bindings() -> Vec<KeyBinding> {
    card_dialog::bindings()
}

/// Handles the shell renders.
#[derive(Clone)]
pub struct KenHandles {
    pub layer: Entity<KenLayer>,
    pub chip: Entity<KenChip>,
}

struct GlobalKen(WeakEntity<KenLayer>);

impl Global for GlobalKen {}

/// Start Ken for this store: reads `[ken]` in the background and, only if
/// enabled, starts scoring. Registers the palette actions app-wide.
pub fn mount(store: Store, window: &mut Window, cx: &mut App) -> KenHandles {
    let layer = cx.new(|cx| KenLayer::new(store, window, cx));
    let chip = cx.new(|cx| KenChip::new(layer.clone(), cx));
    let first = !cx.has_global::<GlobalKen>();
    cx.set_global(GlobalKen(layer.downgrade()));
    if first {
        register_actions(cx);
        crate::celebrate::register(cx);
    }
    KenHandles { layer, chip }
}

/// The mounted layer (celebrations reach it from write paths and views;
/// smoke runs read its hooks).
pub fn layer_entity(cx: &gpui::App) -> Option<Entity<KenLayer>> {
    cx.try_global::<GlobalKen>()
        .and_then(|global| global.0.upgrade())
}

/// A K.O. overlay plays or waits to start.
pub fn ko_showing(cx: &gpui::App) -> bool {
    layer_entity(cx).is_some_and(|layer| layer.read(cx).ko_showing())
}

fn with_layer(cx: &mut App, f: impl FnOnce(&mut KenLayer, &mut gpui::Context<KenLayer>)) {
    let Some(layer) = cx
        .try_global::<GlobalKen>()
        .and_then(|global| global.0.upgrade())
    else {
        return;
    };
    layer.update(cx, f);
}

fn register_actions(cx: &mut App) {
    cx.on_action(|_: &ToggleArcadeMode, cx| with_layer(cx, |layer, cx| layer.toggle_enabled(cx)));
    cx.on_action(|_: &ShowFightCard, cx| {
        with_layer(cx, |layer, cx| layer.show_fight_card(None, false, cx))
    });
    cx.on_action(|_: &ExportFightCard, cx| {
        with_layer(cx, |layer, cx| layer.show_fight_card(None, true, cx))
    });
    cx.on_action(|_: &ToggleStreakPause, cx| {
        with_layer(cx, |layer, cx| layer.toggle_streak_paused(cx))
    });
    cx.on_action(|_: &ToggleQuietMode, cx| with_layer(cx, |layer, cx| layer.toggle_quiet(cx)));
    cx.on_action(|_: &QuietModeForHour, cx| with_layer(cx, |layer, cx| layer.quiet_for_hour(cx)));
}

/// The inbox's unread count; reaching zero from above through triage is a
/// Clean Sweep (once a day).
pub fn inbox_unread(unread: usize, cx: &mut App) {
    with_layer(cx, |layer, _| layer.inbox_unread(unread));
}

/// Settings › Ken, embedded by the Settings window.
pub fn settings_section(window: &mut Window, cx: &mut App) -> Entity<KenSettingsSection> {
    let layer = cx
        .try_global::<GlobalKen>()
        .and_then(|global| global.0.upgrade());
    cx.new(|cx| KenSettingsSection::new(layer, window, cx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ken_commands_are_namespaced_quiet_mode_is_not_and_all_are_original() {
        let commands = commands();
        assert_eq!(commands.len(), 6);
        for command in &commands {
            let quiet = [QUIET_TOGGLE, QUIET_HOUR].contains(&command.title);
            assert_eq!(
                command.title.starts_with("Ken: "),
                !quiet,
                "{}",
                command.title
            );
            let lower = command.title.to_lowercase();
            for banned in adoken_ken::BANNED_NAMES {
                assert!(
                    !lower.contains(banned),
                    "{} contains {banned}",
                    command.title
                );
            }
        }
    }

    /// Spec 0025 AC-7 with an injected clock: while "Quiet mode for 1 hour"
    /// runs, a sound event, a celebration trigger and a Ken K.O. all stay
    /// silent and hidden; 61 minutes later every one of them plays again.
    #[test]
    fn quiet_mode_silences_sounds_celebrations_and_ko_until_the_hour_ends() {
        use adoken_ken::KenSettings;
        use adoken_ken::celebrate::{self, CelebrationSettings, Decision, Memory, Trigger};
        use adoken_ken::settings::Conditions;
        use adoken_model::Millis;
        use adoken_model::time::MINUTE;
        use adoken_sound::{NotifyPriority, Silenced, SoundEvent, SoundSettings, decide};

        const START: Millis = 1_790_328_600_000; // Fri 2026-09-25 09:30 UTC
        let mut ken = KenSettings {
            enabled: true,
            ..KenSettings::default()
        };
        ken.quiet_for_hour(START);
        let sounds = SoundSettings {
            enabled: true,
            ..SoundSettings::default()
        };
        let sound = |now: Millis| {
            let at = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now) * 1_000_000)
                .unwrap();
            let context = crate::sound::context(
                at,
                true,
                false,
                ken.quiet_active(now),
                NotifyPriority::Normal,
            );
            decide(&sounds, SoundEvent::NotifyMention, &context)
        };
        let celebration = |now: Millis, trigger| {
            celebrate::decide(
                trigger,
                &CelebrationSettings::default(),
                celebrate::Conditions {
                    window_active: true,
                    quiet_mode: ken.quiet_active(now),
                    ..celebrate::Conditions::default()
                },
                &mut Memory::default(),
                now,
            )
        };
        let ken_surfaces = |now: Millis| {
            ken.surfaces(Conditions {
                now,
                ..Conditions::default()
            })
        };
        let merged = Trigger::PullRequest { id: 7, ko: false };
        let knockout = Trigger::PullRequest { id: 7, ko: true };

        for now in [START, START + 59 * MINUTE] {
            assert_eq!(sound(now), Err(Silenced::QuietMode));
            assert_eq!(celebration(now, merged), Decision::None);
            assert_eq!(celebration(now, knockout), Decision::None);
            let surfaces = ken_surfaces(now);
            assert!(!surfaces.ko_overlay && !surfaces.toasts && !surfaces.hud && !surfaces.sounds);
        }

        let later = START + 61 * MINUTE;
        assert!(sound(later).is_ok());
        assert!(matches!(
            celebration(later, merged),
            Decision::Celebrate { .. }
        ));
        assert_eq!(celebration(later, knockout), Decision::Ko);
        let surfaces = ken_surfaces(later);
        assert!(surfaces.ko_overlay && surfaces.toasts && surfaces.hud && surfaces.sounds);
    }
}
