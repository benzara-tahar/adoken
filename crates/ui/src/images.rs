//! Embedded illustrations, served by [`crate::Assets`] under `images/`.
//! Paint one with `gpui::img(path)`: gpui decodes it off the UI thread.
//! Provenance and sizes: `assets/manifest.toml` (`kind = "illustration"`).

/// Today's day-summary art, one per mood (spec 0017 FR-7, FR-30).
pub const TODAY_FRESH_START: &str = "images/today/fresh-start.webp";
pub const TODAY_FINDING_FLOW: &str = "images/today/finding-flow.webp";
pub const TODAY_HELPING_GROW: &str = "images/today/helping-grow.webp";
pub const TODAY_WAITING: &str = "images/today/waiting.webp";
pub const TODAY_BLOCKED: &str = "images/today/blocked.webp";
pub const TODAY_SHIPPED: &str = "images/today/shipped.webp";
pub const TODAY_CLEAR: &str = "images/today/clear.webp";
pub const TODAY_STALLED: &str = "images/today/stalled.webp";

/// The first onboarding screen (spec 0001 FR-19).
pub const ONBOARDING_WELCOME: &str = "images/onboarding/welcome.webp";

macro_rules! embedded {
    ($($path:ident => $file:literal),+ $(,)?) => {
        pub(crate) const EMBEDDED: &[(&str, &[u8])] = &[
            $(($path, include_bytes!(concat!("../../../assets/", $file))),)+
        ];
    };
}

embedded! {
    TODAY_FRESH_START => "images/today/fresh-start.webp",
    TODAY_FINDING_FLOW => "images/today/finding-flow.webp",
    TODAY_HELPING_GROW => "images/today/helping-grow.webp",
    TODAY_WAITING => "images/today/waiting.webp",
    TODAY_BLOCKED => "images/today/blocked.webp",
    TODAY_SHIPPED => "images/today/shipped.webp",
    TODAY_CLEAR => "images/today/clear.webp",
    TODAY_STALLED => "images/today/stalled.webp",
    ONBOARDING_WELCOME => "images/onboarding/welcome.webp",
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each served path is the file it embeds, so a renamed asset can't serve
    /// the wrong art.
    #[test]
    fn paths_match_files() {
        for (path, bytes) in EMBEDDED {
            let file = std::fs::read(format!(
                "{}/../../assets/{path}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap();
            assert_eq!(file.as_slice(), *bytes, "{path}");
        }
    }
}
