//! Profile pictures (spec 0012; principles.md §4 Avatars).
//!
//! [`Avatar`] draws a person: their picture once it's decoded, else their
//! initials, which are also the loading state (no spinner, no blank frame).
//! Both fill the same circle, so a picture arriving never moves anything.
//!
//! Drawing an avatar is what asks for its picture (FR-2): the first draw of
//! a person queues one load (deduplicated per id); the load runs off the UI
//! thread and the windows repaint once when it lands.
//!
//! - Connected: [`adoken_sync::avatars::AvatarFetcher`] decides from its disk
//!   cache or fetches through the sync governor, on Tokio.
//! - Demo: generated art ([`art`]) for the people whose demo payload carries
//!   an image URL, initials for the rest; no network (FR-13).
//! - Neither installed (tests, most stories): initials.
//!
//! Decoded pictures are downscaled to `size.avatar.lg` at 2× and kept in a
//! least-recently-drawn cache of at most [`MAX_ENTRIES`] / [`MAX_BYTES`]
//! (FR-11).
//!
//! The circle-clipped `img` inside a circle follows
//! zeron's `crates/ui/src/history.rs` (<https://github.com/zeronsh/zeron>,
//! commit author avatars, MIT).

pub mod art;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use adoken_model::Person;
use adoken_sync::avatars::{AvatarFetcher, Resolved};
use adoken_theme::{radius, size, typography};
use adoken_ui::{ActiveTheme, TypeStyleExt};
use gpui::{
    App, Global, ImageSource, IntoElement, ObjectFit, RenderImage, RenderOnce, SharedString,
    Window, div, img, prelude::*, px,
};
use gpui_tokio::Tokio;
use image::RgbaImage;
use image::imageops::FilterType;

/// Decoded side: `size.avatar.lg` at 2× (FR-11).
pub const DECODED_SIDE: u32 = (size::AVATAR_LG * 2.0) as u32;
/// Decoded cache bounds (FR-11).
pub const MAX_ENTRIES: usize = 512;
pub const MAX_BYTES: usize = 8 << 20;
/// A load that couldn't go out (offline, throttled, budget) is retried on a
/// draw after this long.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Pictures landing together repaint once.
const REPAINT_COALESCE: Duration = Duration::from_millis(16);
/// Larger sources are refused before decoding.
const MAX_SOURCE_SIDE: u32 = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AvatarSize {
    /// `size.avatar.sm` (16).
    Sm,
    /// `size.avatar.md` (20): rows, sidebar.
    #[default]
    Md,
    /// `size.avatar.lg` (28): previews, headers.
    Lg,
}

impl AvatarSize {
    pub fn px(self) -> f32 {
        match self {
            AvatarSize::Sm => size::AVATAR_SM,
            AvatarSize::Md => size::AVATAR_MD,
            AvatarSize::Lg => size::AVATAR_LG,
        }
    }
}

/// `Dana Kim` → `DK`; one word → first letter. A policy group's
/// `[Project]\` prefix doesn't count (`[Contoso]\React PR reviewers` → `RP`).
pub fn initials(name: &str) -> String {
    let name = name
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("]\\"))
        .map_or(name, |(_, rest)| rest);
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect()
}

/// One person's avatar.
#[derive(IntoElement)]
pub struct Avatar {
    id: SharedString,
    name: SharedString,
    group: bool,
    size: AvatarSize,
}

impl Avatar {
    /// `id` is the identity id; an empty id draws initials only.
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Avatar {
            id: id.into(),
            name: name.into(),
            group: false,
            size: AvatarSize::default(),
        }
    }

    pub fn person(person: &Person) -> Self {
        Self::new(person.id.clone(), person.display_name.clone()).group(person.is_group)
    }

    /// Groups and teams always show initials (FR-3).
    pub fn group(mut self, group: bool) -> Self {
        self.group = group;
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for Avatar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let side = px(self.size.px());
        let circle = div().size(side).flex_none().rounded(px(radius::FULL));
        if let Some(image) = picture(&self.id, self.group, cx) {
            return circle.child(
                img(ImageSource::Render(image))
                    .size(side)
                    .rounded(px(radius::FULL))
                    .object_fit(ObjectFit::Cover),
            );
        }
        let theme = cx.theme();
        let mut text = initials(&self.name);
        if self.size == AvatarSize::Sm {
            text.truncate(text.chars().next().map_or(0, char::len_utf8));
        }
        circle
            .bg(theme.colors.accent_wash)
            .text_color(theme.colors.accent_fg)
            .type_style(typography::LABEL, theme)
            .flex()
            .items_center()
            .justify_center()
            .child(text)
    }
}

/// Where pictures come from.
enum Source {
    /// Initials only.
    Off,
    /// Generated art for these ids.
    Demo(HashSet<SharedString>),
    Connected(Arc<AvatarFetcher>),
}

enum Slot {
    Loading,
    Ready { image: Arc<RenderImage>, drawn: u64 },
    Initials,
    Retry(Instant),
}

struct State {
    source: Source,
    /// Bumped when the source changes: late loads for the old one are dropped.
    epoch: u64,
    slots: HashMap<SharedString, Slot>,
    /// Draw counter for least-recently-drawn eviction.
    tick: u64,
    ready: usize,
    bytes: usize,
    repaint_queued: bool,
}

impl State {
    fn new() -> Self {
        State {
            source: Source::Off,
            epoch: 0,
            slots: HashMap::new(),
            tick: 0,
            ready: 0,
            bytes: 0,
            repaint_queued: false,
        }
    }

    /// Store a decoded picture, evicting least-recently-drawn ones past the
    /// bounds. Returns the evicted images (to free their GPU tiles).
    fn insert_ready(&mut self, id: SharedString, image: Arc<RenderImage>) -> Vec<Arc<RenderImage>> {
        self.tick += 1;
        let bytes = image_bytes(&image);
        if let Some(Slot::Ready { image: old, .. }) = self.slots.insert(
            id,
            Slot::Ready {
                image,
                drawn: self.tick,
            },
        ) {
            self.ready -= 1;
            self.bytes -= image_bytes(&old);
        }
        self.ready += 1;
        self.bytes += bytes;
        let mut evicted = Vec::new();
        while self.ready > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some(oldest) = self
                .slots
                .iter()
                .filter_map(|(id, slot)| match slot {
                    Slot::Ready { drawn, .. } => Some((*drawn, id.clone())),
                    _ => None,
                })
                .min()
                .map(|(_, id)| id)
            else {
                break;
            };
            if let Some(Slot::Ready { image, .. }) = self.slots.remove(&oldest) {
                self.ready -= 1;
                self.bytes -= image_bytes(&image);
                evicted.push(image);
            }
        }
        evicted
    }
}

fn image_bytes(image: &RenderImage) -> usize {
    let s = image.size(0);
    s.width.0 as usize * s.height.0 as usize * 4
}

#[derive(Clone)]
struct Avatars(Rc<RefCell<State>>);

impl Global for Avatars {}

/// Register the (empty) picture state: initials until a source is set. No IO.
pub fn init(cx: &mut App) {
    if !cx.has_global::<Avatars>() {
        cx.set_global(Avatars(Rc::new(RefCell::new(State::new()))));
    }
}

/// Demo org / stories: generated pictures for `ids`, initials for others.
/// The same set again keeps what's already decoded.
pub fn use_demo(ids: impl IntoIterator<Item = SharedString>, cx: &mut App) {
    let ids: HashSet<SharedString> = ids.into_iter().collect();
    let same = cx
        .try_global::<Avatars>()
        .is_some_and(|a| matches!(&a.0.borrow().source, Source::Demo(current) if *current == ids));
    if !same {
        set_source(Source::Demo(ids), cx);
    }
}

/// Demo people with a picture: those whose payload carries an image URL.
pub fn demo_ids(people: &[Person]) -> Vec<SharedString> {
    people
        .iter()
        .filter(|p| p.image_url.is_some() && !p.is_group)
        .map(|p| SharedString::from(p.id.clone()))
        .collect()
}

/// Connected: pictures through `fetcher`.
pub fn use_fetcher(fetcher: Arc<AvatarFetcher>, cx: &mut App) {
    set_source(Source::Connected(fetcher), cx);
}

/// Back to initials only (signed out, org closed).
pub fn clear(cx: &mut App) {
    set_source(Source::Off, cx);
}

fn set_source(source: Source, cx: &mut App) {
    init(cx);
    let state = cx.global::<Avatars>().0.clone();
    let dropped: Vec<Arc<RenderImage>> = {
        let mut s = state.borrow_mut();
        s.source = source;
        s.epoch += 1;
        s.ready = 0;
        s.bytes = 0;
        s.slots
            .drain()
            .filter_map(|(_, slot)| match slot {
                Slot::Ready { image, .. } => Some(image),
                _ => None,
            })
            .collect()
    };
    for image in dropped {
        cx.drop_image(image, None);
    }
    cx.refresh_windows();
}

/// The decoded picture for `id`, if ready. Queues a load on first sight.
fn picture(id: &SharedString, group: bool, cx: &mut App) -> Option<Arc<RenderImage>> {
    if group || id.is_empty() {
        return None;
    }
    let state = cx.try_global::<Avatars>()?.0.clone();
    let epoch = {
        let mut s = state.borrow_mut();
        if matches!(s.source, Source::Off) {
            return None;
        }
        s.tick += 1;
        let tick = s.tick;
        match s.slots.get_mut(id) {
            Some(Slot::Ready { image, drawn }) => {
                *drawn = tick;
                return Some(image.clone());
            }
            Some(Slot::Loading | Slot::Initials) => return None,
            Some(Slot::Retry(at)) if Instant::now() < *at => return None,
            Some(Slot::Retry(_)) | None => {}
        }
        s.slots.insert(id.clone(), Slot::Loading);
        s.epoch
    };
    load(state, id.clone(), epoch, cx);
    None
}

enum Loaded {
    Picture(Arc<RenderImage>),
    Initials,
    Later,
}

fn load(state: Rc<RefCell<State>>, id: SharedString, epoch: u64, cx: &mut App) {
    let job = match &state.borrow().source {
        Source::Off => return,
        Source::Demo(ids) => {
            if !ids.contains(&id) {
                None
            } else {
                let id = id.clone();
                Some(cx.background_spawn(async move {
                    Loaded::Picture(render_image(art::generate(&id, DECODED_SIDE)))
                }))
            }
        }
        Source::Connected(fetcher) => {
            let fetcher = fetcher.clone();
            let key = id.to_string();
            let resolved = Tokio::spawn(cx, async move { fetcher.resolve(key, false).await });
            let executor = cx.background_executor().clone();
            Some(executor.clone().spawn(async move {
                match resolved.await {
                    Ok(Resolved::Picture(path)) => {
                        match executor.spawn(async move { decode_file(&path) }).await {
                            Some(image) => Loaded::Picture(render_image(image)),
                            None => Loaded::Initials,
                        }
                    }
                    Ok(Resolved::Initials) => Loaded::Initials,
                    Ok(Resolved::Later) | Err(_) => Loaded::Later,
                }
            }))
        }
    };
    let Some(job) = job else {
        state.borrow_mut().slots.insert(id, Slot::Initials);
        return;
    };
    cx.spawn(async move |cx| {
        let loaded = job.await;
        cx.update(|cx| finish(&state, id, epoch, loaded, cx));
    })
    .detach();
}

fn finish(state: &Rc<RefCell<State>>, id: SharedString, epoch: u64, loaded: Loaded, cx: &mut App) {
    let evicted = {
        let mut s = state.borrow_mut();
        if s.epoch != epoch {
            return;
        }
        match loaded {
            Loaded::Picture(image) => s.insert_ready(id, image),
            Loaded::Initials => {
                s.slots.insert(id, Slot::Initials);
                return;
            }
            Loaded::Later => {
                s.slots
                    .insert(id, Slot::Retry(Instant::now() + RETRY_AFTER));
                return;
            }
        }
    };
    for image in evicted {
        cx.drop_image(image, None);
    }
    queue_repaint(state, cx);
}

fn queue_repaint(state: &Rc<RefCell<State>>, cx: &mut App) {
    if std::mem::replace(&mut state.borrow_mut().repaint_queued, true) {
        return;
    }
    let state = state.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(REPAINT_COALESCE).await;
        cx.update(|cx| {
            state.borrow_mut().repaint_queued = false;
            cx.refresh_windows();
        });
    })
    .detach();
}

/// Blocking: read and decode a cached PNG, downscaled to [`DECODED_SIDE`].
pub fn decode_file(path: &Path) -> Option<RgbaImage> {
    decode(&std::fs::read(path).ok()?)
}

/// Blocking: decode a PNG and fit it to [`DECODED_SIDE`] (never upscaled).
pub fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    reader.limits(limits);
    let image = reader.decode().ok()?;
    if image.width() <= DECODED_SIDE && image.height() <= DECODED_SIDE {
        return Some(image.into_rgba8());
    }
    Some(
        image
            .resize_to_fill(DECODED_SIDE, DECODED_SIDE, FilterType::Triangle)
            .into_rgba8(),
    )
}

fn render_image(mut image: RgbaImage) -> Arc<RenderImage> {
    crate::background::pipeline::to_bgra(&mut image);
    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
}

#[cfg(test)]
mod tests;
