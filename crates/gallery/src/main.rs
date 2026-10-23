//! Live component gallery: every story, stacked, in one theme at a time.
//!
//! `cargo run -p adoken-gallery [-- --themes <dir>]`
//! `secondary-t` cycles themes (built-ins plus any `*.adoken-theme.toml` in
//! `<dir>`, loaded off the UI thread); `secondary-q` quits.

use std::path::PathBuf;

use adoken_gallery::{STORIES, StoryView};
use adoken_theme::{ThemeRegistry, ThemeVariant, load_theme_dir, space, typography};
use adoken_ui::{ActiveTheme, Kbd, Theme, TypeStyleExt};
use gpui::{
    App, Context, Entity, Global, KeyBinding, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, size,
};

actions!(gallery, [CycleTheme, Quit]);

/// Every theme the gallery can show, in cycle order.
struct Themes {
    variants: Vec<ThemeVariant>,
    current: usize,
}

impl Global for Themes {}

fn cycle_theme(_: &CycleTheme, cx: &mut App) {
    let themes = cx.global_mut::<Themes>();
    themes.current = (themes.current + 1) % themes.variants.len();
    let variant = themes.variants[themes.current].clone();
    Theme::install(&variant, cx);
}

struct Gallery {
    stories: Vec<Entity<StoryView>>,
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let colors = &theme.colors;
        div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(space::S16))
            .p(px(space::S24))
            .bg(colors.bg_sidebar)
            .text_color(colors.text_primary)
            .type_style(typography::BODY, theme)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(space::S8))
                    .type_style(typography::TITLE, theme)
                    .child(format!("{} ({})", theme.name, theme.id))
                    .child(Kbd::parse("secondary-t").expect("static binding")),
            )
            .children(STORIES.iter().zip(&self.stories).map(|(story, view)| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::S6))
                    .child(
                        div()
                            .text_color(colors.text_secondary)
                            .type_style(typography::LABEL, theme)
                            .child(story.name),
                    )
                    .child(
                        div()
                            .w(px(story.size.0))
                            .h(px(story.size.1))
                            .border_1()
                            .border_color(colors.border_default)
                            .child(view.clone()),
                    )
            }))
    }
}

fn themes_dir_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--themes" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn main() {
    let themes_dir = themes_dir_arg();
    let application = gpui_platform::application().with_assets(adoken_ui::Assets);
    application.run(move |cx: &mut App| {
        let variants: Vec<ThemeVariant> = ThemeRegistry::builtin().variants().cloned().collect();
        Theme::install(&variants[0], cx);
        cx.set_global(Themes {
            variants,
            current: 0,
        });
        adoken_gallery::init(cx);
        cx.on_action(cycle_theme);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([
            KeyBinding::new("secondary-t", CycleTheme, None),
            KeyBinding::new("secondary-q", Quit, None),
        ]);

        if let Some(dir) = themes_dir {
            // Theme files are read on a background thread; the UI never waits.
            let load = cx
                .background_executor()
                .spawn(async move { load_theme_dir(&dir, ThemeRegistry::builtin()) });
            cx.spawn(async move |cx| {
                let results = load.await;
                cx.update(|cx| {
                    let themes = cx.global_mut::<Themes>();
                    for (path, result) in results {
                        match result {
                            Ok(loaded) => {
                                for issue in &loaded.issues {
                                    eprintln!("{}: {issue}", path.display());
                                }
                                themes.variants.extend(loaded.family.variants);
                            }
                            Err(error) => eprintln!("{error}"),
                        }
                    }
                });
            })
            .detach();
        }

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(760.0), px(720.0)), cx)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| Gallery {
                    stories: STORIES
                        .iter()
                        .map(|story| cx.new(|_| StoryView::new(story)))
                        .collect(),
                })
            },
        )
        .expect("failed to open the gallery window");
        cx.activate(true);
    });
}
