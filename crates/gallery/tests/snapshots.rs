//! Snapshot every gallery story in every house theme. Pins additionally
//! cover every built-in variant; the `theme-strip` story covers the ported
//! community themes for the remaining components.
//!
//! Renders through gpui's `HeadlessAppContext`: deterministic test scheduler,
//! native CoreText shaping, and the real Metal renderer drawing offscreen (no
//! window server, no display needed). Goldens live in
//! `crates/gallery/snapshots/<story>--<variant>.png`.
//!
//! - `UPDATE_SNAPSHOTS=1 cargo test -p adoken-gallery --test snapshots`
//!   rewrites goldens; review the PNG diff before committing.
//! - `SNAPSHOT_FILTER=<substring>` limits the run to stories whose name
//!   contains it (regenerate only your own goldens).
//! - On mismatch the actual image is written to
//!   `target/snapshot-failures/<name>.png`.
//!
//! macOS only for now (the only platform with a headless GPU renderer in the
//! pinned gpui); elsewhere the test reports a skip. Goldens are tied to the
//! macOS font stack, so a comparison tolerance absorbs anti-aliasing noise,
//! not layout changes.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("snapshots: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use adoken_gallery::{STORIES, StoryView};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail};
    use gpui::{AppContext as _, HeadlessAppContext, px, size};
    use image::RgbaImage;

    /// Per-channel difference treated as equal (glyph anti-aliasing jitter).
    const CHANNEL_TOLERANCE: u8 = 8;
    /// Share of pixels allowed beyond the channel tolerance.
    const MAX_DIFFERENT_PIXELS: f64 = 0.002;

    pub fn run() -> anyhow::Result<()> {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let golden_dir = manifest.join("snapshots");
        let failure_dir = manifest.join("../../target/snapshot-failures");
        let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
        let filter = std::env::var("SNAPSHOT_FILTER").unwrap_or_default();

        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        // Goldens show every animation at rest (gpui snaps `with_animation`
        // to its end state and spinners/shimmer to their static frame).
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            adoken_gallery::init(cx);
        });

        let mut failures = Vec::new();
        let mut checked = 0;
        for variant in ThemeRegistry::builtin().variants() {
            let house = ThemeRegistry::house_variants().any(|house| house.id == variant.id);
            cx.update(|cx| Theme::install(variant, cx));
            // Pins and the work item detail (work-item-detail.md §14) also
            // cover every bundled theme.
            for story in STORIES.iter().filter(|s| {
                s.name.contains(filter.as_str())
                    && (house || s.name.starts_with("pins-") || s.name == "work-item-detail")
            }) {
                let name = format!("{}--{}", story.name, variant.id);
                let window = cx
                    .open_window(size(px(story.size.0), px(story.size.1)), |_, cx| {
                        cx.new(|_| StoryView::new(story))
                    })?;
                cx.run_until_parked();
                // One settling paint: stories whose state follows layout
                // (pickers anchored to painted cells) reach it here.
                cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
                cx.run_until_parked();
                let image = cx
                    .update_window(window.into(), |_, window, cx| {
                        window.draw(cx).clear();
                        window.render_to_image()
                    })?
                    .with_context(|| format!("rendering {name}"))?;
                cx.update_window(window.into(), |_, window, _| window.remove_window())?;

                let golden = golden_dir.join(format!("{name}.png"));
                checked += 1;
                if update {
                    std::fs::create_dir_all(&golden_dir)?;
                    image.save(&golden)?;
                    println!("snapshots: wrote {}", golden.display());
                    continue;
                }
                if let Err(reason) = compare(&image, &golden) {
                    std::fs::create_dir_all(&failure_dir)?;
                    let actual = failure_dir.join(format!("{name}.png"));
                    image.save(&actual)?;
                    failures.push(format!("{name}: {reason} (actual: {})", actual.display()));
                }
            }
        }

        if !failures.is_empty() {
            bail!(
                "{} of {checked} snapshots differ:\n{}\nRun with UPDATE_SNAPSHOTS=1 if the change is intended.",
                failures.len(),
                failures.join("\n")
            );
        }
        println!("snapshots: {checked} ok");
        Ok(())
    }

    fn compare(actual: &RgbaImage, golden: &Path) -> Result<(), String> {
        let expected = image::open(golden)
            .map_err(|error| format!("no golden ({error})"))?
            .into_rgba8();
        if expected.dimensions() != actual.dimensions() {
            return Err(format!(
                "size {:?}, golden {:?}",
                actual.dimensions(),
                expected.dimensions()
            ));
        }
        let different = actual
            .pixels()
            .zip(expected.pixels())
            .filter(|(a, e)| {
                a.0.iter()
                    .zip(e.0.iter())
                    .any(|(a, e)| a.abs_diff(*e) > CHANNEL_TOLERANCE)
            })
            .count();
        let total = (actual.width() * actual.height()) as f64;
        let share = different as f64 / total;
        if share > MAX_DIFFERENT_PIXELS {
            return Err(format!("{:.3}% of pixels differ", share * 100.0));
        }
        Ok(())
    }
}
