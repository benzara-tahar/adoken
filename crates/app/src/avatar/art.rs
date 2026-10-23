//! Generated pictures for demo people (spec 0012 FR-13): abstract art
//! seeded by the identity id, never a photo of anyone. Two colors from the
//! demo illustration palette in a diagonal wash, one soft disc and one band.

use adoken_theme::Color;
use adoken_theme::background::palette::{DUSK, LANTERN_GLOWS, illustration as art};
use image::{Rgba, RgbaImage};

/// Pairs (wash, accent); each reads on the other.
const PAIRS: [(Color, Color); 6] = [
    (art::SKY[1], art::SUN),
    (art::RIDGES[2], art::BLOSSOM[0]),
    (art::GATE_SHADE, LANTERN_GLOWS[1].color),
    (art::SKY[0], art::SKY[3]),
    (art::RIDGES[0], art::STAR),
    (DUSK[1], DUSK[2]),
];

/// FNV-1a: stable across runs and platforms (snapshots depend on it).
pub fn seed(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn mix(a: Color, b: Color, t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| x as f32 + (y as f32 - x as f32) * t;
    [lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b)]
}

/// A `side` × `side` opaque RGBA picture for identity `id`.
pub fn generate(id: &str, side: u32) -> RgbaImage {
    let h = seed(id);
    let (wash, accent) = PAIRS[(h % PAIRS.len() as u64) as usize];
    let byte = |shift: u32| ((h >> shift) & 0xFF) as f32 / 255.0;
    // Disc center and radius, band angle, all in unit space.
    let (cx, cy) = (0.25 + 0.5 * byte(8), 0.25 + 0.5 * byte(16));
    let radius = 0.22 + 0.18 * byte(24);
    let tilt = byte(32) * 2.0 - 1.0;
    let band = 0.55 + 0.3 * byte(40);
    let s = side as f32;
    RgbaImage::from_fn(side, side, |x, y| {
        let (u, v) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
        let mut rgb = mix(wash, accent, (u + v) * 0.25);
        // Band: a darker stripe across the lower part.
        let edge = v - (band + tilt * (u - 0.5) * 0.4);
        if edge > 0.0 {
            let shade = mix(wash, accent, 0.1);
            let t = (edge * s).clamp(0.0, 1.0) * 0.7;
            for (c, d) in rgb.iter_mut().zip(shade) {
                *c += (d * 0.6 - *c) * t;
            }
        }
        // Disc, anti-aliased over one pixel.
        let d = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - radius;
        let cover = (0.5 - d * s).clamp(0.0, 1.0);
        let lit = mix(accent, wash, 0.08);
        for (c, l) in rgb.iter_mut().zip(lit) {
            *c += (l - *c) * cover;
        }
        Rgba([rgb[0] as u8, rgb[1] as u8, rgb[2] as u8, 255])
    })
}
