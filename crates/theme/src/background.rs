//! Custom backgrounds (docs/design/tokens.md §10, spec 0007 FR-4..FR-6):
//! the `[background]` settings table, the built-in procedural palettes, and
//! the readability guard. Pure data and math; decoding, blurring and
//! painting live in the app (`crates/app/src/background`).

use std::fmt;

use toml::{Table, Value};

use crate::file::suggest;
use crate::{Appearance, Color, ColorTokens};

/// Name of the settings table (`settings.toml [background]`).
pub const SETTINGS_TABLE: &str = "background";

/// Backdrop blur radius (px at 1x) for overlays (palette, menus, dialogs)
/// painted over a custom background: frosts the part of the image and panes
/// they cover. Overlays stay ≥ 0.9 opaque (§10.2), so this only softens edges
/// and detail; the full-window blur is baked into the texture (FR-4).
pub const OVERLAY_BLUR: f32 = 16.0;

/// Where the image comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BackgroundSource {
    Builtin(BuiltinBackground),
    /// The user's path as written (may start with `~`); the app renders the
    /// imported copy under `<data>/backgrounds/` (§10.3).
    Image(String),
}

impl BackgroundSource {
    const BUILTIN_PREFIX: &str = "builtin:";

    fn parse(text: &str) -> Result<Option<Self>, IssueKind> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        match text.strip_prefix(Self::BUILTIN_PREFIX) {
            Some(id) => BuiltinBackground::parse(id)
                .map(|builtin| Some(Self::Builtin(builtin)))
                .ok_or_else(|| IssueKind::UnknownBuiltin {
                    id: id.to_owned(),
                    suggestion: suggest(id, &BuiltinBackground::IDS),
                }),
            None => Ok(Some(Self::Image(text.to_owned()))),
        }
    }

    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin(_))
    }
}

impl fmt::Display for BackgroundSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(builtin) => write!(f, "{}{}", Self::BUILTIN_PREFIX, builtin.id()),
            Self::Image(path) => f.write_str(path),
        }
    }
}

/// tokens.md §10.5: original, code-generated backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinBackground {
    Dusk,
    Lantern,
    Ink,
}

impl BuiltinBackground {
    pub const ALL: [Self; 3] = [Self::Dusk, Self::Lantern, Self::Ink];
    const IDS: [&'static str; 3] = ["dusk", "lantern", "ink"];

    pub fn id(self) -> &'static str {
        Self::IDS[self as usize]
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|builtin| builtin.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dusk => "Dusk",
            Self::Lantern => "Lantern",
            Self::Ink => "Ink",
        }
    }
}

/// A soft radial light: `color` at `strength`, centered at (`x`, `y`) as
/// fractions of the image, fading out over `radius` (fraction of the diagonal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glow {
    pub color: Color,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub strength: f32,
}

/// Colors of the code-generated backgrounds. They live here because only
/// `crates/theme` may spell colors (0007 FR-1); the generators in the app read
/// them. Values from tokens.md §10.5 unless noted.
pub mod palette {
    use super::Glow;
    use crate::Color;

    /// `builtin:dusk` vertical gradient stops, top → bottom.
    pub const DUSK: [Color; 3] = [
        Color::hex("#1B0F1E"),
        Color::hex("#3A1420"),
        Color::hex("#F26B3A"),
    ];
    /// Share of the height (from the bottom) the brightest stop occupies.
    pub const DUSK_HORIZON: f32 = 0.08;
    pub const DUSK_GRAIN: f32 = 0.03;

    pub const LANTERN_BASE: Color = Color::hex("#120C0A");
    pub const LANTERN_GLOWS: [Glow; 2] = [
        Glow {
            color: Color::hex("#F26B3A"),
            x: 0.30,
            y: 0.70,
            radius: 0.55,
            strength: 0.85,
        },
        Glow {
            color: Color::hex("#FFD23F"),
            x: 0.75,
            y: 0.25,
            radius: 0.42,
            strength: 0.55,
        },
    ];
    pub const LANTERN_GRAIN: f32 = 0.03;

    /// `builtin:ink` sumi-e wash: noise mapped between these two.
    pub const INK: [Color; 2] = [Color::hex("#0E0F13"), Color::hex("#2A2C35")];
    pub const INK_GRAIN: f32 = 0.02;

    /// The demo illustration (original, procedural): a dusk sky over layered
    /// mountains, a shrine gate on the near ridge, a blossoming branch and
    /// drifting petals. Not a built-in source; used by demo mode and gallery
    /// stories to exercise the custom-image path.
    pub mod illustration {
        use crate::Color;

        /// Sky gradient, zenith → horizon.
        pub const SKY: [Color; 4] = [
            Color::hex("#141236"),
            Color::hex("#3B2767"),
            Color::hex("#B35A86"),
            Color::hex("#FFB28A"),
        ];
        pub const SUN: Color = Color::hex("#FFE9C2");
        pub const SUN_GLOW: Color = Color::hex("#FF9A6B");
        pub const STAR: Color = Color::hex("#FFF4E0");
        pub const CLOUD: Color = Color::hex("#E88AA0");
        /// Mountain ranges, far → near (aerial perspective).
        pub const RIDGES: [Color; 4] = [
            Color::hex("#8A5A8E"),
            Color::hex("#5C3D72"),
            Color::hex("#34244F"),
            Color::hex("#191230"),
        ];
        pub const GATE: Color = Color::hex("#D9442B");
        pub const GATE_SHADE: Color = Color::hex("#7A2218");
        pub const BRANCH: Color = Color::hex("#1A0F1C");
        pub const BLOSSOM: [Color; 3] = [
            Color::hex("#FFC7D6"),
            Color::hex("#F59AB6"),
            Color::hex("#E0708F"),
        ];
        pub const GRAIN: f32 = 0.02;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    #[default]
    Cover,
    Contain,
    Fill,
    Tile,
    Center,
}

impl Fit {
    const ALL: [(Self, &'static str); 5] = [
        (Self::Cover, "cover"),
        (Self::Contain, "contain"),
        (Self::Fill, "fill"),
        (Self::Tile, "tile"),
        (Self::Center, "center"),
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    Center,
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Anchor {
    const ALL: [(Self, &'static str); 9] = [
        (Self::Center, "center"),
        (Self::Top, "top"),
        (Self::Bottom, "bottom"),
        (Self::Left, "left"),
        (Self::Right, "right"),
        (Self::TopLeft, "top-left"),
        (Self::TopRight, "top-right"),
        (Self::BottomLeft, "bottom-left"),
        (Self::BottomRight, "bottom-right"),
    ];

    /// Where the overflow goes, as (x, y) fractions: 0 = keep the left/top
    /// edge, 1 = keep the right/bottom edge.
    pub fn fraction(self) -> (f32, f32) {
        match self {
            Self::Center => (0.5, 0.5),
            Self::Top => (0.5, 0.0),
            Self::Bottom => (0.5, 1.0),
            Self::Left => (0.0, 0.5),
            Self::Right => (1.0, 0.5),
            Self::TopLeft => (0.0, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::BottomRight => (1.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Legibility {
    /// §10.4: raise dim, then surface opacity, until text passes.
    #[default]
    Auto,
    /// Keep the user's values; warn when the check fails.
    Off,
}

impl Legibility {
    const ALL: [(Self, &'static str); 2] = [(Self::Auto, "auto"), (Self::Off, "off")];
}

/// `tint`: a color or a token alias (`"@accent.solid"`), resolved against the
/// active theme at paint time so it follows theme changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Color(Color),
    /// Index into [`ColorTokens::KEYS`].
    Token(usize),
}

impl Tint {
    fn parse(text: &str) -> Result<Option<Self>, IssueKind> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        if let Some(key) = text.strip_prefix('@') {
            return ColorTokens::<Color>::KEYS
                .iter()
                .position(|candidate| *candidate == key)
                .map(|index| Some(Self::Token(index)))
                .ok_or(IssueKind::InvalidValue {
                    expected: "a color or an alias to a known token",
                });
        }
        text.parse()
            .map(|color| Some(Self::Color(color)))
            .map_err(|_| IssueKind::InvalidValue {
                expected: "a color (#rrggbb) or an alias (@token.key)",
            })
    }

    pub fn resolve(self, colors: &ColorTokens) -> Color {
        match self {
            Self::Color(color) => color,
            Self::Token(index) => colors
                .iter()
                .nth(index)
                .map_or(Color::TRANSPARENT, |(_, color)| *color),
        }
    }
}

/// tokens.md §10.1 view ids for `[background.views.<view>]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewId {
    Onboarding,
    MyWork,
    Inbox,
    PullRequests,
    PrReview,
    WorkItems,
    Pipelines,
    SearchResults,
    Settings,
}

impl ViewId {
    pub const ALL: [Self; 9] = [
        Self::Onboarding,
        Self::MyWork,
        Self::Inbox,
        Self::PullRequests,
        Self::PrReview,
        Self::WorkItems,
        Self::Pipelines,
        Self::SearchResults,
        Self::Settings,
    ];
    const IDS: [&'static str; 9] = [
        "onboarding",
        "my_work",
        "inbox",
        "pull_requests",
        "pr_review",
        "work_items",
        "pipelines",
        "search_results",
        "settings",
    ];

    pub fn id(self) -> &'static str {
        Self::IDS[self as usize]
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|view| view.id() == id)
    }
}

/// The effective background for one (appearance, view).
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundConfig {
    pub enabled: bool,
    pub source: Option<BackgroundSource>,
    pub fit: Fit,
    pub anchor: Anchor,
    /// px at 1x, 0–64. Baked into the texture.
    pub blur: u8,
    /// 0.0–0.9; black scrim on dark, white on light. Paint-time.
    pub dim: f32,
    /// 0.0–1.5. Baked.
    pub saturation: f32,
    pub tint: Option<Tint>,
    /// 0.0–0.5. Paint-time.
    pub tint_strength: f32,
    /// Alpha of `bg.sidebar` / `bg.surface` / `bg.raised` over the image, 0.6–1.0.
    pub surface_opacity: f32,
    /// Alpha of `bg.overlay`, 0.9–1.0.
    pub overlay_opacity: f32,
    pub legibility: Legibility,
}

impl BackgroundConfig {
    /// Something to paint: enabled with a source.
    pub fn is_active(&self) -> bool {
        self.enabled && self.source.is_some()
    }

    /// Scrim color for [`Self::dim`] (§10.2 step 3).
    pub fn dim_color(appearance: Appearance) -> Color {
        if appearance.is_dark() {
            Color::BLACK
        } else {
            Color::WHITE
        }
    }
}

/// One settings layer; `None` = inherit. `Some(None)` on `source`/`tint`
/// means explicitly cleared (`source = ""`).
#[derive(Debug, Clone, Default, PartialEq)]
struct Layer {
    enabled: Option<bool>,
    source: Option<Option<BackgroundSource>>,
    fit: Option<Fit>,
    anchor: Option<Anchor>,
    blur: Option<u8>,
    dim: Option<f32>,
    saturation: Option<f32>,
    tint: Option<Option<Tint>>,
    tint_strength: Option<f32>,
    surface_opacity: Option<f32>,
    overlay_opacity: Option<f32>,
    legibility: Option<Legibility>,
}

/// Defaults (§10.1 and, for built-in sources, §10.5).
const DEFAULT_BLUR: u8 = 24;
const DEFAULT_DIM: f32 = 0.35;
const BUILTIN_BLUR: u8 = 0;
const BUILTIN_DIM: f32 = 0.2;
const DEFAULT_SURFACE_OPACITY: f32 = 0.82;
const DEFAULT_OVERLAY_OPACITY: f32 = 0.96;

const BLUR_RANGE: (f32, f32) = (0.0, 64.0);
const DIM_RANGE: (f32, f32) = (0.0, 0.9);
const SATURATION_RANGE: (f32, f32) = (0.0, 1.5);
const TINT_RANGE: (f32, f32) = (0.0, 0.5);
const SURFACE_RANGE: (f32, f32) = (0.6, 1.0);
const OVERLAY_RANGE: (f32, f32) = (0.9, 1.0);

/// Keys valid in any layer. The video keys are v0.2 (§10.6): accepted and
/// ignored so v0.2 settings files don't warn on v0.1.
const LAYER_KEYS: &[&str] = &[
    "enabled",
    "source",
    "fit",
    "anchor",
    "blur",
    "dim",
    "saturation",
    "tint",
    "tint_strength",
    "surface_opacity",
    "overlay_opacity",
    "legibility",
    "playback",
    "fps_cap",
    "pause_when",
];
const TOP_KEYS: &[&str] = &[
    "enabled",
    "source",
    "fit",
    "anchor",
    "blur",
    "dim",
    "saturation",
    "tint",
    "tint_strength",
    "surface_opacity",
    "overlay_opacity",
    "legibility",
    "playback",
    "fps_cap",
    "pause_when",
    "dark",
    "light",
    "views",
];

/// The parsed `[background]` table with its per-appearance and per-view
/// overrides. Precedence (last wins): `[background]` →
/// `[background.<appearance>]` → `[background.views.<view>]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BackgroundSettings {
    base: Layer,
    dark: Layer,
    light: Layer,
    views: Vec<(ViewId, Layer)>,
}

/// A tolerated problem in the `[background]` table. The offending key is
/// ignored or clamped; nothing else is affected.
#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundIssue {
    /// Dotted settings path, e.g. `background.views.pr_review.blur`.
    pub key: String,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IssueKind {
    UnknownKey {
        suggestion: Option<&'static str>,
    },
    UnknownView {
        suggestion: Option<&'static str>,
    },
    UnknownBuiltin {
        id: String,
        suggestion: Option<&'static str>,
    },
    InvalidValue {
        expected: &'static str,
    },
    /// Out of range; `clamped` is what is used.
    Clamped {
        value: f64,
        clamped: f64,
    },
}

impl fmt::Display for BackgroundIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = &self.key;
        let hint = |f: &mut fmt::Formatter<'_>, suggestion: &Option<&str>| match suggestion {
            Some(candidate) => write!(f, " Did you mean `{candidate}`?"),
            None => Ok(()),
        };
        match &self.kind {
            IssueKind::UnknownKey { suggestion } => {
                write!(f, "`{key}`: unknown key, ignored.")?;
                hint(f, suggestion)
            }
            IssueKind::UnknownView { suggestion } => {
                write!(f, "`{key}`: unknown view, ignored.")?;
                hint(f, suggestion)
            }
            IssueKind::UnknownBuiltin { id, suggestion } => {
                write!(f, "`{key}`: no built-in background `{id}`.")?;
                hint(f, suggestion)
            }
            IssueKind::InvalidValue { expected } => {
                write!(f, "`{key}`: expected {expected}; ignored.")
            }
            IssueKind::Clamped { value, clamped } => {
                write!(f, "`{key}`: {value} is out of range; using {clamped}.")
            }
        }
    }
}

impl BackgroundSettings {
    /// Parse the `[background]` table out of a whole `settings.toml`. A
    /// missing table is the default (no background). Only a TOML syntax error
    /// fails; bad values become issues and fall back.
    pub fn from_settings_toml(
        source: &str,
    ) -> Result<(Self, Vec<BackgroundIssue>), toml::de::Error> {
        let mut document: Table = toml::from_str(source)?;
        Ok(match document.remove(SETTINGS_TABLE) {
            Some(Value::Table(table)) => Self::from_table(table),
            Some(_) => (
                Self::default(),
                vec![BackgroundIssue {
                    key: SETTINGS_TABLE.into(),
                    kind: IssueKind::InvalidValue {
                        expected: "a table",
                    },
                }],
            ),
            None => (Self::default(), Vec::new()),
        })
    }

    /// Parse the contents of the `[background]` table.
    pub fn from_table(mut table: Table) -> (Self, Vec<BackgroundIssue>) {
        let mut issues = Vec::new();
        let mut settings = Self::default();
        for (name, slot) in [("dark", &mut settings.dark), ("light", &mut settings.light)] {
            if let Some(value) = table.remove(name) {
                let key = format!("{SETTINGS_TABLE}.{name}");
                match value {
                    Value::Table(sub) => *slot = Layer::parse(&key, sub, LAYER_KEYS, &mut issues),
                    _ => issues.push(invalid(key, "a table")),
                }
            }
        }
        if let Some(value) = table.remove("views") {
            let prefix = format!("{SETTINGS_TABLE}.views");
            match value {
                Value::Table(views) => {
                    for (id, value) in views {
                        let key = format!("{prefix}.{id}");
                        let Some(view) = ViewId::parse(&id) else {
                            issues.push(BackgroundIssue {
                                key,
                                kind: IssueKind::UnknownView {
                                    suggestion: suggest(&id, &ViewId::IDS),
                                },
                            });
                            continue;
                        };
                        match value {
                            Value::Table(sub) => {
                                let layer = Layer::parse(&key, sub, LAYER_KEYS, &mut issues);
                                settings.views.push((view, layer));
                            }
                            _ => issues.push(invalid(key, "a table")),
                        }
                    }
                }
                _ => issues.push(invalid(prefix, "a table")),
            }
        }
        settings.base = Layer::parse(SETTINGS_TABLE, table, TOP_KEYS, &mut issues);
        (settings, issues)
    }

    /// Settings using `source` with everything else default.
    pub fn with_source(source: BackgroundSource) -> Self {
        Self {
            base: Layer {
                source: Some(Some(source)),
                ..Layer::default()
            },
            ..Self::default()
        }
    }

    /// The effective config for `appearance`, optionally inside `view`.
    pub fn resolve(&self, appearance: Appearance, view: Option<ViewId>) -> BackgroundConfig {
        let appearance_layer = match appearance {
            Appearance::Dark => &self.dark,
            Appearance::Light => &self.light,
        };
        let view_layer = view.and_then(|view| {
            self.views
                .iter()
                .find(|(candidate, _)| *candidate == view)
                .map(|(_, layer)| layer)
        });
        let layers: Vec<&Layer> = [Some(&self.base), Some(appearance_layer), view_layer]
            .into_iter()
            .flatten()
            .collect();
        // Last layer that sets the field wins.
        macro_rules! pick {
            ($field:ident) => {
                layers.iter().rev().find_map(|layer| layer.$field.clone())
            };
        }

        let source: Option<BackgroundSource> = pick!(source).flatten();
        let builtin = source.as_ref().is_some_and(BackgroundSource::is_builtin);
        let (default_blur, default_dim) = if builtin {
            (BUILTIN_BLUR, BUILTIN_DIM)
        } else {
            (DEFAULT_BLUR, DEFAULT_DIM)
        };
        BackgroundConfig {
            enabled: pick!(enabled).unwrap_or(true),
            fit: pick!(fit).unwrap_or_default(),
            anchor: pick!(anchor).unwrap_or_default(),
            blur: pick!(blur).unwrap_or(default_blur),
            dim: pick!(dim).unwrap_or(default_dim),
            saturation: pick!(saturation).unwrap_or(1.0),
            tint: pick!(tint).flatten(),
            tint_strength: pick!(tint_strength).unwrap_or(0.0),
            surface_opacity: pick!(surface_opacity).unwrap_or(DEFAULT_SURFACE_OPACITY),
            overlay_opacity: pick!(overlay_opacity).unwrap_or(DEFAULT_OVERLAY_OPACITY),
            legibility: pick!(legibility).unwrap_or_default(),
            source,
        }
    }
}

fn invalid(key: String, expected: &'static str) -> BackgroundIssue {
    BackgroundIssue {
        key,
        kind: IssueKind::InvalidValue { expected },
    }
}

impl Layer {
    fn parse(
        prefix: &str,
        table: Table,
        known: &[&'static str],
        issues: &mut Vec<BackgroundIssue>,
    ) -> Self {
        let mut layer = Self::default();
        for (name, value) in table {
            let key = format!("{prefix}.{name}");
            let mut report = |kind| {
                issues.push(BackgroundIssue {
                    key: key.clone(),
                    kind,
                })
            };
            match name.as_str() {
                "enabled" => match value {
                    Value::Boolean(enabled) => layer.enabled = Some(enabled),
                    _ => report(IssueKind::InvalidValue {
                        expected: "true or false",
                    }),
                },
                "source" => match value {
                    Value::String(text) => match BackgroundSource::parse(&text) {
                        Ok(source) => layer.source = Some(source),
                        Err(kind) => report(kind),
                    },
                    _ => report(IssueKind::InvalidValue {
                        expected: "a path or \"builtin:<id>\"",
                    }),
                },
                "fit" => layer.fit = parse_enum(value, &Fit::ALL, "a fit", &mut report),
                "anchor" => {
                    layer.anchor = parse_enum(value, &Anchor::ALL, "an anchor", &mut report)
                }
                "legibility" => {
                    layer.legibility =
                        parse_enum(value, &Legibility::ALL, "\"auto\" or \"off\"", &mut report)
                }
                "blur" => {
                    layer.blur =
                        parse_number(value, BLUR_RANGE, &mut report).map(|blur| blur.round() as u8)
                }
                "dim" => layer.dim = parse_number(value, DIM_RANGE, &mut report),
                "saturation" => {
                    layer.saturation = parse_number(value, SATURATION_RANGE, &mut report)
                }
                "tint_strength" => {
                    layer.tint_strength = parse_number(value, TINT_RANGE, &mut report)
                }
                "surface_opacity" => {
                    layer.surface_opacity = parse_number(value, SURFACE_RANGE, &mut report)
                }
                "overlay_opacity" => {
                    layer.overlay_opacity = parse_number(value, OVERLAY_RANGE, &mut report)
                }
                "tint" => match value {
                    Value::String(text) => match Tint::parse(&text) {
                        Ok(tint) => layer.tint = Some(tint),
                        Err(kind) => report(kind),
                    },
                    _ => report(IssueKind::InvalidValue {
                        expected: "a color or an alias",
                    }),
                },
                _ if known.contains(&name.as_str()) => {}
                _ => report(IssueKind::UnknownKey {
                    suggestion: suggest(&name, known),
                }),
            }
        }
        layer
    }
}

fn parse_enum<T: Copy>(
    value: Value,
    options: &[(T, &'static str)],
    expected: &'static str,
    report: &mut impl FnMut(IssueKind),
) -> Option<T> {
    let found = match &value {
        Value::String(text) => options.iter().find(|(_, id)| id == text).map(|(v, _)| *v),
        _ => None,
    };
    if found.is_none() {
        report(IssueKind::InvalidValue { expected });
    }
    found
}

fn parse_number(
    value: Value,
    (min, max): (f32, f32),
    report: &mut impl FnMut(IssueKind),
) -> Option<f32> {
    let number = match value {
        Value::Integer(number) => number as f64,
        Value::Float(number) if number.is_finite() => number,
        _ => {
            report(IssueKind::InvalidValue {
                expected: "a number",
            });
            return None;
        }
    };
    let clamped = number.clamp(min as f64, max as f64);
    if clamped != number {
        report(IssueKind::Clamped {
            value: number,
            clamped,
        });
    }
    Some(clamped as f32)
}

/// Outcome of the readability guard (§10.4, FR-5, AC-7).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readability {
    /// Dim to paint; ≥ the configured dim. Settings shows it as the slider's
    /// enforced minimum.
    pub dim: f32,
    /// Surface opacity to paint; ≥ the configured value.
    pub surface_opacity: f32,
    /// Lowest ratio of the checked pairs at the effective values, relative to
    /// its requirement (≥ 1.0 = every pair passes).
    pub margin: f32,
}

impl Readability {
    pub fn passes(&self) -> bool {
        self.margin >= 1.0
    }

    /// The guard changed the user's values ("Adjusted for readability").
    pub fn adjusted(&self, config: &BackgroundConfig) -> bool {
        self.dim > config.dim + f32::EPSILON
            || self.surface_opacity > config.surface_opacity + f32::EPSILON
    }
}

/// Simulate the paint stack (§10.2 steps 1–4) over `sample` and raise dim,
/// then surface opacity, until `text.primary/secondary/tertiary` reach 4.5:1
/// and `border.focus` 3:1 on every translucent pane.
///
/// `sample` is the image's worst pixel for the appearance: 95th-percentile
/// luminance on dark, 5th on light (see [`legibility_sample`]).
pub fn readability(
    config: &BackgroundConfig,
    colors: &ColorTokens,
    appearance: Appearance,
    sample: Color,
) -> Readability {
    const DIM_STEP: f32 = 0.05;
    const OPACITY_STEP: f32 = 0.02;
    let margin =
        |dim: f32, opacity: f32| stack_margin(config, colors, appearance, sample, dim, opacity);

    let mut result = Readability {
        dim: config.dim,
        surface_opacity: config.surface_opacity,
        margin: margin(config.dim, config.surface_opacity),
    };
    if config.legibility == Legibility::Off || result.passes() {
        return result;
    }
    // Integer steps so float drift can't skip the 0.9 / 1.0 caps.
    let dim_steps = ((DIM_RANGE.1 - config.dim) / DIM_STEP).ceil().max(0.0) as u32;
    for step in 1..=dim_steps {
        let dim = (config.dim + step as f32 * DIM_STEP).min(DIM_RANGE.1);
        result.dim = dim;
        result.margin = margin(dim, result.surface_opacity);
        if result.passes() {
            return result;
        }
    }
    let opacity_steps = ((SURFACE_RANGE.1 - config.surface_opacity) / OPACITY_STEP)
        .ceil()
        .max(0.0) as u32;
    for step in 1..=opacity_steps {
        let opacity = (config.surface_opacity + step as f32 * OPACITY_STEP).min(SURFACE_RANGE.1);
        result.surface_opacity = opacity;
        result.margin = margin(result.dim, opacity);
        if result.passes() {
            break;
        }
    }
    result
}

/// Worst checked contrast / its requirement for one dim + opacity.
fn stack_margin(
    config: &BackgroundConfig,
    colors: &ColorTokens,
    appearance: Appearance,
    sample: Color,
    dim: f32,
    surface_opacity: f32,
) -> f32 {
    let mut backdrop = Color::rgb(sample.r, sample.g, sample.b);
    if let Some(tint) = config.tint {
        backdrop = mix(tint.resolve(colors), config.tint_strength, backdrop);
    }
    backdrop = mix(BackgroundConfig::dim_color(appearance), dim, backdrop);
    let checks: [(Color, f32); 4] = [
        (colors.text_primary, 4.5),
        (colors.text_secondary, 4.5),
        (colors.text_tertiary, 4.5),
        (colors.border_focus, 3.0),
    ];
    [colors.bg_sidebar, colors.bg_surface, colors.bg_raised]
        .into_iter()
        .map(|pane| mix(pane, pane.a as f32 / 255.0 * surface_opacity, backdrop))
        .flat_map(|base| {
            checks
                .iter()
                .map(move |(fg, required)| fg.contrast(base) / required)
        })
        .fold(f32::INFINITY, f32::min)
}

/// `color` (its own alpha replaced by `alpha`) composited over opaque `base`.
fn mix(color: Color, alpha: f32, base: Color) -> Color {
    let alpha = (color.a as f32 / 255.0).min(1.0) * alpha.clamp(0.0, 1.0);
    let opaque = Color::rgba(color.r, color.g, color.b, (alpha * 255.0).round() as u8);
    opaque.blend_over(base)
}

/// §10.4 sample: the 95th-percentile-luminance pixel (dark) or the
/// 5th-percentile one (light) of an RGBA8 buffer, typically a 64 × 64
/// downsample of the processed image. `None` for an empty buffer.
pub fn legibility_sample(rgba: &[u8], appearance: Appearance) -> Option<Color> {
    let mut pixels: Vec<(f32, Color)> = rgba
        .chunks_exact(4)
        .map(|px| {
            let color = Color::rgb(px[0], px[1], px[2]);
            (color.luminance(), color)
        })
        .collect();
    if pixels.is_empty() {
        return None;
    }
    let percentile = if appearance.is_dark() { 0.95 } else { 0.05 };
    let index = ((pixels.len() - 1) as f32 * percentile).round() as usize;
    let (_, &mut (_, color), _) = pixels.select_nth_unstable_by(index, |a, b| a.0.total_cmp(&b.0));
    Some(color)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThemeRegistry;

    fn parse(source: &str) -> (BackgroundSettings, Vec<BackgroundIssue>) {
        BackgroundSettings::from_settings_toml(source).expect("valid toml")
    }

    const SPEC_EXAMPLE: &str = r##"
[appearance]
mode = "system"

[background]
enabled = true
source = "~/Pictures/wallpapers/night-city.jpg"
fit = "cover"
anchor = "top-left"
blur = 24
dim = 0.35
saturation = 1.0
tint = "@accent.solid"
tint_strength = 0.1
surface_opacity = 0.82
overlay_opacity = 0.96
legibility = "auto"
playback = "loop"
fps_cap = 30
pause_when = ["unfocused"]

[background.dark]
source = "~/Pictures/wallpapers/night-city.jpg"

[background.light]
source = "~/Pictures/wallpapers/morning-shrine.jpg"
dim = 0.25

[background.views.pr_review]
blur = 48
surface_opacity = 0.94

[background.views.onboarding]
enabled = false
"##;

    #[test]
    fn tokens_md_example_parses_without_issues() {
        let (settings, issues) = parse(SPEC_EXAMPLE);
        assert_eq!(issues, []);
        let dark = settings.resolve(Appearance::Dark, None);
        assert!(dark.is_active());
        assert_eq!(
            dark.source,
            Some(BackgroundSource::Image(
                "~/Pictures/wallpapers/night-city.jpg".into()
            ))
        );
        assert_eq!(dark.anchor, Anchor::TopLeft);
        assert_eq!(dark.anchor.fraction(), (0.0, 0.0));
        assert_eq!(dark.blur, 24);
        assert_eq!(dark.dim, 0.35);
        let accent = ColorTokens::<Color>::KEYS
            .iter()
            .position(|key| *key == "accent.solid");
        assert_eq!(dark.tint, accent.map(Tint::Token));
    }

    #[test]
    fn precedence_is_base_then_appearance_then_view() {
        let (settings, _) = parse(SPEC_EXAMPLE);
        let light = settings.resolve(Appearance::Light, None);
        assert_eq!(
            light.source,
            Some(BackgroundSource::Image(
                "~/Pictures/wallpapers/morning-shrine.jpg".into()
            ))
        );
        assert_eq!(light.dim, 0.25);
        assert_eq!(light.blur, 24, "inherited from [background]");

        let review = settings.resolve(Appearance::Light, Some(ViewId::PrReview));
        assert_eq!((review.blur, review.surface_opacity), (48, 0.94));
        assert_eq!(
            review.dim, 0.25,
            "appearance layer still applies under a view"
        );

        let onboarding = settings.resolve(Appearance::Dark, Some(ViewId::Onboarding));
        assert!(!onboarding.enabled);
        assert!(!onboarding.is_active());
        assert!(
            settings
                .resolve(Appearance::Dark, Some(ViewId::Inbox))
                .is_active()
        );
    }

    #[test]
    fn missing_table_means_no_background() {
        let (settings, issues) = parse("[appearance]\nmode = \"dark\"\n");
        assert!(issues.is_empty());
        let config = settings.resolve(Appearance::Dark, None);
        assert!(!config.is_active());
        assert_eq!(config.surface_opacity, DEFAULT_SURFACE_OPACITY);
    }

    #[test]
    fn view_can_clear_the_source() {
        let (settings, issues) = parse(
            "[background]\nsource = \"builtin:ink\"\n[background.views.inbox]\nsource = \"\"\n",
        );
        assert!(issues.is_empty());
        assert!(settings.resolve(Appearance::Dark, None).is_active());
        assert!(
            !settings
                .resolve(Appearance::Dark, Some(ViewId::Inbox))
                .is_active()
        );
    }

    #[test]
    fn builtin_sources_use_builtin_defaults() {
        let (settings, _) = parse("[background]\nsource = \"builtin:dusk\"\n");
        let config = settings.resolve(Appearance::Dark, None);
        assert_eq!(
            config.source,
            Some(BackgroundSource::Builtin(BuiltinBackground::Dusk))
        );
        assert_eq!((config.blur, config.dim), (BUILTIN_BLUR, BUILTIN_DIM));

        let (settings, _) = parse("[background]\nsource = \"builtin:dusk\"\nblur = 4\n");
        assert_eq!(settings.resolve(Appearance::Dark, None).blur, 4);

        let (settings, _) = parse("[background]\nsource = \"/tmp/a.png\"\n");
        let config = settings.resolve(Appearance::Dark, None);
        assert_eq!((config.blur, config.dim), (DEFAULT_BLUR, DEFAULT_DIM));
    }

    #[test]
    fn out_of_range_values_clamp_with_an_issue() {
        let (settings, issues) = parse(
            "[background]\nsource = \"builtin:ink\"\nblur = 200\ndim = 1.5\nsurface_opacity = 0.1\noverlay_opacity = 2\n",
        );
        let config = settings.resolve(Appearance::Dark, None);
        assert_eq!(config.blur, 64);
        assert_eq!(config.dim, 0.9);
        assert_eq!(config.surface_opacity, 0.6);
        assert_eq!(config.overlay_opacity, 1.0);
        let mut keys: Vec<_> = issues.iter().map(|issue| issue.key.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "background.blur",
                "background.dim",
                "background.overlay_opacity",
                "background.surface_opacity"
            ]
        );
        let blur = issues.iter().find(|issue| issue.key == "background.blur");
        assert!(matches!(
            blur.map(|issue| &issue.kind),
            Some(IssueKind::Clamped { value, clamped }) if *value == 200.0 && *clamped == 64.0
        ));
    }

    #[test]
    fn bad_values_fall_back_and_name_the_key() {
        let (settings, issues) = parse(
            r##"
[background]
source = "builtin:dsuk"
fit = "stretch"
blurr = 3
tint = "#zzz"
dim = "lots"

[background.views.pr_reviews]
blur = 1

[background.views.inbox]
bluur = 2
"##,
        );
        let config = settings.resolve(Appearance::Dark, None);
        assert_eq!(config.source, None);
        assert_eq!(config.fit, Fit::Cover);
        assert_eq!(config.tint, None);
        let rendered: Vec<String> = issues.iter().map(ToString::to_string).collect();
        let expect = [
            "`background.views.pr_reviews`: unknown view, ignored. Did you mean `pr_review`?",
            "`background.views.inbox.bluur`: unknown key, ignored. Did you mean `blur`?",
            "`background.blurr`: unknown key, ignored. Did you mean `blur`?",
            "`background.dim`: expected a number; ignored.",
            "`background.fit`: expected a fit; ignored.",
            "`background.source`: no built-in background `dsuk`. Did you mean `dusk`?",
            "`background.tint`: expected a color (#rrggbb) or an alias (@token.key); ignored.",
        ];
        for line in expect {
            assert!(
                rendered.iter().any(|issue| issue == line),
                "{line} in {rendered:#?}"
            );
        }
        assert_eq!(rendered.len(), expect.len(), "{rendered:#?}");
    }

    #[test]
    fn syntax_errors_fail_the_parse() {
        assert!(BackgroundSettings::from_settings_toml("[background\nblur = 1").is_err());
    }

    fn variant(id: &str) -> &'static crate::ThemeVariant {
        ThemeRegistry::builtin().variant(id).expect("built-in")
    }

    fn active(dim: f32, surface_opacity: f32) -> BackgroundConfig {
        BackgroundConfig {
            dim,
            surface_opacity,
            ..BackgroundSettings::with_source(BackgroundSource::Builtin(BuiltinBackground::Ink))
                .resolve(Appearance::Dark, None)
        }
    }

    /// AC-7: a pure-white image under a dark theme is made readable by
    /// raising dim (the enforced minimum the slider shows).
    #[test]
    fn white_image_on_dark_theme_raises_dim_until_text_passes() {
        for id in ["adoken-dark", "arcade-dark"] {
            let variant = variant(id);
            let config = active(0.0, 0.6);
            let result = readability(&config, &variant.colors, Appearance::Dark, Color::WHITE);
            assert!(result.passes(), "{id}: {result:?}");
            assert!(result.adjusted(&config), "{id}: {result:?}");
            assert!(result.dim > 0.0, "{id}");

            // Body text on every translucent pane now measures ≥ 4.5:1.
            let colors = &variant.colors;
            let backdrop = mix(Color::BLACK, result.dim, Color::WHITE);
            for pane in [colors.bg_sidebar, colors.bg_surface, colors.bg_raised] {
                let base = mix(pane, result.surface_opacity, backdrop);
                assert!(colors.text_primary.contrast(base) >= 4.5, "{id}");
                assert!(colors.text_tertiary.contrast(base) >= 4.5, "{id}");
            }

            // The enforced value is the minimum: one step less fails.
            if result.surface_opacity == config.surface_opacity {
                let less = stack_margin(
                    &config,
                    colors,
                    Appearance::Dark,
                    Color::WHITE,
                    result.dim - 0.05,
                    result.surface_opacity,
                );
                assert!(less < 1.0, "{id}: dim {} was not minimal", result.dim);
            }
        }
    }

    #[test]
    fn black_image_on_light_theme_raises_white_scrim() {
        for id in ["adoken-light", "sakura-light"] {
            let variant = variant(id);
            let config = active(0.0, 0.6);
            let result = readability(&config, &variant.colors, Appearance::Light, Color::BLACK);
            assert!(result.passes(), "{id}: {result:?}");
            assert!(result.adjusted(&config));
        }
    }

    #[test]
    fn readable_image_is_left_alone() {
        let variant = variant("adoken-dark");
        let config = active(0.35, 0.82);
        let dark_pixel = variant.colors.bg_app;
        let result = readability(&config, &variant.colors, Appearance::Dark, dark_pixel);
        assert!(result.passes());
        assert!(!result.adjusted(&config));
        assert_eq!((result.dim, result.surface_opacity), (0.35, 0.82));
    }

    #[test]
    fn unfixable_palette_stops_at_both_caps() {
        // Tertiary text equal to its pane can never pass: the guard must walk
        // dim to 0.9, then surface opacity to 1.0, and report the failure.
        let mut colors = variant("adoken-dark").colors;
        colors.text_tertiary = colors.bg_surface;
        let config = active(0.3, 0.7);
        let result = readability(&config, &colors, Appearance::Dark, Color::WHITE);
        assert_eq!((result.dim, result.surface_opacity), (0.9, 1.0));
        assert!(!result.passes());
    }

    #[test]
    fn legibility_off_keeps_values_and_reports_failure() {
        let variant = variant("adoken-dark");
        let config = BackgroundConfig {
            legibility: Legibility::Off,
            ..active(0.0, 0.6)
        };
        let result = readability(&config, &variant.colors, Appearance::Dark, Color::WHITE);
        assert_eq!((result.dim, result.surface_opacity), (0.0, 0.6));
        assert!(!result.passes());
    }

    #[test]
    fn sample_takes_the_percentile_for_the_appearance() {
        // 19 black pixels and 1 white: the 95th percentile (index 18 of 19)
        // is black, so a single hot pixel doesn't dominate.
        let mut rgba = vec![0u8; 20 * 4];
        rgba[..4].copy_from_slice(&[255, 255, 255, 255]);
        assert_eq!(
            legibility_sample(&rgba, Appearance::Dark),
            Some(Color::BLACK)
        );
        // Mostly white with 3 black pixels: 5th percentile on light is black.
        let mut rgba = vec![255u8; 20 * 4];
        for px in rgba.chunks_exact_mut(4).take(3) {
            px.copy_from_slice(&[0, 0, 0, 255]);
        }
        assert_eq!(
            legibility_sample(&rgba, Appearance::Light),
            Some(Color::BLACK)
        );
        assert_eq!(legibility_sample(&[], Appearance::Dark), None);
    }

    #[test]
    fn source_round_trips_through_display() {
        for builtin in BuiltinBackground::ALL {
            let text = BackgroundSource::Builtin(builtin).to_string();
            assert_eq!(
                BackgroundSource::parse(&text),
                Ok(Some(BackgroundSource::Builtin(builtin)))
            );
        }
    }
}
