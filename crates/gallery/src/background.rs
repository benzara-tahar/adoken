//! Custom background stories (spec 0007 FR-4..FR-6): real views painted over
//! each built-in background, plus the demo illustration behind a diff.
//!
//! The texture is processed synchronously here (the app does it on a
//! background executor) and cached across themes. The view renders with the
//! translucent theme the app installs while a background shows; [`scoped`]
//! swaps it in for this story's draw only, so other stories keep the plain
//! theme.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use adoken_app::background::pipeline::{Input, Recipe, process};
use adoken_app::background::procedural::Art;
use adoken_app::background::{Frame, Texture, demo_settings, paint};
use adoken_app::shell::Nav;
use adoken_theme::ThemeRegistry;
use adoken_theme::background::{
    BackgroundConfig, BackgroundSettings, BackgroundSource, BuiltinBackground,
};
use adoken_ui::{ActiveTheme, Theme};
use gpui::{
    AnyElement, App, Bounds, Element, GlobalElementId, InspectorElementId, IntoElement, LayoutId,
    Pixels, Window,
};

type Key = (Art, (u32, u32), u8);

static TEXTURES: LazyLock<Mutex<HashMap<Key, Texture>>> = LazyLock::new(Default::default);

fn texture(art: Art, config: &BackgroundConfig, window: &Window) -> Texture {
    let scale = window.scale_factor();
    let viewport = window.viewport_size();
    let size = (
        (f32::from(viewport.width) * scale).round() as u32,
        (f32::from(viewport.height) * scale).round() as u32,
    );
    let key = (art, size, config.blur);
    let mut cache = TEXTURES.lock().unwrap_or_else(|poison| poison.into_inner());
    cache
        .entry(key)
        .or_insert_with(|| {
            let recipe = Recipe::new(Input::Art(art), config, size, scale);
            Texture::from_processed(process(&recipe).expect("generated art always processes"))
        })
        .clone()
}

/// `view` over `art` with `settings`, as the app paints it.
fn over(
    art: Art,
    settings: BackgroundSettings,
    view: impl FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let variant = ThemeRegistry::builtin()
        .variant(&theme.id)
        .unwrap_or_else(|| ThemeRegistry::builtin().default_variant(theme.appearance));
    let config = settings.resolve(variant.appearance, None);
    let frame = Frame::new(texture(art, &config, window), config, variant);
    let translucent = frame.translucent_theme(variant);
    scoped(translucent, move |window, cx| {
        paint(Some(&frame), view(window, cx)).into_any_element()
    })
    .into_any_element()
}

fn builtin(kind: BuiltinBackground, window: &mut Window, cx: &mut App) -> AnyElement {
    let settings = BackgroundSettings::with_source(BackgroundSource::Builtin(kind));
    over(
        Art::Builtin(kind),
        settings,
        |window, cx| crate::shell::shell("background-shell", Nav::Search, window, cx),
        window,
        cx,
    )
}

pub fn dusk_story(window: &mut Window, cx: &mut App) -> AnyElement {
    builtin(BuiltinBackground::Dusk, window, cx)
}

pub fn lantern_story(window: &mut Window, cx: &mut App) -> AnyElement {
    builtin(BuiltinBackground::Lantern, window, cx)
}

pub fn ink_story(window: &mut Window, cx: &mut App) -> AnyElement {
    builtin(BuiltinBackground::Ink, window, cx)
}

/// The demo illustration as a user image (demo-mode settings) behind the
/// Pull requests view: the list shows it through translucent panes, the diff
/// keeps its opaque `bg.code` surface (FR-6 / AC-8).
pub fn illustration_story(window: &mut Window, cx: &mut App) -> AnyElement {
    let settings = demo_settings(Path::new("demo-illustration.png"));
    over(
        Art::Illustration,
        settings,
        crate::review::list_story,
        window,
        cx,
    )
}

/// Render `child` with `theme` as the active theme, restoring the previous
/// theme once the subtree has painted.
fn scoped(
    theme: Theme,
    child: impl FnOnce(&mut Window, &mut App) -> AnyElement + 'static,
) -> Scoped {
    Scoped {
        theme: Some(theme),
        build: Some(Box::new(child)),
        child: None,
        saved: None,
    }
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement>;

struct Scoped {
    theme: Option<Theme>,
    build: Option<Build>,
    child: Option<AnyElement>,
    saved: Option<Theme>,
}

impl Scoped {
    fn swap_in(&mut self, cx: &mut App) {
        if let Some(theme) = self.theme.take() {
            self.saved = Some(cx.theme().clone());
            cx.set_global(theme);
        }
    }
}

impl Element for Scoped {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.swap_in(cx);
        let mut child = (self.build.take().expect("built once"))(window, cx);
        let layout = child.request_layout(window, cx);
        self.child = Some(child);
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = &mut self.child {
            child.prepaint(window, cx);
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = &mut self.child {
            child.paint(window, cx);
        }
        if let Some(saved) = self.saved.take() {
            cx.set_global(saved);
        }
    }
}

impl IntoElement for Scoped {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}
