//! Nested scrolling through real gpui wheel dispatch: a list that calls
//! `contain_scroll` scrolls alone, its scrolling host stays put (Settings ›
//! Appearance theme grid and code font list). A plain nested list still drags
//! its host along, which is the bug `contain_scroll` exists for.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("scroll_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_theme::ThemeRegistry;
    use adoken_ui::{ContainScroll, Theme};
    use anyhow::ensure;
    use gpui::{
        AppContext as _, Context, HeadlessAppContext, Modifiers, PlatformInput, Render,
        ScrollDelta, ScrollHandle, ScrollWheelEvent, TouchPhase, Window, WindowHandle, div, point,
        prelude::*, px, size as gsize,
    };

    struct Host {
        page: ScrollHandle,
        contained: ScrollHandle,
        plain: ScrollHandle,
    }

    fn list(id: &'static str) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .h(px(200.0))
            .overflow_y_scroll()
            .children((0..40).map(|_| div().h(px(30.0))))
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            // Page 400 high over 1000 of content: the contained list at
            // y 0–200, the plain one at y 200–400, then filler.
            div()
                .id("page")
                .size_full()
                .overflow_y_scroll()
                .track_scroll(&self.page)
                .child(
                    list("contained")
                        .track_scroll(&self.contained)
                        .contain_scroll(),
                )
                .child(list("plain").track_scroll(&self.plain))
                .child(div().h(px(600.0)))
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx));
        let host = Host {
            page: ScrollHandle::new(),
            contained: ScrollHandle::new(),
            plain: ScrollHandle::new(),
        };
        let (page, contained, plain) = (
            host.page.clone(),
            host.contained.clone(),
            host.plain.clone(),
        );
        let window: WindowHandle<Host> =
            cx.open_window(gsize(px(400.0), px(400.0)), move |_, cx| cx.new(|_| host))?;

        wheel(&mut cx, window, 100.0, -60.0)?;
        ensure!(
            contained.offset().y == px(-60.0),
            "the contained list scrolls: {:?}",
            contained.offset()
        );
        ensure!(
            page.offset().y == px(0.0),
            "its host doesn't scroll with it: {:?}",
            page.offset()
        );

        wheel(&mut cx, window, 300.0, -60.0)?;
        ensure!(
            plain.offset().y == px(-60.0) && page.offset().y == px(-60.0),
            "a plain nested list drags its host along: list {:?}, page {:?}",
            plain.offset(),
            page.offset()
        );

        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        Ok(())
    }

    fn wheel(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Host>,
        y: f32,
        dy: f32,
    ) -> anyhow::Result<()> {
        let event = PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: point(px(100.0), px(y)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        });
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_event(event, cx);
            window.draw(cx).clear();
        })?;
        cx.run_until_parked();
        Ok(())
    }
}
