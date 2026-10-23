//! sRGB color with CSS hex parsing and WCAG contrast helpers.
//!
//! Adapted from zeron `crates/theme/src/lib.rs` (`Color`, `ColorParseError`)
//! at d1010657d69c9730f8020edc36cc2649b136b6fa, MIT, Copyright (c) 2026 Wing.
//! See THIRD_PARTY_NOTICES.md.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An 8-bit sRGB color, serialized as `#rrggbb` or `#rrggbbaa`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    /// Fully transparent; "no paint" without writing a literal outside this crate.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse a CSS hex literal at compile time. Panics (a build error in const
    /// context) on malformed input, so built-in palettes cannot ship typos.
    pub const fn hex(value: &str) -> Self {
        match parse_hex(value.as_bytes()) {
            Ok(color) => color,
            Err(_) => panic!("invalid hex color literal"),
        }
    }

    pub fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Composite this (possibly translucent) color over an opaque background.
    pub fn blend_over(self, background: Self) -> Self {
        let alpha = self.a as f32 / 255.0;
        let blend = |front: u8, back: u8| {
            (front as f32 * alpha + back as f32 * (1.0 - alpha)).round() as u8
        };
        Self::rgb(
            blend(self.r, background.r),
            blend(self.g, background.g),
            blend(self.b, background.b),
        )
    }

    /// WCAG 2.1 contrast ratio (1.0..=21.0). A translucent foreground is first
    /// composited over `background`, which is what the eye actually receives.
    pub fn contrast(self, background: Self) -> f32 {
        let foreground = if self.is_opaque() {
            self
        } else {
            self.blend_over(background)
        };
        let (a, b) = (foreground.luminance(), background.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// WCAG 2.1 relative luminance of the opaque channels.
    pub fn luminance(self) -> f32 {
        let linear = |channel: u8| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// This color with alpha `alpha` (0.0..=1.0).
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// Linear sRGB-channel interpolation toward `other` (`amount` 0 = self,
    /// 1 = other), alpha included.
    pub fn mix(self, other: Self, amount: f32) -> Self {
        let t = amount.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Self::rgba(
            lerp(self.r, other.r),
            lerp(self.g, other.g),
            lerp(self.b, other.b),
            lerp(self.a, other.a),
        )
    }

    /// OKLab perceptual lightness (0.0..=1.0) of the opaque channels.
    pub fn lightness(self) -> f32 {
        self.to_oklab()[0]
    }

    /// This color with OKLab lightness `l`, hue and chroma kept (chroma is
    /// scaled down only if the result would leave the sRGB gamut). Alpha is
    /// kept.
    pub fn with_lightness(self, l: f32) -> Self {
        let [_, a, b] = self.to_oklab();
        let l = l.clamp(0.0, 1.0);
        let mut chroma = 1.0;
        loop {
            if let Some(color) = from_oklab([l, a * chroma, b * chroma]) {
                return Self { a: self.a, ..color };
            }
            chroma -= 0.02;
            if chroma <= 0.0 {
                let grey = from_oklab([l, 0.0, 0.0]).unwrap_or(self);
                return Self { a: self.a, ..grey };
            }
        }
    }

    /// tokens.md §9.3 contrast enforcement: step OKLab lightness by 0.01 away
    /// from the backgrounds until this color reaches `minimum` against every
    /// one of them. Unchanged when it already passes.
    pub fn ensure_contrast(self, backgrounds: &[Self], minimum: f32) -> Self {
        let requirements: Vec<(Self, f32)> = backgrounds.iter().map(|b| (*b, minimum)).collect();
        self.ensure_contrast_each(&requirements)
    }

    /// [`Self::ensure_contrast`] with a minimum per background. Tries the
    /// direction with more headroom first (lighter on dark backgrounds), then
    /// the other; returns the closest attempt if neither reaches every
    /// minimum.
    pub fn ensure_contrast_each(self, requirements: &[(Self, f32)]) -> Self {
        // Worst ratio-to-minimum; ≥ 1 means every requirement passes.
        let score = |color: Self| {
            requirements
                .iter()
                .map(|(background, minimum)| color.contrast(*background) / minimum)
                .fold(f32::INFINITY, f32::min)
        };
        if score(self) >= 1.0 {
            return self;
        }
        let mean_luminance = requirements.iter().map(|(b, _)| b.luminance()).sum::<f32>()
            / requirements.len().max(1) as f32;
        let lighter_first = mean_luminance < 0.18;
        let start = self.lightness();
        let mut best = (score(self), self);
        for lighter in [lighter_first, !lighter_first] {
            let direction = if lighter { 0.01 } else { -0.01 };
            for step in 1..=100 {
                let l = start + direction * step as f32;
                if !(0.0..=1.0).contains(&l) {
                    break;
                }
                let candidate = self.with_lightness(l);
                let candidate_score = score(candidate);
                if candidate_score >= 1.0 {
                    return candidate;
                }
                if candidate_score > best.0 {
                    best = (candidate_score, candidate);
                }
            }
        }
        best.1
    }

    fn to_oklab(self) -> [f32; 3] {
        let lin = |channel: u8| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, b) = (lin(self.r), lin(self.g), lin(self.b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }
}

/// OKLab → opaque sRGB, `None` when out of gamut.
fn from_oklab([l, a, b]: [f32; 3]) -> Option<Color> {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let r = 4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_;
    let g = -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_;
    let b = -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_;
    const EPSILON: f32 = 1e-4;
    let encode = |value: f32| {
        if !(-EPSILON..=1.0 + EPSILON).contains(&value) {
            return None;
        }
        let value = value.clamp(0.0, 1.0);
        let srgb = if value <= 0.003_130_8 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        Some((srgb * 255.0).round() as u8)
    };
    Some(Color::rgb(encode(r)?, encode(g)?, encode(b)?))
}

impl fmt::Debug for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if !self.is_opaque() {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected a CSS hex color (#rgb, #rgba, #rrggbb, or #rrggbbaa)")]
pub struct ColorParseError;

impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_hex(value.trim().as_bytes())
    }
}

const fn nibble(c: u8) -> Result<u8, ColorParseError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(ColorParseError),
    }
}

const fn short(v: u8) -> u8 {
    (v << 4) | v
}

const fn long(hi: u8, lo: u8) -> u8 {
    (hi << 4) | lo
}

/// `const` so [`Color::hex`] can reject malformed literals at compile time;
/// hence the explicit matches instead of `?`.
const fn parse_hex(bytes: &[u8]) -> Result<Color, ColorParseError> {
    let [b'#', digits @ ..] = bytes else {
        return Err(ColorParseError);
    };
    let mut values = [0u8; 8];
    let mut i = 0;
    while i < digits.len() && i < 8 {
        values[i] = match nibble(digits[i]) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        i += 1;
    }
    let v = values;
    match digits.len() {
        3 => Ok(Color::rgb(short(v[0]), short(v[1]), short(v[2]))),
        4 => Ok(Color::rgba(
            short(v[0]),
            short(v[1]),
            short(v[2]),
            short(v[3]),
        )),
        6 => Ok(Color::rgb(
            long(v[0], v[1]),
            long(v[2], v[3]),
            long(v[4], v[5]),
        )),
        8 => Ok(Color::rgba(
            long(v[0], v[1]),
            long(v[2], v[3]),
            long(v[4], v[5]),
            long(v[6], v[7]),
        )),
        _ => Err(ColorParseError),
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_css_hex_length_and_round_trips_display() {
        let cases = [
            ("#abc", Color::rgb(0xaa, 0xbb, 0xcc), "#aabbcc"),
            ("#abcd", Color::rgba(0xaa, 0xbb, 0xcc, 0xdd), "#aabbccdd"),
            ("#102030", Color::rgb(0x10, 0x20, 0x30), "#102030"),
            (
                "#10203040",
                Color::rgba(0x10, 0x20, 0x30, 0x40),
                "#10203040",
            ),
            (" #FFFFFF ", Color::WHITE, "#ffffff"),
        ];
        for (source, expected, display) in cases {
            let parsed: Color = source.parse().unwrap();
            assert_eq!(parsed, expected, "{source}");
            assert_eq!(parsed.to_string(), display);
            assert_eq!(display.parse::<Color>().unwrap(), expected);
        }
    }

    #[test]
    fn rejects_malformed_hex() {
        for source in [
            "",
            "#",
            "fff",
            "#ff",
            "#fffff",
            "#ggg",
            "#123456789",
            "rgb(0,0,0)",
        ] {
            assert_eq!(source.parse::<Color>(), Err(ColorParseError), "{source:?}");
        }
    }

    #[test]
    fn contrast_matches_wcag_reference_values() {
        assert!((Color::BLACK.contrast(Color::WHITE) - 21.0).abs() < 0.01);
        assert!((Color::WHITE.contrast(Color::WHITE) - 1.0).abs() < 0.001);
        // #767676 on white is the canonical 4.54:1 AA boundary.
        let grey = Color::hex("#767676");
        assert!((grey.contrast(Color::WHITE) - 4.54).abs() < 0.01);
    }

    #[test]
    fn translucent_foreground_is_composited_before_contrast() {
        let half_black = Color::rgba(0, 0, 0, 128);
        let composited = half_black.blend_over(Color::WHITE);
        assert_eq!(composited, Color::rgb(127, 127, 127));
        assert_eq!(
            half_black.contrast(Color::WHITE),
            composited.contrast(Color::WHITE)
        );
    }

    #[test]
    fn oklab_lightness_round_trips_and_keeps_hue() {
        for hex in ["#f26b3a", "#62a8ff", "#3fd18a", "#15161c", "#ffffff"] {
            let color = Color::hex(hex);
            let same = color.with_lightness(color.lightness());
            let close = |a: u8, b: u8| a.abs_diff(b) <= 1;
            assert!(
                close(same.r, color.r) && close(same.g, color.g) && close(same.b, color.b),
                "{hex} → {same}"
            );
        }
        let lighter = Color::hex("#c24416").with_lightness(0.8);
        assert!(lighter.r > lighter.b, "still orange: {lighter}");
    }

    #[test]
    fn ensure_contrast_moves_away_from_the_background_until_it_passes() {
        let dark = [Color::hex("#1e1e2e"), Color::hex("#313244")];
        let dim = Color::hex("#585b70");
        let fixed = dim.ensure_contrast(&dark, 4.5);
        assert!(fixed.lightness() > dim.lightness());
        assert!(dark.iter().all(|bg| fixed.contrast(*bg) >= 4.5));

        let light = [Color::WHITE, Color::hex("#eff1f5")];
        let pale = Color::hex("#df8e1d");
        let fixed = pale.ensure_contrast(&light, 4.5);
        assert!(fixed.lightness() < pale.lightness());
        assert!(light.iter().all(|bg| fixed.contrast(*bg) >= 4.5));

        // Already passing: untouched.
        assert_eq!(Color::WHITE.ensure_contrast(&dark, 4.5), Color::WHITE);
    }

    #[test]
    fn mix_and_with_alpha() {
        assert_eq!(
            Color::BLACK.mix(Color::WHITE, 0.5),
            Color::rgb(128, 128, 128)
        );
        assert_eq!(Color::WHITE.with_alpha(0.0), Color::rgba(255, 255, 255, 0));
        assert_eq!(Color::WHITE.with_alpha(0.12).a, 31);
    }
}
