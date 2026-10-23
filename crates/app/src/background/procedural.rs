//! Code-generated backgrounds (docs/design/tokens.md §10.5) and the demo
//! illustration. Original, deterministic, resolution independent: no bitmap
//! assets, so no licensing surface. Colors come from
//! `adoken_theme::background::palette` (only the theme crate spells colors).
//!
//! Smooth fields are evaluated at quarter resolution and upscaled; grain and
//! crisp shapes are drawn at full resolution. Runs on a background executor.

use adoken_theme::Color;
use adoken_theme::background::{BuiltinBackground, Glow, palette};
use image::{RgbaImage, imageops};

/// What to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Art {
    Builtin(BuiltinBackground),
    /// Dusk sky, layered ridges, a shrine gate, a blossoming branch.
    Illustration,
}

pub fn render(art: Art, width: u32, height: u32) -> RgbaImage {
    let (width, height) = (width.max(1), height.max(1));
    match art {
        Art::Builtin(BuiltinBackground::Dusk) => {
            let mut image = smooth(width, height, dusk);
            grain(&mut image, palette::DUSK_GRAIN, 0xD05C);
            image
        }
        Art::Builtin(BuiltinBackground::Lantern) => {
            let mut image = smooth(width, height, lantern);
            grain(&mut image, palette::LANTERN_GRAIN, 0x1A57);
            image
        }
        Art::Builtin(BuiltinBackground::Ink) => {
            let mut image = smooth(width, height, ink);
            grain(&mut image, palette::INK_GRAIN, 0x1417);
            image
        }
        Art::Illustration => illustration::render(width, height),
    }
}

type Rgb = [f32; 3];

fn rgb(color: Color) -> Rgb {
    [color.r as f32, color.g as f32, color.b as f32]
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Evaluate `field(u, v, aspect)` at quarter resolution, upscale bilinearly.
fn smooth(width: u32, height: u32, field: fn(f32, f32, f32) -> Rgb) -> RgbaImage {
    let (qw, qh) = (width.div_ceil(4).max(2), height.div_ceil(4).max(2));
    let aspect = width as f32 / height as f32;
    let small = RgbaImage::from_fn(qw, qh, |x, y| {
        let u = (x as f32 + 0.5) / qw as f32;
        let v = (y as f32 + 0.5) / qh as f32;
        let [r, g, b] = field(u, v, aspect);
        image::Rgba([r as u8, g as u8, b as u8, 255])
    });
    imageops::resize(&small, width, height, imageops::FilterType::Triangle)
}

/// Monochrome film grain at `amount` opacity.
fn grain(image: &mut RgbaImage, amount: f32, seed: u32) {
    let mut rng = Rng(seed | 1);
    let scale = amount * 255.0;
    for pixel in image.pixels_mut() {
        let noise = rng.signed() * scale;
        for channel in &mut pixel.0[..3] {
            *channel = (*channel as f32 + noise).clamp(0.0, 255.0) as u8;
        }
    }
}

fn dusk(_u: f32, v: f32, _aspect: f32) -> Rgb {
    let [top, mid, glow] = palette::DUSK.map(rgb);
    let horizon = 1.0 - palette::DUSK_HORIZON;
    let base = lerp(top, mid, smoothstep(0.0, horizon, v));
    // The vermilion band rises softly from the bottom 8 %.
    let band = smoothstep(horizon - 0.12, 1.0, v).powf(1.6);
    lerp(base, glow, band * 0.9)
}

fn lantern(u: f32, v: f32, aspect: f32) -> Rgb {
    let mut color = rgb(palette::LANTERN_BASE);
    for glow in &palette::LANTERN_GLOWS {
        color = add_glow(color, glow, u, v, aspect);
    }
    color
}

fn add_glow(base: Rgb, glow: &Glow, u: f32, v: f32, aspect: f32) -> Rgb {
    let dx = (u - glow.x) * aspect;
    let dy = v - glow.y;
    let diagonal = (aspect * aspect + 1.0).sqrt();
    let d = (dx * dx + dy * dy).sqrt() / (glow.radius * diagonal);
    let falloff = (1.0 - d).max(0.0).powf(2.2) * glow.strength;
    let color = rgb(glow.color);
    [
        (base[0] + (color[0] - base[0]) * falloff).min(255.0),
        (base[1] + (color[1] - base[1]) * falloff).min(255.0),
        (base[2] + (color[2] - base[2]) * falloff).min(255.0),
    ]
}

fn ink(u: f32, v: f32, aspect: f32) -> Rgb {
    let [dark, light] = palette::INK.map(rgb);
    let (x, y) = (u * aspect * 2.2, v * 2.2);
    // Domain warp (fbm of fbm): flowing, brush-like wash.
    let qx = fbm(x, y, 4, 11);
    let qy = fbm(x + 5.2, y + 1.3, 4, 12);
    let n = fbm(x + 3.0 * qx, y + 3.0 * qy, 5, 13);
    lerp(dark, light, smoothstep(0.30, 0.78, n))
}

/// Hash-based value noise in 0..1.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (xf, yf) = (x - xi, y - yi);
    let (xi, yi) = (xi as i32, yi as i32);
    let corner = |dx: i32, dy: i32| hash(xi + dx, yi + dy, seed);
    let (sx, sy) = (xf * xf * (3.0 - 2.0 * xf), yf * yf * (3.0 - 2.0 * yf));
    let top = corner(0, 0) + (corner(1, 0) - corner(0, 0)) * sx;
    let bottom = corner(0, 1) + (corner(1, 1) - corner(0, 1)) * sx;
    top + (bottom - top) * sy
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27D4_EB2D)
        ^ (y as u32).wrapping_mul(0x1656_67B1)
        ^ seed.wrapping_mul(0x9E37_79B9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amplitude, mut frequency, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for octave in 0..octaves {
        sum += value_noise(x * frequency, y * frequency, seed + octave) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        frequency *= 2.03;
    }
    sum / norm
}

/// Deterministic xorshift.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x as f32 / u32::MAX as f32
    }

    fn signed(&mut self) -> f32 {
        self.next() * 2.0 - 1.0
    }
}

mod illustration {
    use super::*;
    use palette::illustration as p;

    /// Float canvas; shapes blend with coverage for anti-aliased edges.
    struct Canvas {
        width: u32,
        height: u32,
        pixels: Vec<Rgb>,
    }

    impl Canvas {
        fn blend(&mut self, x: i64, y: i64, color: Rgb, alpha: f32) {
            if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 || alpha <= 0.0 {
                return;
            }
            let pixel = &mut self.pixels[(y as u32 * self.width + x as u32) as usize];
            *pixel = lerp(*pixel, color, alpha.min(1.0));
        }

        /// Filled disc (or ellipse via `squash`) with a soft 1 px edge.
        fn disc(&mut self, cx: f32, cy: f32, radius: f32, squash: f32, color: Rgb, alpha: f32) {
            let (rx, ry) = (radius, radius * squash);
            for y in (cy - ry - 1.0).floor() as i64..=(cy + ry + 1.0).ceil() as i64 {
                for x in (cx - rx - 1.0).floor() as i64..=(cx + rx + 1.0).ceil() as i64 {
                    let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                    let d = (dx * dx + dy * dy).sqrt();
                    let coverage = ((1.0 - d) * rx.min(ry)).clamp(0.0, 1.0);
                    self.blend(x, y, color, coverage * alpha);
                }
            }
        }

        /// Axis-aligned rectangle with anti-aliased edges.
        fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb) {
            for y in y0.floor() as i64..y1.ceil() as i64 {
                let cy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).clamp(0.0, 1.0);
                for x in x0.floor() as i64..x1.ceil() as i64 {
                    let cx = (x1.min(x as f32 + 1.0) - x0.max(x as f32)).clamp(0.0, 1.0);
                    self.blend(x, y, color, cx * cy);
                }
            }
        }

        /// A tapered stroke from `a` to `b`, as overlapping discs.
        fn stroke(&mut self, a: (f32, f32), b: (f32, f32), width: (f32, f32), color: Rgb) {
            let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let steps = (length / (width.1.max(1.0) * 0.35)).ceil().max(1.0) as u32;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let radius = (width.0 + (width.1 - width.0) * t) / 2.0;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                self.disc(x, y, radius, 1.0, color, 1.0);
            }
        }
    }

    pub fn render(width: u32, height: u32) -> RgbaImage {
        let (w, h) = (width as f32, height as f32);
        let unit = w.min(h);
        let aspect = w / h;

        // Sky, sun glow and cloud streaks: smooth, so quarter resolution.
        let sun = (0.66, 0.47);
        let sky = smooth(width, height, |u, v, aspect| {
            let stops = p::SKY.map(rgb);
            let horizon = 0.66;
            let t = (v / horizon).clamp(0.0, 1.0);
            let mut color = if t < 0.45 {
                lerp(stops[0], stops[1], t / 0.45)
            } else if t < 0.8 {
                lerp(stops[1], stops[2], (t - 0.45) / 0.35)
            } else {
                lerp(stops[2], stops[3], (t - 0.8) / 0.2)
            };
            let glow = Glow {
                color: p::SUN_GLOW,
                x: 0.66,
                y: 0.47,
                radius: 0.42,
                strength: 0.75,
            };
            color = add_glow(color, &glow, u, v, aspect);
            // Long horizontal cloud bands lit from below.
            for (center, thickness, seed) in [(0.36, 0.018, 3), (0.43, 0.012, 5), (0.51, 0.02, 7)] {
                let wobble = (fbm(u * 3.0, center * 10.0, 3, seed) - 0.5) * 0.05;
                let d = ((v - center - wobble) / thickness).abs();
                let breakup = smoothstep(0.35, 0.65, fbm(u * 7.0 * aspect, v * 20.0, 3, seed + 1));
                let alpha = (1.0 - d).max(0.0).powf(1.5) * breakup * 0.55;
                color = lerp(color, rgb(p::CLOUD), alpha);
            }
            color
        });
        let mut canvas = Canvas {
            width,
            height,
            pixels: sky
                .pixels()
                .map(|px| [px.0[0] as f32, px.0[1] as f32, px.0[2] as f32])
                .collect(),
        };

        // Stars, fading toward the horizon.
        let mut rng = Rng(0x57A2);
        let stars = (w * h / 5_500.0) as u32;
        for _ in 0..stars {
            let (x, y) = (rng.next() * w, rng.next().powf(1.6) * h * 0.5);
            let fade = 1.0 - y / (h * 0.5);
            let size = 0.4 + rng.next() * unit / 900.0;
            canvas.disc(
                x,
                y,
                size,
                1.0,
                rgb(p::STAR),
                fade * (0.35 + rng.next() * 0.65),
            );
        }

        // Sun disc, half set behind the ridges.
        canvas.disc(sun.0 * w, sun.1 * h, unit * 0.085, 1.0, rgb(p::SUN), 0.95);

        // Mountain ridges, far to near, each a noise skyline with a lighter rim.
        let ridges = [
            (0.58, 0.15, 2.0),
            (0.65, 0.13, 3.0),
            (0.73, 0.08, 4.5),
            (0.81, 0.05, 2.5),
        ];
        let near_ridge = ridges.len() - 1;
        let mut skyline = vec![0.0f32; width as usize];
        for (layer, &(base, amplitude, frequency)) in ridges.iter().enumerate() {
            let color = rgb(p::RIDGES[layer]);
            let rim = lerp(color, rgb(p::SUN_GLOW), 0.3);
            for (x, top) in skyline.iter_mut().enumerate() {
                let u = x as f32 / w;
                let n = fbm(
                    u * frequency * aspect,
                    layer as f32 * 7.3,
                    5,
                    40 + layer as u32,
                );
                // Ridged peaks for the far ranges, rolling hills up close.
                let shape = if layer < 2 {
                    1.0 - (n * 2.0 - 1.0).abs()
                } else {
                    n
                };
                *top = (base - amplitude * shape) * h;
            }
            for (x, &top) in skyline.iter().enumerate() {
                let start = top.floor().max(0.0) as i64;
                for y in start..height as i64 {
                    let coverage = (y as f32 + 1.0 - top).clamp(0.0, 1.0);
                    let depth = ((y as f32 - top) / (h * 0.012)).clamp(0.0, 1.0);
                    let shade = lerp(rim, color, depth);
                    canvas.blend(x as i64, y, shade, coverage);
                }
            }
            if layer == near_ridge {
                gate(&mut canvas, &skyline, w, h);
            }
        }

        branch(&mut canvas, w, h, unit);

        // Drifting petals.
        let mut rng = Rng(0xB105);
        let petals = (w * h / 28_000.0).max(12.0) as u32;
        for _ in 0..petals {
            let (x, y) = (rng.next() * w, rng.next() * h);
            let size = unit * (0.004 + rng.next() * 0.005);
            let color = rgb(p::BLOSSOM[(rng.next() * 2.99) as usize]);
            canvas.disc(x, y, size, 0.55, color, 0.75);
        }

        let mut image = RgbaImage::from_fn(width, height, |x, y| {
            let [r, g, b] = canvas.pixels[(y * width + x) as usize];
            image::Rgba([r as u8, g as u8, b as u8, 255])
        });
        grain(&mut image, p::GRAIN, 0x6A1F);
        image
    }

    /// A shrine gate standing on the near ridge at 24 % width.
    fn gate(canvas: &mut Canvas, skyline: &[f32], w: f32, h: f32) {
        let cx = w * 0.24;
        let ground = skyline[(cx as usize).min(skyline.len() - 1)] + h * 0.01;
        let height = h * 0.2;
        let span = height * 0.95;
        let pillar = height * 0.075;
        let (color, shade) = (rgb(p::GATE), rgb(p::GATE_SHADE));
        let top = ground - height;
        for side in [-1.0f32, 1.0] {
            // Pillars lean slightly inward.
            let x = cx + side * span / 2.0;
            canvas.rect(
                x - pillar / 2.0,
                top + height * 0.12,
                x + pillar / 2.0,
                ground,
                color,
            );
            canvas.rect(
                x + pillar * 0.15,
                top + height * 0.12,
                x + pillar / 2.0,
                ground,
                shade,
            );
        }
        // Lower beam (nuki), then the curved top beam (kasagi).
        let nuki = top + height * 0.3;
        canvas.rect(
            cx - span * 0.62,
            nuki,
            cx + span * 0.62,
            nuki + height * 0.055,
            color,
        );
        let beam = height * 0.075;
        let half = span * 0.72;
        let steps = (half * 2.0) as i64;
        for i in 0..=steps {
            let t = i as f32 / steps as f32 * 2.0 - 1.0;
            let lift = t.abs().powi(3) * height * 0.07;
            let x = cx + t * half;
            let y = top + height * 0.05 - lift;
            canvas.rect(x, y, x + 1.0, y + beam, color);
            canvas.rect(x, y + beam * 0.7, x + 1.0, y + beam, shade);
        }
        canvas.rect(
            cx - span * 0.55,
            top + height * 0.13,
            cx + span * 0.55,
            top + height * 0.18,
            shade,
        );
    }

    /// A blossoming branch reaching in from the top-left corner.
    fn branch(canvas: &mut Canvas, w: f32, h: f32, unit: f32) {
        let wood = rgb(p::BRANCH);
        let thick = unit * 0.022;
        // (from, to, (start width, end width)) as fractions of the image.
        type Limb = ((f32, f32), (f32, f32), (f32, f32));
        let limbs: [Limb; 6] = [
            ((-0.02, 0.10), (0.18, 0.16), (1.0, 0.65)),
            ((0.18, 0.16), (0.36, 0.13), (0.65, 0.35)),
            ((0.36, 0.13), (0.47, 0.18), (0.35, 0.15)),
            ((0.12, 0.145), (0.20, 0.27), (0.5, 0.2)),
            ((0.26, 0.15), (0.30, 0.05), (0.4, 0.15)),
            ((0.05, 0.12), (0.09, 0.00), (0.5, 0.2)),
        ];
        let mut tips = Vec::new();
        for (a, b, (w0, w1)) in limbs {
            let a = (a.0 * w, a.1 * h);
            let b = (b.0 * w, b.1 * h);
            canvas.stroke(a, b, (thick * w0, thick * w1), wood);
            tips.push((a, b));
        }
        let mut rng = Rng(0xF10E);
        for (a, b) in tips {
            let clusters = 9;
            for i in 0..clusters {
                let t = 0.25 + 0.75 * i as f32 / clusters as f32;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                let (x, y) = (
                    x + rng.signed() * unit * 0.02,
                    y + rng.signed() * unit * 0.02,
                );
                flower(canvas, x, y, unit * (0.009 + rng.next() * 0.006), &mut rng);
            }
        }
    }

    /// Five petals around a darker heart.
    fn flower(canvas: &mut Canvas, x: f32, y: f32, size: f32, rng: &mut Rng) {
        let turn = rng.next() * std::f32::consts::TAU;
        let petal = rgb(p::BLOSSOM[(rng.next() * 1.99) as usize]);
        for k in 0..5 {
            let angle = turn + k as f32 * std::f32::consts::TAU / 5.0;
            let (px, py) = (x + angle.cos() * size, y + angle.sin() * size);
            canvas.disc(px, py, size * 0.72, 1.0, petal, 0.95);
        }
        canvas.disc(x, y, size * 0.35, 1.0, rgb(p::BLOSSOM[2]), 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mean_luminance(image: &RgbaImage) -> f32 {
        image
            .pixels()
            .map(|px| Color::rgb(px.0[0], px.0[1], px.0[2]).luminance())
            .sum::<f32>()
            / (image.width() * image.height()) as f32
    }

    #[test]
    fn deterministic_at_the_requested_size() {
        for art in [
            Art::Builtin(BuiltinBackground::Dusk),
            Art::Builtin(BuiltinBackground::Lantern),
            Art::Builtin(BuiltinBackground::Ink),
            Art::Illustration,
        ] {
            let a = render(art, 160, 100);
            assert_eq!(a.dimensions(), (160, 100), "{art:?}");
            assert_eq!(a, render(art, 160, 100), "{art:?}");
            assert!(a.pixels().all(|px| px.0[3] == 255), "{art:?} is opaque");
        }
    }

    #[test]
    fn builtins_look_like_their_recipe() {
        // Dusk: dark top, vermilion bottom edge.
        let dusk = render(Art::Builtin(BuiltinBackground::Dusk), 64, 200);
        let (top, bottom) = (dusk.get_pixel(32, 2).0, dusk.get_pixel(32, 198).0);
        assert!(bottom[0] > top[0] + 120, "{top:?} → {bottom:?}");
        // Lantern: brightest near the vermilion glow at (30 %, 70 %).
        let lantern = render(Art::Builtin(BuiltinBackground::Lantern), 200, 200);
        let near = lantern.get_pixel(60, 140).0;
        let corner = lantern.get_pixel(199, 199).0;
        assert!(near[0] > corner[0] + 60, "{near:?} vs {corner:?}");
        // Ink: stays inside its dark wash.
        let ink = render(Art::Builtin(BuiltinBackground::Ink), 200, 120);
        assert!(mean_luminance(&ink) < 0.05);
    }

    #[test]
    fn illustration_has_a_bright_sky_and_dark_foreground() {
        let art = render(Art::Illustration, 320, 200);
        let sky = art.get_pixel(211, 60).0;
        let ground = art.get_pixel(300, 195).0;
        let luminance = |px: [u8; 4]| Color::rgb(px[0], px[1], px[2]).luminance();
        assert!(
            luminance(sky) > luminance(ground) * 3.0,
            "{sky:?} {ground:?}"
        );
    }
}
