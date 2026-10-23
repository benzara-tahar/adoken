//! Spec 0007 AC-8 / FR-6: with a custom background showing, code surfaces
//! stay opaque — no background pixel reaches the screen behind code.
//!
//! Renders a code block (`bg.code`, the diff surface) and a translucent pane
//! over a busy, saturated background with the real paint stack and theme,
//! then samples pixels: every pixel inside the code block is exactly
//! `bg.code`; the pane next to it does let the image through (so the test
//! would catch a backdrop that silently stopped painting).
//!
//! Headless Metal like `snapshots.rs`; macOS only.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("backdrop: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_app::background::{Frame, Texture, paint, pipeline};
    use adoken_theme::background::{BackgroundSettings, BackgroundSource, BuiltinBackground};
    use adoken_theme::{Color, ThemeRegistry};
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AppContext as _, Context, HeadlessAppContext, IntoElement, Render, Window, div, prelude::*,
        px, size,
    };
    use image::RgbaImage;

    const W: f32 = 400.0;
    const H: f32 = 200.0;

    struct Scene {
        frame: Frame,
        code: Color,
        pane: Color,
    }

    impl Render for Scene {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let hsla = adoken_ui::theme::hsla;
            let content = div()
                .size_full()
                .flex()
                .child(div().w(px(W / 2.0)).h_full().bg(hsla(self.code)))
                .child(div().w(px(W / 2.0)).h_full().bg(hsla(self.pane)))
                .into_any_element();
            paint(Some(&self.frame), content)
        }
    }

    /// Loud stripes: any bleed-through shows up as a non-`bg.code` pixel.
    fn stripes(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, _| {
            image::Rgba(match (x / 7) % 3 {
                0 => [255, 0, 255, 255],
                1 => [0, 255, 0, 255],
                _ => [255, 255, 255, 255],
            })
        })
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| cx.set_reduce_motion(true));

        let mut checked = 0;
        for variant in ThemeRegistry::builtin().variants() {
            let config =
                BackgroundSettings::with_source(BackgroundSource::Builtin(BuiltinBackground::Ink))
                    .resolve(variant.appearance, None);
            let processed = pipeline::finish(stripes(800, 400));
            let frame = Frame::new(Texture::from_processed(processed), config, variant);
            let theme: Theme = frame.translucent_theme(variant);
            let (code, pane) = (theme.raw.bg_code, theme.raw.bg_surface);
            ensure!(code.is_opaque(), "{}: bg.code must stay opaque", variant.id);
            ensure!(!pane.is_opaque(), "{}: panes are translucent", variant.id);
            cx.update(|cx| cx.set_global(theme));

            let window = cx.open_window(size(px(W), px(H)), |_, cx| {
                cx.new(|_| Scene { frame, code, pane })
            })?;
            cx.run_until_parked();
            let image = cx
                .update_window(window.into(), |_, window, cx| {
                    window.draw(cx).clear();
                    window.render_to_image()
                })?
                .with_context(|| format!("rendering {}", variant.id))?;
            cx.update_window(window.into(), |_, window, _| window.remove_window())?;

            let (width, height) = image.dimensions();
            let half = width / 2;
            // Inset one device pixel from the split to skip edge AA.
            let bleed = (0..height)
                .flat_map(|y| (0..half - 1).map(move |x| (x, y)))
                .filter(|&(x, y)| {
                    let [r, g, b, _] = image.get_pixel(x, y).0;
                    [r.abs_diff(code.r), g.abs_diff(code.g), b.abs_diff(code.b)]
                        .iter()
                        .any(|d| *d > 1)
                })
                .count();
            if bleed > 0 {
                bail!("{}: {bleed} code pixels show the background", variant.id);
            }
            let distinct: std::collections::HashSet<[u8; 4]> = (half + 1..width)
                .map(|x| image.get_pixel(x, height / 2).0)
                .collect();
            ensure!(
                distinct.len() > 2,
                "{}: the translucent pane should show the striped background",
                variant.id
            );
            checked += 1;
        }
        println!("backdrop: code surfaces opaque over a background in {checked} themes");
        Ok(())
    }
}
