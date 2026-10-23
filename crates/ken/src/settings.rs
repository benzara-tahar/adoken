//! `settings.toml [ken]` (ken.md §1), Quiet mode (ken.md §7, spec 0025
//! FR-8), and the surface matrix of §2/§6: which Ken surfaces show,
//! animate and sound for a given moment.

use adoken_model::Millis;
use adoken_model::time::HOUR;
use adoken_notify::Weekdays;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::moves::Naming;

/// The `[ken]` table name in `settings.toml`.
pub const SETTINGS_TABLE: &str = "ken";
/// "Quiet mode for 1 hour".
pub const QUIET_HOUR: Millis = HOUR;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intensity {
    /// Toasts and statusbar only.
    Subtle,
    /// HUD, overlays, sparks.
    #[default]
    Arcade,
}

impl Intensity {
    pub fn label(self) -> &'static str {
        match self {
            Intensity::Subtle => "Subtle",
            Intensity::Arcade => "Arcade",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct KenSettings {
    /// Opt-in; off after a fresh install.
    pub enabled: bool,
    pub intensity: Intensity,
    pub naming: Naming,
    pub combo: bool,
    pub ko: bool,
    pub streak: bool,
    pub badges: bool,
    /// Arcade only; forced off under reduce motion.
    pub screen_shake: bool,
    /// Arcade only; forced off under reduce motion.
    pub particles: bool,
    /// Streak calendar.
    pub working_days: Weekdays,
    /// Quiet mode on until turned off (spec 0025 FR-8). Lives here, next
    /// to the Ken state it hides, and works with Ken off.
    pub quiet: bool,
    /// RFC 3339 end of "Quiet mode for 1 hour"; empty otherwise. (The
    /// former `calm_until` key is ignored on read and never written.)
    pub quiet_until: String,
}

impl Default for KenSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            intensity: Intensity::Arcade,
            naming: Naming::EnglishKanji,
            combo: true,
            ko: true,
            streak: true,
            badges: true,
            screen_shake: true,
            particles: true,
            working_days: Weekdays::WORKDAYS,
            quiet: false,
            quiet_until: String::new(),
        }
    }
}

impl KenSettings {
    /// `[ken]` out of a whole `settings.toml`. A missing table means the
    /// defaults (Ken off); a malformed one is logged and also means defaults.
    pub fn from_settings_toml(text: &str) -> KenSettings {
        let table = match text.parse::<toml::Table>() {
            Ok(table) => table,
            Err(error) => {
                log::warn!("ken: settings.toml: {error}");
                return KenSettings::default();
            }
        };
        match table.get(SETTINGS_TABLE) {
            Some(value) => value.clone().try_into().unwrap_or_else(|error| {
                log::warn!("ken: settings.toml [ken]: {error}");
                KenSettings::default()
            }),
            None => KenSettings::default(),
        }
    }

    /// The `[ken]` table to write back.
    pub fn to_table(&self) -> toml::Table {
        toml::Table::try_from(self).expect("ken settings serialize to a table")
    }

    /// End of "Quiet mode for 1 hour", if one is set (past or future).
    pub fn quiet_until_ms(&self) -> Option<Millis> {
        if self.quiet_until.is_empty() {
            return None;
        }
        OffsetDateTime::parse(&self.quiet_until, &Rfc3339)
            .ok()
            .map(|at| (at.unix_timestamp_nanos() / 1_000_000) as Millis)
    }

    /// Quiet mode as a value, for gates outside Ken (sounds).
    pub fn quiet_mode(&self) -> QuietMode {
        QuietMode {
            on: self.quiet,
            until: self.quiet_until_ms(),
        }
    }

    /// Quiet mode is on at `now`: no sounds, celebrations or Ken surfaces.
    pub fn quiet_active(&self, now: Millis) -> bool {
        self.quiet_mode().active(now)
    }

    /// "Quiet mode: turn on or off": off when on (timed or not), otherwise
    /// on until turned off.
    pub fn toggle_quiet(&mut self, now: Millis) {
        self.quiet = !self.quiet_active(now);
        self.quiet_until.clear();
    }

    /// "Quiet mode for 1 hour", from `now` (replaces any Quiet mode on).
    pub fn quiet_for_hour(&mut self, now: Millis) {
        self.quiet = false;
        self.quiet_until =
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(now + QUIET_HOUR) * 1_000_000)
                .ok()
                .and_then(|at| at.format(&Rfc3339).ok())
                .unwrap_or_default();
    }
}

/// Quiet mode (spec 0025 FR-8): on until turned off, or until a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuietMode {
    pub on: bool,
    pub until: Option<Millis>,
}

impl QuietMode {
    pub fn active(self, now: Millis) -> bool {
        self.on || self.until.is_some_and(|until| now < until)
    }
}

/// What the moment allows (ken.md §2 surfaces × §6 conditions).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surfaces {
    /// Statusbar chip (combo + streak flame).
    pub chip: bool,
    /// Floating combo HUD (arcade).
    pub hud: bool,
    /// HUD pops, rank-up crossfades, timer bar (off: numbers update in place
    /// and the timer reads "6 min").
    pub hud_motion: bool,
    /// Full-window K.O. overlay; otherwise K.O. is a toast.
    pub ko_overlay: bool,
    /// Badge / K.O. / combo-finish toasts.
    pub toasts: bool,
    pub sparks: bool,
    pub shake: bool,
    /// `ken.*` sounds (still subject to the sound settings).
    pub sounds: bool,
}

/// The environment a surface decision needs besides the settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Conditions {
    pub reduce_motion: bool,
    /// Spec 0005 quiet hours active.
    pub quiet_hours: bool,
    pub now: Millis,
}

impl KenSettings {
    pub fn surfaces(&self, conditions: Conditions) -> Surfaces {
        if !self.enabled || self.quiet_active(conditions.now) {
            return Surfaces::default();
        }
        let arcade = self.intensity == Intensity::Arcade;
        let motion = !conditions.reduce_motion;
        Surfaces {
            chip: true,
            hud: arcade && self.combo,
            hud_motion: arcade && motion,
            ko_overlay: arcade && motion && !conditions.quiet_hours && self.ko,
            toasts: true,
            sparks: arcade && motion && self.particles,
            shake: arcade && motion && self.screen_shake,
            sounds: !conditions.quiet_hours,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_model::time::MINUTE;

    const NOW: Millis = 1_790_328_600_000; // Fri 2026-09-25 09:30 UTC

    fn on() -> KenSettings {
        KenSettings {
            enabled: true,
            ..KenSettings::default()
        }
    }

    #[test]
    fn off_by_default_and_missing_table_is_default() {
        assert!(!KenSettings::default().enabled);
        assert_eq!(
            KenSettings::from_settings_toml("[sound]\nenabled = true\n"),
            KenSettings::default()
        );
        assert_eq!(
            KenSettings::from_settings_toml("not toml ["),
            KenSettings::default()
        );
    }

    #[test]
    fn reads_the_documented_table() {
        let text = r#"
[ken]
enabled = true
intensity = "subtle"
naming = "romaji_kanji"
particles = false
working_days = ["mon", "wed"]
quiet_until = "2026-09-25T10:30:00Z"
"#;
        let settings = KenSettings::from_settings_toml(text);
        assert!(settings.enabled);
        assert_eq!(settings.intensity, Intensity::Subtle);
        assert_eq!(settings.naming, Naming::RomajiKanji);
        assert!(!settings.particles);
        assert!(settings.combo, "unspecified keys keep defaults");
        assert_eq!(settings.working_days, Weekdays(0b101));
        assert!(settings.quiet_active(NOW));
        assert!(!settings.quiet_active(NOW + HOUR));
        let back = KenSettings::from_settings_toml(&format!(
            "[ken]\n{}",
            toml::to_string(&settings.to_table()).unwrap()
        ));
        assert_eq!(back, settings);
    }

    /// Spec 0025 FR-14: the old calm state loads without error, keeps the
    /// rest of the table, and is never written back.
    #[test]
    fn old_calm_state_is_ignored_and_dropped() {
        let settings = KenSettings::from_settings_toml(
            "[ken]\nenabled = true\ncombo = false\ncalm_until = \"2026-09-25T10:30:00Z\"\n",
        );
        assert!(settings.enabled && !settings.combo);
        assert!(!settings.quiet_active(NOW), "calm state isn't Quiet mode");
        assert!(!settings.to_table().contains_key("calm_until"));
    }

    /// Spec 0025 AC-7 (clock part): on until turned off; "for 1 hour" ends
    /// by minute 61; either one turns off with the toggle; it survives a
    /// restart (a re-read of the written table).
    #[test]
    fn quiet_mode_toggles_and_the_hour_expires() {
        let mut settings = KenSettings::default();
        settings.toggle_quiet(NOW);
        assert!(
            settings.quiet_active(NOW + 365 * 24 * HOUR),
            "until turned off"
        );
        settings.toggle_quiet(NOW + MINUTE);
        assert!(!settings.quiet_active(NOW + MINUTE));

        settings.quiet_for_hour(NOW);
        assert!(settings.quiet_active(NOW + 59 * MINUTE));
        let restarted = KenSettings::from_settings_toml(&format!(
            "[ken]\n{}",
            toml::to_string(&settings.to_table()).unwrap()
        ));
        assert!(restarted.quiet_active(NOW + 59 * MINUTE));
        assert!(!restarted.quiet_active(NOW + 61 * MINUTE));

        // The toggle ends a timed Quiet mode early, then turns it back on.
        settings.toggle_quiet(NOW + 10 * MINUTE);
        assert!(!settings.quiet_active(NOW + 10 * MINUTE));
        settings.toggle_quiet(NOW + 61 * MINUTE);
        assert!(settings.quiet_active(NOW + 2 * HOUR));
    }

    #[test]
    fn surface_matrix() {
        let normal = Conditions {
            now: NOW,
            ..Conditions::default()
        };
        assert_eq!(KenSettings::default().surfaces(normal), Surfaces::default());

        let arcade = on().surfaces(normal);
        assert!(arcade.hud && arcade.hud_motion && arcade.ko_overlay && arcade.sparks);
        assert!(arcade.shake && arcade.sounds && arcade.chip && arcade.toasts);

        let subtle = KenSettings {
            intensity: Intensity::Subtle,
            ..on()
        }
        .surfaces(normal);
        assert!(subtle.chip && subtle.toasts && subtle.sounds);
        assert!(!subtle.hud && !subtle.ko_overlay && !subtle.sparks && !subtle.shake);

        let reduced = on().surfaces(Conditions {
            reduce_motion: true,
            ..normal
        });
        assert!(reduced.hud && !reduced.hud_motion, "HUD updates in place");
        assert!(!reduced.ko_overlay && !reduced.sparks && !reduced.shake);
        assert!(reduced.sounds, "sounds still play under reduce motion");

        let quiet = on().surfaces(Conditions {
            quiet_hours: true,
            ..normal
        });
        assert!(!quiet.sounds && !quiet.ko_overlay && quiet.chip);

        let mut quiet_mode = on();
        quiet_mode.quiet_for_hour(NOW);
        assert_eq!(quiet_mode.surfaces(normal), Surfaces::default());
        assert!(
            quiet_mode
                .surfaces(Conditions {
                    now: NOW + 61 * MINUTE,
                    ..normal
                })
                .chip
        );
    }
}
