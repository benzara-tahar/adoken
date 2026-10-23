//! Paint Ken's arcade glyphs and sparks with gpui paths (no fonts, no
//! assets): the geometry is `adoken_ken::glyph`, colors are the `ken.*`
//! tokens, every layer is one non-zero-filled path.

use adoken_ken::glyph::{self, Poly};
use adoken_ken::sparks::Particle;
use gpui::{
    App, Bounds, FillOptions, Hsla, IntoElement, PathBuilder, PathStyle, Pixels, RenderOnce,
    Window, canvas, point, prelude::*, px,
};

/// Outline width in grid units: 3 px at 40 px tall (ken.md §4.1).
const OUTLINE_UNITS: f32 = 7.5;
/// Spark star radius and dot size (px).
const STAR_RADIUS: f32 = 4.0;
const DOT: f32 = 3.0;

/// Three token colors of a glyph run.
#[derive(Clone, Copy, Debug)]
pub struct GlyphColors {
    pub fill: Hsla,
    pub bevel: Hsla,
    pub outline: Hsla,
}

impl GlyphColors {
    pub fn ken(theme: &adoken_ui::Theme) -> Self {
        Self {
            fill: theme.colors.ken_primary,
            bevel: theme.colors.ken_secondary,
            outline: theme.colors.ken_outline,
        }
    }
}

/// A run of arcade glyphs (`"×7"`, `"K"`), `height` px tall, with an
/// optional transform about its center: scale, rotation (degrees) and
/// opacity for the pop / K.O. motions.
#[derive(IntoElement)]
pub struct ArcadeText {
    text: String,
    height: f32,
    colors: GlyphColors,
    scale: f32,
    rotate_deg: f32,
    opacity: f32,
}

impl ArcadeText {
    pub fn new(text: impl Into<String>, height: f32, colors: GlyphColors) -> Self {
        Self {
            text: text.into(),
            height,
            colors,
            scale: 1.0,
            rotate_deg: 0.0,
            opacity: 1.0,
        }
    }

    pub fn scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn rotate(mut self, degrees: f32) -> Self {
        self.rotate_deg = degrees;
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Laid-out width in px (untransformed).
    pub fn width(text: &str, height: f32) -> f32 {
        glyph::run(text, OUTLINE_UNITS).width * height / glyph::HEIGHT
    }
}

impl RenderOnce for ArcadeText {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let run = glyph::run(&self.text, OUTLINE_UNITS);
        let unit = self.height / glyph::HEIGHT;
        let margin = OUTLINE_UNITS * unit;
        let width = run.width * unit;
        let height = self.height + 2.0 * margin;
        let Self {
            colors,
            scale,
            rotate_deg,
            opacity,
            ..
        } = self;
        canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                if opacity <= 0.0 || scale <= 0.0 {
                    return;
                }
                let center = (width / 2.0, height / 2.0);
                let (sin, cos) = rotate_deg.to_radians().sin_cos();
                let map = |p: &glyph::Pt| {
                    let (x, y) = (p.x * unit - center.0, p.y * unit + margin - center.1);
                    let (x, y) = (x * scale, y * scale);
                    let (x, y) = (x * cos - y * sin, x * sin + y * cos);
                    point(
                        bounds.origin.x + px(x + center.0),
                        bounds.origin.y + px(y + center.1),
                    )
                };
                for (polys, color) in [
                    (&run.layers.outline, colors.outline),
                    (&run.layers.fill, colors.fill),
                    (&run.layers.bevel, colors.bevel),
                ] {
                    paint_polys(polys, &map, color.opacity(opacity * color.a), window);
                }
            },
        )
        .w(px(width))
        .h(px(height))
        .flex_none()
    }
}

fn paint_polys(
    polys: &[Poly],
    map: &impl Fn(&glyph::Pt) -> gpui::Point<Pixels>,
    color: Hsla,
    window: &mut Window,
) {
    if polys.is_empty() {
        return;
    }
    let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(FillOptions::non_zero()));
    for poly in polys {
        let points: Vec<_> = poly.iter().map(map).collect();
        builder.add_polygon(&points, true);
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// Paint sparks (positions relative to `bounds`' origin).
pub fn paint_sparks(
    particles: &[Particle],
    spark: Hsla,
    secondary: Hsla,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let star = glyph::spark();
    for p in particles {
        let color = if p.spark_color { spark } else { secondary };
        let color = color.opacity(p.opacity() * color.a);
        let origin = (bounds.origin.x + px(p.x), bounds.origin.y + px(p.y));
        let mut builder = PathBuilder::fill();
        if p.star {
            let r = STAR_RADIUS * p.scale();
            let points: Vec<_> = star
                .iter()
                .map(|s| point(origin.0 + px(s.x * r), origin.1 + px(s.y * r)))
                .collect();
            builder.add_polygon(&points, true);
        } else {
            let d = DOT * p.scale() / 2.0;
            builder.add_polygon(
                &[
                    point(origin.0 - px(d), origin.1 - px(d)),
                    point(origin.0 + px(d), origin.1 - px(d)),
                    point(origin.0 + px(d), origin.1 + px(d)),
                    point(origin.0 - px(d), origin.1 + px(d)),
                ],
                true,
            );
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}
