//! Keycap hint for a shortcut, e.g. `⌘P` on macOS or `Ctrl+P` elsewhere.
//! Keyboard-first: every action surfaced in UI shows its binding with this.

use adoken_theme::{radius, space, typography};
use gpui::{
    App, IntoElement, InvalidKeystrokeError, Keystroke, RenderOnce, SharedString, Window, div,
    prelude::*, px,
};

use crate::theme::{ActiveTheme, TypeStyleExt};

/// Which platform's conventions to format a shortcut with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPlatform {
    Mac,
    Other,
}

impl KeyPlatform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::Other
        }
    }
}

/// Format one keystroke for display.
///
/// macOS uses Apple's glyphs in Apple's modifier order (⌃⌥⇧⌘), no
/// separators. Other platforms spell modifiers out, `+`-separated, in the
/// Windows order (Ctrl, Alt, Shift, Win).
pub fn format_keystroke(keystroke: &Keystroke, platform: KeyPlatform) -> String {
    let m = &keystroke.modifiers;
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    match platform {
        KeyPlatform::Mac => {
            for (on, glyph) in [
                (m.function, "fn"),
                (m.control, "⌃"),
                (m.alt, "⌥"),
                (m.shift, "⇧"),
                (m.platform, "⌘"),
            ] {
                if on {
                    parts.push(glyph);
                }
            }
        }
        KeyPlatform::Other => {
            for (on, name) in [
                (m.control, "Ctrl"),
                (m.alt, "Alt"),
                (m.shift, "Shift"),
                (m.platform, "Win"),
                (m.function, "Fn"),
            ] {
                if on {
                    parts.push(name);
                }
            }
        }
    }
    let key = key_label(&keystroke.key, platform);
    match platform {
        KeyPlatform::Mac => {
            let mut out = parts.concat();
            out.push_str(&key);
            out
        }
        KeyPlatform::Other => {
            let mut out = parts.join("+");
            if !out.is_empty() {
                out.push('+');
            }
            out.push_str(&key);
            out
        }
    }
}

fn key_label(key: &str, platform: KeyPlatform) -> String {
    let mac = platform == KeyPlatform::Mac;
    let named = match key {
        "enter" => Some(if mac { "↩" } else { "Enter" }),
        "escape" => Some("Esc"),
        "backspace" => Some(if mac { "⌫" } else { "Backspace" }),
        "delete" => Some(if mac { "⌦" } else { "Del" }),
        "tab" => Some(if mac { "⇥" } else { "Tab" }),
        "space" => Some("Space"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "left" => Some("←"),
        "right" => Some("→"),
        "pageup" => Some("PgUp"),
        "pagedown" => Some("PgDn"),
        "home" => Some("Home"),
        "end" => Some("End"),
        _ => None,
    };
    match named {
        Some(label) => label.to_owned(),
        None => {
            let mut chars = key.chars();
            match (chars.next(), chars.next()) {
                // Single characters read as keycaps: `p` → `P`.
                (Some(c), None) => c.to_uppercase().collect(),
                // Function keys and anything else: `f5` → `F5`.
                _ => key.to_uppercase(),
            }
        }
    }
}

/// A rendered shortcut (one or more keystrokes, e.g. a `cmd-k cmd-t` chord).
#[derive(IntoElement)]
pub struct Kbd {
    label: SharedString,
}

impl Kbd {
    pub fn new(keystrokes: &[Keystroke]) -> Self {
        let platform = KeyPlatform::current();
        let label = keystrokes
            .iter()
            .map(|keystroke| format_keystroke(keystroke, platform))
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            label: label.into(),
        }
    }

    /// Parse gpui keybinding syntax: `"cmd-p"`, `"cmd-k cmd-t"`.
    pub fn parse(source: &str) -> Result<Self, InvalidKeystrokeError> {
        let keystrokes = source
            .split_whitespace()
            .map(Keystroke::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(&keystrokes))
    }

    /// An already formatted label (e.g. from [`format_keystroke`]).
    pub fn from_label(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
        }
    }

    pub fn label(&self) -> &SharedString {
        &self.label
    }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex_none()
            .px(px(space::S4))
            .rounded(px(radius::XS))
            .border_1()
            .border_color(theme.colors.border_default)
            .bg(theme.colors.bg_raised)
            .text_color(theme.colors.text_secondary)
            .type_style(typography::KBD, theme)
            .child(self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(source: &str, platform: KeyPlatform) -> String {
        format_keystroke(&Keystroke::parse(source).unwrap(), platform)
    }

    #[test]
    fn mac_uses_glyphs_in_apple_modifier_order() {
        assert_eq!(fmt("cmd-p", KeyPlatform::Mac), "⌘P");
        // Apple order is ⌃⌥⇧⌘ regardless of how the binding was written.
        assert_eq!(fmt("cmd-shift-alt-ctrl-k", KeyPlatform::Mac), "⌃⌥⇧⌘K");
        assert_eq!(fmt("enter", KeyPlatform::Mac), "↩");
        assert_eq!(fmt("cmd-backspace", KeyPlatform::Mac), "⌘⌫");
    }

    #[test]
    fn other_platforms_spell_modifiers_with_plus() {
        assert_eq!(fmt("ctrl-p", KeyPlatform::Other), "Ctrl+P");
        assert_eq!(
            fmt("shift-ctrl-alt-f5", KeyPlatform::Other),
            "Ctrl+Alt+Shift+F5"
        );
        assert_eq!(fmt("escape", KeyPlatform::Other), "Esc");
        assert_eq!(fmt("up", KeyPlatform::Other), "↑");
    }

    #[test]
    fn chords_are_space_separated_and_bad_syntax_is_rejected() {
        let label = Kbd::parse("cmd-k cmd-t").unwrap().label().to_string();
        let expected = match KeyPlatform::current() {
            KeyPlatform::Mac => "⌘K ⌘T",
            KeyPlatform::Other => "Win+K Win+T",
        };
        assert_eq!(label, expected);
        // A key followed by another component is not a keystroke.
        assert!(Kbd::parse("cmd-p-q").is_err());
    }
}
