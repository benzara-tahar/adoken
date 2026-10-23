//! The semantic token contract (docs/design/tokens.md).
//!
//! Color keys are declared exactly once, in [`color_tokens!`] below: the
//! dotted key a theme file uses and the Rust field components read are
//! generated from the same line, so the file format and the code cannot drift.

use crate::Color;

macro_rules! color_tokens {
    ($($field:ident => $key:literal,)+) => {
        /// Every semantic color role. Generic over the color representation so
        /// renderers can hold a pre-converted copy (e.g. `ColorTokens<gpui::Hsla>`)
        /// without re-declaring the key list.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct ColorTokens<C = Color> {
            $(#[doc = concat!("`", $key, "`")] pub $field: C,)+
        }

        impl<C> ColorTokens<C> {
            /// Dotted theme-file keys, in declaration order.
            pub const KEYS: &'static [&'static str] = &[$($key),+];

            pub(crate) fn from_fn(mut value: impl FnMut(usize) -> C) -> Self {
                let mut index = 0..;
                Self {
                    $($field: value(index.next().unwrap_or_default()),)+
                }
            }

            pub fn map<D>(&self, mut f: impl FnMut(&C) -> D) -> ColorTokens<D> {
                ColorTokens { $($field: f(&self.$field),)+ }
            }

            pub fn get(&self, key: &str) -> Option<&C> {
                match key {
                    $($key => Some(&self.$field),)+
                    _ => None,
                }
            }

            pub fn iter(&self) -> impl Iterator<Item = (&'static str, &C)> {
                [$(($key, &self.$field)),+].into_iter()
            }
        }
    };
}

color_tokens! {
    bg_app => "bg.app",
    bg_sidebar => "bg.sidebar",
    bg_surface => "bg.surface",
    bg_raised => "bg.raised",
    bg_overlay => "bg.overlay",
    bg_scrim => "bg.scrim",
    bg_input => "bg.input",
    bg_code => "bg.code",
    bg_hover => "bg.hover",
    bg_active => "bg.active",
    bg_selected => "bg.selected",
    bg_selected_muted => "bg.selected_muted",

    border_subtle => "border.subtle",
    border_default => "border.default",
    border_strong => "border.strong",
    border_control => "border.control",
    border_focus => "border.focus",

    text_primary => "text.primary",
    text_secondary => "text.secondary",
    text_tertiary => "text.tertiary",
    text_disabled => "text.disabled",
    text_link => "text.link",
    text_match => "text.match",

    accent_solid => "accent.solid",
    accent_solid_hover => "accent.solid_hover",
    accent_on => "accent.on",
    accent_fg => "accent.fg",
    accent_wash => "accent.wash",

    status_success_fg => "status.success.fg",
    status_success_wash => "status.success.wash",
    status_warning_fg => "status.warning.fg",
    status_warning_wash => "status.warning.wash",
    status_danger_fg => "status.danger.fg",
    status_danger_wash => "status.danger.wash",
    status_danger_solid => "status.danger.solid",
    status_danger_on => "status.danger.on",
    status_info_fg => "status.info.fg",
    status_info_wash => "status.info.wash",

    shadow_color => "shadow.color",

    diff_add_bg => "diff.add.bg",
    diff_add_word => "diff.add.word",
    diff_add_gutter => "diff.add.gutter",
    diff_del_bg => "diff.del.bg",
    diff_del_word => "diff.del.word",
    diff_del_gutter => "diff.del.gutter",
    diff_hunk_bg => "diff.hunk.bg",
    diff_hunk_fg => "diff.hunk.fg",
    diff_comment => "diff.comment",

    syntax_keyword => "syntax.keyword",
    syntax_string => "syntax.string",
    syntax_number => "syntax.number",
    syntax_constant => "syntax.constant",
    syntax_comment => "syntax.comment",
    syntax_function => "syntax.function",
    syntax_type => "syntax.type",
    syntax_variable => "syntax.variable",
    syntax_property => "syntax.property",
    syntax_operator => "syntax.operator",
    syntax_punctuation => "syntax.punctuation",
    syntax_tag => "syntax.tag",
    syntax_attribute => "syntax.attribute",

    pr_active => "pr.active",
    pr_draft => "pr.draft",
    pr_completed => "pr.completed",
    pr_abandoned => "pr.abandoned",

    vote_approved => "vote.approved",
    vote_approved_suggestions => "vote.approved_suggestions",
    vote_waiting => "vote.waiting",
    vote_rejected => "vote.rejected",
    vote_none => "vote.none",

    pipeline_succeeded => "pipeline.succeeded",
    pipeline_failed => "pipeline.failed",
    pipeline_running => "pipeline.running",
    pipeline_queued => "pipeline.queued",
    pipeline_canceled => "pipeline.canceled",
    pipeline_partial => "pipeline.partial",

    wi_bug => "wi.bug",
    wi_task => "wi.task",
    wi_story => "wi.story",
    wi_feature => "wi.feature",
    wi_epic => "wi.epic",
    wi_issue => "wi.issue",
    wi_other => "wi.other",

    priority_p1 => "priority.p1",
    priority_p2 => "priority.p2",
    priority_p3 => "priority.p3",
    priority_p4 => "priority.p4",

    ken_primary => "ken.primary",
    ken_secondary => "ken.secondary",
    ken_outline => "ken.outline",
    ken_spark => "ken.spark",
}

pub(crate) fn key_index(key: &str) -> Option<usize> {
    static INDEX: std::sync::LazyLock<std::collections::HashMap<&'static str, usize>> =
        std::sync::LazyLock::new(|| {
            ColorTokens::<Color>::KEYS
                .iter()
                .enumerate()
                .map(|(slot, key)| (*key, slot))
                .collect()
        });
    INDEX.get(key).copied()
}

// Non-color tokens are built-in constants in v0.1 (tokens.md §1). Values in
// logical pixels at `font_scale = 1.0`.

/// tokens.md §5.1 — 4 px grid; names are the px values.
pub mod space {
    pub const S0: f32 = 0.0;
    pub const S2: f32 = 2.0;
    pub const S4: f32 = 4.0;
    pub const S6: f32 = 6.0;
    pub const S8: f32 = 8.0;
    pub const S12: f32 = 12.0;
    pub const S16: f32 = 16.0;
    pub const S20: f32 = 20.0;
    pub const S24: f32 = 24.0;
    pub const S32: f32 = 32.0;
    pub const S40: f32 = 40.0;
    pub const S48: f32 = 48.0;
    pub const S64: f32 = 64.0;
}

/// tokens.md §5.2 — layout sizes used so far.
pub mod size {
    pub const TITLEBAR: f32 = 38.0;
    pub const CONTROL_SM: f32 = 24.0;
    pub const CONTROL_MD: f32 = 28.0;
    pub const CONTROL_LG: f32 = 36.0;
    pub const ICON_XS: f32 = 12.0;
    pub const ICON_SM: f32 = 14.0;
    pub const ICON_MD: f32 = 16.0;
    pub const ICON_LG: f32 = 20.0;
    /// `size.avatar.sm` / `md` / `lg` (principles.md §4 Avatars).
    pub const AVATAR_SM: f32 = 16.0;
    pub const AVATAR_MD: f32 = 20.0;
    pub const AVATAR_LG: f32 = 28.0;
    pub const HIT_MIN: f32 = 24.0;
    pub const BORDER: f32 = 1.0;
    pub const PALETTE_WIDTH: f32 = 680.0;
    /// Palette width once the window is at least [`PALETTE_WIDE_MIN_WINDOW`] wide.
    pub const PALETTE_WIDTH_WIDE: f32 = 960.0;
    pub const PALETTE_WIDE_MIN_WINDOW: f32 = 1200.0;
    pub const PALETTE_ROW_COMPACT: f32 = 36.0;
    pub const PALETTE_ROW_DEFAULT: f32 = 44.0;
    pub const PALETTE_ROW_COMFORTABLE: f32 = 52.0;
    /// `size.sidebar` / `size.rail` (principles.md §2): the expanded sidebar
    /// and its collapsed icon rail; the pins peek is `SIDEBAR` wide.
    pub const SIDEBAR: f32 = 232.0;
    pub const RAIL: f32 = 56.0;
    /// Rail count badge pill height (`type.label` line height).
    pub const BADGE: f32 = 14.0;
    /// Standard toast width (principles.md §4).
    pub const TOAST: f32 = 360.0;
    pub const ACTION_MENU_WIDTH: f32 = 280.0;
    /// Pin row text caps (pins.md §3.3, §3.5): the work-item state word, and
    /// the fetching row's skeleton bar height.
    pub const PIN_STATE_WORD_MAX: f32 = 64.0;
    pub const PIN_SKELETON_BAR: f32 = 8.0;
    /// Work item detail (work-item-detail.md §15): pane width at which the
    /// properties rail appears, the rail's width, the prose column's max
    /// width, and one property grid cell's height.
    pub const DETAIL_WIDE_MIN: f32 = 1000.0;
    pub const DETAIL_PROPERTIES: f32 = 280.0;
    pub const DETAIL_PROSE_MAX: f32 = 720.0;
    pub const PROPERTY_CELL: f32 = 52.0;
    /// Activity drawer (activity.md §11): dense table rows (`size.row.dense`,
    /// independent of density), the drawer's minimum height, and its header
    /// and toolbar height (fixed across densities).
    pub const ROW_DENSE: f32 = 24.0;
    pub const DRAWER_MIN: f32 = 160.0;
    pub const DRAWER_HEADER: f32 = 32.0;
    /// `size.row.compact` (tokens.md §5.2): one-line rows of anchored menus
    /// and pickers (work-item-detail.md §3.3, §4.5).
    pub const ROW_COMPACT: f32 = 32.0;
    /// `size.picker_popover` (work-item-detail.md §15): minimum width of
    /// anchored pickers (people, area, iteration).
    pub const PICKER_POPOVER: f32 = 320.0;
    /// `size.coach_mark` (tours.md §9): guided tour coach mark width at
    /// `font_scale` 1.0 (scaled, at most the window width − 16).
    pub const COACH_MARK: f32 = 320.0;
}

/// tokens.md §6 z-layers: paint order, highest last.
pub mod z {
    pub const BASE: u16 = 0;
    pub const STICKY: u16 = 10;
    pub const DROPDOWN: u16 = 20;
    pub const TOAST: u16 = 30;
    pub const PALETTE: u16 = 40;
    pub const DIALOG: u16 = 50;
    /// Never takes focus, never intercepts pointer.
    pub const KEN: u16 = 60;
    pub const ONBOARDING: u16 = 70;
}

/// tokens.md §5.3.
pub mod radius {
    pub const XS: f32 = 3.0;
    pub const SM: f32 = 6.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const FULL: f32 = 9999.0;
}

/// tokens.md §5.4: element opacity for content that stays informative but
/// steps back (not a color: applied to whole elements such as avatars).
pub mod opacity {
    /// `opacity.dimmed`: e.g. required reviewers who haven't voted yet.
    pub const DIMMED: f32 = 0.5;
}

/// Which family a type step renders in (tokens.md §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRole {
    /// The UI font.
    Ui,
    /// Code blocks and diffs: the configured code font at the configured
    /// code font size (spec 0009 §3 Code font).
    Code,
    /// Code inside UI text (paths, ids): the code font, at the step's own
    /// size so it stays in proportion with the text around it.
    InlineCode,
    /// Keycaps: the platform's monospace face, whatever the code font.
    SystemMono,
}

/// One step of the type scale (tokens.md §4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeStyle {
    pub size: f32,
    pub line_height: f32,
    /// GPUI `FontWeight` numeric value.
    pub weight: f32,
    pub font: FontRole,
}

impl TypeStyle {
    const fn new(font: FontRole, size: f32, line_height: f32, weight: f32) -> Self {
        Self {
            size,
            line_height,
            weight,
            font,
        }
    }

    /// This step at code font size `size`: only [`FontRole::Code`] scales,
    /// its line height keeping the step's ratio (`type.code` 12/18 → 16/24),
    /// rounded to whole pixels so virtualized rows stay pixel-aligned.
    pub fn at_code_size(self, size: f32) -> Self {
        if self.font != FontRole::Code || size == self.size {
            return self;
        }
        Self {
            line_height: (self.line_height * size / self.size).round(),
            size,
            ..self
        }
    }
}

/// tokens.md §4 type scale.
pub mod typography {
    use super::{FontRole, TypeStyle};

    pub const DISPLAY: TypeStyle = TypeStyle::new(FontRole::Ui, 28.0, 34.0, 650.0);
    pub const TITLE: TypeStyle = TypeStyle::new(FontRole::Ui, 20.0, 26.0, 600.0);
    pub const HEADING: TypeStyle = TypeStyle::new(FontRole::Ui, 15.0, 20.0, 600.0);
    pub const BODY: TypeStyle = TypeStyle::new(FontRole::Ui, 13.0, 20.0, 400.0);
    pub const BODY_STRONG: TypeStyle = TypeStyle::new(FontRole::Ui, 13.0, 20.0, 560.0);
    pub const SMALL: TypeStyle = TypeStyle::new(FontRole::Ui, 12.0, 16.0, 400.0);
    pub const LABEL: TypeStyle = TypeStyle::new(FontRole::Ui, 11.0, 14.0, 560.0);
    pub const CODE: TypeStyle = TypeStyle::new(FontRole::Code, 12.0, 18.0, 400.0);
    pub const CODE_INLINE: TypeStyle = TypeStyle::new(FontRole::InlineCode, 12.0, 16.0, 400.0);
    pub const KBD: TypeStyle = TypeStyle::new(FontRole::SystemMono, 11.0, 14.0, 500.0);
}

/// One drop shadow of an elevation level; color is `shadow.color` at the
/// appearance's alpha (tokens.md §6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadowLayer {
    pub y: f32,
    pub blur: f32,
    pub spread: f32,
    pub alpha_dark: f32,
    pub alpha_light: f32,
}

const fn shadow(y: f32, blur: f32, spread: f32, alpha_dark: f32, alpha_light: f32) -> ShadowLayer {
    ShadowLayer {
        y,
        blur,
        spread,
        alpha_dark,
        alpha_light,
    }
}

/// tokens.md §6. Levels ≥ 1 also draw a `border.default` edge.
pub mod elevation {
    use super::{ShadowLayer, shadow};

    pub const LEVEL_0: &[ShadowLayer] = &[];
    pub const LEVEL_1: &[ShadowLayer] = &[shadow(1.0, 2.0, 0.0, 0.24, 0.06)];
    pub const LEVEL_2: &[ShadowLayer] = &[
        shadow(2.0, 6.0, 0.0, 0.30, 0.06),
        shadow(8.0, 24.0, -4.0, 0.45, 0.12),
    ];
    pub const LEVEL_3: &[ShadowLayer] = &[
        shadow(4.0, 12.0, 0.0, 0.30, 0.08),
        shadow(24.0, 64.0, -8.0, 0.55, 0.20),
    ];
}

/// tokens.md §7.1 durations in milliseconds. Under reduce motion every
/// animation snaps to its end state.
pub mod motion {
    pub const INSTANT: u32 = 0;
    pub const FAST: u32 = 90;
    pub const QUICK: u32 = 140;
    pub const BASE: u32 = 200;
    pub const SLOW: u32 = 320;
    pub const EXPRESSIVE: u32 = 480;

    /// Exit durations as listed in tokens.md §7.1 (≈ ⅔ of the entrance).
    pub const QUICK_EXIT: u32 = 90;
    pub const BASE_EXIT: u32 = 130;
    pub const SLOW_EXIT: u32 = 210;
    pub const EXPRESSIVE_EXIT: u32 = 320;

    /// `motion.hover_intent`: dwell before a hover-opened surface opens
    /// (pins peek, pins.md §5.3; Z-7 submenus).
    pub const HOVER_INTENT: u32 = 150;
    /// Z-7 grace: a hover-opened surface stays this long after the pointer
    /// leaves the trigger, the surface and the corridor between them.
    pub const HOVER_GRACE: u32 = 300;
    /// `glyph-spin`: one turn of the running `loader-circle` (tokens.md §7.3).
    pub const GLYPH_SPIN: u32 = 1000;
    /// Standard toast lifetime (principles.md §4; pins Undo toast, 0014 §3.3).
    pub const TOAST_LIFE: u32 = 5000;
    /// Commits arrive in bursts during sync; derived snapshots recompute
    /// once they settle (pins status, like Today's day).
    pub const COMMIT_DEBOUNCE: u32 = 400;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique_and_none_is_a_prefix_of_another() {
        // TOML cannot hold both `a.b = "#..."` and `a.b.c = "#..."`, so a key
        // that prefixes another would be unwritable in a theme file.
        let keys = ColorTokens::<Color>::KEYS;
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
                assert!(!b.starts_with(&format!("{a}.")), "{a} prefixes {b}");
                assert!(!a.starts_with(&format!("{b}.")), "{b} prefixes {a}");
            }
        }
    }

    #[test]
    fn from_fn_get_and_iter_agree_on_key_order() {
        let tokens = ColorTokens::from_fn(|i| i);
        for (i, (key, value)) in tokens.iter().enumerate() {
            assert_eq!(*value, i);
            assert_eq!(tokens.get(key), Some(&i));
            assert_eq!(key_index(key), Some(i));
        }
        assert_eq!(tokens.get("bg.nope"), None);
    }

    #[test]
    fn only_code_blocks_scale_with_the_code_font_size() {
        let code = typography::CODE.at_code_size(16.0);
        assert_eq!((code.size, code.line_height), (16.0, 24.0));
        // 13 × 1.5 = 19.5 → whole pixels.
        assert_eq!(typography::CODE.at_code_size(13.0).line_height, 20.0);
        assert_eq!(typography::CODE.at_code_size(11.0).line_height, 17.0);
        for step in [typography::CODE_INLINE, typography::KBD, typography::BODY] {
            assert_eq!(step.at_code_size(16.0), step);
        }
    }
}
