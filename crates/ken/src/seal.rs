//! The 済 seal (ken.md §9, §12.3): an original, in-house drawing of *sumi*
//! ("settled") as tapered brush strokes inside a rounded-square seal border
//! with a slightly broken, hand-pressed edge, on a 100-unit grid. Two
//! layers colored at runtime: `outline` (`ken.outline`, painted first, the
//! 2 px stroke at 40 px) and `fill` (`ken.primary`). Like the arcade
//! glyphs, every shape is a convex polygon, so the same geometry paints in
//! gpui (`PathBuilder`, no raster or decode) and as the SVG asset
//! `assets/ken/seal-done.svg` ([`svg`]; a test keeps the file in sync).

use crate::glyph::{Poly, Pt, grow, path_data, signed_area};

/// Grid size.
pub const SIZE: f32 = 100.0;
/// How far the outline reaches past the fill, in grid units: the outer
/// half of a 2 px stroke at the 40 px seal.
pub const OUTLINE_UNITS: f32 = 2.5;

fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

/// Clockwise (y down) so overlapping shapes union under non-zero fill.
fn clockwise(mut poly: Poly) -> Poly {
    if signed_area(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

/// A brush stroke from `a` to `b`, `w0` wide where it starts and `w1`
/// where it lifts.
fn stroke(a: (f32, f32), b: (f32, f32), w0: f32, w1: f32) -> Poly {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
    let (nx, ny) = (-dy / len, dx / len);
    clockwise(vec![
        pt(a.0 + nx * w0 / 2.0, a.1 + ny * w0 / 2.0),
        pt(b.0 + nx * w1 / 2.0, b.1 + ny * w1 / 2.0),
        pt(b.0 - nx * w1 / 2.0, b.1 - ny * w1 / 2.0),
        pt(a.0 - nx * w0 / 2.0, a.1 - ny * w0 / 2.0),
    ])
}

/// One slice of a ring around `(cx, cy)` between angles `a0` and `a1`
/// (degrees, y down): a convex quad.
fn arc(c: (f32, f32), outer: f32, inner: f32, a0: f32, a1: f32) -> Poly {
    let at = |r: f32, a: f32| {
        let (sin, cos) = a.to_radians().sin_cos();
        pt(c.0 + cos * r, c.1 + sin * r)
    };
    clockwise(vec![
        at(outer, a0),
        at(outer, a1),
        at(inner, a1),
        at(inner, a0),
    ])
}

/// The seal border: four sides with two pressed-out breaks, uneven ink,
/// and rounded corners in three slices each.
fn border() -> Vec<Poly> {
    let quad = |x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32| {
        clockwise(vec![pt(x0, y0), pt(x1, y1), pt(x2, y2), pt(x3, y3)])
    };
    let mut shapes = vec![
        // Top, broken right of center.
        quad(16.0, 6.0, 60.0, 6.5, 60.0, 13.0, 16.0, 13.5),
        quad(65.0, 6.8, 84.0, 6.0, 84.0, 12.5, 65.0, 12.8),
        // Right, a touch thinner at the foot where the press lifted.
        quad(87.0, 16.0, 94.0, 16.0, 93.6, 84.0, 87.8, 84.0),
        // Bottom.
        quad(16.0, 86.6, 84.0, 87.0, 84.0, 94.0, 16.0, 93.6),
        // Left, broken low.
        quad(6.0, 16.0, 13.4, 16.0, 13.0, 67.0, 6.2, 67.0),
        quad(6.4, 72.5, 13.0, 72.5, 13.0, 84.0, 6.0, 84.0),
    ];
    let corners = [
        ((16.0, 16.0), 180.0),
        ((84.0, 16.0), 270.0),
        ((84.0, 84.0), 0.0),
        ((16.0, 84.0), 90.0),
    ];
    for (center, start) in corners {
        for i in 0..3 {
            let a0 = start + 30.0 * i as f32;
            // Slight overlap so the slices join without hairline seams.
            shapes.push(arc(center, 10.0, 3.0, a0 - 0.5, a0 + 30.5));
        }
    }
    shapes
}

/// 済: 氵 on the left, 斉 on the right, as tapered brush strokes.
fn kanji() -> Vec<Poly> {
    vec![
        // 氵: two drops and the rising tick.
        stroke((22.0, 25.0), (28.5, 31.5), 7.0, 3.5),
        stroke((20.0, 41.0), (27.0, 47.5), 7.0, 3.5),
        stroke((22.0, 74.0), (32.5, 56.0), 7.5, 2.5),
        // 亠: the dot and the long bar.
        stroke((58.0, 19.5), (61.5, 26.0), 6.5, 3.5),
        stroke((38.0, 31.5), (80.0, 30.0), 4.5, 5.5),
        // The crossing strokes.
        stroke((67.5, 34.0), (42.0, 53.5), 5.5, 2.0),
        stroke((48.0, 36.0), (79.0, 52.5), 3.0, 7.5),
        // The short bar, then the two legs joined by a middle bar.
        stroke((46.0, 58.5), (73.5, 58.0), 4.5, 4.0),
        stroke((50.0, 60.0), (43.5, 80.5), 5.5, 2.0),
        stroke((69.0, 60.0), (69.5, 81.5), 5.5, 5.0),
        stroke((50.5, 69.5), (69.0, 69.0), 3.5, 3.5),
    ]
}

/// The seal's two layers in grid units.
#[derive(Clone, Debug, PartialEq)]
pub struct Seal {
    pub outline: Vec<Poly>,
    pub fill: Vec<Poly>,
}

pub fn seal() -> Seal {
    let fill: Vec<Poly> = border().into_iter().chain(kanji()).collect();
    Seal {
        outline: fill.iter().map(|p| grow(p, OUTLINE_UNITS)).collect(),
        fill,
    }
}

/// `assets/ken/seal-done.svg`: the two layers as `<g id="outline">` and
/// `<g id="fill">`, uncolored (colored at runtime).
pub fn svg() -> String {
    let seal = seal();
    let pad = OUTLINE_UNITS;
    let size = SIZE + 2.0 * pad;
    let group = |id: &str, polys: &[Poly]| {
        format!(
            "  <g id=\"{id}\">\n    <path d=\"{}\"/>\n  </g>\n",
            path_data(polys, 1.0, pad, pad).trim_end()
        )
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" \
         width=\"{size}\" height=\"{size}\">\n  \
         <!-- 済 seal, drawn in-house for adoken (docs/design/ken.md §9, §12.3). \
         Layers colored at runtime: outline = ken.outline, fill = ken.primary. -->\n{}{}</svg>\n",
        group("outline", &seal.outline),
        group("fill", &seal.fill)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_convex_clockwise_and_inside_the_grid() {
        let seal = seal();
        assert_eq!(seal.fill.len(), seal.outline.len());
        for poly in seal.fill.iter().chain(&seal.outline) {
            assert!(signed_area(poly) > 0.0);
            for p in poly {
                let m = OUTLINE_UNITS + 1.0;
                assert!((-m..=SIZE + m).contains(&p.x) && (-m..=SIZE + m).contains(&p.y));
            }
        }
    }

    /// ken.md §9: the asset in the repo is this geometry.
    #[test]
    fn asset_matches_the_geometry() {
        let asset = include_str!("../../../assets/ken/seal-done.svg");
        assert_eq!(
            asset,
            svg(),
            "regenerate assets/ken/seal-done.svg from seal::svg()"
        );
    }
}
