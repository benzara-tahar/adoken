//! Theme drafts: derive every semantic token from a small seed palette,
//! enforce the contrast contract, and write a `.adoken-theme.toml`.
//!
//! Used by `examples/port_zeron_themes.rs`, which generated the community
//! built-ins in `themes/`. The seed shape (editor background, shell, raised, card,
//! text ramp, accent, status, twelve syntax roles) follows zeron
//! `crates/theme/src/builtins.rs` (`Seeds`, `variant`) at
//! d1010657d69c9730f8020edc36cc2649b136b6fa (MIT); the mapping onto
//! adoken's tokens follows docs/design/tokens.md §2–§3.

use std::fmt;

use crate::file::resolve;
use crate::tokens::key_index;
use crate::{Appearance, Color, ColorTokens, TokenSpec, contrast_pairs};

/// The handful of colors a theme is recognizable by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seeds {
    pub appearance: Appearance,
    /// Main pane (the editor background in a VS Code theme).
    pub background: Color,
    /// Window chrome and sidebar.
    pub shell: Color,
    /// A lifted tone (list hover, inputs); tints overlays.
    pub raised: Color,
    /// Cards and panels.
    pub card: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    /// Links, info status, running pipelines.
    pub info: Color,
    /// Completed PRs, features.
    pub magenta: Color,
    pub syntax: SyntaxSeeds,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxSeeds {
    pub comment: Color,
    pub keyword: Color,
    pub string: Color,
    pub number: Color,
    pub type_name: Color,
    pub function: Color,
    pub property: Color,
    pub variable: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub tag: Color,
    pub attribute: Color,
}

/// One contrast repair made by [`DraftVariant::enforce_contrast`].
#[derive(Debug, Clone, PartialEq)]
pub struct Adjustment {
    pub key: &'static str,
    pub from: Color,
    pub to: Color,
    /// The worst pair before the repair.
    pub background: &'static str,
    pub before: f32,
    pub minimum: f32,
}

impl fmt::Display for Adjustment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} → {} (was {:.2}:1 on {}, needs {:.1}:1)",
            self.key, self.from, self.to, self.before, self.background, self.minimum
        )
    }
}

/// A complete, writable variant: every key defined (no fallback needed).
#[derive(Debug, Clone, PartialEq)]
pub struct DraftVariant {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
    /// Comment lines written above the variant.
    pub notes: Vec<String>,
    specs: Vec<TokenSpec>,
}

/// Alphas per appearance (dark, light), from the Dojo themes (tokens.md §2).
const HOVER: (f32, f32) = (0.06, 0.04);
const ACTIVE: (f32, f32) = (0.09, 0.08);
const SELECTED: (f32, f32) = (0.16, 0.12);
const SELECTED_MUTED: (f32, f32) = (0.07, 0.06);
const BORDER_SUBTLE: (f32, f32) = (0.06, 0.06);
const BORDER_DEFAULT: (f32, f32) = (0.10, 0.12);
const BORDER_STRONG: (f32, f32) = (0.18, 0.20);
const WASH: (f32, f32) = (0.12, 0.08);
const DIFF_LINE: (f32, f32) = (0.12, 0.10);
const DIFF_WORD: (f32, f32) = (0.24, 0.15);
const HUNK: (f32, f32) = (0.08, 0.06);
const SCRIM: (f32, f32) = (0.60, 0.32);

/// Near-black ink for text on light solids (tokens.md §3 `accent.on`).
const INK: Color = Color::hex("#0E0F13");

impl DraftVariant {
    /// Derive all tokens from `seeds`. Colors are the upstream values; call
    /// [`Self::enforce_contrast`] afterwards to meet the contract.
    pub fn from_seeds(id: impl Into<String>, name: impl Into<String>, seeds: &Seeds) -> Self {
        let dark = seeds.appearance.is_dark();
        let pick = |(d, l): (f32, f32)| if dark { d } else { l };
        // Washes and borders tint with white on dark, with the ink on light.
        let tone = if dark { Color::WHITE } else { seeds.text };
        let syntax = &seeds.syntax;
        let (accent_solid, accent_on) = solid_with_on(seeds.accent);
        let (danger_solid, danger_on) = solid_with_on(seeds.danger);
        let hover_l = if dark { 0.05 } else { -0.05 };

        let mut draft = Self {
            id: id.into(),
            name: name.into(),
            appearance: seeds.appearance,
            notes: Vec::new(),
            specs: vec![TokenSpec::Color(Color::BLACK); ColorTokens::<Color>::KEYS.len()],
        };
        let colors: [(&str, Color); 49] = [
            ("bg.app", seeds.shell),
            ("bg.sidebar", seeds.shell),
            ("bg.surface", seeds.background),
            ("bg.raised", seeds.card),
            (
                "bg.overlay",
                seeds.card.mix(seeds.raised, if dark { 0.34 } else { 0.02 }),
            ),
            (
                "bg.scrim",
                if dark { Color::BLACK } else { seeds.text }.with_alpha(pick(SCRIM)),
            ),
            (
                "bg.input",
                if dark {
                    seeds.shell.mix(seeds.background, 0.5)
                } else {
                    seeds.card
                },
            ),
            ("bg.code", seeds.background),
            ("bg.hover", tone.with_alpha(pick(HOVER))),
            ("bg.active", tone.with_alpha(pick(ACTIVE))),
            ("bg.selected", seeds.accent.with_alpha(pick(SELECTED))),
            ("bg.selected_muted", tone.with_alpha(pick(SELECTED_MUTED))),
            ("border.subtle", tone.with_alpha(pick(BORDER_SUBTLE))),
            ("border.default", tone.with_alpha(pick(BORDER_DEFAULT))),
            ("border.strong", tone.with_alpha(pick(BORDER_STRONG))),
            ("border.control", seeds.faint),
            ("text.primary", seeds.text),
            ("text.secondary", seeds.muted),
            ("text.tertiary", seeds.faint),
            ("text.disabled", seeds.faint.mix(seeds.background, 0.45)),
            ("text.link", seeds.info),
            ("accent.solid", accent_solid),
            (
                "accent.solid_hover",
                accent_solid.with_lightness(accent_solid.lightness() + hover_l),
            ),
            ("accent.on", accent_on),
            ("accent.fg", seeds.accent),
            ("accent.wash", seeds.accent.with_alpha(pick(WASH))),
            ("status.success.fg", seeds.success),
            ("status.success.wash", seeds.success.with_alpha(pick(WASH))),
            ("status.warning.fg", seeds.warning),
            ("status.warning.wash", seeds.warning.with_alpha(pick(WASH))),
            ("status.danger.fg", seeds.danger),
            ("status.danger.wash", seeds.danger.with_alpha(pick(WASH))),
            ("status.danger.solid", danger_solid),
            ("status.danger.on", danger_on),
            ("status.info.fg", seeds.info),
            ("status.info.wash", seeds.info.with_alpha(pick(WASH))),
            ("shadow.color", if dark { Color::BLACK } else { seeds.text }),
            ("diff.add.bg", seeds.success.with_alpha(pick(DIFF_LINE))),
            ("diff.add.word", seeds.success.with_alpha(pick(DIFF_WORD))),
            ("diff.del.bg", seeds.danger.with_alpha(pick(DIFF_LINE))),
            ("diff.del.word", seeds.danger.with_alpha(pick(DIFF_WORD))),
            ("diff.hunk.bg", seeds.info.with_alpha(pick(HUNK))),
            ("pr.completed", seeds.magenta),
            ("wi.epic", seeds.warning.mix(seeds.danger, 0.5)),
            ("wi.issue", seeds.magenta.mix(seeds.danger, 0.4)),
            ("ken.primary", accent_solid),
            ("ken.secondary", seeds.warning),
            (
                "ken.outline",
                if dark {
                    seeds.shell.mix(Color::BLACK, 0.6)
                } else {
                    seeds.text.mix(Color::BLACK, 0.4)
                },
            ),
            ("ken.spark", seeds.warning.mix(Color::WHITE, 0.5)),
        ];
        for (key, color) in colors {
            draft.set(key, color);
        }
        let syntax_colors = [
            ("syntax.keyword", syntax.keyword),
            ("syntax.string", syntax.string),
            ("syntax.number", syntax.number),
            ("syntax.comment", syntax.comment),
            ("syntax.function", syntax.function),
            ("syntax.type", syntax.type_name),
            ("syntax.variable", syntax.variable),
            ("syntax.property", syntax.property),
            ("syntax.operator", syntax.operator),
            ("syntax.punctuation", syntax.punctuation),
            ("syntax.tag", syntax.tag),
            ("syntax.attribute", syntax.attribute),
        ];
        for (key, color) in syntax_colors {
            draft.set(key, color);
        }
        // tokens.md §2.6/§2.8 and §3: roles that follow another token.
        let aliases = [
            ("border.focus", "accent.fg"),
            ("text.match", "accent.fg"),
            ("diff.add.gutter", "status.success.fg"),
            ("diff.del.gutter", "status.danger.fg"),
            ("diff.hunk.fg", "text.link"),
            ("diff.comment", "status.warning.fg"),
            ("syntax.constant", "syntax.number"),
            ("pr.active", "status.info.fg"),
            ("pr.draft", "text.tertiary"),
            ("pr.abandoned", "text.disabled"),
            ("vote.approved", "status.success.fg"),
            ("vote.approved_suggestions", "status.success.fg"),
            ("vote.waiting", "status.warning.fg"),
            ("vote.rejected", "status.danger.fg"),
            ("vote.none", "text.tertiary"),
            ("pipeline.succeeded", "status.success.fg"),
            ("pipeline.failed", "status.danger.fg"),
            ("pipeline.partial", "status.warning.fg"),
            ("pipeline.running", "status.info.fg"),
            ("pipeline.queued", "text.tertiary"),
            ("pipeline.canceled", "text.tertiary"),
            ("wi.bug", "status.danger.fg"),
            ("wi.task", "status.warning.fg"),
            ("wi.story", "status.info.fg"),
            ("wi.feature", "pr.completed"),
            ("wi.other", "text.secondary"),
            ("priority.p1", "status.danger.fg"),
            ("priority.p2", "status.warning.fg"),
            ("priority.p3", "text.secondary"),
            ("priority.p4", "text.tertiary"),
        ];
        for (key, target) in aliases {
            draft.alias(key, target);
        }
        debug_assert_eq!(
            colors.len() + syntax_colors.len() + aliases.len(),
            ColorTokens::<Color>::KEYS.len(),
            "every key is derived exactly once"
        );
        draft
    }

    /// Set `key` to a color. `false` for an unknown key.
    pub fn set(&mut self, key: &str, color: Color) -> bool {
        key_index(key)
            .map(|slot| self.specs[slot] = TokenSpec::Color(color))
            .is_some()
    }

    /// Make `key` follow `target`. `false` for an unknown key or target.
    pub fn alias(&mut self, key: &str, target: &str) -> bool {
        match (key_index(key), key_index(target)) {
            (Some(slot), Some(target)) => {
                self.specs[slot] = TokenSpec::Alias(target);
                true
            }
            _ => false,
        }
    }

    /// The resolved color of `key`.
    pub fn get(&self, key: &str) -> Option<Color> {
        key_index(key).and_then(|slot| resolve(slot, &self.specs))
    }

    /// Every token resolved (a cycle resolves to black; drafts built by
    /// [`Self::from_seeds`] have none).
    pub fn colors(&self) -> ColorTokens {
        ColorTokens::from_fn(|slot| resolve(slot, &self.specs).unwrap_or(Color::BLACK))
    }

    /// Nudge failing foregrounds in OKLab lightness (tokens.md §9.3) until
    /// every contrast rule passes, plus the pairs components draw on
    /// overlays ([`COMPONENT_PAIRS`]). Aliased foregrounds repair the token
    /// they alias. Returns what changed, one entry per repaired token.
    pub fn enforce_contrast(&mut self) -> Vec<Adjustment> {
        let mut adjustments: Vec<Adjustment> = Vec::new();
        for _ in 0..8 {
            let colors = self.colors();
            let pairs = requirements(&colors);
            // Failing roots, in key order for deterministic output.
            let mut roots: Vec<usize> = pairs
                .iter()
                .filter(|pair| !pair.passes(&colors))
                .map(|pair| self.root(pair.foreground))
                .collect();
            roots.sort_unstable();
            roots.dedup();
            if roots.is_empty() {
                break;
            }
            for root in roots {
                let TokenSpec::Color(current) = self.specs[root] else {
                    continue;
                };
                let own: Vec<&Pair> = pairs
                    .iter()
                    .filter(|pair| self.root(pair.foreground) == root)
                    .collect();
                let worst = own
                    .iter()
                    .min_by(|a, b| a.ratio(&colors).total_cmp(&b.ratio(&colors)))
                    .expect("a failing pair exists");
                let required: Vec<(Color, f32)> = own
                    .iter()
                    .map(|pair| (pair.background, pair.minimum))
                    .collect();
                let repaired = current.ensure_contrast_each(&required);
                if repaired == current {
                    continue;
                }
                self.specs[root] = TokenSpec::Color(repaired);
                let key = ColorTokens::<Color>::KEYS[root];
                match adjustments.iter_mut().find(|a| a.key == key) {
                    Some(existing) => existing.to = repaired,
                    None => adjustments.push(Adjustment {
                        key,
                        from: current,
                        to: repaired,
                        background: worst.background_key,
                        before: worst.ratio(&colors),
                        minimum: worst.minimum,
                    }),
                }
            }
        }
        adjustments
    }

    /// The slot an alias chain ends at.
    fn root(&self, key: &str) -> usize {
        let mut slot = key_index(key).expect("contrast pairs name real keys");
        for _ in 0..=self.specs.len() {
            match self.specs[slot] {
                TokenSpec::Color(_) => return slot,
                TokenSpec::Alias(target) => slot = target,
            }
        }
        slot
    }
}

/// An opaque solid plus the better of white / ink on it. When neither
/// reaches 4.5:1 the solid's lightness moves (tokens.md §3) until one does.
fn solid_with_on(base: Color) -> (Color, Color) {
    let best_on = |solid: Color| {
        if Color::WHITE.contrast(solid) >= INK.contrast(solid) {
            Color::WHITE
        } else {
            INK
        }
    };
    let on = best_on(base);
    let solid = if on.contrast(base) >= 4.5 {
        base
    } else {
        // Away from the text color: darker under white, lighter under ink.
        let requirement = [(on, 4.5)];
        base.ensure_contrast_each(&requirement)
    };
    (solid, on)
}

/// Component pairs beyond the file contract that the built-in widgets draw
/// (chips and option cards on overlays): `(foreground, wash, base)`.
pub const COMPONENT_PAIRS: &[(&str, &str, &str)] = &[
    ("text.secondary", "bg.active", "bg.overlay"),
    ("accent.fg", "accent.wash", "bg.overlay"),
    ("status.success.fg", "status.success.wash", "bg.overlay"),
    ("status.warning.fg", "status.warning.wash", "bg.overlay"),
    ("status.danger.fg", "status.danger.wash", "bg.overlay"),
    ("status.info.fg", "status.info.wash", "bg.overlay"),
    ("text.primary", "accent.wash", "bg.raised"),
    ("text.secondary", "accent.wash", "bg.raised"),
];

struct Pair {
    foreground: &'static str,
    background_key: &'static str,
    background: Color,
    minimum: f32,
}

impl Pair {
    fn ratio(&self, colors: &ColorTokens) -> f32 {
        colors
            .get(self.foreground)
            .expect("real key")
            .contrast(self.background)
    }

    fn passes(&self, colors: &ColorTokens) -> bool {
        self.ratio(colors) + f32::EPSILON >= self.minimum
    }
}

fn requirements(colors: &ColorTokens) -> Vec<Pair> {
    let lookup = |key: &str| *colors.get(key).expect("real key");
    let mut pairs: Vec<Pair> = contrast_pairs(colors)
        .map(|(foreground, background_key, background, minimum)| Pair {
            foreground,
            background_key,
            background,
            minimum,
        })
        .collect();
    for &(foreground, wash, base) in COMPONENT_PAIRS {
        let base = lookup(base).blend_over(colors.bg_app);
        pairs.push(Pair {
            foreground: ColorTokens::<Color>::KEYS[key_index(foreground).expect("real key")],
            background_key: ColorTokens::<Color>::KEYS[key_index(wash).expect("real key")],
            background: lookup(wash).blend_over(base),
            minimum: 4.5,
        });
    }
    pairs
}

/// A theme file in the making.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftFamily {
    pub name: String,
    pub author: Option<String>,
    pub license: Option<String>,
    /// Comment lines written at the top of the file.
    pub notes: Vec<String>,
    pub variants: Vec<DraftVariant>,
}

impl DraftFamily {
    /// The `.adoken-theme.toml` text (tokens.md §9.2): every key written, in
    /// contract order, one blank line between token groups.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        let comment = |out: &mut String, lines: &[String]| {
            for line in lines {
                for part in line.lines() {
                    if part.is_empty() {
                        out.push_str("#\n");
                    } else {
                        out.push_str(&format!("# {part}\n"));
                    }
                }
            }
        };
        comment(&mut out, &self.notes);
        out.push_str(&format!("name = {}\n", quote(&self.name)));
        if let Some(author) = &self.author {
            out.push_str(&format!("author = {}\n", quote(author)));
        }
        if let Some(license) = &self.license {
            out.push_str(&format!("license = {}\n", quote(license)));
        }
        for variant in &self.variants {
            out.push('\n');
            comment(&mut out, &variant.notes);
            out.push_str("[[variant]]\n");
            out.push_str(&format!(
                "appearance = \"{}\"\n",
                variant.appearance.as_str()
            ));
            out.push_str(&format!("id = {}\n", quote(&variant.id)));
            out.push_str(&format!("name = {}\n", quote(&variant.name)));
            out.push_str("\n[variant.colors]\n");
            let mut group = "";
            for (slot, key) in ColorTokens::<Color>::KEYS.iter().enumerate() {
                let head = key.split('.').next().unwrap_or(key);
                if !group.is_empty() && head != group && !same_block(group, head) {
                    out.push('\n');
                }
                group = head;
                let value = match variant.specs[slot] {
                    TokenSpec::Color(color) => color.to_string(),
                    TokenSpec::Alias(target) => format!("@{}", ColorTokens::<Color>::KEYS[target]),
                };
                out.push_str(&format!("{key} = \"{value}\"\n"));
            }
        }
        out
    }
}

/// Status and shadow share a block, as do the domain groups (tokens.md §2).
fn same_block(previous: &str, next: &str) -> bool {
    const DOMAIN: &[&str] = &["pr", "vote", "pipeline", "wi", "priority"];
    (previous == "status" && next == "shadow")
        || (DOMAIN.contains(&previous) && DOMAIN.contains(&next))
}

fn quote(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_theme;

    fn seeds(appearance: Appearance) -> Seeds {
        let c = Color::hex;
        let dark = appearance.is_dark();
        // Deliberately weak upstream text ramp and syntax so enforcement has
        // work to do.
        Seeds {
            appearance,
            background: if dark { c("#1e1e2e") } else { c("#eff1f5") },
            shell: if dark { c("#181825") } else { c("#e6e9ef") },
            raised: if dark { c("#313244") } else { c("#ccd0da") },
            card: if dark { c("#242436") } else { c("#ffffff") },
            text: if dark { c("#cdd6f4") } else { c("#4c4f69") },
            muted: if dark { c("#a6adc8") } else { c("#6c6f85") },
            faint: if dark { c("#6c7086") } else { c("#9ca0b0") },
            accent: if dark { c("#cba6f7") } else { c("#8839ef") },
            danger: if dark { c("#f38ba8") } else { c("#d20f39") },
            warning: if dark { c("#f9e2af") } else { c("#df8e1d") },
            success: if dark { c("#a6e3a1") } else { c("#40a02b") },
            info: if dark { c("#89b4fa") } else { c("#1e66f5") },
            magenta: if dark { c("#f5c2e7") } else { c("#ea76cb") },
            syntax: SyntaxSeeds {
                comment: if dark { c("#585b70") } else { c("#acb0be") },
                keyword: if dark { c("#cba6f7") } else { c("#8839ef") },
                string: if dark { c("#a6e3a1") } else { c("#40a02b") },
                number: if dark { c("#fab387") } else { c("#fe640b") },
                type_name: if dark { c("#f9e2af") } else { c("#df8e1d") },
                function: if dark { c("#89b4fa") } else { c("#1e66f5") },
                property: if dark { c("#94e2d5") } else { c("#179299") },
                variable: if dark { c("#cdd6f4") } else { c("#4c4f69") },
                operator: if dark { c("#89dceb") } else { c("#04a5e5") },
                punctuation: if dark { c("#9399b2") } else { c("#7c7f93") },
                tag: if dark { c("#f38ba8") } else { c("#d20f39") },
                attribute: if dark { c("#f5c2e7") } else { c("#ea76cb") },
            },
        }
    }

    #[test]
    fn enforced_drafts_write_strict_files_without_issues() {
        for appearance in Appearance::ALL {
            let mut variant = DraftVariant::from_seeds(
                format!("t-{}", appearance.as_str()),
                "T",
                &seeds(appearance),
            );
            let adjustments = variant.enforce_contrast();
            assert!(!adjustments.is_empty(), "the weak seeds need repairs");
            let family = DraftFamily {
                name: "Test \"quoted\"".into(),
                author: None,
                license: Some("MIT".into()),
                notes: vec!["generated".into(), String::new()],
                variants: vec![variant],
            };
            let loaded = parse_theme("t", &family.to_toml(), None).unwrap();
            let issues: Vec<String> = loaded.issues.iter().map(ToString::to_string).collect();
            assert!(issues.is_empty(), "{appearance:?}: {}", issues.join("\n"));
            assert_eq!(loaded.family.name, "Test \"quoted\"");
        }
    }

    #[test]
    fn enforcement_changes_only_failing_tokens_and_keeps_aliases() {
        let mut variant = DraftVariant::from_seeds("t", "T", &seeds(Appearance::Dark));
        let before = variant.colors();
        let adjustments = variant.enforce_contrast();
        let after = variant.colors();
        // text.primary already passed: untouched.
        assert_eq!(after.text_primary, before.text_primary);
        // The dim comment was lifted, lighter (dark theme), and reported once.
        let comment = adjustments
            .iter()
            .find(|a| a.key == "syntax.comment")
            .expect("comment repaired");
        assert_eq!(comment.from, Color::hex("#585b70"));
        assert!(comment.to.lightness() > comment.from.lightness());
        assert!(comment.before < 4.5 && comment.minimum == 4.5);
        // Aliases survive: pr.draft still follows text.tertiary.
        assert_eq!(after.pr_draft, after.text_tertiary);
        assert!(variant.get("pr.draft") == Some(after.text_tertiary));
    }

    #[test]
    fn component_pairs_hold_after_enforcement() {
        for appearance in Appearance::ALL {
            let mut variant = DraftVariant::from_seeds("t", "T", &seeds(appearance));
            variant.enforce_contrast();
            let colors = variant.colors();
            for pair in requirements(&colors) {
                assert!(
                    pair.passes(&colors),
                    "{} on {}: {:.2}",
                    pair.foreground,
                    pair.background_key,
                    pair.ratio(&colors)
                );
            }
        }
    }

    #[test]
    fn solids_get_a_readable_on_color() {
        for base in ["#cba6f7", "#007acc", "#f9e2af", "#808080", "#ff79c6"] {
            let (solid, on) = solid_with_on(Color::hex(base));
            assert!(on.contrast(solid) >= 4.5, "{base}: {solid} / {on}");
        }
    }

    #[test]
    fn set_and_alias_reject_unknown_keys() {
        let mut variant = DraftVariant::from_seeds("t", "T", &seeds(Appearance::Light));
        assert!(!variant.set("bg.nope", Color::WHITE));
        assert!(!variant.alias("bg.app", "bg.nope"));
        assert!(variant.alias("text.link", "accent.fg"));
        assert_eq!(variant.get("text.link"), variant.get("accent.fg"));
    }
}
