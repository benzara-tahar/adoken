//! Pins through the real shell, focused inputs and actual key dispatch.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("pins_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::Duration;

    use adoken_app::pins::Pins;
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_model::PinSubject;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AppContext as _, Context, Entity, Focusable, HeadlessAppContext, IntoElement, Keystroke,
        Render, Window, WindowHandle, div, point, prelude::*, px, size,
    };

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
            adoken_gallery::init(cx);
            cx.clear_key_bindings();
            cx.bind_keys(adoken_app::key_bindings());
        });
        let dir = tempfile::tempdir()?;
        let store = adoken_demo::open_store_at(dir.path(), adoken_demo::DEMO_NOW)?;
        let initial = store.pins()?;
        ensure!(initial.len() == 4, "demo seeds the four prescribed pins");
        let window = cx.open_window(size(px(1200.0), px(760.0)), |window, cx| {
            cx.new(|cx| {
                Shell::new(
                    StoreSource::Open {
                        store: store.clone(),
                        demo: true,
                    },
                    adoken_app::shell::commands(),
                    window,
                    cx,
                )
            })
        })?;
        settle(&mut cx, |cx| {
            Ok(cx.update(|cx| {
                Pins::global(cx).is_some_and(|pins| pins.read(cx).snapshot().pins.len() == 4)
            }))
        })?;
        let mut frames = adoken_perf::FrameRecorder::start();
        paint(&mut cx, window, "expanded")?;

        open_pin(&mut cx, window, 1)?;
        ensure!(
            window.update(&mut cx, |shell, _, _| shell.nav())? == Nav::PullRequests,
            "⌘J ↓ ↩ opens the second PR's canonical view"
        );
        settle(&mut cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &initial[1].subject))?
                .is_some_and(|pin| pin.opened_at.is_some()))
        })?;
        press(&mut cx, window, "cmd-d")?;
        settle(&mut cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &initial[1].subject))?
                .is_none())
        })?;
        press(&mut cx, window, "cmd-z")?;
        settle(&mut cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &initial[1].subject))?
                .is_some())
        })?;
        ensure!(
            store
                .pins()?
                .iter()
                .map(|pin| &pin.subject)
                .eq(initial.iter().map(|pin| &pin.subject)),
            "Undo restores the original order"
        );
        let PinSubject::Pr(composing_pr) = initial[1].subject else {
            bail!("second demo pin is a PR");
        };
        settle(&mut cx, |cx| {
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.available_actions(cx).iter().any(|action| {
                    action
                        .as_any()
                        .is::<adoken_app::review::pr::CommentOnPullRequest>()
                })
            })
        })
        .context("waiting for the canonical review's composer action")?;
        press(&mut cx, window, "shift-c")?;
        for character in "keepdraft".chars() {
            press(&mut cx, window, &character.to_string())?;
        }
        paint(&mut cx, window, "composer-draft")?;
        open_pin(&mut cx, window, 0)?;
        settle(&mut cx, |_| {
            Ok(adoken_app::review::writes::drafts(&store, composing_pr)?
                .iter()
                .any(|draft| draft.body == "keepdraft"))
        })
        .context("preserving the draft across a PR-to-PR pin open")?;
        open_pin(&mut cx, window, 1)?;
        println!(
            "pins_smoke: ⌘J while the real review composer has focus preserves the unsent draft"
        );

        let work_item = initial
            .iter()
            .find(|pin| matches!(pin.subject, PinSubject::WorkItem(_)))
            .context("work-item demo pin")?
            .subject
            .clone();
        cx.update(|cx| {
            Pins::global(cx).unwrap().update(cx, |pins, cx| {
                pins.rename(&work_item, Some("Fix SSO redirect".into()), cx);
                pins.move_by(&work_item, -2, cx);
            });
        });
        settle(&mut cx, |_| {
            Ok(store.pins()?.first().is_some_and(|pin| {
                pin.subject == work_item && pin.label.as_deref() == Some("Fix SSO redirect")
            }))
        })?;
        open_pin(&mut cx, window, 0)?;
        ensure!(
            window.update(&mut cx, |shell, _, _| shell.nav())? == Nav::WorkItems,
            "⌘J order follows persisted reorder"
        );

        press(&mut cx, window, "cmd-j")?;
        ensure!(
            window.update(&mut cx, |shell, _, cx| shell
                .palette()
                .read(cx)
                .is_visible())?,
            "Cmd+J opens the pins palette"
        );
        window.update(&mut cx, |shell, _, cx| {
            shell
                .palette()
                .update(cx, |palette, cx| palette.set_query("pin: ssO", cx));
        })?;
        cx.run_until_parked();
        ensure!(
            window.update(&mut cx, |shell, _, cx| shell
                .palette()
                .read(cx)
                .selected_pin_subject())?
                == Some(work_item.clone()),
            "pin scope filters local labels case-insensitively"
        );
        paint(&mut cx, window, "palette")?;
        press(&mut cx, window, "enter")?;
        let confirmed = window.update(&mut cx, |shell, _, cx| {
            (shell.palette().read(cx).is_visible(), shell.nav())
        })?;
        ensure!(
            confirmed == (false, Nav::WorkItems),
            "↩ opens the filtered pin and dismisses the palette; observed {confirmed:?}"
        );

        open_pin(&mut cx, window, 3)?;
        ensure!(
            window.update(&mut cx, |shell, _, cx| !shell
                .palette()
                .read(cx)
                .is_visible()
                && shell.nav() == Nav::Pipelines)?,
            "⌘J ↓↓↓ ↩ opens the fourth pin and dismisses the palette"
        );
        press(&mut cx, window, "ctrl-1")?;
        ensure!(
            window.update(&mut cx, |shell, _, _| shell.nav())? == Nav::Pipelines,
            "⌃1 opens no pin (spec 0025 AC-2)"
        );
        press(&mut cx, window, "cmd-b")?;
        paint(&mut cx, window, "rail")?;
        twenty_pin_peek_smoke(&mut cx, window, &store)?;
        frames.collect();
        if let Some(summary) = frames.draw().summary() {
            println!(
                "pins_smoke frames: n={}, p50={:?}, p99={:?}",
                summary.count, summary.p50, summary.p99
            );
        }
        drop(frames);
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        menu_focus_smoke(&mut cx)?;
        println!(
            "pins_smoke: ⌘J opens, composer-focus open, toggle/Undo, persisted order, local label filter, no number jumps and palette/rail rendering passed"
        );
        Ok(())
    }

    fn twenty_pin_peek_smoke(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        store: &adoken_store::Store,
    ) -> anyhow::Result<()> {
        let dataset = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let candidates = dataset
            .prs
            .iter()
            .filter(|pr| pr.status == adoken_model::PrStatus::Active)
            .map(|pr| (PinSubject::Pr(pr.id), pr.title.clone()))
            .chain(
                dataset
                    .work_items
                    .iter()
                    .filter(|wi| {
                        !matches!(
                            wi.state_category,
                            adoken_model::StateCategory::Completed
                                | adoken_model::StateCategory::Removed
                        )
                    })
                    .map(|wi| (PinSubject::WorkItem(wi.id), wi.title.clone())),
            )
            .take(adoken_model::MAX_PINS)
            .collect::<Vec<_>>();
        ensure!(
            candidates.len() == 20,
            "demo has twenty nonterminal subjects"
        );
        let mut template = store.pins()?.remove(0);
        template.label = None;
        template.opened_at = None;
        template.seen_result = None;
        template.unavailable = None;
        template.pinned_at = adoken_demo::now_ms();
        store.write(|connection| {
            for pin in adoken_store::read::pins(connection)? {
                adoken_store::write::unpin(connection, &pin.subject)?;
            }
            for (position, (subject, title)) in candidates.iter().enumerate() {
                let mut pin = template.clone();
                pin.subject = subject.clone();
                pin.title = title.clone();
                pin.position = position as u32;
                adoken_store::write::add_pin(connection, &pin)?;
            }
            Ok(())
        })?;
        cx.update(|cx| {
            Pins::global(cx)
                .unwrap()
                .update(cx, |pins, cx| pins.refresh(cx))
        });
        settle(cx, |cx| {
            Ok(cx.update(|cx| Pins::global(cx).unwrap().read(cx).snapshot().pins.len() == 20))
        })?;
        paint(cx, window, "twenty-rail")?;
        cx.run_until_parked();
        let baseline = render_image(cx, window)?;
        let first_row_y = adoken_theme::size::TITLEBAR
            + adoken_theme::space::S8
            + Nav::ALL.len() as f32 * (adoken_theme::size::CONTROL_MD + adoken_theme::space::S2)
            + adoken_theme::size::CONTROL_MD
            + adoken_theme::space::S2
            + adoken_theme::size::CONTROL_MD / 2.0;
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position: point(px(28.0), px(first_row_y)),
                    ..Default::default()
                }),
                cx,
            );
        })?;
        cx.run_until_parked();
        cx.advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        let hover = render_image(cx, window)?;
        let scale = baseline.width() / 1200;
        ensure!(
            image::imageops::crop_imm(&baseline, 64 * scale, 240 * scale, 232 * scale, 350 * scale)
                .to_image()
                != image::imageops::crop_imm(
                    &hover,
                    64 * scale,
                    240 * scale,
                    232 * scale,
                    350 * scale
                )
                .to_image(),
            "hover dwell reveals the expanded twenty-pin peek"
        );
        let content_start = 320 * scale;
        ensure!(
            image::imageops::crop_imm(
                &baseline,
                content_start,
                0,
                baseline.width() - content_start,
                baseline.height()
            )
            .to_image()
                == image::imageops::crop_imm(
                    &hover,
                    content_start,
                    0,
                    hover.width() - content_start,
                    hover.height()
                )
                .to_image(),
            "opening the peek does not reflow the canonical content"
        );
        paint(cx, window, "twenty-hover-peek")?;
        let unpin_position = point(
            px(
                adoken_theme::size::RAIL + adoken_theme::space::S4 + adoken_theme::size::SIDEBAR
                    - adoken_theme::space::S8
                    - adoken_theme::space::S2
                    - adoken_theme::size::CONTROL_SM / 2.0,
            ),
            px(first_row_y + adoken_theme::space::S8),
        );
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                    position: unpin_position,
                    ..Default::default()
                }),
                cx,
            );
        })?;
        paint(cx, window, "twenty-unpin-hover")?;
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Left,
                    position: unpin_position,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
            window.dispatch_event(
                gpui::PlatformInput::MouseUp(gpui::MouseUpEvent {
                    button: gpui::MouseButton::Left,
                    position: unpin_position,
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        })?;
        settle(cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &candidates[0].0))?
                .is_none())
        })
        .context("pointer unpin removes the hovered pin without opening it")?;
        press(cx, window, "cmd-z")?;
        settle(cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &candidates[0].0))?
                .is_some_and(|pin| pin.position == 0))
        })?;
        println!("pins_smoke: hovered unpin button removes its pin and Undo restores its position");
        press(cx, window, "escape")?;
        cx.update_window(window.into(), |_, window, cx| {
            window.dispatch_event(
                gpui::PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: gpui::MouseButton::Left,
                    position: point(px(28.0), px(first_row_y)),
                    click_count: 1,
                    ..Default::default()
                }),
                cx,
            );
        })?;
        for _ in 0..19 {
            press(cx, window, "down")?;
        }
        paint(cx, window, "twenty-keyboard-peek")?;
        press(cx, window, "enter")?;
        settle(cx, |_| {
            Ok(store
                .with_conn(|connection| adoken_store::read::pin(connection, &candidates[19].0))?
                .is_some_and(|pin| pin.opened_at.is_some()))
        })?;
        println!(
            "pins_smoke: twenty-pin hover peek, unchanged content geometry, keyboard scrolling and final-slot canonical open passed"
        );
        Ok(())
    }

    #[derive(Default)]
    struct MenuHost {
        input: Option<Entity<adoken_ui::TextInput>>,
        menu: adoken_ui::action_menu::MenuSlot,
    }

    impl MenuHost {
        fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            let owner = cx.entity().downgrade();
            self.menu.open(
                "Draft actions",
                vec![adoken_ui::action_menu::MenuItem::new(
                    "Clear draft",
                    move |_, cx| {
                        owner
                            .update(cx, |host, cx| {
                                host.input
                                    .as_ref()
                                    .unwrap()
                                    .update(cx, |input, cx| input.set_text("", cx));
                            })
                            .ok();
                    },
                )],
                adoken_ui::action_menu::MenuPosition::at_pointer(point(px(20.0), px(60.0))),
                adoken_theme::z::DROPDOWN,
                |host| &mut host.menu,
                window,
                cx,
            );
        }
    }

    impl Render for MenuHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .key_context("Workspace")
                .child(self.input.as_ref().unwrap().clone())
                .children(self.menu.element())
        }
    }

    fn menu_focus_smoke(cx: &mut HeadlessAppContext) -> anyhow::Result<()> {
        let window = cx.open_window(size(px(480.0), px(260.0)), |window, cx| {
            cx.new(|cx| {
                let input = cx.new(|cx| {
                    let mut input = adoken_ui::TextInput::new("Draft", cx);
                    input.set_text("draft kept", cx);
                    input
                });
                window.focus(&input.focus_handle(cx), cx);
                MenuHost {
                    input: Some(input),
                    ..Default::default()
                }
            })
        })?;
        window.update(cx, |host, window, cx| host.open_menu(window, cx))?;
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })?;
        window.update(cx, |host, window, cx| host.open_menu(window, cx))?;
        cx.run_until_parked();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(Keystroke::parse("escape").unwrap(), cx);
        })?;
        cx.run_until_parked();
        ensure!(
            window.update(cx, |host, window, cx| {
                let input = host.input.as_ref().unwrap();
                !host.menu.is_open()
                    && input.focus_handle(cx).is_focused(window)
                    && input.read(cx).text() == "draft kept"
            })?,
            "replacing then dismissing a row menu restores original input focus and draft"
        );
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        println!("pins_smoke: replacement row menu restores original focus and draft");
        Ok(())
    }

    fn settle(
        cx: &mut HeadlessAppContext,
        mut done: impl FnMut(&mut HeadlessAppContext) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        for _ in 0..500 {
            cx.run_until_parked();
            if done(cx)? {
                return Ok(());
            }
            cx.advance_clock(Duration::from_millis(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        bail!("timed out waiting for the pins store/controller transition")
    }

    fn press(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        keys: &str,
    ) -> anyhow::Result<()> {
        let keystroke = Keystroke::parse(keys).context("keystroke")?;
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.dispatch_keystroke(keystroke, cx);
        })?;
        cx.run_until_parked();
        Ok(())
    }

    /// Pins have no number keys (spec 0025 FR-2): ⌘J, ↓ × `index`, ↩.
    fn open_pin(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        index: usize,
    ) -> anyhow::Result<()> {
        press(cx, window, "cmd-j")?;
        for _ in 0..index {
            press(cx, window, "down")?;
        }
        press(cx, window, "enter")
    }

    fn render_image(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
    ) -> anyhow::Result<image::RgbaImage> {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
            window.render_to_image()
        })?
        .context("rendered shell image")
    }

    fn paint(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        name: &str,
    ) -> anyhow::Result<()> {
        let image = render_image(cx, window)?;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/pins-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
