//! The active theme as a gpui [`Global`], pre-converted to paint colors.
//!
//! Colors are converted once per theme change (not per frame). Components
//! read them with `cx.theme()`; nothing in `adoken-ui` hard-codes a color.

use adoken_theme::code_font::{BUNDLED_CODE_FONT, DEFAULT_CODE_FONT_SIZE};
use adoken_theme::{
    Appearance, Color, ColorTokens, FontRole, ShadowLayer, ThemeVariant, TypeStyle,
};
use gpui::{
    App, BoxShadow, FontFeatures, FontWeight, Global, Hsla, Rgba, SharedString, Styled, point, px,
};

/// Convert a token color to gpui's paint color.
pub fn hsla(color: Color) -> Hsla {
    Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: f32::from(color.a) / 255.0,
    }
    .into()
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub id: SharedString,
    pub name: SharedString,
    pub appearance: Appearance,
    /// Paint colors, one per token.
    pub colors: ColorTokens<Hsla>,
    /// The same tokens in sRGB, for composition math (flattening washes,
    /// contrast checks) that must not round-trip through HSL.
    pub raw: ColorTokens<Color>,
    pub ui_font: SharedString,
    /// Keycaps (`type.kbd`): the platform's monospace face.
    pub keycap_font: SharedString,
    /// Code blocks, diffs and inline code (spec 0009 §3 Code font).
    pub code_font: CodeFontStyle,
}

/// The code font as it renders: a family the text system resolved (the
/// app validates the configured one and falls back), its size, features.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeFontStyle {
    pub family: SharedString,
    /// `type.code` size in pixels; `type.code.inline` keeps its own.
    pub size: f32,
    pub features: FontFeatures,
}

impl CodeFontStyle {
    pub fn new(family: impl Into<SharedString>, size: f32, ligatures: bool) -> Self {
        Self {
            family: family.into(),
            size,
            features: if ligatures {
                FontFeatures::default()
            } else {
                FontFeatures::disable_ligatures()
            },
        }
    }

    pub fn ligatures(&self) -> bool {
        self.features.is_calt_enabled() != Some(false)
    }
}

impl Default for CodeFontStyle {
    /// Bundled JetBrains Mono at `type.code` size, no ligatures.
    fn default() -> Self {
        Self::new(BUNDLED_CODE_FONT, f32::from(DEFAULT_CODE_FONT_SIZE), false)
    }
}

impl Global for Theme {}

impl Theme {
    pub fn from_variant(variant: &ThemeVariant) -> Self {
        Self {
            id: variant.id.clone().into(),
            name: variant.name.clone().into(),
            appearance: variant.appearance,
            colors: variant.colors.map(|color| hsla(*color)),
            raw: variant.colors,
            ui_font: default_ui_font().into(),
            keycap_font: keycap_font().into(),
            code_font: CodeFontStyle::default(),
        }
    }

    /// Make `variant` the active theme and repaint every window.
    pub fn install(variant: &ThemeVariant, cx: &mut App) {
        Self::from_variant(variant).activate(cx);
    }

    /// Make this the active theme and repaint, keeping the code font of the
    /// theme it replaces (the code font is a setting, not part of a theme).
    pub fn activate(mut self, cx: &mut App) {
        if let Some(current) = cx.try_global::<Self>() {
            self.code_font = current.code_font.clone();
        }
        cx.set_global(self);
        cx.refresh_windows();
    }

    /// Swap the code font of the active theme and repaint (next frame).
    /// No-op before a theme is installed.
    pub fn set_code_font(code_font: CodeFontStyle, cx: &mut App) {
        if cx
            .try_global::<Self>()
            .is_none_or(|current| current.code_font == code_font)
        {
            return;
        }
        cx.global_mut::<Self>().code_font = code_font;
        cx.refresh_windows();
    }

    /// `style` as it renders: the code step at the configured code size.
    pub fn resolve(&self, style: TypeStyle) -> TypeStyle {
        style.at_code_size(self.code_font.size)
    }

    /// Drop shadows for an elevation level (`adoken_theme::elevation`).
    pub fn shadows(&self, layers: &[ShadowLayer]) -> Vec<BoxShadow> {
        layers
            .iter()
            .map(|layer| {
                let alpha = if self.appearance.is_dark() {
                    layer.alpha_dark
                } else {
                    layer.alpha_light
                };
                BoxShadow {
                    color: self.colors.shadow_color.opacity(alpha),
                    offset: point(px(0.0), px(layer.y)),
                    blur_radius: px(layer.blur),
                    spread_radius: px(layer.spread),
                    inset: false,
                }
            })
            .collect()
    }

    /// `translucent` composited over `base` as an opaque paint color.
    pub fn flatten(&self, translucent: Color, base: Color) -> Hsla {
        hsla(translucent.blend_over(base))
    }
}

/// tokens.md §4: system UI font until settings exist.
fn default_ui_font() -> &'static str {
    if cfg!(target_os = "windows") {
        "Segoe UI Variable"
    } else {
        ".SystemUIFont"
    }
}

/// Keycaps keep the platform's monospace face whatever the code font.
fn keycap_font() -> &'static str {
    if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    }
}

/// Read the active theme from any context that derefs to [`App`].
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

/// Apply a type-scale step (tokens.md §4) to any styled element.
pub trait TypeStyleExt: Styled + Sized {
    fn type_style(self, style: TypeStyle, theme: &Theme) -> Self {
        let style = theme.resolve(style);
        let this = match style.font {
            FontRole::Ui => self.font_family(theme.ui_font.clone()),
            FontRole::Code | FontRole::InlineCode => self
                .font_family(theme.code_font.family.clone())
                .font_features(theme.code_font.features.clone()),
            FontRole::SystemMono => self.font_family(theme.keycap_font.clone()),
        };
        this.text_size(px(style.size))
            .line_height(px(style.line_height))
            .font_weight(FontWeight(style.weight))
    }
}

impl<T: Styled> TypeStyleExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_steps_resolve_to_the_configured_code_font() {
        let registry = adoken_theme::ThemeRegistry::builtin();
        let mut theme = Theme::from_variant(registry.default_variant(Appearance::Dark));
        assert_eq!(theme.code_font.family.as_ref(), "JetBrains Mono");
        assert!(!theme.code_font.ligatures(), "ligatures are off by default");
        assert_eq!(
            theme.resolve(adoken_theme::typography::CODE).line_height,
            18.0
        );

        theme.code_font = CodeFontStyle::new("Menlo", 15.0, true);
        assert!(theme.code_font.ligatures());
        let code = theme.resolve(adoken_theme::typography::CODE);
        assert_eq!((code.size, code.line_height), (15.0, 23.0));
        let inline = adoken_theme::typography::CODE_INLINE;
        assert_eq!(theme.resolve(inline), inline, "inline code keeps its size");
    }

    #[test]
    fn hsla_conversion_preserves_channels_and_alpha() {
        for hex in ["#0e0f13", "#f26b3a", "#ffffff0f", "#15161c52", "#62a8ff"] {
            let color = Color::hex(hex);
            let rgba = Rgba::from(hsla(color));
            let back = |v: f32| (v * 255.0).round() as u8;
            assert_eq!(
                Color::rgba(back(rgba.r), back(rgba.g), back(rgba.b), back(rgba.a)),
                color,
                "{hex}"
            );
        }
    }

    #[test]
    fn shadows_use_the_appearance_alpha() {
        let registry = adoken_theme::ThemeRegistry::builtin();
        for appearance in Appearance::ALL {
            let theme = Theme::from_variant(registry.default_variant(appearance));
            let shadows = theme.shadows(adoken_theme::elevation::LEVEL_2);
            let expected: Vec<f32> = adoken_theme::elevation::LEVEL_2
                .iter()
                .map(|layer| {
                    if appearance.is_dark() {
                        layer.alpha_dark
                    } else {
                        layer.alpha_light
                    }
                })
                .collect();
            let actual: Vec<f32> = shadows.iter().map(|shadow| shadow.color.a).collect();
            for (actual, expected) in actual.iter().zip(&expected) {
                assert!((actual - expected).abs() < 1e-3, "{appearance:?}");
            }
        }
    }
}
