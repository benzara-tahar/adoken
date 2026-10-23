//! The weekly fight card's share image (ken.md §5): a 1200 × 675 PNG (X card
//! ratio) with the stat tiles and a small "adoken.dev" wordmark, composed as
//! SVG from theme tokens and the arcade glyphs, rasterized with resvg.
//!
//! Private by default: the image shows only numbers unless the user opts
//! into repository names, PR titles, or their background.
//!
//! Rasterizing loads the system font database (≈ 100 ms): background
//! executor only, and only on export or preview.

use std::fmt::Write;
use std::path::{Path, PathBuf};

use adoken_theme::{Color, ColorTokens};
use resvg::{tiny_skia, usvg};

use crate::glyph::{self, Poly};
use crate::moves::Naming;
use crate::week::FightCard;

pub const WIDTH: u32 = 1200;
pub const HEIGHT: u32 = 675;
/// Stat tiles.
const TILE_X: f32 = 64.0;
const TILE_Y: f32 = 150.0;
const TILE_W: f32 = 252.0;
const TILE_H: f32 = 214.0;
const TILE_GAP: f32 = 24.0;
/// Numeral height inside a tile.
const NUMERAL_H: f32 = 72.0;
/// K.O. lines listed at most.
const MAX_KO_LINES: usize = 3;
/// Font stack for text (arcade numerals are glyph paths, not text).
const FONT: &str = "'Helvetica Neue', 'Hiragino Sans', Arial, sans-serif";

/// Export options; every one defaults to off (ken.md §5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportOptions {
    pub show_repos: bool,
    pub show_titles: bool,
    /// "Include my background": the user's background image file, when
    /// they chose one (procedural backgrounds have no file to include).
    pub background: Option<PathBuf>,
}

#[derive(Debug)]
pub enum CardError {
    Svg(usvg::Error),
    Encode(String),
    Io(std::io::Error),
}

impl std::fmt::Display for CardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CardError::Svg(e) => write!(f, "couldn't lay out the fight card: {e}"),
            CardError::Encode(e) => write!(f, "couldn't encode the fight card: {e}"),
            CardError::Io(e) => write!(f, "couldn't save the fight card: {e}"),
        }
    }
}

impl std::error::Error for CardError {}

/// Paint colors, from the active theme's tokens.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    pub tile: Color,
    pub tile_border: Color,
    pub scrim: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_tertiary: Color,
    pub ken_primary: Color,
    pub ken_secondary: Color,
    pub ken_outline: Color,
    pub ken_spark: Color,
}

impl Palette {
    pub fn from_tokens(tokens: &ColorTokens<Color>) -> Self {
        // `bg.overlay` may be translucent (frosted surfaces): flatten it on
        // the app background so the image is opaque.
        let bg = tokens
            .bg_overlay
            .blend_over(tokens.bg_app.blend_over(Color::BLACK));
        Self {
            bg,
            tile: tokens.bg_raised.blend_over(bg),
            tile_border: tokens.border_subtle,
            scrim: tokens.bg_scrim,
            text_primary: tokens.text_primary,
            text_secondary: tokens.text_secondary,
            text_tertiary: tokens.text_tertiary,
            ken_primary: tokens.ken_primary,
            ken_secondary: tokens.ken_secondary,
            ken_outline: tokens.ken_outline,
            ken_spark: tokens.ken_spark,
        }
    }
}

/// `fill="#rrggbb" fill-opacity="a"`.
fn fill(color: Color) -> String {
    format!(
        "fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{:.3}\"",
        color.r,
        color.g,
        color.b,
        f32::from(color.a) / 255.0
    )
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Arcade numerals as three path layers, `height` px tall, top-left at
/// `(x, y)`. Returns the SVG and the run's width in px.
fn numerals(text: &str, x: f32, y: f32, height: f32, palette: &Palette) -> (String, f32) {
    let scale = height / glyph::HEIGHT;
    let run = glyph::run(text, 7.5);
    let layer = |polys: &[Poly], color: Color| {
        format!(
            "<path d=\"{}\" {}/>",
            glyph::path_data(polys, scale, x, y),
            fill(color)
        )
    };
    let svg = format!(
        "<g aria-hidden=\"true\">{}{}{}</g>",
        layer(&run.layers.outline, palette.ken_outline),
        layer(&run.layers.fill, palette.ken_primary),
        layer(&run.layers.bevel, palette.ken_secondary),
    );
    (svg, run.width * scale)
}

fn text(
    out: &mut String,
    (x, y): (f32, f32),
    size: f32,
    weight: u16,
    color: Color,
    anchor: &str,
    body: &str,
) {
    let _ = write!(
        out,
        "<text x=\"{x}\" y=\"{y}\" font-family=\"{FONT}\" font-size=\"{size}\" \
         font-weight=\"{weight}\" text-anchor=\"{anchor}\" {}>{}</text>",
        fill(color),
        escape(body)
    );
}

/// The card as SVG (1200 × 675 user units).
pub fn svg(card: &FightCard, palette: &Palette, naming: Naming, options: &ExportOptions) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{WIDTH}\" height=\"{HEIGHT}\" viewBox=\"0 0 {WIDTH} {HEIGHT}\">"
    );
    let _ = write!(
        out,
        "<rect width=\"{WIDTH}\" height=\"{HEIGHT}\" {}/>",
        fill(palette.bg)
    );
    if let Some(path) = &options.background {
        let _ = write!(
            out,
            "<image href=\"{}\" width=\"{WIDTH}\" height=\"{HEIGHT}\" \
             preserveAspectRatio=\"xMidYMid slice\"/>\
             <rect width=\"{WIDTH}\" height=\"{HEIGHT}\" {}/>",
            escape(&path.to_string_lossy()),
            fill(palette.scrim)
        );
    }

    // Header: week + emblem.
    text(
        &mut out,
        (TILE_X, 104.0),
        40.0,
        650,
        palette.text_primary,
        "start",
        &card.week.label(),
    );
    emblem(&mut out, WIDTH as f32 - TILE_X - 36.0, 88.0, palette);

    // Stat tiles.
    let best = card
        .best_combo
        .map(|(hits, rank)| (format!("×{hits}"), rank.name().text(naming)));
    let shields = match card.shields {
        0 => String::new(),
        1 => " · 1 shield".to_owned(),
        n => format!(" · {n} shields"),
    };
    let tiles = [
        ("Hits", card.hits.to_string(), String::new()),
        (
            "Best combo",
            best.as_ref().map_or("0".to_owned(), |(n, _)| n.clone()),
            best.map_or_else(|| "—".to_owned(), |(_, rank)| rank),
        ),
        ("K.O.s", card.kos.len().to_string(), String::new()),
        (
            "Streak",
            card.streak_days.to_string(),
            format!(
                "{}{shields}",
                if card.streak_days == 1 { "day" } else { "days" }
            ),
        ),
    ];
    for (i, (label, value, caption)) in tiles.iter().enumerate() {
        let x = TILE_X + i as f32 * (TILE_W + TILE_GAP);
        let _ = write!(
            out,
            "<rect x=\"{x}\" y=\"{TILE_Y}\" width=\"{TILE_W}\" height=\"{TILE_H}\" rx=\"16\" {} \
             stroke=\"#{:02x}{:02x}{:02x}\" stroke-opacity=\"{:.3}\" stroke-width=\"1\"/>",
            fill(palette.tile),
            palette.tile_border.r,
            palette.tile_border.g,
            palette.tile_border.b,
            f32::from(palette.tile_border.a) / 255.0,
        );
        text(
            &mut out,
            (x + 24.0, TILE_Y + 44.0),
            22.0,
            560,
            palette.text_secondary,
            "start",
            label,
        );
        let (glyphs, _) = numerals(value, x + 20.0, TILE_Y + 70.0, NUMERAL_H, palette);
        out.push_str(&glyphs);
        if !caption.is_empty() {
            text(
                &mut out,
                (x + 24.0, TILE_Y + TILE_H - 26.0),
                22.0,
                560,
                palette.ken_secondary,
                "start",
                caption,
            );
        }
    }

    // Secondary row.
    let secondary = format!(
        "First strikes {} · Comebacks {} · Assists {} · PRs reviewed {}",
        card.first_strikes, card.comebacks, card.assists, card.prs_reviewed
    );
    text(
        &mut out,
        (TILE_X, TILE_Y + TILE_H + 60.0),
        26.0,
        400,
        palette.text_secondary,
        "start",
        &secondary,
    );

    // K.O. list.
    let mut y = TILE_Y + TILE_H + 112.0;
    if card.kos.is_empty() {
        text(
            &mut out,
            (TILE_X, y),
            24.0,
            400,
            palette.text_tertiary,
            "start",
            "No K.O.s this week",
        );
    }
    for ko in card.kos.iter().take(MAX_KO_LINES) {
        let mut line = format!("K.O. · !{} · {}", ko.pr, ko.when);
        if options.show_repos
            && let Some(repo) = &ko.repo
        {
            let _ = write!(line, " · {repo}");
        }
        if options.show_titles {
            let _ = write!(line, " · {}", ko.title);
        }
        text(
            &mut out,
            (TILE_X, y),
            24.0,
            400,
            palette.text_primary,
            "start",
            &line,
        );
        y += 36.0;
    }
    if card.kos.len() > MAX_KO_LINES {
        text(
            &mut out,
            (TILE_X, y),
            22.0,
            400,
            palette.text_tertiary,
            "start",
            &format!("+{} more", card.kos.len() - MAX_KO_LINES),
        );
    }

    // Wordmark.
    text(
        &mut out,
        (WIDTH as f32 - TILE_X, HEIGHT as f32 - 44.0),
        24.0,
        600,
        palette.text_tertiary,
        "end",
        "adoken.dev",
    );
    out.push_str("</svg>");
    out
}

/// The 拳 emblem: kanji in a ken-colored ring (brush artwork pending).
fn emblem(out: &mut String, cx: f32, cy: f32, palette: &Palette) {
    let _ = write!(
        out,
        "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"34\" fill=\"none\" \
         stroke=\"#{:02x}{:02x}{:02x}\" stroke-width=\"4\"/>",
        palette.ken_primary.r, palette.ken_primary.g, palette.ken_primary.b
    );
    text(
        out,
        (cx, cy + 13.0),
        36.0,
        700,
        palette.ken_primary,
        "middle",
        "拳",
    );
}

/// Font database for rasterizing (system fonts).
pub fn options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options
}

/// Rasterize card SVG to PNG bytes.
pub fn png(svg: &str, options: &usvg::Options<'_>) -> Result<Vec<u8>, CardError> {
    let tree = usvg::Tree::from_str(svg, options).map_err(CardError::Svg)?;
    let mut pixmap = tiny_skia::Pixmap::new(WIDTH, HEIGHT)
        .ok_or_else(|| CardError::Encode("empty canvas".into()))?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    let mut rgba = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for pixel in pixmap.pixels() {
        let c = pixel.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    let image = image::RgbaImage::from_raw(WIDTH, HEIGHT, rgba)
        .ok_or_else(|| CardError::Encode("pixel buffer size".into()))?;
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| CardError::Encode(e.to_string()))?;
    Ok(bytes)
}

/// Write PNG bytes atomically (temp file + rename) to the chosen path.
pub fn save(bytes: &[u8], path: &Path) -> Result<(), CardError> {
    let tmp = path.with_extension("png.partial");
    std::fs::write(&tmp, bytes).map_err(CardError::Io)?;
    std::fs::rename(&tmp, path).map_err(CardError::Io)
}

/// Suggested file name: `adoken-week-2026-09-21.png`-style, from the label.
pub fn file_name(card: &FightCard) -> String {
    format!(
        "adoken-fight-card-{}.png",
        card.week
            .label()
            .trim_start_matches("Week of ")
            .replace(' ', "-")
            .to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::Rank;
    use crate::week::{CardKo, Week};
    use adoken_notify::FakeClock;
    use adoken_theme::ThemeRegistry;

    fn card() -> FightCard {
        let clock = FakeClock::new(1_790_328_600_000, 0);
        FightCard {
            week: Week::of(1_790_328_600_000, &clock),
            hits: 34,
            best_combo: Some((9, Rank::Surge)),
            kos: vec![CardKo {
                pr: 4821,
                title: "Retry idempotency keys <v2>".into(),
                repo: Some("payments-api".into()),
                when: "Tue 14:02".into(),
                double: false,
            }],
            streak_days: 12,
            shields: 1,
            first_strikes: 2,
            comebacks: 1,
            assists: 5,
            prs_reviewed: 14,
        }
    }

    fn palette() -> Palette {
        let variant = ThemeRegistry::builtin().variants().next().unwrap();
        Palette::from_tokens(&variant.colors)
    }

    #[test]
    fn private_by_default_numbers_only() {
        let svg = svg(
            &card(),
            &palette(),
            Naming::EnglishKanji,
            &ExportOptions::default(),
        );
        assert!(svg.contains("!4821"));
        assert!(!svg.contains("payments-api"));
        assert!(!svg.contains("Retry idempotency"));
        assert!(svg.contains("Surge 怒涛"));
        assert!(svg.contains("1 shield"));
        assert!(svg.contains("adoken.dev"));
    }

    #[test]
    fn opted_in_names_are_escaped() {
        let options = ExportOptions {
            show_repos: true,
            show_titles: true,
            background: None,
        };
        let svg = svg(&card(), &palette(), Naming::English, &options);
        assert!(svg.contains("payments-api"));
        assert!(svg.contains("Retry idempotency keys &lt;v2&gt;"));
        assert!(!svg.contains("怒涛"), "English naming");
    }

    #[test]
    fn rasterizes_to_a_1200_by_675_png() {
        let svg = svg(
            &card(),
            &palette(),
            Naming::EnglishKanji,
            &ExportOptions::default(),
        );
        let bytes = png(&svg, &usvg::Options::default()).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (WIDTH, HEIGHT));
        // Opaque background from the theme, never transparent.
        assert_eq!(image.to_rgba8().get_pixel(2, 2)[3], 255);
    }

    #[test]
    fn file_name_is_dated() {
        assert_eq!(file_name(&card()), "adoken-fight-card-sep-21.png");
    }
}
