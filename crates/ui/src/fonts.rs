//! Bundled fonts: JetBrains Mono v2.304 (OFL-1.1; `assets/fonts/jetbrains-mono`,
//! see `assets/manifest.toml`), the default code font (spec 0009 §3 Code font).
//!
//! Static Regular, Italic and Bold faces are compiled in and served by
//! [`crate::Assets`] under `fonts/`. [`register`] hands the borrowed bytes to
//! gpui's text system before the first frame: no copy, no file IO, and the
//! faces are only parsed when text first asks for the family.

use std::borrow::Cow;

use gpui::App;

/// `(asset path, bytes)` of every embedded face.
pub(crate) const EMBEDDED: [(&str, &[u8]); 3] = [
    (
        "fonts/jetbrains-mono-regular.ttf",
        include_bytes!("../../../assets/fonts/jetbrains-mono/jetbrains-mono-regular.ttf"),
    ),
    (
        "fonts/jetbrains-mono-italic.ttf",
        include_bytes!("../../../assets/fonts/jetbrains-mono/jetbrains-mono-italic.ttf"),
    ),
    (
        "fonts/jetbrains-mono-bold.ttf",
        include_bytes!("../../../assets/fonts/jetbrains-mono/jetbrains-mono-bold.ttf"),
    ),
];

/// Add the bundled faces to the text system. On error the caller renders
/// code in the system monospace face.
pub fn register(cx: &App) -> gpui::Result<()> {
    let fonts = EMBEDDED
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    cx.text_system().add_fonts(fonts)
}
