//! Guided tours' anchor, key and copy coverage (spec 0020 AC-7, FR-10,
//! FR-12): every step's anchor paints visibly (FR-8) in its tour's
//! declared gallery story; every key reference resolves to a binding of
//! the app keymap in the step's view context; and the copy keeps to the
//! limits (title ≤ 32 characters, body ≤ 140 with keys counted as their
//! label, sentence case, no `!` outside literal query text, no emoji, ≤ 5
//! key references with a range counting once, 3–5 steps per tour).
//! Headless, macOS only, like `snapshots.rs`.

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::run()
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tour_anchors: skipped (headless rendering is macOS-only for now)");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use adoken_app::tours::defs::{Part, Step, parse};
    use adoken_app::tours::{TourId, keys};
    use adoken_gallery::{STORIES, Story};
    use adoken_theme::ThemeRegistry;
    use adoken_ui::Theme;
    use adoken_ui::tour::Segment;
    use anyhow::{Context as _, bail};
    use gpui::{
        AppContext as _, Bounds, Context, HeadlessAppContext, IntoElement, Render, Window, div,
        point, prelude::*, px, size,
    };

    const TITLE_MAX: usize = 32;
    const BODY_MAX: usize = 140;
    const KEYS_MAX: usize = 5;
    /// Proper nouns and scope names that keep their capital mid-sentence.
    const CAPITALIZED: [&str; 5] = ["Azure", "DevOps", "Code", "Read", "Work"];

    pub fn run() -> anyhow::Result<()> {
        let mut failures = Vec::new();
        copy_and_keys(&mut failures);
        anchors(&mut failures)?;
        if !failures.is_empty() {
            bail!(
                "{} tour coverage problems:\n{}",
                failures.len(),
                failures.join("\n")
            );
        }
        println!("tour_anchors: ok");
        Ok(())
    }

    fn copy_and_keys(failures: &mut Vec<String>) {
        let bindings = adoken_app::key_bindings();
        for tour in TourId::ALL {
            let def = tour.tour();
            if !(3..=5).contains(&def.steps.len()) {
                failures.push(format!("{}: {} steps", tour.id(), def.steps.len()));
            }
            let contexts = keys::contexts(def.contexts);
            for (ix, step) in def.steps.iter().enumerate() {
                let at = format!("{} step {}", tour.id(), ix + 1);
                lint_title(&at, step, failures);
                for body in std::iter::once(step.body).chain(step.read_only) {
                    let parts = parse(body);
                    let references = parts
                        .iter()
                        .filter(|p| matches!(p, Part::Key { .. } | Part::Range { .. }))
                        .count();
                    if references > KEYS_MAX {
                        failures.push(format!("{at}: {references} key references"));
                    }
                    for part in &parts {
                        let refs: Vec<(&str, &str)> = match part {
                            Part::Key { action, keys } => vec![(action, keys)],
                            Part::Range { from, to } => vec![*from, *to],
                            Part::Text(text) if text.contains('[') => {
                                failures
                                    .push(format!("{at}: malformed key reference near {text:?}"));
                                Vec::new()
                            }
                            _ => Vec::new(),
                        };
                        for (action, documented) in refs {
                            if keys::binding(bindings.iter(), action, documented, &contexts)
                                .is_none()
                            {
                                failures.push(format!(
                                    "{at}: {action} has no binding in {:?}",
                                    def.contexts
                                ));
                            }
                        }
                    }
                    let segments = keys::segments(body, |action, documented| {
                        keys::binding(bindings.iter(), action, documented, &contexts)
                    });
                    lint_body(&at, &segments, failures);
                }
            }
        }
    }

    fn lint_title(at: &str, step: &Step, failures: &mut Vec<String>) {
        let title = step.title;
        if title.chars().count() > TITLE_MAX {
            failures.push(format!(
                "{at}: title {title:?} is over {TITLE_MAX} characters"
            ));
        }
        if !title.chars().next().is_some_and(char::is_uppercase) {
            failures.push(format!("{at}: title {title:?} starts lowercase"));
        }
        for word in title.split_whitespace().skip(1) {
            if word.chars().next().is_some_and(char::is_uppercase) && !CAPITALIZED.contains(&word) {
                failures.push(format!("{at}: title {title:?} isn't sentence case"));
            }
        }
        if title.contains('!') || title.chars().any(is_emoji) {
            failures.push(format!("{at}: title {title:?} has `!` or an emoji"));
        }
    }

    fn lint_body(at: &str, segments: &[Segment], failures: &mut Vec<String>) {
        let mut length = 0;
        // Whether the next word starts a sentence.
        let mut sentence_start = true;
        for segment in segments {
            match segment {
                Segment::Key { label, .. } => {
                    length += label.chars().count();
                    sentence_start = false;
                }
                Segment::Code(code) => {
                    length += code.chars().count();
                    sentence_start = false;
                }
                Segment::Text(text) => {
                    length += text.chars().count();
                    if text.contains('!') {
                        failures.push(format!("{at}: `!` outside literal query text"));
                    }
                    if text.chars().any(is_emoji) {
                        failures.push(format!("{at}: emoji in the body"));
                    }
                    for word in text.split_inclusive(char::is_whitespace) {
                        let trimmed = word.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        let bare = trimmed.trim_matches(|c: char| !c.is_alphanumeric());
                        if !sentence_start
                            && bare.chars().next().is_some_and(char::is_uppercase)
                            && !CAPITALIZED.contains(&bare)
                        {
                            failures.push(format!("{at}: {bare:?} capitalized mid-sentence"));
                        }
                        sentence_start = trimmed.ends_with(['.', '?', ':']);
                    }
                }
            }
        }
        if length > BODY_MAX {
            failures.push(format!(
                "{at}: body is {length} characters (max {BODY_MAX})"
            ));
        }
    }

    fn is_emoji(c: char) -> bool {
        matches!(u32::from(c), 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0xFE0F)
    }

    /// The story with an anchor-committing layer over it.
    struct Probe {
        story: &'static Story,
    }

    impl Render for Probe {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let viewport = Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
            div()
                .size_full()
                .child((self.story.render)(window, cx))
                .child(adoken_ui::tour::layer(viewport).into_deferred())
        }
    }

    fn anchors(failures: &mut Vec<String>) -> anyhow::Result<()> {
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
        });
        let _recording = cx.update(adoken_ui::tour::record);
        for tour in TourId::ALL {
            let def = tour.tour();
            let story = STORIES
                .iter()
                .find(|s| s.name == def.story)
                .with_context(|| format!("{}: no story {:?}", tour.id(), def.story))?;
            let window = cx.open_window(size(px(story.size.0), px(story.size.1)), |_, cx| {
                cx.new(|_| Probe { story })
            })?;
            for _ in 0..3 {
                cx.run_until_parked();
                cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear())?;
            }
            let window_id = window.window_id();
            let painted = cx.update(|cx| adoken_ui::tour::anchors(window_id, cx));
            let content = Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(story.size.0), px(story.size.1)),
            );
            for (ix, step) in def.steps.iter().enumerate() {
                if adoken_ui::tour::visible(&painted, step.anchor, content).is_none() {
                    let seen: Vec<String> = painted
                        .iter()
                        .filter(|(id, _)| *id == step.anchor)
                        .map(|(_, rect)| format!("{:?} clip {:?}", rect.bounds, rect.clip))
                        .collect();
                    failures.push(format!(
                        "{} step {}: anchor {} not visible in {} (painted: {seen:?})",
                        tour.id(),
                        ix + 1,
                        step.anchor,
                        def.story
                    ));
                }
            }
            cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        }
        Ok(())
    }
}
