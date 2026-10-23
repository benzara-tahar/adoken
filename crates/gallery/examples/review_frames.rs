//! Spec 0004 AC-3 / §7 perf note: frame cost of the PR review diff on a
//! 5k-line file, scrolled at "maximum trackpad velocity" (a full viewport per
//! frame), in both layouts, drawn headless with the real Metal renderer.
//!
//! `cargo run -p adoken-gallery --example review_frames`
//!
//! Prints p50 / p95 / max of `Window::draw` (layout + prepaint + paint) per
//! layout against the 16 ms frame budget. Debug builds overstate the cost
//! (adoken crates at opt-level 0); the budget is judged on release numbers.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("review_frames: headless rendering is macOS-only for now");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use adoken_app::review::data::Layout;
    use adoken_app::review::{Part, story};
    use adoken_model::{
        ChangeType, Comment, LineRange, PrDetail, PrFileChange, PrThread, ThreadAnchor,
        ThreadStatus,
    };
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use gpui::{AppContext as _, HeadlessAppContext, px, size};

    const LINES: usize = 5_000;
    const FRAMES: usize = 240;
    const BUDGET: Duration = Duration::from_millis(16);

    /// Rust-ish source, deterministic.
    fn source(lines: usize) -> String {
        (0..lines)
            .map(|i| match i % 7 {
                0 => format!(
                    "pub fn handler_{i}(ctx: &Context, retries: u32) -> Result<(), Error> {{\n"
                ),
                1 => format!("    let value_{i} = ctx.lookup(\"key-{}\")?;\n", i % 97),
                2 => format!(
                    "    if value_{} > {} {{ return Err(Error::Limit); }}\n",
                    i - 1,
                    i % 1000
                ),
                3 => "    // Retry with jittered backoff.\n".to_owned(),
                4 => format!("    self.metrics.record({}, retries);\n", i % 13),
                5 => "    Ok(())\n".to_owned(),
                _ => "}\n".to_owned(),
            })
            .collect()
    }

    /// ~10% of lines edited or inserted, so most of the file shows.
    fn edited(src: &str) -> String {
        let mut out = String::with_capacity(src.len() + src.len() / 8);
        for (i, line) in src.lines().enumerate() {
            match i % 10 {
                3 => {
                    out.push_str(&line.replace("retries", "retry_budget"));
                    out.push('\n');
                }
                7 => {
                    out.push_str(line);
                    out.push_str("\n    tracing::debug!(\"inserted\");\n");
                }
                _ => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        out
    }

    fn thread(id: i64, line: u32, author: &str) -> PrThread {
        PrThread {
            id,
            pr_id: 1,
            status: ThreadStatus::Active,
            anchor: Some(ThreadAnchor {
                file_path: "/src/engine/handlers.rs".into(),
                right: Some(LineRange {
                    start: line,
                    end: line,
                }),
                left: None,
                iteration_blob: None,
            }),
            iteration: None,
            comments: vec![Comment {
                id: 1,
                parent_id: None,
                author: author.into(),
                body: "Does this need the jitter too?".into(),
                created: 0,
                updated: 0,
                is_system: false,
                is_deleted: false,
            }],
            updated: 0,
        }
    }

    pub fn run() -> anyhow::Result<()> {
        let ds = adoken_demo::dataset(adoken_demo::DEFAULT_SEED);
        let old = source(LINES);
        let new = edited(&old);
        let author = ds.people[1].id.clone();
        let detail = PrDetail {
            pr: ds.prs[0].clone(),
            iterations: Vec::new(),
            files: vec![PrFileChange {
                path: "/src/engine/handlers.rs".into(),
                original_path: None,
                change_type: ChangeType::Edit,
                old_content: Some("old".into()),
                new_content: Some("new".into()),
                change_tracking_id: Some(1),
            }],
            threads: (0..40)
                .map(|i| thread(i, (i as u32 + 1) * 130, &author))
                .collect(),
        };
        let blobs = HashMap::from([("old".to_owned(), old), ("new".to_owned(), new)]);

        let text_system = gpui_platform::current_platform(true).text_system();
        let mut cx = HeadlessAppContext::with_platform(
            text_system,
            Arc::new(adoken_ui::Assets),
            gpui_platform::current_headless_renderer,
        );
        cx.update(|cx| {
            cx.set_reduce_motion(true);
            let variant = ThemeRegistry::builtin().variants().next().expect("a theme");
            Theme::install(variant, cx);
        });

        println!("review_frames: {LINES}-line file, {FRAMES} frames per layout, 1440x900");
        for layout in [Layout::Unified, Layout::Split] {
            let (detail, blobs, people, me) = (
                detail.clone(),
                blobs.clone(),
                ds.people.clone(),
                ds.me.clone(),
            );
            let window = cx.open_window(size(px(1440.0), px(900.0)), move |window, cx| {
                cx.new(|cx| {
                    story::review_from(detail, blobs, people, me, 0, layout, Part::All, window, cx)
                })
            })?;
            cx.run_until_parked();
            let review = window.root(&mut cx)?;
            let rows = cx.update(|cx| review.read(cx).diff_rows());
            // ~48 rows fill the pane: jump a full viewport per frame.
            let stride = 48;
            let mut samples = Vec::with_capacity(FRAMES);
            for frame in 0..FRAMES {
                let row = (frame * stride) % rows.max(1);
                cx.update(|cx| review.update(cx, |r, cx| r.scroll_diff_to(row, cx)));
                let took = cx.update_window(window.into(), |_, window, cx| {
                    let started = Instant::now();
                    window.draw(cx).clear();
                    started.elapsed()
                })?;
                samples.push(took);
            }
            cx.update_window(window.into(), |_, window, _| window.remove_window())?;
            samples.sort_unstable();
            let pick = |p: f64| samples[((samples.len() - 1) as f64 * p).round() as usize];
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            println!(
                "  {layout:?}: {rows} rows · draw p50 {:.2} ms · p95 {:.2} ms · max {:.2} ms · budget {} ms → {}",
                ms(pick(0.5)),
                ms(pick(0.95)),
                ms(pick(1.0)),
                BUDGET.as_millis(),
                if pick(0.95) <= BUDGET { "ok" } else { "OVER" },
            );
        }
        Ok(())
    }
}
