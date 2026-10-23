//! Background image pipeline (docs/design/tokens.md §10.3): import, decode,
//! fit, blur, saturation, legibility samples, disk cache. Pure CPU work on
//! plain `image` buffers; the controller runs it on a background executor
//! and only uploads the result.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use adoken_theme::background::{
    Anchor, BackgroundConfig, BackgroundSource, Fit, legibility_sample,
};
use adoken_theme::{Appearance, Color};
use image::{ImageReader, RgbaImage, imageops};
use sha2::{Digest, Sha256};

use super::procedural::{self, Art};

/// §10.3 import limits.
pub const MAX_IMPORT_BYTES: u64 = 40 * 1024 * 1024;
pub const MAX_IMPORT_SIDE: u32 = 12_000;
/// Largest texture (device px); keeps a 5K window within the 64 MB budget
/// (3840 × 2400 × 4 B ≈ 35 MB).
pub const MAX_TEXTURE: (u32, u32) = (3840, 2400);
/// Blur at quarter resolution from this radius up (visually identical).
const QUARTER_BLUR_FROM: u8 = 8;
/// Bump when processing output changes so stale disk-cache files miss.
const PIPELINE_VERSION: u32 = 1;
const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// The pixel source to process, after resolving settings and imports.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Input {
    Art(Art),
    /// An imported copy under `<data>/backgrounds/`.
    File(PathBuf),
}

/// Everything that changes the baked texture (dim, tint and opacities are
/// paint-time and not part of it).
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub input: Input,
    pub fit: Fit,
    pub anchor: Anchor,
    pub blur: u8,
    pub saturation: f32,
    /// Target texture size in device pixels.
    pub size: (u32, u32),
    /// Device pixels per logical pixel (blur is specified at 1x).
    pub scale: f32,
}

impl Recipe {
    pub fn new(input: Input, config: &BackgroundConfig, size: (u32, u32), scale: f32) -> Self {
        Self {
            input,
            fit: config.fit,
            anchor: config.anchor,
            blur: config.blur,
            saturation: config.saturation,
            size: texture_size(size),
            scale,
        }
    }

    /// Disk-cache key: stable across runs, changes with any baked input.
    pub fn cache_key(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(PIPELINE_VERSION.to_le_bytes());
        match &self.input {
            Input::Art(art) => hasher.update(format!("art:{art:?}")),
            Input::File(path) => hasher.update(format!("file:{}", path.display())),
        }
        hasher.update(format!(
            "{:?}|{:?}|{}|{:.3}|{}x{}|{:.2}",
            self.fit, self.anchor, self.blur, self.saturation, self.size.0, self.size.1, self.scale
        ));
        hex(&hasher.finalize()[..12])
    }
}

/// A processed background ready to upload.
#[derive(Debug, Clone)]
pub struct Processed {
    pub image: RgbaImage,
    /// §10.4 samples: 95th-percentile luminance pixel (dark) and 5th (light).
    pub dark_sample: Color,
    pub light_sample: Color,
}

impl Processed {
    pub fn sample(&self, appearance: Appearance) -> Color {
        match appearance {
            Appearance::Dark => self.dark_sample,
            Appearance::Light => self.light_sample,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("Background image not found")]
    Missing,
    #[error("Image too large (max 40 MB)")]
    TooLarge,
    #[error("Image too large (max {MAX_IMPORT_SIDE} × {MAX_IMPORT_SIDE})")]
    TooManyPixels,
    #[error("Unsupported image (use PNG, JPEG, WebP or GIF)")]
    Unsupported,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Window size in device px → texture size: exact aspect, capped.
pub fn texture_size((width, height): (u32, u32)) -> (u32, u32) {
    let (width, height) = (width.max(1), height.max(1));
    let scale = (MAX_TEXTURE.0 as f32 / width as f32)
        .min(MAX_TEXTURE.1 as f32 / height as f32)
        .min(1.0);
    (
        ((width as f32 * scale).round() as u32).max(1),
        ((height as f32 * scale).round() as u32).max(1),
    )
}

/// Decode (or generate), fit, blur, saturate, sample. Heavy: background only.
pub fn process(recipe: &Recipe) -> Result<Processed, PipelineError> {
    let (width, height) = recipe.size;
    let fitted = match &recipe.input {
        // Generated art is resolution independent: render at the exact size.
        Input::Art(art) => procedural::render(*art, width, height),
        Input::File(path) => {
            let source = decode(path)?;
            fit(&source, recipe.fit, recipe.anchor, recipe.size)
        }
    };
    let mut image = blur(fitted, recipe.blur, recipe.scale);
    saturate(&mut image, recipe.saturation);
    Ok(finish(image))
}

/// Compute the legibility samples for an already processed image.
pub fn finish(image: RgbaImage) -> Processed {
    let small = imageops::thumbnail(&image, 64, 64);
    // Transparent letterbox areas show `bg.app`, which themes guarantee
    // readable; sample only painted pixels.
    let painted: Vec<u8> = small
        .pixels()
        .filter(|px| px.0[3] > 0)
        .flat_map(|px| px.0)
        .collect();
    let sample = |appearance| legibility_sample(&painted, appearance).unwrap_or(Color::BLACK);
    Processed {
        dark_sample: sample(Appearance::Dark),
        light_sample: sample(Appearance::Light),
        image,
    }
}

fn decode(path: &Path) -> Result<RgbaImage, PipelineError> {
    let mut reader = ImageReader::open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => PipelineError::Missing,
            _ => PipelineError::Io(error),
        })?
        .with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMPORT_SIDE);
    limits.max_image_height = Some(MAX_IMPORT_SIDE);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| match error {
        image::ImageError::Limits(_) => PipelineError::TooManyPixels,
        _ => PipelineError::Unsupported,
    })?;
    Ok(image.into_rgba8())
}

/// Place `source` into a `size` canvas per `fit`/`anchor`. Uncovered areas
/// stay transparent (the theme's `bg.app` shows through).
pub fn fit(source: &RgbaImage, fit: Fit, anchor: Anchor, size: (u32, u32)) -> RgbaImage {
    let (tw, th) = (size.0 as f32, size.1 as f32);
    let (sw, sh) = (source.width() as f32, source.height() as f32);
    let (ax, ay) = anchor.fraction();
    let mut canvas = RgbaImage::new(size.0, size.1);
    match fit {
        Fit::Fill => return resize(source, size.0, size.1),
        Fit::Cover | Fit::Contain => {
            let scale = if fit == Fit::Cover {
                (tw / sw).max(th / sh)
            } else {
                (tw / sw).min(th / sh)
            };
            let (w, h) = (
                ((sw * scale).round() as u32).max(1),
                ((sh * scale).round() as u32).max(1),
            );
            let scaled = resize(source, w, h);
            let x = ((tw - w as f32) * ax).round() as i64;
            let y = ((th - h as f32) * ay).round() as i64;
            imageops::overlay(&mut canvas, &scaled, x, y);
        }
        Fit::Center => {
            let x = ((tw - sw) * ax).round() as i64;
            let y = ((th - sh) * ay).round() as i64;
            imageops::overlay(&mut canvas, source, x, y);
        }
        Fit::Tile => {
            let (w, h) = (source.width().max(1) as i64, source.height().max(1) as i64);
            let (ox, oy) = (
                (((tw as i64 - w) as f32 * ax) as i64).rem_euclid(w) - w,
                (((th as i64 - h) as f32 * ay) as i64).rem_euclid(h) - h,
            );
            let mut y = oy;
            while y < th as i64 {
                let mut x = ox;
                while x < tw as i64 {
                    imageops::overlay(&mut canvas, source, x, y);
                    x += w;
                }
                y += h;
            }
        }
    }
    canvas
}

/// Downscale with a cheap box pass first (huge photos), then filter.
fn resize(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if source.dimensions() == (width, height) {
        return source.clone();
    }
    if source.width() >= width * 2 && source.height() >= height * 2 {
        let halfway = imageops::thumbnail(source, width * 2, height * 2);
        return imageops::resize(&halfway, width, height, imageops::FilterType::Triangle);
    }
    imageops::resize(source, width, height, imageops::FilterType::Triangle)
}

/// §10.3 step 2: σ = blur (1x px); from 8 px up, blur a quarter-size copy
/// with σ / 4 and upscale bilinearly (≈ 16× cheaper, visually identical).
pub fn blur(image: RgbaImage, radius: u8, scale: f32) -> RgbaImage {
    if radius == 0 {
        return image;
    }
    let sigma = radius as f32 * scale.max(1.0);
    let (width, height) = image.dimensions();
    if radius < QUARTER_BLUR_FROM || width < 16 || height < 16 {
        return imageops::fast_blur(&image, sigma);
    }
    let quarter = imageops::thumbnail(&image, width.div_ceil(4), height.div_ceil(4));
    let blurred = imageops::fast_blur(&quarter, sigma / 4.0);
    imageops::resize(&blurred, width, height, imageops::FilterType::Triangle)
}

/// §10.3 step 3: mix each pixel with its luma (0 = grey, 1 = unchanged).
pub fn saturate(image: &mut RgbaImage, saturation: f32) {
    if (saturation - 1.0).abs() < f32::EPSILON {
        return;
    }
    for pixel in image.pixels_mut() {
        let [r, g, b, _] = pixel.0.map(f32::from);
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        for (channel, value) in pixel.0[..3].iter_mut().zip([r, g, b]) {
            *channel = (luma + (value - luma) * saturation).clamp(0.0, 255.0) as u8;
        }
    }
}

/// RGBA → BGRA in place (gpui's `RenderImage` layout).
pub fn to_bgra(image: &mut RgbaImage) {
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
}

/// Where a user image renders from: the imported copy recorded for
/// `source`, else import it now. §10.3 / FR-9 / AC-12.
pub fn resolve_source(
    source: &BackgroundSource,
    data_dir: Option<&Path>,
) -> Result<Input, PipelineError> {
    match source {
        BackgroundSource::Builtin(builtin) => Ok(Input::Art(Art::Builtin(*builtin))),
        BackgroundSource::Image(path) => {
            let original = expand_home(path);
            let Some(data_dir) = data_dir else {
                return Ok(Input::File(original));
            };
            let index = Imports::load(data_dir);
            if let Some(copy) = index.copy_for(path, data_dir) {
                return Ok(Input::File(copy));
            }
            let copy = import(&original, data_dir)?;
            Imports::record(data_dir, path, &copy)?;
            Ok(Input::File(copy))
        }
    }
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map_or_else(|| PathBuf::from(path), |home| home.join(rest)),
        None => PathBuf::from(path),
    }
}

/// Copy `original` to `<data>/backgrounds/<sha256>.<ext>` after validating
/// size, format and dimensions. Idempotent (same bytes → same file).
pub fn import(original: &Path, data_dir: &Path) -> Result<PathBuf, PipelineError> {
    let metadata = std::fs::metadata(original).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => PipelineError::Missing,
        _ => PipelineError::Io(error),
    })?;
    if metadata.len() > MAX_IMPORT_BYTES {
        return Err(PipelineError::TooLarge);
    }
    let extension = original
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|ext| EXTENSIONS.contains(&ext.as_str()))
        .ok_or(PipelineError::Unsupported)?;
    let (width, height) = ImageReader::open(original)?
        .with_guessed_format()?
        .into_dimensions()
        .map_err(|_| PipelineError::Unsupported)?;
    if width > MAX_IMPORT_SIDE || height > MAX_IMPORT_SIDE {
        return Err(PipelineError::TooManyPixels);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::fs::File::open(original)?.read_to_end(&mut bytes)?;
    let digest = hex(&Sha256::digest(&bytes));
    let dir = backgrounds_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join(format!("{digest}.{extension}"));
    if !copy.exists() {
        let partial = dir.join(format!(".{digest}.partial"));
        std::fs::write(&partial, &bytes)?;
        std::fs::rename(&partial, &copy)?;
    }
    Ok(copy)
}

pub fn backgrounds_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backgrounds")
}

fn cache_path(data_dir: &Path, recipe: &Recipe) -> PathBuf {
    backgrounds_dir(data_dir)
        .join("cache")
        .join(format!("{}.png", recipe.cache_key()))
}

/// Cold-start path: a previously processed texture, if cached.
pub fn load_cached(data_dir: &Path, recipe: &Recipe) -> Option<Processed> {
    let image = image::open(cache_path(data_dir, recipe)).ok()?.into_rgba8();
    (image.dimensions() == recipe.size).then(|| finish(image))
}

/// Keep at most `keep` cache files (newest first), then write this one.
pub fn store_cached(data_dir: &Path, recipe: &Recipe, processed: &Processed) {
    const KEEP: usize = 4;
    let path = cache_path(data_dir, recipe);
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let partial = path.with_extension("partial");
    if processed
        .image
        .save_with_format(&partial, image::ImageFormat::Png)
        .is_ok()
    {
        let _ = std::fs::rename(&partial, &path);
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, stale) in entries.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(stale);
    }
}

/// `<data>/backgrounds/imports.toml`: original path → imported copy, so the
/// original can move or disappear (AC-12).
struct Imports(toml::Table);

impl Imports {
    const FILE: &str = "imports.toml";

    fn load(data_dir: &Path) -> Self {
        let path = backgrounds_dir(data_dir).join(Self::FILE);
        let table = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default();
        Self(table)
    }

    fn copy_for(&self, original: &str, data_dir: &Path) -> Option<PathBuf> {
        let name = self.0.get(original)?.as_str()?;
        let copy = backgrounds_dir(data_dir).join(name);
        copy.is_file().then_some(copy)
    }

    fn record(data_dir: &Path, original: &str, copy: &Path) -> std::io::Result<()> {
        let mut imports = Self::load(data_dir);
        let name = copy
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        imports
            .0
            .insert(original.to_owned(), toml::Value::String(name));
        let dir = backgrounds_dir(data_dir);
        std::fs::create_dir_all(&dir)?;
        let text = toml::to_string(&imports.0).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(Self::FILE), text)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::background::{BackgroundSettings, BuiltinBackground};

    fn solid(width: u32, height: u32, value: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, image::Rgba(value))
    }

    #[test]
    fn texture_size_keeps_aspect_and_caps() {
        assert_eq!(texture_size((1440, 900)), (1440, 900));
        assert_eq!(texture_size((5120, 2880)), (3840, 2160));
        assert_eq!(texture_size((7680, 2400)), (3840, 1200));
        assert_eq!(texture_size((0, 0)), (1, 1));
    }

    #[test]
    fn cover_crops_by_anchor_and_contain_letterboxes() {
        // Left half red, right half blue, 200 × 100 into a 100 × 100 canvas.
        let source = RgbaImage::from_fn(200, 100, |x, _| {
            image::Rgba(if x < 100 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        });
        let left = fit(&source, Fit::Cover, Anchor::Left, (100, 100));
        assert!(left.pixels().all(|px| px.0 == [255, 0, 0, 255]));
        let right = fit(&source, Fit::Cover, Anchor::Right, (100, 100));
        assert!(right.pixels().all(|px| px.0 == [0, 0, 255, 255]));

        let contain = fit(&source, Fit::Contain, Anchor::Top, (100, 100));
        assert_eq!(contain.get_pixel(50, 10).0[3], 255, "image at the top");
        assert_eq!(contain.get_pixel(50, 90).0[3], 0, "transparent letterbox");
    }

    #[test]
    fn tile_repeats_and_center_keeps_native_size() {
        let dot = RgbaImage::from_fn(10, 10, |x, y| {
            image::Rgba(if x == 0 && y == 0 {
                [255; 4]
            } else {
                [0, 0, 0, 255]
            })
        });
        let tiled = fit(&dot, Fit::Tile, Anchor::TopLeft, (30, 30));
        let lit = tiled.pixels().filter(|px| px.0[0] == 255).count();
        assert_eq!(lit, 9);
        let centered = fit(&dot, Fit::Center, Anchor::Center, (30, 30));
        assert_eq!(centered.get_pixel(10, 10).0, [255; 4]);
        assert_eq!(centered.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn blur_softens_an_edge_both_paths() {
        let edge = RgbaImage::from_fn(128, 32, |x, _| {
            image::Rgba(if x < 64 {
                [0, 0, 0, 255]
            } else {
                [255, 255, 255, 255]
            })
        });
        for radius in [4, 24] {
            let blurred = blur(edge.clone(), radius, 1.0);
            assert_eq!(blurred.dimensions(), (128, 32));
            let near = blurred.get_pixel(62, 16).0[0];
            assert!(near > 20 && near < 235, "radius {radius}: {near}");
        }
        assert_eq!(blur(edge.clone(), 0, 2.0), edge);
    }

    #[test]
    fn saturation_zero_is_grey() {
        let mut image = solid(4, 4, [200, 40, 40, 255]);
        saturate(&mut image, 0.0);
        let [r, g, b, _] = image.get_pixel(0, 0).0;
        assert!(r == g && g == b, "{r} {g} {b}");
    }

    #[test]
    fn white_image_samples_white_and_letterbox_is_ignored() {
        let mut image = solid(100, 100, [255, 255, 255, 255]);
        for y in 0..50 {
            for x in 0..100 {
                image.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
        let processed = finish(image);
        assert_eq!(processed.sample(Appearance::Dark), Color::WHITE);
        assert_eq!(processed.sample(Appearance::Light), Color::WHITE);
    }

    #[test]
    fn processes_builtins_and_files_to_the_recipe_size() {
        let dir = tempfile::tempdir().unwrap();
        let config =
            BackgroundSettings::with_source(BackgroundSource::Builtin(BuiltinBackground::Lantern))
                .resolve(Appearance::Dark, None);
        let recipe = Recipe::new(
            Input::Art(Art::Builtin(BuiltinBackground::Lantern)),
            &config,
            (120, 80),
            1.0,
        );
        let processed = process(&recipe).unwrap();
        assert_eq!(processed.image.dimensions(), (120, 80));

        let path = dir.path().join("photo.png");
        solid(300, 100, [10, 200, 30, 255]).save(&path).unwrap();
        let recipe = Recipe {
            input: Input::File(path),
            blur: 12,
            ..recipe
        };
        let processed = process(&recipe).unwrap();
        assert_eq!(processed.image.dimensions(), (120, 80));
        assert!(
            processed.image.pixels().all(|px| px.0[3] == 255),
            "cover fills"
        );
    }

    /// FR-9 / AC-12: the imported copy keeps rendering after the original
    /// is deleted.
    #[test]
    fn imported_copy_survives_deleting_the_original() {
        let data = tempfile::tempdir().unwrap();
        let pictures = tempfile::tempdir().unwrap();
        let original = pictures.path().join("night.png");
        solid(64, 48, [30, 60, 90, 255]).save(&original).unwrap();
        let source = BackgroundSource::Image(original.display().to_string());

        let Input::File(copy) = resolve_source(&source, Some(data.path())).unwrap() else {
            panic!("file input");
        };
        assert!(copy.starts_with(backgrounds_dir(data.path())));
        std::fs::remove_file(&original).unwrap();

        let again = resolve_source(&source, Some(data.path())).unwrap();
        assert_eq!(again, Input::File(copy.clone()));
        let decoded = decode(&copy).unwrap();
        assert_eq!(decoded.get_pixel(0, 0).0, [30, 60, 90, 255]);

        // Neither original nor copy: missing, never a panic.
        std::fs::remove_file(&copy).unwrap();
        assert!(matches!(
            resolve_source(&source, Some(data.path())),
            Err(PipelineError::Missing)
        ));
    }

    #[test]
    fn import_rejects_unsupported_and_oversized_files() {
        let data = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, "hello").unwrap();
        assert!(matches!(
            import(&text, data.path()),
            Err(PipelineError::Unsupported)
        ));

        let fake = dir.path().join("fake.png");
        std::fs::write(&fake, "not a png").unwrap();
        assert!(matches!(
            import(&fake, data.path()),
            Err(PipelineError::Unsupported)
        ));

        let big = dir.path().join("big.png");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(MAX_IMPORT_BYTES + 1)
            .unwrap();
        assert!(matches!(
            import(&big, data.path()),
            Err(PipelineError::TooLarge)
        ));
    }

    #[test]
    fn disk_cache_round_trips_and_keys_on_baked_inputs() {
        let data = tempfile::tempdir().unwrap();
        let config =
            BackgroundSettings::with_source(BackgroundSource::Builtin(BuiltinBackground::Ink))
                .resolve(Appearance::Dark, None);
        let recipe = Recipe::new(
            Input::Art(Art::Builtin(BuiltinBackground::Ink)),
            &config,
            (64, 40),
            2.0,
        );
        assert!(load_cached(data.path(), &recipe).is_none());
        let processed = process(&recipe).unwrap();
        store_cached(data.path(), &recipe, &processed);
        let cached = load_cached(data.path(), &recipe).expect("cache hit");
        assert_eq!(cached.image, processed.image);

        let blurred = Recipe {
            blur: 8,
            ..recipe.clone()
        };
        assert_ne!(blurred.cache_key(), recipe.cache_key());
        assert!(load_cached(data.path(), &blurred).is_none());
    }
}
