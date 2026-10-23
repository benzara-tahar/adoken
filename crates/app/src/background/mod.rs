//! Custom backgrounds (spec 0007 FR-4..FR-6, FR-9; docs/design/tokens.md §10).
//!
//! - Config: `settings.toml [background]`, parsed by
//!   `adoken_theme::background` (per-appearance and per-view overrides).
//! - Pipeline ([`pipeline`]): import → decode → fit → blur → saturation →
//!   legibility samples, once per (image, window size, blur), on a background
//!   executor; disk-cached for later launches; an LRU of 2 in memory.
//! - Paint ([`Backdrop`]): image → tint → dim quads under the app, which
//!   paints its panes with the theme's translucent surfaces (installed by
//!   [`install_theme`]) while `bg.code` stays opaque (FR-6).
//! - Readability guard: `adoken_theme::background::readability` raises dim,
//!   then surface opacity, until text passes AA over the image (FR-5).
//! - Overlays frost what is under them with [`frosted`] (fork BackdropBlur).
//!
//! Cold start: nothing here runs before the first frame. [`init`] only
//! registers an empty controller; settings are read and images processed on
//! the background executor once a window reports its size, then the image
//! crossfades in (`motion.base`; snaps under reduce motion).

mod frost;
pub mod pipeline;
pub mod procedural;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use adoken_theme::background::{
    BackgroundConfig, BackgroundIssue, BackgroundSettings, BackgroundSource, Readability, ViewId,
    readability,
};
use adoken_theme::{Appearance, Color, ThemeVariant, motion};
use adoken_ui::Theme;
use adoken_ui::motion::{EASE_IN_OUT, animation};
use adoken_ui::theme::hsla;
use gpui::{
    AnimationExt, AnyElement, App, AppContext as _, Context, ElementId, Entity, Global, Hsla,
    ImageSource, IntoElement, ObjectFit, RenderImage, RenderOnce, SharedString, Task, Window, div,
    img, prelude::*,
};

pub use frost::{Frosted, frosted};
use pipeline::{Input, PipelineError, Processed, Recipe};
use procedural::Art;

/// §10.3: resize reprocessing waits for the window to settle.
pub const RESIZE_DEBOUNCE: Duration = Duration::from_millis(150);
/// Demo mode's generated illustration, inside the demo directory. Bump the
/// version when `procedural::illustration` changes so old files regenerate.
pub const DEMO_ILLUSTRATION: &str = "backgrounds/demo-illustration-v1.png";
const DEMO_ILLUSTRATION_SIZE: (u32, u32) = (2400, 1500);

/// What the background is doing, for Settings › Appearance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Off,
    Processing,
    Ready,
    /// §10.7: source and imported copy both gone; `bg.app` shows.
    Missing,
    /// Unsupported / too large / IO; previous background unchanged.
    Failed(String),
}

/// Everything that changes the baked texture (paint-time values excluded).
#[derive(Debug, Clone, PartialEq)]
struct Want {
    source: BackgroundSource,
    config: BackgroundConfig,
    size: (u32, u32),
    scale: f32,
}

impl Want {
    fn bakes_same(&self, other: &Self) -> bool {
        self.source == other.source
            && self.size == other.size
            && self.scale == other.scale
            && self.config.fit == other.config.fit
            && self.config.anchor == other.config.anchor
            && self.config.blur == other.config.blur
            && self.config.saturation == other.config.saturation
    }
}

/// A texture ready to paint.
#[derive(Clone)]
pub struct Texture {
    image: Arc<RenderImage>,
    dark_sample: Color,
    light_sample: Color,
}

impl Texture {
    /// Upload-ready texture from a processed image (converts to BGRA).
    pub fn from_processed(processed: Processed) -> Self {
        let Processed {
            mut image,
            dark_sample,
            light_sample,
        } = processed;
        pipeline::to_bgra(&mut image);
        Self {
            image: Arc::new(RenderImage::new(vec![image::Frame::new(image)])),
            dark_sample,
            light_sample,
        }
    }

    fn sample(&self, appearance: Appearance) -> Color {
        match appearance {
            Appearance::Dark => self.dark_sample,
            Appearance::Light => self.light_sample,
        }
    }
}

/// The effective paint values for one frame.
#[derive(Clone)]
pub struct Frame {
    pub texture: Texture,
    pub config: BackgroundConfig,
    pub readability: Readability,
    appearance: Appearance,
    /// `bg.app` of the underlying (opaque) theme: shows under letterboxing
    /// and before the image fades in.
    base: Hsla,
    tint: Option<(Hsla, f32)>,
}

impl Frame {
    /// Resolve paint values (tint, guard) for `variant`.
    pub fn new(texture: Texture, config: BackgroundConfig, variant: &ThemeVariant) -> Self {
        let appearance = variant.appearance;
        let readability = readability(
            &config,
            &variant.colors,
            appearance,
            texture.sample(appearance),
        );
        let tint = config
            .tint
            .filter(|_| config.tint_strength > 0.0)
            .map(|tint| (hsla(tint.resolve(&variant.colors)), config.tint_strength));
        Self {
            texture,
            readability,
            appearance,
            base: hsla(variant.colors.bg_app),
            tint,
            config,
        }
    }

    /// The theme to install while this frame shows: panes at the (guarded)
    /// surface opacity, overlays at `overlay_opacity`, no `bg.app` (the
    /// backdrop paints it), `bg.code` untouched (opaque, FR-6).
    pub fn translucent_theme(&self, variant: &ThemeVariant) -> Theme {
        let mut theme = Theme::from_variant(variant);
        let surface = self.readability.surface_opacity;
        let overlay = self.config.overlay_opacity;
        theme.colors.bg_app = hsla(Color::TRANSPARENT);
        theme.raw.bg_app = Color::TRANSPARENT;
        let fade = |color: Color, factor: f32| {
            Color::rgba(
                color.r,
                color.g,
                color.b,
                (color.a as f32 * factor).round() as u8,
            )
        };
        theme.raw.bg_sidebar = fade(theme.raw.bg_sidebar, surface);
        theme.raw.bg_surface = fade(theme.raw.bg_surface, surface);
        theme.raw.bg_raised = fade(theme.raw.bg_raised, surface);
        theme.raw.bg_overlay = fade(theme.raw.bg_overlay, overlay);
        theme.colors.bg_sidebar = hsla(theme.raw.bg_sidebar);
        theme.colors.bg_surface = hsla(theme.raw.bg_surface);
        theme.colors.bg_raised = hsla(theme.raw.bg_raised);
        theme.colors.bg_overlay = hsla(theme.raw.bg_overlay);
        theme
    }
}

/// Paint `child` over `frame` (§10.2 steps 1–3). Stateless; the app uses it
/// through [`Backdrop`], gallery stories call it directly.
pub fn paint(frame: Option<&Frame>, child: AnyElement) -> gpui::Div {
    let Some(frame) = frame else {
        return div().size_full().child(child);
    };
    let dim = hsla(BackgroundConfig::dim_color(frame.appearance)).opacity(frame.readability.dim);
    let id = frame.texture.image.id.0 as u64;
    let image = img(ImageSource::from(frame.texture.image.clone()))
        .absolute()
        .inset_0()
        .size_full()
        .object_fit(ObjectFit::Fill)
        .with_animation(
            ElementId::Integer(id),
            animation(motion::BASE, EASE_IN_OUT),
            |image, delta| image.opacity(delta),
        );
    div()
        .relative()
        .size_full()
        .bg(frame.base)
        .child(image)
        .when_some(frame.tint, |this, (color, strength)| {
            this.child(div().absolute().inset_0().bg(color.opacity(strength)))
        })
        .child(div().absolute().inset_0().bg(dim))
        .child(div().relative().size_full().child(child))
}

/// Root wrapper for a window's content: reports the window size to the
/// controller and paints the active background behind `child`.
#[derive(IntoElement)]
pub struct Backdrop {
    child: AnyElement,
}

pub fn backdrop(child: impl IntoElement) -> Backdrop {
    Backdrop {
        child: child.into_any_element(),
    }
}

impl RenderOnce for Backdrop {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(controller) = cx.try_global::<GlobalBackgrounds>().map(|g| g.0.clone()) else {
            return paint(None, self.child);
        };
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let size = (
            (f32::from(viewport.width) * scale).round() as u32,
            (f32::from(viewport.height) * scale).round() as u32,
        );
        if controller.read(cx).viewport != Some((size, scale)) {
            controller.update(cx, |controller, cx| {
                controller.set_viewport(size, scale, cx)
            });
        }
        let frame = controller.read(cx).frame.clone();
        paint(frame.as_ref(), self.child)
    }
}

struct GlobalBackgrounds(Entity<Backgrounds>);

impl Global for GlobalBackgrounds {}

/// The app-wide background controller.
pub struct Backgrounds {
    settings: BackgroundSettings,
    issues: Vec<BackgroundIssue>,
    data_dir: Option<PathBuf>,
    view: Option<ViewId>,
    variant: Option<ThemeVariant>,
    viewport: Option<((u32, u32), f32)>,
    /// What is showing (texture + the want that produced it).
    shown: Option<(Want, Texture)>,
    /// Previously shown textures, most recent first (CPU copies; evicted from
    /// the GPU while not showing, §10.3 64 MB budget).
    recent: Vec<(Want, Texture)>,
    frame: Option<Frame>,
    status: Status,
    pending: Option<(Want, Task<()>)>,
    debounce: Option<Task<()>>,
}

const RECENT: usize = 1;

/// Register the (empty) controller. Cheap: no IO, no processing.
pub fn init(cx: &mut App) {
    let controller = cx.new(|_| Backgrounds {
        settings: BackgroundSettings::default(),
        issues: Vec::new(),
        data_dir: None,
        view: None,
        variant: None,
        viewport: None,
        shown: None,
        recent: Vec::new(),
        frame: None,
        status: Status::Off,
        pending: None,
        debounce: None,
    });
    cx.set_global(GlobalBackgrounds(controller));
}

fn controller(cx: &App) -> Option<Entity<Backgrounds>> {
    cx.try_global::<GlobalBackgrounds>().map(|g| g.0.clone())
}

/// Read `[background]` from `settings_path` off the UI thread and apply it.
/// `data_dir` holds imports and the disk cache (`<data>/backgrounds/`).
/// `demo`: with no `[background]` table, show the generated demo
/// illustration (written into `data_dir` first).
pub fn load(settings_path: PathBuf, data_dir: PathBuf, demo: bool, cx: &mut App) {
    let Some(controller) = controller(cx) else {
        return;
    };
    let dir = data_dir.clone();
    let job = cx.background_executor().spawn(async move {
        let text = std::fs::read_to_string(&settings_path).unwrap_or_default();
        let has_table = toml::from_str::<toml::Table>(&text)
            .is_ok_and(|table| table.contains_key(adoken_theme::background::SETTINGS_TABLE));
        if demo && !has_table {
            return match ensure_demo_illustration(&dir) {
                Ok(path) => (demo_settings(&path), Vec::new()),
                Err(error) => {
                    log::warn!("background: demo illustration: {error}");
                    (BackgroundSettings::default(), Vec::new())
                }
            };
        }
        BackgroundSettings::from_settings_toml(&text).unwrap_or_else(|error| {
            log::warn!("background: {}: {error}", settings_path.display());
            (BackgroundSettings::default(), Vec::new())
        })
    });
    controller.update(cx, |this, cx| {
        this.data_dir = Some(data_dir);
        cx.spawn(async move |this, cx| {
            let (settings, issues) = job.await;
            this.update(cx, |this, cx| {
                this.issues = issues;
                this.set_settings(settings, cx);
            })
            .ok();
        })
        .detach();
    });
}

/// Demo defaults: the illustration with a light blur and see-through panes,
/// like a user wallpaper (the readability guard still applies).
pub fn demo_settings(illustration: &Path) -> BackgroundSettings {
    let text = format!(
        "[background]\nsource = {:?}\nblur = 6\ndim = 0.3\nsurface_opacity = 0.7\n",
        illustration.display().to_string()
    );
    BackgroundSettings::from_settings_toml(&text)
        .map(|(settings, _)| settings)
        .unwrap_or_default()
}

/// Write the demo illustration once (deterministic and versioned by file
/// name, so an existing file is current).
pub fn ensure_demo_illustration(dir: &Path) -> Result<PathBuf, PipelineError> {
    let path = dir.join(DEMO_ILLUSTRATION);
    if !path.is_file() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let (width, height) = DEMO_ILLUSTRATION_SIZE;
        let art = procedural::render(Art::Illustration, width, height);
        let partial = path.with_extension("partial");
        art.save_with_format(&partial, image::ImageFormat::Png)
            .map_err(|error| PipelineError::Io(std::io::Error::other(error)))?;
        std::fs::rename(&partial, &path)?;
    }
    Ok(path)
}

/// Replace the settings (live preview from Settings; hot reload).
pub fn set_settings(settings: BackgroundSettings, cx: &mut App) {
    if let Some(controller) = controller(cx) {
        controller.update(cx, |this, cx| this.set_settings(settings, cx));
    }
}

/// The view now in front (`[background.views.<view>]` overrides).
pub fn set_view(view: Option<ViewId>, cx: &mut App) {
    if let Some(controller) = controller(cx) {
        controller.update(cx, |this, cx| {
            if this.view != view {
                this.view = view;
                this.refresh(cx);
            }
        });
    }
}

/// The view now in front, as the shell last said ([`set_view`]); `None`
/// before the shell shows one (and in stories). Spec 0019 FR-19: the PR
/// status commands know whether the Pull requests view is showing.
pub fn front_view(cx: &App) -> Option<ViewId> {
    controller(cx).and_then(|controller| controller.read(cx).view)
}

/// Install `variant` as the active theme, translucent when a background is
/// showing. Use instead of `Theme::install` so the two stay in sync.
pub fn install_theme(variant: &ThemeVariant, cx: &mut App) {
    match controller(cx) {
        Some(controller) => controller.update(cx, |this, cx| {
            this.variant = Some(variant.clone());
            this.refresh(cx);
            this.apply_theme(cx);
        }),
        None => Theme::install(variant, cx),
    }
}

/// Settings' live preview: the texture showing now, painted with
/// `config`'s paint-time values (dim, tint, readability guard). `None`
/// when nothing is showing or `config` has no background.
pub fn preview(config: BackgroundConfig, cx: &App) -> Option<Frame> {
    let controller = controller(cx)?;
    let this = controller.read(cx);
    let (_, texture) = this.shown.as_ref()?;
    let variant = this.variant.as_ref()?;
    config
        .is_active()
        .then(|| Frame::new(texture.clone(), config, variant))
}

/// A background is painted right now.
pub fn is_showing(cx: &App) -> bool {
    controller(cx).is_some_and(|controller| controller.read(cx).frame.is_some())
}

pub fn status(cx: &App) -> Status {
    controller(cx).map_or(Status::Off, |controller| controller.read(cx).status.clone())
}

/// Settings issues from the last load (Settings › Appearance).
pub fn issues(cx: &App) -> Vec<BackgroundIssue> {
    controller(cx).map_or_else(Vec::new, |controller| controller.read(cx).issues.clone())
}

impl Backgrounds {
    fn appearance(&self) -> Appearance {
        self.variant
            .as_ref()
            .map_or(Appearance::Dark, |variant| variant.appearance)
    }

    fn set_settings(&mut self, settings: BackgroundSettings, cx: &mut Context<Self>) {
        self.settings = settings;
        self.refresh(cx);
    }

    fn set_viewport(&mut self, size: (u32, u32), scale: f32, cx: &mut Context<Self>) {
        let first = self.viewport.is_none();
        self.viewport = Some((size, scale));
        if first || self.shown.is_none() {
            self.refresh(cx);
            return;
        }
        // Resizing: keep painting the stretched texture; reprocess once the
        // window settles.
        self.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RESIZE_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                this.debounce = None;
                this.refresh(cx);
            })
            .ok();
        }));
    }

    /// Bring the texture in line with settings, view, appearance and size.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let config = self.settings.resolve(self.appearance(), self.view);
        let source = config.source.clone().filter(|_| config.enabled);
        let (Some(source), Some((size, scale))) = (source, self.viewport) else {
            self.pending = None;
            if self.viewport.is_some() || !config.is_active() {
                self.show(None, cx);
                self.status = Status::Off;
            }
            return;
        };
        let want = Want {
            source,
            config,
            size,
            scale,
        };
        if let Some((shown, texture)) = &self.shown
            && shown.bakes_same(&want)
        {
            // Only paint-time values changed (dim, tint, opacity, view).
            let texture = texture.clone();
            self.show(Some((want, texture)), cx);
            return;
        }
        if let Some(index) = self
            .recent
            .iter()
            .position(|(recent, _)| recent.bakes_same(&want))
        {
            let (_, texture) = self.recent.remove(index);
            self.show(Some((want, texture)), cx);
            return;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|(pending, _)| pending.bakes_same(&want))
        {
            return;
        }
        self.status = Status::Processing;
        let data_dir = self.data_dir.clone();
        let job_want = want.clone();
        let job = cx.background_executor().spawn(async move {
            let input = pipeline::resolve_source(&job_want.source, data_dir.as_deref())?;
            let recipe = Recipe::new(input, &job_want.config, job_want.size, job_want.scale);
            if let Some(cached) = data_dir
                .as_deref()
                .and_then(|dir| pipeline::load_cached(dir, &recipe))
            {
                return Ok(Texture::from_processed(cached));
            }
            let processed = pipeline::process(&recipe)?;
            // Generated art is cheaper to regenerate than to decode a PNG.
            if let (Some(dir), Input::File(_)) = (data_dir.as_deref(), &recipe.input) {
                pipeline::store_cached(dir, &recipe, &processed);
            }
            Ok::<_, PipelineError>(Texture::from_processed(processed))
        });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            this.update(cx, |this, cx| {
                let Some((want, _)) = this.pending.take() else {
                    return;
                };
                match result {
                    Ok(texture) => this.show(Some((want, texture)), cx),
                    Err(PipelineError::Missing) => {
                        this.show(None, cx);
                        this.status = Status::Missing;
                    }
                    Err(error) => {
                        log::warn!("background: {error}");
                        this.status = Status::Failed(error.to_string());
                        cx.notify();
                    }
                }
            })
            .ok();
        });
        self.pending = Some((want, task));
    }

    fn show(&mut self, next: Option<(Want, Texture)>, cx: &mut Context<Self>) {
        let showing = next.is_some();
        let previous = std::mem::replace(&mut self.shown, next);
        if let Some((want, texture)) = previous {
            let same = self
                .shown
                .as_ref()
                .is_some_and(|(_, current)| Arc::ptr_eq(&current.image, &texture.image));
            if !same {
                cx.drop_image(texture.image.clone(), None);
                self.recent.insert(0, (want, texture));
                self.recent.truncate(RECENT);
            }
        }
        self.frame = match (&self.shown, &self.variant) {
            (Some((want, texture)), Some(variant)) => {
                Some(Frame::new(texture.clone(), want.config.clone(), variant))
            }
            _ => None,
        };
        self.status = if showing { Status::Ready } else { Status::Off };
        self.apply_theme(cx);
        cx.notify();
    }

    fn apply_theme(&self, cx: &mut Context<Self>) {
        let Some(variant) = &self.variant else {
            return;
        };
        let theme = match &self.frame {
            Some(frame) => frame.translucent_theme(variant),
            None => Theme::from_variant(variant),
        };
        theme.activate(cx);
    }
}

/// "Adjusted for readability: dim 0.35 → 0.50" (§10.4), if the guard moved
/// anything.
pub fn readability_note(frame: &Frame) -> Option<SharedString> {
    let (config, result) = (&frame.config, &frame.readability);
    if !result.passes() {
        return Some("Text may be hard to read".into());
    }
    if !result.adjusted(config) {
        return None;
    }
    let mut parts = Vec::new();
    if result.dim > config.dim + f32::EPSILON {
        parts.push(format!("dim {:.2} → {:.2}", config.dim, result.dim));
    }
    if result.surface_opacity > config.surface_opacity + f32::EPSILON {
        parts.push(format!(
            "surface opacity {:.2} → {:.2}",
            config.surface_opacity, result.surface_opacity
        ));
    }
    Some(format!("Adjusted for readability: {}", parts.join(", ")).into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::ThemeRegistry;
    use adoken_theme::background::BuiltinBackground;

    fn frame_over(color: [u8; 4], variant_id: &str, dim: f32) -> (Frame, &'static ThemeVariant) {
        let variant = ThemeRegistry::builtin().variant(variant_id).unwrap();
        let image = image::RgbaImage::from_pixel(32, 32, image::Rgba(color));
        let texture = Texture::from_processed(pipeline::finish(image));
        let config = BackgroundConfig {
            dim,
            ..BackgroundSettings::with_source(BackgroundSource::Builtin(BuiltinBackground::Ink))
                .resolve(variant.appearance, None)
        };
        (Frame::new(texture, config, variant), variant)
    }

    /// AC-7 through the app path: the sample comes from the processed image.
    #[test]
    fn white_image_raises_dim_and_says_so() {
        let (frame, _) = frame_over([255; 4], "adoken-dark", 0.1);
        assert!(frame.readability.passes());
        assert!(frame.readability.dim > 0.1);
        let note = readability_note(&frame).unwrap();
        assert!(
            note.starts_with("Adjusted for readability: dim 0.10 → "),
            "{note}"
        );
    }

    #[test]
    fn translucent_theme_keeps_code_opaque() {
        let (frame, variant) = frame_over([20, 20, 30, 255], "adoken-dark", 0.3);
        assert!(readability_note(&frame).is_none());
        let theme = frame.translucent_theme(variant);
        assert_eq!(theme.raw.bg_app, Color::TRANSPARENT);
        assert!(theme.raw.bg_surface.a < 255);
        assert_eq!(theme.raw.bg_code, variant.colors.bg_code);
        assert!(theme.raw.bg_code.is_opaque());
        let expected = (255.0 * frame.readability.surface_opacity).round() as u8;
        assert_eq!(theme.raw.bg_surface.a, expected);
    }

    #[test]
    fn demo_illustration_is_written_once_into_the_demo_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = ensure_demo_illustration(dir.path()).unwrap();
        assert!(path.starts_with(dir.path()));
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(ensure_demo_illustration(dir.path()).unwrap(), path);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        let config = demo_settings(&path).resolve(Appearance::Dark, None);
        assert!(config.is_active());
        assert_eq!(config.blur, 6);
    }
}
