//! The work item list's area / iteration dropdown (spec 0008 FR-8, AC-10,
//! AC-11 merge, AC-13) through the real shell on the demo org: real key
//! dispatch, the list it filters, focus, and screenshots in
//! `target/workitems-filter-smoke/`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("workitems_filter_smoke: skipped (headless rendering is macOS-only)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adoken_app::shell::{Nav, Shell, StoreSource};
    use adoken_app::workitems::model::{Tab, under};
    use adoken_app::workitems::{FilterWorkItemsByIteration, WorkItemsView};
    use adoken_model::{ClassNode, NodeKind};
    use adoken_store::WorkItemFilter;
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use anyhow::{Context as _, bail, ensure};
    use gpui::{
        AppContext as _, Entity, Focusable, HeadlessAppContext, Keystroke, WindowHandle, px, size,
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
        let now = adoken_demo::DEMO_NOW;
        let store = adoken_demo::open_store_at(dir.path(), now)?;
        let ds = adoken_demo::dataset_at(adoken_demo::DEFAULT_SEED, now);
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
        window.update(&mut cx, |shell, window, cx| {
            shell.show(Nav::WorkItems, window, cx)
        })?;
        settle(&mut cx, |cx| {
            Ok(cx.update(|cx| !view.read(cx).listed().0.is_empty()))
        })
        .context("the list loads")?;

        // ── By area: ← → reach it; `/` opens the dropdown ──────────────
        for _ in 0..3 {
            press(&mut cx, window, "right")?;
        }
        ensure!(
            cx.update(|cx| view.read(cx).list_filter().tab) == Tab::Area,
            "→ ×3 reaches By area"
        );
        render(&mut cx, window)?;
        let before = cx.update(|cx| view.read(cx).list_filter());
        let started = Instant::now();
        press(&mut cx, window, "/")?;
        render(&mut cx, window)?;
        let open_ms = started.elapsed().as_secs_f64() * 1000.0;
        ensure!(picker_open(&mut cx, &view), "/ opens the dropdown");
        paint(&mut cx, window, "area-open")?;
        let shown = visible(&mut cx, &view)?;
        println!(
            "workitems_filter_smoke: dropdown open + paint {open_ms:.1} ms, {} rows: {shown:?}",
            shown.len()
        );
        ensure!(
            shown.iter().any(|p| !p.contains('\\')),
            "project roots listed"
        );

        // ── Esc leaves the filter unchanged, keyboard back on the list ──
        type_text(&mut cx, window, "zzz")?;
        press(&mut cx, window, "escape")?;
        ensure!(!picker_open(&mut cx, &view), "Esc closes");
        ensure!(
            cx.update(|cx| view.read(cx).list_filter()) == before,
            "Esc changes nothing"
        );
        ensure!(
            list_focused(&mut cx, window, &view)?,
            "Esc: keyboard on the list"
        );

        // ── Type to jump: "infra" lists matches with their ancestors ───
        press(&mut cx, window, "/")?;
        type_text(&mut cx, window, "infra")?;
        let shown = visible(&mut cx, &view)?;
        ensure!(!shown.is_empty(), "infra matches");
        for path in &shown {
            ensure!(
                path.to_lowercase().contains("infra")
                    || shown.iter().any(|p| p != path && under(p, path)),
                "{path} is a match or an ancestor of one"
            );
        }
        paint(&mut cx, window, "area-infra")?;

        // The tree arrives while the dropdown is open: query and highlight
        // stay, the new node shows.
        let payments = ds
            .projects
            .iter()
            .find(|p| p.name == "Payments")
            .context("demo project Payments")?
            .clone();
        press(&mut cx, window, "down")?;
        let highlighted = picker_selected(&mut cx, &view)?;
        let mut tree = store
            .with_conn(|c| adoken_store::wi::class_tree(c, &payments.id))?
            .context("demo tree stored")?;
        tree.areas.push(ClassNode {
            kind: NodeKind::Area,
            path: "Payments\\Infra\\Edge".into(),
            name: "Edge".into(),
            depth: 2,
            start_ms: None,
            finish_ms: None,
        });
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.story_tree_arrived(&payments.id, tree, cx)
            })
        });
        settle(&mut cx, |cx| {
            Ok(visible(cx, &view)?
                .iter()
                .any(|p| p == "Payments\\Infra\\Edge"))
        })
        .context("the arriving tree merges under the same query")?;
        ensure!(
            picker_selected(&mut cx, &view)? == highlighted,
            "the highlight stays on {highlighted:?}"
        );
        paint(&mut cx, window, "area-merged")?;
        press(&mut cx, window, "escape")?;

        // ── Choose a parent node: the project root, node and below ─────
        press(&mut cx, window, "/")?;
        type_text(&mut cx, window, "Payments")?;
        ensure!(
            picker_selected(&mut cx, &view)?.as_deref() == Some("Payments"),
            "the root is the first match"
        );
        press(&mut cx, window, "enter")?;
        ensure!(!picker_open(&mut cx, &view), "↩ closes");
        let label = cx.update(|cx| view.read(cx).path_label());
        ensure!(
            label.as_deref() == Some("Payments and below"),
            "label: {label:?}"
        );
        ensure!(
            list_focused(&mut cx, window, &view)?,
            "↩: keyboard on the list"
        );
        let (listed, selected) = cx.update(|cx| view.read(cx).listed());
        let expected = store
            .work_items(&WorkItemFilter::default())?
            .into_iter()
            .filter(|i| i.is_open() && under(&i.area_path, "Payments"))
            .count();
        ensure!(
            listed.len() == expected,
            "lists exactly the open items under Payments: {} of {expected}",
            listed.len()
        );
        ensure!(selected == listed.first().copied(), "first item selected");
        println!(
            "workitems_filter_smoke: Payments and below lists {} open items, #{:?} selected",
            listed.len(),
            selected
        );
        paint(&mut cx, window, "area-payments-and-below")?;

        // ── `]` from a chosen parent starts at the busiest path ────────
        press(&mut cx, window, "]")?;
        let after = cx.update(|cx| view.read(cx).list_filter());
        ensure!(
            after.area.as_deref().is_some_and(|a| a.contains('\\')),
            "] steps to the busiest path: {:?}",
            after.area
        );

        // ── From Pull requests, "Filter by iteration…" ─────────────────
        window.update(&mut cx, |shell, window, cx| {
            shell.show(Nav::PullRequests, window, cx)
        })?;
        cx.run_until_parked();
        render(&mut cx, window)?;
        window.update(&mut cx, |_, window, cx| {
            window.dispatch_action(Box::new(FilterWorkItemsByIteration), cx);
        })?;
        cx.run_until_parked();
        render(&mut cx, window)?;
        let nav = window.update(&mut cx, |shell, _, _| shell.nav())?;
        ensure!(nav == Nav::WorkItems, "Work items shown: {nav:?}");
        ensure!(
            cx.update(|cx| view.read(cx).list_filter().tab) == Tab::Iteration,
            "By iteration"
        );
        ensure!(picker_open(&mut cx, &view), "with the dropdown open");
        paint(&mut cx, window, "iteration-from-pull-requests")?;
        press(&mut cx, window, "escape")?;

        cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        println!("workitems_filter_smoke: ok");
        Ok(())
    }

    fn picker_open(cx: &mut HeadlessAppContext, view: &Entity<WorkItemsView>) -> bool {
        cx.update(|cx| view.read(cx).path_picker_open())
    }

    fn visible(
        cx: &mut HeadlessAppContext,
        view: &Entity<WorkItemsView>,
    ) -> anyhow::Result<Vec<String>> {
        let picker = cx
            .update(|cx| view.read(cx).path_picker())
            .context("dropdown open")?;
        Ok(cx.update(|cx| picker.read(cx).visible_paths()))
    }

    fn picker_selected(
        cx: &mut HeadlessAppContext,
        view: &Entity<WorkItemsView>,
    ) -> anyhow::Result<Option<String>> {
        let picker = cx
            .update(|cx| view.read(cx).path_picker())
            .context("dropdown open")?;
        Ok(cx.update(|cx| picker.read(cx).selected_path()))
    }

    fn list_focused(
        cx: &mut HeadlessAppContext,
        window: WindowHandle<Shell>,
        view: &Entity<WorkItemsView>,
    ) -> anyhow::Result<bool> {
        window.update(cx, |_, window, cx| {
            view.read(cx).focus_handle(cx).is_focused(window)
        })
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
        bail!("timed out")
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
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/workitems-filter-smoke");
        std::fs::create_dir_all(&directory)?;
        image.save(directory.join(format!("{name}.png")))?;
        Ok(())
    }
}
