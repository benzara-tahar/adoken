//! Work item editing through the real shell on the demo org (spec 0015
//! slices A + B): real key dispatch, focused editors, the demo write path
//! (`apply_local`), store effects, frame timings and screenshots in
//! `target/workitems-smoke/`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("workitems_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::Duration;

    use adoken_app::palette::catalog::Target;
    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::workitems::WorkItemsView;
    use adoken_app::workitems::detail::model::Stop;
    use adoken_demo::rich::{MARKDOWN, SCENARIO};
    use adoken_store::Store;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{AppContext as _, Entity, HeadlessAppContext, Keystroke, WindowHandle, px, size};

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
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir.path(), now)?;
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, now);
        let scenario = ds.work_items[SCENARIO].id;
        let teammate = ds
            .people
            .iter()
            .find(|p| p.display_name == "Priya Shah")
            .context("demo teammate")?
            .clone();
        let window = cx.open_window(size(px(1440.0), px(900.0)), |window, cx| {
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
            window.update(cx, |shell, _, _| shell.work_items().is_some())
        })?;
        let view = window
            .update(&mut cx, |shell, _, _| shell.work_items().cloned())?
            .context("work items view")?;
        cx.update(|cx| {
            if let Some(edits) = adoken_app::workitems::writes::WiEdits::global(cx) {
                edits.update(cx, |edits, cx| edits.set_clock(now, cx));
            }
        });
        let mut frames = adoken_perf::FrameRecorder::start();

        open(&mut cx, window, &view, scenario)?;
        paint(&mut cx, window, "detail")?;

        // ── state (FR-11): s, then ↩ on the next state ─────────────────
        let before = item(&store, scenario)?.state;
        press(&mut cx, window, "s")?;
        ensure!(view_picker_open(&mut cx, &view)?, "s opens the state menu");
        paint(&mut cx, window, "state-menu")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |_| Ok(item(&store, scenario)?.state != before))
            .context("state change applies to the demo store")?;
        println!(
            "workitems_smoke: state {before} → {}",
            item(&store, scenario)?.state
        );

        // ── assign (FR-13): a, "pri", ↩ ───────────────────────────────
        press(&mut cx, window, "a")?;
        ensure!(
            view_picker_open(&mut cx, &view)?,
            "a opens the people picker"
        );
        type_text(&mut cx, window, "pri")?;
        paint(&mut cx, window, "assign-picker")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |_| {
            Ok(item(&store, scenario)?.assigned_to.as_deref() == Some(teammate.id.as_str()))
        })
        .context("assignment applies")?;

        // ── title (FR-12), keys inert while typing (AC-25) ─────────────
        press(&mut cx, window, "t")?;
        type_text(&mut cx, window, "Settlement uses the merchant's day")?;
        ensure!(
            !view_picker_open(&mut cx, &view)?,
            "typing s in the title editor opened nothing"
        );
        let typed = cx.update(|cx| view.read(cx).title_editor_text(cx));
        ensure!(
            typed.as_deref() == Some("Settlement uses the merchant's day"),
            "letters type into the title: {typed:?}"
        );
        paint(&mut cx, window, "title-editor")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |_| {
            Ok(item(&store, scenario)?.title == "Settlement uses the merchant's day")
        })
        .context("title applies")?;
        // Empty titles are refused locally (AC-11).
        press(&mut cx, window, "t")?;
        press(&mut cx, window, "cmd-a")?;
        press(&mut cx, window, "backspace")?;
        press(&mut cx, window, "enter")?;
        let error = cx.update(|cx| view.read(cx).title_error());
        ensure!(
            error == Some("Title can't be empty"),
            "empty title refused: {error:?}"
        );
        press(&mut cx, window, "escape")?;
        ensure!(
            item(&store, scenario)?.title == "Settlement uses the merchant's day",
            "Esc sends nothing"
        );

        // ── comment with a mention (FR-25, FR-26) ──────────────────────
        press(&mut cx, window, "c")?;
        type_text(&mut cx, window, "@pri")?;
        ensure!(
            view_picker_open(&mut cx, &view)?,
            "@ opens the mention list"
        );
        paint(&mut cx, window, "mention")?;
        press(&mut cx, window, "enter")?;
        type_text(&mut cx, window, "please verify")?;
        let markdown = cx.update(|cx| view.read(cx).composer_markdown(cx));
        let token = format!("@<{}>", teammate.id);
        ensure!(
            markdown.as_deref() == Some(format!("{token} please verify").as_str()),
            "mention chip serializes as @<id>: {markdown:?}"
        );
        press(&mut cx, window, "cmd-enter")?;
        settle(&mut cx, |_| {
            Ok(store
                .with_conn(|c| adoken_store::wi::comments(c, scenario))?
                .iter()
                .any(|c| c.source.as_deref().is_some_and(|s| s.contains(&token))))
        })
        .context("comment posts with the mention")?;
        press(&mut cx, window, "escape")?;

        // ── add and remove a Related link (FR-21, FR-22) ───────────────
        let target = ds.work_items[SCENARIO + 10].id;
        focus_detail(&mut cx, window, &view)?;
        press(&mut cx, window, "l")?;
        // Related is the third relation; menus have no digit picks (0025 FR-3).
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "down")?;
        press(&mut cx, window, "enter")?;
        type_text(&mut cx, window, &format!("#{target}"))?;
        settle(&mut cx, |_| Ok(true))?;
        paint(&mut cx, window, "link-picker")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).link_row_of(target)).is_some())
        })
        .context("the link lands")?;
        let row = cx
            .update(|cx| view.read(cx).link_row_of(target))
            .context("link row")?;
        window.update(&mut cx, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.story_cursor(Stop::Link(row), window, cx)
            })
        })?;
        press(&mut cx, window, "backspace")?;
        paint(&mut cx, window, "remove-confirm")?;
        press(&mut cx, window, "enter")?;
        settle(&mut cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).link_row_of(target)).is_none())
        })
        .context("the link is removed")?;

        // ── description: Write → Preview → save (FR-17) ────────────────
        let markdown_item = ds.work_items[MARKDOWN].id;
        open(&mut cx, window, &view, markdown_item)?;
        press(&mut cx, window, "d")?;
        type_text(&mut cx, window, "Signed off")?;
        let editing = cx.update(|cx| view.read(cx).description_editor(cx));
        ensure!(
            editing
                .as_ref()
                .is_some_and(|(preview, text)| !preview && text.ends_with("Signed off")),
            "d opens the Markdown source and letters type (AC-13, AC-25)"
        );
        press(&mut cx, window, "cmd-e")?;
        settle(&mut cx, |_| Ok(true))?;
        paint(&mut cx, window, "description-preview")?;
        ensure!(
            cx.update(|cx| view.read(cx).description_editor(cx))
                .is_some_and(|(preview, _)| preview),
            "⌘E shows the Preview"
        );
        press(&mut cx, window, "cmd-e")?;
        press(&mut cx, window, "cmd-enter")?;
        settle(&mut cx, |_| {
            Ok(store
                .with_conn(|c| adoken_store::wi::extras(c, markdown_item))?
                .and_then(|e| e.description)
                .is_some_and(|d| d.source.ends_with("Signed off")))
        })
        .context("description saves")?;

        // ── FR-28 / AC-8: read-only token: edit commands listed disabled
        // with the reason; confirming runs nothing ─────────────────────
        cx.update(|cx| {
            adoken_app::workitems::writes::set_access(
                adoken_app::workitems::writes::Access::None,
                cx,
            )
        });
        let state_before = item(&store, markdown_item)?.state;
        press(&mut cx, window, "cmd-p")?;
        window.update(&mut cx, |shell, _, cx| {
            shell
                .palette()
                .update(cx, |palette, cx| palette.set_query(">Change state", cx));
        })?;
        settle(&mut cx, |cx| {
            window.update(cx, |shell, _, cx| {
                shell
                    .palette()
                    .read(cx)
                    .selected_title()
                    .contains("Change state")
            })
        })
        .context("the command is listed")?;
        let gate = window.update(&mut cx, |shell, _, cx| {
            shell.palette().read(cx).selected_gate(cx)
        })?;
        ensure!(
            gate.as_deref() == Some(adoken_app::workitems::writes::READ_ONLY_HINT),
            "listed disabled with the reason: {gate:?}"
        );
        paint(&mut cx, window, "palette-read-only")?;
        press(&mut cx, window, "enter")?;
        ensure!(
            window.update(&mut cx, |shell, _, cx| shell
                .palette()
                .read(cx)
                .is_visible())?,
            "confirming a gated command keeps the palette open"
        );
        ensure!(
            !view_picker_open(&mut cx, &view)?,
            "confirming a gated command dispatches nothing"
        );
        press(&mut cx, window, "escape")?;
        ensure!(
            item(&store, markdown_item)?.state == state_before,
            "nothing was sent"
        );
        cx.update(|cx| {
            adoken_app::workitems::writes::set_access(
                adoken_app::workitems::writes::Access::Local,
                cx,
            )
        });

        // ── sync this item (FR-30): demo explains, nothing breaks ─────
        press(&mut cx, window, "cmd-shift-r")?;
        paint(&mut cx, window, "after-sync")?;

        frames.collect();
        let summary = frames.draw().summary();
        if let Some(summary) = &summary {
            println!(
                "workitems_smoke frames: n={}, p50={:?}, p99={:?}",
                summary.count, summary.p50, summary.p99
            );
            adoken_perf::ensure_budget!(
                summary.p99 < Duration::from_millis(16),
                "frame p99 under 16 ms: {:?}",
                summary.p99
            );
        }
        drop(frames);
        ensure!(
            window.update(&mut cx, |shell, _, _| shell.nav())? == Nav::WorkItems,
            "stays in Work items"
        );
        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        println!(
            "workitems_smoke: state, assign, title (+ empty refusal), comment with mention, add/remove link, description Write→Preview→save, sync this item and inert keys in editors passed"
        );
        Ok(())
    }

    fn item(store: &Store, id: i64) -> anyhow::Result<adoken_model::WorkItem> {
        store
            .with_conn(|c| adoken_store::read::work_item(c, id))?
            .context("stored work item")
    }

    fn open(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        view: &Entity<WorkItemsView>,
        id: i64,
    ) -> anyhow::Result<()> {
        window.update(cx, |shell, window, cx| {
            shell.show(Nav::WorkItems, window, cx);
            shell.open_target(Target::WorkItem(id), window, cx);
        })?;
        settle(cx, |cx| {
            Ok(cx.update(|cx| view.read(cx).detail_item().is_some_and(|i| i.id == id)))
        })
        .context("detail loads")?;
        // One paint so pickers anchor to real cells.
        render(cx, window)?;
        Ok(())
    }

    fn focus_detail(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        view: &Entity<WorkItemsView>,
    ) -> anyhow::Result<()> {
        window.update(cx, |_, window, cx| {
            view.update(cx, |view, cx| view.story_cursor(Stop::Title, window, cx))
        })?;
        Ok(())
    }

    fn view_picker_open(
        cx: &mut HeadlessAppContext,
        view: &Entity<WorkItemsView>,
    ) -> anyhow::Result<bool> {
        Ok(cx.update(|cx| view.read(cx).picker_open()))
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
        bail!("timed out waiting for the work item transition")
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

    fn type_text(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        text: &str,
    ) -> anyhow::Result<()> {
        for ch in text.chars() {
            let key = match ch {
                ' ' => "space".to_owned(),
                c if c.is_ascii_uppercase() => format!("shift-{}", c.to_ascii_lowercase()),
                '#' => "shift-3".to_owned(),
                '@' => "shift-2".to_owned(),
                '\'' => "'".to_owned(),
                c => c.to_string(),
            };
            let mut keystroke = Keystroke::parse(&key).context("keystroke")?;
            keystroke.key_char = Some(ch.to_string());
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear();
                window.dispatch_keystroke(keystroke, cx);
            })?;
            cx.run_until_parked();
        }
        Ok(())
    }

    fn render(
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
        let image = render(cx, window)?;
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/workitems-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
