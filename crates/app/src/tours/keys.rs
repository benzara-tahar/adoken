//! Key references in tour copy, rendered from the live keymap (spec 0020
//! FR-12) with the cheat sheet's formatter, plus their spoken form
//! (FR-18: "Shift V").

use adoken_ui::tour::Segment;
use adoken_ui::{KeyPlatform, format_keystroke};
use gpui::{KeyBinding, KeyContext, Keystroke};

use super::defs::{Part, parse};

/// The binding a key reference shows: among the bindings of `action` that
/// apply in `contexts`, the documented `keys` when still bound, else the
/// highest-precedence one (the last added). `None` when nothing binds it
/// there (the anchor test fails on that, FR-12).
pub fn binding<'a>(
    bindings: impl DoubleEndedIterator<Item = &'a KeyBinding>,
    action: &str,
    keys: &str,
    contexts: &[KeyContext],
) -> Option<Vec<Keystroke>> {
    let documented: Option<Vec<Keystroke>> = keys
        .split_whitespace()
        .map(|k| Keystroke::parse(k).ok())
        .collect();
    let mut fallback = None;
    for binding in bindings.rev() {
        if binding.action().name() != action {
            continue;
        }
        if let Some(predicate) = binding.predicate()
            && predicate.depth_of(contexts).is_none()
        {
            continue;
        }
        let strokes: Vec<Keystroke> = binding
            .keystrokes()
            .iter()
            .map(|k| k.inner().clone())
            .collect();
        if documented
            .as_ref()
            .is_some_and(|documented| same_keys(documented, &strokes))
        {
            return Some(strokes);
        }
        fallback.get_or_insert(strokes);
    }
    fallback
}

fn same_keys(a: &[Keystroke], b: &[Keystroke]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            format_keystroke(a, KeyPlatform::Mac) == format_keystroke(b, KeyPlatform::Mac)
        })
}

/// The tour's view contexts, outermost first.
pub fn contexts(names: &[&str]) -> Vec<KeyContext> {
    names
        .iter()
        .filter_map(|name| KeyContext::parse(name).ok())
        .collect()
}

/// A keycap segment for `strokes`.
pub fn key_segment(strokes: &[Keystroke]) -> Segment {
    let platform = KeyPlatform::current();
    let strokes: Vec<Keystroke> = strokes.iter().map(as_typed).collect();
    Segment::Key {
        label: strokes
            .iter()
            .map(|k| format_keystroke(k, platform))
            .collect::<Vec<_>>()
            .join(" ")
            .into(),
        spoken: strokes
            .iter()
            .map(spoken)
            .collect::<Vec<_>>()
            .join(", ")
            .into(),
    }
}

/// A shifted digit bound by its character (`!`) reads as the keys the user
/// presses (`⇧1`), as the Inbox's own filter chips show it.
fn as_typed(keystroke: &Keystroke) -> Keystroke {
    const SHIFTED: [(&str, &str); 10] = [
        ("!", "1"),
        ("@", "2"),
        ("#", "3"),
        ("$", "4"),
        ("%", "5"),
        ("^", "6"),
        ("&", "7"),
        ("*", "8"),
        ("(", "9"),
        (")", "0"),
    ];
    match SHIFTED.iter().find(|(symbol, _)| *symbol == keystroke.key) {
        Some((_, digit)) if !keystroke.modifiers.modified() => {
            let mut typed = keystroke.clone();
            typed.key = (*digit).to_owned();
            typed.modifiers.shift = true;
            typed.key_char = None;
            typed
        }
        _ => keystroke.clone(),
    }
}

/// "Command Shift D", "Shift V", "J", "Return".
pub fn spoken(keystroke: &Keystroke) -> String {
    let m = &keystroke.modifiers;
    let mut words: Vec<String> = Vec::new();
    if m.control {
        words.push("Control".into());
    }
    if m.alt {
        words.push("Option".into());
    }
    if m.platform {
        words.push(
            if cfg!(target_os = "macos") {
                "Command"
            } else {
                "Windows"
            }
            .into(),
        );
    }
    if m.shift {
        words.push("Shift".into());
    }
    let key = match keystroke.key.as_str() {
        "enter" => "Return".to_owned(),
        "escape" => "Escape".to_owned(),
        "backspace" => "Delete".to_owned(),
        "space" => "Space".to_owned(),
        "tab" => "Tab".to_owned(),
        "left" => "Left arrow".to_owned(),
        "right" => "Right arrow".to_owned(),
        "up" => "Up arrow".to_owned(),
        "down" => "Down arrow".to_owned(),
        "[" => "Left bracket".to_owned(),
        "]" => "Right bracket".to_owned(),
        "," => "Comma".to_owned(),
        "/" => "Slash".to_owned(),
        "?" => "Question mark".to_owned(),
        key => key.to_uppercase(),
    };
    words.push(key);
    words.join(" ")
}

/// A body template as segments, keys from `resolve` (the live keymap in
/// the app). An unresolvable reference shows its documented keys rather
/// than a blank keycap.
pub fn segments(
    body: &str,
    mut resolve: impl FnMut(&str, &str) -> Option<Vec<Keystroke>>,
) -> Vec<Segment> {
    let mut key = |action: &str, keys: &str| {
        let strokes = resolve(action, keys).unwrap_or_else(|| {
            keys.split_whitespace()
                .filter_map(|k| Keystroke::parse(k).ok())
                .collect()
        });
        key_segment(&strokes)
    };
    let mut out = Vec::new();
    for part in parse(body) {
        match part {
            Part::Text(text) => match out.last_mut() {
                Some(Segment::Text(previous)) => {
                    *previous = format!("{previous}{text}").into();
                }
                _ => out.push(Segment::Text(text.to_owned().into())),
            },
            Part::Code(code) => out.push(Segment::Code(code.to_owned().into())),
            Part::Key { action, keys } => out.push(key(action, keys)),
            Part::Range { from, to } => {
                let (
                    Segment::Key {
                        label: a,
                        spoken: sa,
                    },
                    Segment::Key {
                        label: b,
                        spoken: sb,
                    },
                ) = (key(from.0, from.1), key(to.0, to.1))
                else {
                    continue;
                };
                out.push(Segment::Key {
                    label: format!("{a}–{b}").into(),
                    spoken: format!("{sa} to {sb}").into(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    gpui::actions!(tour_keys_test, [Next, Other]);

    fn label(segment: &Segment) -> &str {
        match segment {
            Segment::Key { label, .. } => label,
            Segment::Text(text) | Segment::Code(text) => text,
        }
    }

    #[test]
    fn documented_binding_wins_then_the_last_applicable_one() {
        let bindings = [
            KeyBinding::new("j", Next, Some("PrReview")),
            KeyBinding::new("down", Next, Some("PrReview")),
            KeyBinding::new("x", Next, Some("Inbox")),
        ];
        let review = contexts(&["Workspace", "PrReview"]);
        let strokes = |keys| binding(bindings.iter(), "tour_keys_test::Next", keys, &review);
        assert_eq!(strokes("j").unwrap()[0].key, "j");
        // `j` rebound away: the live binding shows instead.
        assert_eq!(strokes("n").unwrap()[0].key, "down");
        // Bound only in another view's context.
        let today = contexts(&["Workspace", "Today"]);
        assert!(binding(bindings.iter(), "tour_keys_test::Next", "x", &today).is_none());
        assert!(binding(bindings.iter(), "tour_keys_test::Other", "o", &review).is_none());
    }

    #[test]
    fn segments_render_keys_ranges_and_spoken_forms() {
        let body = "[a::B|shift-v] votes, [a::One|1]–[a::Five|5] filter.";
        let segments = segments(body, |_, keys| Some(vec![Keystroke::parse(keys).unwrap()]));
        let labels: Vec<&str> = segments.iter().map(label).collect();
        assert_eq!(labels, ["⇧V", " votes, ", "1–5", " filter."]);
        assert_eq!(
            adoken_ui::tour::spoken(&segments),
            "Shift V votes, 1 to 5 filter."
        );
        assert_eq!(
            spoken(&Keystroke::parse("cmd-shift-d").unwrap()),
            "Command Shift D"
        );
    }
}
