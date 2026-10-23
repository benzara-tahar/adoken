//! Ken, adoken's opt-in arcade layer (docs/design/ken.md): review hits,
//! combos with rank captions, K.O.s when your PRs merge, daily streaks with
//! shields and pause, badges, and the weekly fight card.
//!
//! - [`engine`]: the pure, clock-injected state machine and anti-gaming
//!   rules (§3). Input [`Action`]s, output [`Effect`]s + a [`Journal`].
//! - [`source`]: store → actions (sync events, the review view's viewed
//!   marks). [`persist`]: local-only `ken_*` tables via
//!   `Store::migrate_ext("ken")`.
//! - [`settings`]: `settings.toml [ken]` and the surface matrix (§2, §6).
//! - [`week`] + [`card`]: the fight card (§5) and its 1200 × 675 PNG.
//! - [`glyph`] + [`sparks`]: original arcade glyph geometry and particles.
//! - [`celebrate`] + [`confetti`]: celebrations for finished work (spec
//!   0022, ken.md §12), with Ken on or off.
//!
//! No gpui dependency; the app (`crates/app/src/ken`) renders and plays.
//! Private: nothing here talks to Azure DevOps or leaves the machine except
//! an explicit fight-card export.

pub mod card;
pub mod celebrate;
pub mod confetti;
pub mod engine;
pub mod glyph;
pub mod moves;
pub mod persist;
pub mod seal;
pub mod settings;
pub mod source;
pub mod sparks;
pub mod streak;
pub mod week;

pub use engine::{Action, Applied, Badge, Ctx, Effect, Journal, Ken, Origin, PrFacts, Rejection};
pub use moves::{HitKind, Name, Naming, Rank};
pub use settings::{Conditions, Intensity, KenSettings, Surfaces};
pub use streak::Streak;
pub use week::{FightCard, Week};

/// ken.md §8: names that must never appear in Ken copy (lowercase
/// substrings). Checked by tests over every string Ken shows.
pub const BANNED_NAMES: &[&str] = &[
    "hadou",
    "hadok",
    "shoryu",
    "tatsumaki",
    "senpukyaku",
    "shinku",
    "metsu",
    "sonic boom",
    "spinning bird",
    "yoga fire",
    "yoga flame",
    "tiger uppercut",
    "tiger shot",
    "kikoken",
    "raging demon",
    "shun goku",
    "focus attack",
    "super combo",
    "ultra combo",
    "ryu",
    "ken masters",
    "chun-li",
    "guile",
    "akuma",
    "gouki",
    "gouken",
    "sagat",
    "bison",
    "vega",
    "finish him",
    "fatality",
    "flawless victory",
    "toasty",
    "new challenger",
    "round 1",
    "fight!",
    "you win",
    "you lose",
    "perfect",
    "continue?",
    "next battle",
];
