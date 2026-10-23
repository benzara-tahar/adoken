//! Original arcade glyphs (ken.md §9): `0–9`, `×`, `K`, `O`, `.` on a
//! 100-unit-tall grid with chunky 18-unit strokes, a 12° forward slant and
//! three layers colored at runtime: `outline` (`ken.outline`, painted
//! first), `fill` (`ken.primary`) and `bevel` (`ken.secondary`, the top 30 %
//! band). No fonts: every glyph is convex polygons, so the same geometry
//! paints in gpui (`PathBuilder`) and in the fight-card SVG.
//!
//! The spark (§4.4) is a 4-point star with concave sides.

/// Glyph height in grid units.
pub const HEIGHT: f32 = 100.0;
/// Stroke weight in grid units.
pub const STROKE: f32 = 18.0;
/// Top band painted in the bevel color.
pub const BEVEL_BAND: f32 = 30.0;
/// Forward slant.
pub const SLANT_DEG: f32 = 12.0;
/// Gap between glyphs in a run, grid units.
pub const TRACKING: f32 = 8.0;

/// A point in grid units, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// A convex polygon.
pub type Poly = Vec<Pt>;

/// One glyph's three layers, slanted, in grid units.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layers {
    pub outline: Vec<Poly>,
    pub fill: Vec<Poly>,
    pub bevel: Vec<Poly>,
}

impl Layers {
    fn offset(&mut self, dx: f32) {
        for poly in self
            .outline
            .iter_mut()
            .chain(&mut self.fill)
            .chain(&mut self.bevel)
        {
            for p in poly {
                p.x += dx;
            }
        }
    }

    fn extend(&mut self, other: Layers) {
        self.outline.extend(other.outline);
        self.fill.extend(other.fill);
        self.bevel.extend(other.bevel);
    }
}

/// Characters with a glyph.
pub fn supports(c: char) -> bool {
    matches!(c, '0'..='9' | '×' | 'K' | 'O' | '.')
}

/// Advance width (before tracking) of a supported character.
pub fn advance(c: char) -> f32 {
    match c {
        '1' => 56.0,
        '×' => 60.0,
        'K' => 72.0,
        'O' => 80.0,
        '.' => 22.0,
        _ => 64.0,
    }
}

/// Unslanted stroke shapes of a character.
fn shapes(c: char) -> Vec<Poly> {
    let t = STROKE;
    let w = advance(c);
    let top = || rect(0.0, 0.0, w, t);
    let bottom = || rect(0.0, HEIGHT - t, w, HEIGHT);
    let middle = || rect(0.0, 41.0, w, 59.0);
    let left = |y0, y1| rect(0.0, y0, t, y1);
    let right = |y0, y1| rect(w - t, y0, w, y1);
    match c {
        '0' | 'O' => vec![top(), bottom(), left(0.0, HEIGHT), right(0.0, HEIGHT)],
        '1' => vec![
            rect(8.0, 0.0, 38.0, t),
            rect(20.0, 0.0, 38.0, HEIGHT),
            rect(4.0, HEIGHT - t, 52.0, HEIGHT),
        ],
        '2' => vec![
            top(),
            right(0.0, 59.0),
            middle(),
            left(41.0, HEIGHT),
            bottom(),
        ],
        '3' => vec![
            top(),
            right(0.0, HEIGHT),
            rect(14.0, 41.0, w, 59.0),
            bottom(),
        ],
        '4' => vec![left(0.0, 59.0), middle(), right(0.0, HEIGHT)],
        '5' => vec![
            top(),
            left(0.0, 59.0),
            middle(),
            right(41.0, HEIGHT),
            bottom(),
        ],
        '6' => vec![
            top(),
            left(0.0, HEIGHT),
            middle(),
            right(41.0, HEIGHT),
            bottom(),
        ],
        '7' => vec![
            top(),
            bar(pt(w - t / 2.0, t), pt(24.0, HEIGHT - t / 2.0), t),
        ],
        '8' => vec![
            top(),
            bottom(),
            left(0.0, HEIGHT),
            right(0.0, HEIGHT),
            middle(),
        ],
        '9' => vec![
            top(),
            left(0.0, 59.0),
            middle(),
            right(0.0, HEIGHT),
            bottom(),
        ],
        '×' => vec![
            bar(pt(10.0, 36.0), pt(50.0, 88.0), 15.0),
            bar(pt(50.0, 36.0), pt(10.0, 88.0), 15.0),
        ],
        'K' => vec![
            left(0.0, HEIGHT),
            bar(pt(t, 56.0), pt(w - 6.0, 6.0), 20.0),
            bar(pt(24.0, 40.0), pt(w - 4.0, HEIGHT - 6.0), 20.0),
        ],
        '.' => vec![rect(0.0, HEIGHT - 22.0, 22.0, HEIGHT)],
        _ => Vec::new(),
    }
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Poly {
    vec![pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)]
}

/// A thick segment from `a` to `b` with square ends, as a quad.
fn bar(a: Pt, b: Pt, thickness: f32) -> Poly {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    let (nx, ny) = (-dy / len * thickness / 2.0, dx / len * thickness / 2.0);
    vec![
        pt(a.x + nx, a.y + ny),
        pt(b.x + nx, b.y + ny),
        pt(b.x - nx, b.y - ny),
        pt(a.x - nx, a.y - ny),
    ]
}

/// Grow a convex polygon outward by `by` (miter joins, capped).
pub(crate) fn grow(poly: &Poly, by: f32) -> Poly {
    let n = poly.len();
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let sign = if area >= 0.0 { 1.0 } else { -1.0 };
    let normal = |a: Pt, b: Pt| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
        // Outward for either winding.
        (dy / len * sign, -dx / len * sign)
    };
    (0..n)
        .map(|i| {
            let prev = poly[(i + n - 1) % n];
            let cur = poly[i];
            let next = poly[(i + 1) % n];
            let (n1x, n1y) = normal(prev, cur);
            let (n2x, n2y) = normal(cur, next);
            let (mx, my) = (n1x + n2x, n1y + n2y);
            let dot = 1.0 + n1x * n2x + n1y * n2y;
            let scale = (by * 2.0 / dot.max(0.5)).min(by * 2.0);
            pt(cur.x + mx * scale / 2.0, cur.y + my * scale / 2.0)
        })
        .collect()
}

/// Keep the part of a convex polygon with `y <= limit` (Sutherland–Hodgman).
fn clip_above(poly: &Poly, limit: f32) -> Option<Poly> {
    let n = poly.len();
    let mut out = Vec::with_capacity(n + 2);
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (a_in, b_in) = (a.y <= limit, b.y <= limit);
        if a_in {
            out.push(a);
        }
        if a_in != b_in {
            let t = (limit - a.y) / (b.y - a.y);
            out.push(pt(a.x + (b.x - a.x) * t, limit));
        }
    }
    (out.len() >= 3).then_some(out)
}

/// Slant, then orient clockwise (y down) so overlapping polygons union
/// under the non-zero fill rule (SVG's default, gpui's `FillRule::NonZero`).
fn slant(poly: &mut Poly) {
    let k = SLANT_DEG.to_radians().tan();
    for p in poly.iter_mut() {
        p.x += (HEIGHT - p.y) * k;
    }
    if signed_area(poly) < 0.0 {
        poly.reverse();
    }
}

pub(crate) fn signed_area(poly: &Poly) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f32>()
        / 2.0
}

/// Extra width the slant adds at the top of a run.
pub fn slant_overhang() -> f32 {
    HEIGHT * SLANT_DEG.to_radians().tan()
}

/// One character's layers at the origin; `outline` is the stroke width in
/// grid units (3 px at 40 px tall ≈ 7.5 units).
pub fn glyph(c: char, outline: f32) -> Layers {
    let shapes = shapes(c);
    let mut layers = Layers {
        outline: shapes.iter().map(|s| grow(s, outline)).collect(),
        bevel: shapes
            .iter()
            .filter_map(|s| clip_above(s, BEVEL_BAND))
            .collect(),
        fill: shapes,
    };
    for poly in layers
        .outline
        .iter_mut()
        .chain(&mut layers.fill)
        .chain(&mut layers.bevel)
    {
        slant(poly);
    }
    layers
}

/// A laid-out run of glyphs ("×7", "K.O.").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub layers: Layers,
    /// Total width in grid units, including the slant overhang.
    pub width: f32,
}

/// Lay out `text` left to right; unsupported characters are skipped.
pub fn run(text: &str, outline: f32) -> Run {
    let mut layers = Layers::default();
    let mut x = outline;
    let mut first = true;
    for c in text.chars().filter(|c| supports(*c)) {
        if !first {
            x += TRACKING;
        }
        first = false;
        let mut g = glyph(c, outline);
        g.offset(x);
        layers.extend(g);
        x += advance(c);
    }
    Run {
        layers,
        width: x + slant_overhang() + outline,
    }
}

/// The spark: 4-point star, concave sides, unit radius around the origin.
/// Non-convex: paint as one filled polygon (gpui's tessellator and SVG both
/// fill it correctly).
pub fn spark() -> Poly {
    let inner = 0.28;
    (0..8)
        .map(|i| {
            let angle = std::f32::consts::FRAC_PI_4 * i as f32 - std::f32::consts::FRAC_PI_2;
            let r = if i % 2 == 0 { 1.0 } else { inner };
            pt(angle.cos() * r, angle.sin() * r)
        })
        .collect()
}

/// SVG path data for polygons scaled by `scale` and moved to `(x, y)`.
pub fn path_data(polys: &[Poly], scale: f32, x: f32, y: f32) -> String {
    use std::fmt::Write;
    let mut d = String::new();
    for poly in polys {
        for (i, p) in poly.iter().enumerate() {
            let cmd = if i == 0 { 'M' } else { 'L' };
            let _ = write!(d, "{cmd}{:.2} {:.2} ", x + p.x * scale, y + p.y * scale);
        }
        d.push_str("Z ");
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_strokes_share_one_winding() {
        for c in "0123456789×KO.".chars() {
            let g = glyph(c, 7.5);
            for poly in g.outline.iter().chain(&g.fill).chain(&g.bevel) {
                assert!(signed_area(poly) > 0.0, "{c}");
            }
        }
    }

    #[test]
    fn every_glyph_has_three_non_empty_layers() {
        for c in "0123456789×KO.".chars() {
            let g = glyph(c, 7.5);
            assert!(!g.fill.is_empty(), "{c}");
            assert_eq!(g.fill.len(), g.outline.len(), "{c}");
            // `.` sits at the baseline, below the bevel band.
            assert_eq!(g.bevel.is_empty(), matches!(c, '.' | '×'), "{c}");
        }
    }

    #[test]
    fn bevel_stays_in_the_top_band_and_outline_contains_fill() {
        let g = glyph('8', 7.5);
        // After slanting y is unchanged.
        for poly in &g.bevel {
            assert!(poly.iter().all(|p| p.y <= BEVEL_BAND + 0.01));
        }
        let (fill_min, outline_min) = (
            g.fill
                .iter()
                .flatten()
                .map(|p| p.y)
                .fold(f32::MAX, f32::min),
            g.outline
                .iter()
                .flatten()
                .map(|p| p.y)
                .fold(f32::MAX, f32::min),
        );
        assert!(outline_min < fill_min);
    }

    #[test]
    fn slant_leans_forward() {
        let g = glyph('1', 0.0);
        let stem = &g.fill[1];
        let top_x = stem
            .iter()
            .filter(|p| p.y == 0.0)
            .map(|p| p.x)
            .fold(0.0, f32::max);
        let bottom_x = stem
            .iter()
            .filter(|p| p.y == HEIGHT)
            .map(|p| p.x)
            .fold(0.0, f32::max);
        assert!(top_x > bottom_x);
    }

    #[test]
    fn runs_advance_and_skip_unknown_characters() {
        let one = run("7", 0.0);
        let two = run("77", 0.0);
        assert!((two.width - one.width - advance('7') - TRACKING).abs() < 0.01);
        assert_eq!(
            run("×?7", 0.0).layers.fill.len(),
            run("×7", 0.0).layers.fill.len()
        );
    }

    #[test]
    fn spark_is_a_four_point_star() {
        let star = spark();
        assert_eq!(star.len(), 8);
        let tips = star
            .iter()
            .filter(|p| (p.x.hypot(p.y) - 1.0).abs() < 1e-4)
            .count();
        assert_eq!(tips, 4);
    }

    #[test]
    fn path_data_closes_every_polygon() {
        let d = path_data(&glyph('O', 0.0).fill, 1.0, 0.0, 0.0);
        assert_eq!(d.matches('Z').count(), 4);
        assert!(d.starts_with('M'));
    }
}
