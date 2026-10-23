//! `RichText` through a real gpui window (spec 0015 FR-3, §7): clicks on a
//! link, a `#123` reference and an image chip reach the callbacks with the
//! right target, and a 20 KB description plus the visible comments draws
//! within the frame budget once parsed (frame recorder).

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("rich_text_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;

    use adoken_perf::{Budget, FrameRecorder, Samples};
    use adoken_richtext::{Doc, Format, LinkTarget, parse};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::{RichText, Theme};
    use anyhow::ensure;
    use gpui::{
        AppContext as _, Context, HeadlessAppContext, Modifiers, MouseButton, MouseDownEvent,
        MouseMoveEvent, MouseUpEvent, PlatformInput, Render, Window, WindowHandle, div, point,
        prelude::*, px, size as gsize,
    };

    struct Host {
        docs: Vec<(Arc<Doc>, Option<f32>)>,
        events: Rc<RefCell<Vec<String>>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().gap(px(16.0)).children(
                self.docs.iter().enumerate().map(|(i, (doc, clip))| {
                    let links = self.events.clone();
                    let images = self.events.clone();
                    RichText::new(("doc", i), doc.clone())
                        .max_height(*clip)
                        .on_link(move |target, _, _| {
                            links.borrow_mut().push(match target {
                                LinkTarget::Web(url) => format!("web {url}"),
                                LinkTarget::WorkItem(id) => format!("work item {id}"),
                                LinkTarget::PullRequest(id) => format!("pr {id}"),
                            })
                        })
                        .on_image(move |src, _, _| images.borrow_mut().push(format!("image {src}")))
                }),
            )
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            Theme::install(ThemeRegistry::house_variants().next().unwrap(), cx);
        });
        clicks(&mut cx)?;
        frames(&mut cx)?;
        Ok(())
    }

    fn open(
        cx: &mut HeadlessAppContext,
        docs: Vec<(Arc<Doc>, Option<f32>)>,
        events: Rc<RefCell<Vec<String>>>,
    ) -> anyhow::Result<WindowHandle<Host>> {
        cx.open_window(gsize(px(720.0), px(900.0)), move |_, cx| {
            cx.new(|_| Host { docs, events })
        })
    }

    fn click(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Host>,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        let position = point(px(x), px(y));
        let events = [
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
        ];
        for event in events {
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_event(event, cx);
            })?;
            cx.run_until_parked();
        }
        Ok(())
    }

    /// First paragraph line is y 0–20; the next block starts after the
    /// `space.8` gap.
    fn clicks(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let doc = |md: &str| (Arc::new(parse(md, Format::Markdown)), None);
        let window = open(
            cx,
            vec![
                doc("[the runbook](https://contoso.example/runbook)"),
                doc("#4821"),
                doc("![shot](https://dev.azure.com/c/_apis/wit/attachments/1?fileName=a.png)"),
                doc("@<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607> plain text"),
            ],
            events.clone(),
        )?;
        // Blocks: link 0–20, gap 16, #4821 36–56, gap 16, image chip 72–96,
        // gap 16, mention 112–132.
        click(cx, window, 12.0, 10.0)?;
        click(cx, window, 12.0, 46.0)?;
        click(cx, window, 12.0, 84.0)?;
        click(cx, window, 12.0, 122.0)?;
        let got = events.borrow().clone();
        ensure!(
            got == [
                "web https://contoso.example/runbook",
                "work item 4821",
                "image https://dev.azure.com/c/_apis/wit/attachments/1?fileName=a.png",
            ],
            "clicks reach on_link / on_image, mentions aren't links: {got:?}"
        );
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        Ok(())
    }

    /// Spec 0015 §7: a 20 KB description plus comments, parsed off the UI
    /// thread, paints within a frame: unclipped ("Show all") and clipped at
    /// 480 (design §5.1), with the last five comments (design Q-3).
    fn frames(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let description = Arc::new(parse(&fixture(), Format::Html));
        let comments: Vec<_> = (0..200)
            .map(|i| {
                Arc::new(parse(
                    &format!(
                        "Comment {i}: **verified** on staging, see #48{i:02} and \
                         @<d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607>.\n\n- step one\n- step `two`"
                    ),
                    Format::Markdown,
                ))
            })
            .collect();
        for clip in [None, Some(480.0)] {
            let mut docs = vec![(description.clone(), clip)];
            docs.extend(comments[195..].iter().map(|c| (c.clone(), None)));
            let window = open(cx, docs, Rc::default())?;
            let mut recorder = FrameRecorder::start();
            for _ in 0..30 {
                cx.update_window(window.into(), |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear();
                })?;
                cx.run_until_parked();
            }
            recorder.collect();
            let draws = recorder.draw().as_slice();
            ensure!(draws.len() >= 30, "recorded {} frames", draws.len());
            // The first frame prepares the document (once per (id, rev));
            // every later frame reuses it.
            let mut warm = Samples::new("rich text warm frame");
            draws[1..].iter().for_each(|d| warm.record(*d));
            let summary = warm.summary().expect("warm frames");
            let check = Budget::FRAME.check(&summary);
            println!(
                "rich text frames ({}): first {:.2} ms; warm {}",
                if clip.is_some() {
                    "clipped 480"
                } else {
                    "show all"
                },
                draws[0].as_secs_f64() * 1e3,
                check.row(),
            );
            ensure!(check.passed(), "warm frames over budget: {}", check.row());
            cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        }
        Ok(())
    }

    /// ~20 KB of Azure DevOps editor HTML.
    fn fixture() -> String {
        let unit = "<div style=\"box-sizing:border-box;\"><span style=\"box-sizing:border-box;display:inline !important;\">Settlement runs at <b>00:05 UTC</b>, but merchants in UTC+10 see the previous day's totals. See <a href=\"https://dev.azure.com/org/Payments/_workitems/edit/4821\">#4821</a> and https://learn.microsoft.com/azure/devops.</span><br style=\"box-sizing:border-box;\"> </div>\
<ul><li>Repro: set merchant tz to <code>Australia/Sydney</code></li><li>Expected: totals for the local calendar day<ul><li>nested <i>detail</i></li></ul></li></ul>\
<div><a href=\"#\" data-vss-mention=\"version:2.0,d2c4c61b-5a1a-6cf0-8a11-f7dd5385b607\">@Dana Kim</a> can you verify on staging?</div>\
<table><tr><th>Region</th><th>Offset</th></tr><tr><td>AU</td><td>+10</td></tr></table>\
<pre>fn settle(day: Date) -&gt; Total {\n    todo()\n}</pre>";
        let mut html = String::new();
        while html.len() < 20 * 1024 {
            html.push_str(unit);
        }
        html
    }
}
