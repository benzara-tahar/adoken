//! Embedded Lucide icons and the gpui [`AssetSource`] that serves them.
//!
//! Icons come from `assets/icons/lucide` (Lucide 1.48.0, ISC; see
//! `assets/manifest.toml`). Only the glyphs the UI uses are compiled in.
//! Lucide draws with `stroke="currentColor"`; gpui renders SVGs as an alpha
//! mask tinted with the element's text color, so every icon takes its color
//! from a theme token at the call site.
//!
//! The embed-and-serve pattern (`include_bytes!` per icon behind one
//! `AssetSource`) follows zeron `crates/ui/src/icons.rs` at `d1010657`.

use std::borrow::Cow;

use gpui::{
    App, AssetSource, Hsla, IntoElement, RenderOnce, Result, SharedString, Transformation, Window,
    prelude::*, px, svg,
};

macro_rules! icons {
    ($($variant:ident => $file:literal,)+) => {
        /// Every icon compiled into the binary.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum IconName {
            $($variant,)+
        }

        impl IconName {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// Asset path served by [`Assets`].
            pub const fn path(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("icons/", $file, ".svg"),)+
                }
            }

            fn bytes(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => include_bytes!(concat!(
                        "../../../assets/icons/lucide/", $file, ".svg"
                    )),)+
                }
            }
        }
    };
}

icons! {
    Check => "check",
    ChevronLeft => "chevron-left",
    CircleAlert => "circle-alert",
    CircleCheck => "circle-check",
    CircleDashed => "circle-dashed",
    CircleX => "circle-x",
    Copy => "copy",
    ExternalLink => "external-link",
    Eye => "eye",
    EyeOff => "eye-off",
    FolderKanban => "folder-kanban",
    GitPullRequest => "git-pull-request",
    KeyRound => "key-round",
    LoaderCircle => "loader-circle",
    Lock => "lock",
    RefreshCw => "refresh-cw",
    Rocket => "rocket",
    Square => "square",
    SquareCheck => "square-check",
    Terminal => "terminal",
    Users => "users",
    WifiOff => "wifi-off",
    // PR review (spec 0004; vote/pr icons per tokens.md §2.8).
    BadgeCheck => "badge-check",
    CircleCheckBig => "circle-check-big",
    Clock => "clock",
    OctagonX => "octagon-x",
    ChevronRight => "chevron-right",
    ChevronDown => "chevron-down",
    GitMerge => "git-merge",
    GitPullRequestDraft => "git-pull-request-draft",
    GitPullRequestClosed => "git-pull-request-closed",
    MessageSquare => "message-square",
    Folder => "folder",
    ArrowRight => "arrow-right",
    Columns2 => "columns-2",
    Rows2 => "rows-2",
    // App shell sidebar (principles.md §2).
    Sun => "sun",
    Inbox => "inbox",
    Search => "search",
    PanelLeft => "panel-left",
    // Notification center (spec 0005; priority icons per tokens.md §2.8).
    AtSign => "at-sign",
    Bell => "bell",
    BellOff => "bell-off",
    AlarmClock => "alarm-clock",
    Moon => "moon",
    ChevronsUp => "chevrons-up",
    ChevronUp => "chevron-up",
    Minus => "minus",
    Lightbulb => "lightbulb",
    Undo2 => "undo-2",
    SlidersHorizontal => "sliders-horizontal",
    // Work items + pipelines views (tokens.md §2.8: `wi.*`, `pipeline.*`).
    Bug => "bug",
    BookOpen => "book-open",
    Trophy => "trophy",
    Crown => "crown",
    CircleDotDashed => "circle-dot-dashed",
    CircleSlash => "circle-slash",
    GitBranch => "git-branch",
    Timer => "timer",
    User => "user",
    ListTodo => "list-todo",
    Workflow => "workflow",
    CircleDot => "circle-dot",
    // Sidebar rail + shortcut cheat sheet.
    Settings => "settings",
    Keyboard => "keyboard",
    PanelLeftClose => "panel-left-close",
    PanelLeftOpen => "panel-left-open",
    // Settings tabs and controls.
    Palette => "palette",
    Image => "image",
    ImagePlus => "image-plus",
    Volume2 => "volume-2",
    Info => "info",
    Plus => "plus",
    Play => "play",
    Swords => "swords",
    LogOut => "log-out",
    // Ken arcade layer (ken.md §4.3, §7): streak flame, pause, shields.
    Flame => "flame",
    Pause => "pause",
    Shield => "shield",
    // Today (docs/design/today.md): changed-file counts; the merged-today
    // glyph (spec 0017 FR-17, also 0022's Reduce Motion fallback).
    FileDiff => "file-diff",
    Sparkles => "sparkles",
    // Review header: linked work items (spec 0004 FR-17).
    Link => "link",
    Unlink => "unlink",
    // Pins (spec 0014, pins.md §13).
    Pin => "pin",
    PinOff => "pin-off",
    // Work item detail (work-item-detail.md §15).
    Pencil => "pencil",
    Ellipsis => "ellipsis",
    FolderTree => "folder-tree",
    CalendarRange => "calendar-range",
    UserX => "user-x",
    TriangleAlert => "triangle-alert",
    GitCommitHorizontal => "git-commit-horizontal",
    ArrowUpFromLine => "arrow-up-from-line",
    ArrowDownToLine => "arrow-down-to-line",
    ArrowLeft => "arrow-left",
    // Activity drawer (activity.md §11).
    ArrowUpDown => "arrow-up-down",
    ArrowDown => "arrow-down",
    Download => "download",
    Maximize2 => "maximize-2",
    Minimize2 => "minimize-2",
    X => "x",
}

/// Serves the embedded icons, fonts and illustrations. Register once:
/// `gpui_platform::application().with_assets(adoken_ui::Assets)`.
pub struct Assets;

impl Assets {
    fn embedded() -> impl Iterator<Item = (&'static str, &'static [u8])> {
        IconName::ALL
            .iter()
            .map(|icon| (icon.path(), icon.bytes()))
            .chain(crate::fonts::EMBEDDED.iter().copied())
            .chain(crate::images::EMBEDDED.iter().copied())
    }
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::embedded()
            .find(|(served, _)| *served == path)
            .map(|(_, bytes)| Cow::Borrowed(bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::embedded()
            .map(|(served, _)| served)
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

/// One icon, square, tinted with a theme color.
#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: f32,
    color: Option<Hsla>,
    transformation: Option<Transformation>,
}

impl Icon {
    /// 16 px (`size.icon.md`) by default; color defaults to the inherited
    /// text color.
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: adoken_theme::size::ICON_MD,
            color: None,
            transformation: None,
        }
    }

    /// One of the `size::ICON_*` steps.
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Scale/rotate the glyph (motion only; layout size is unchanged).
    pub fn transform(mut self, transformation: Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        svg()
            .path(self.name.path())
            .flex_none()
            .size(px(self.size))
            .text_color(self.color.unwrap_or_else(|| window.text_style().color))
            .when_some(self.transformation, |this, t| this.with_transformation(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_served_and_is_a_stroked_svg() {
        let listed = Assets.list("icons/").unwrap();
        assert_eq!(listed.len(), IconName::ALL.len());
        for path in listed {
            let bytes = Assets.load(&path).unwrap().expect("listed icon loads");
            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text.contains("<svg") && text.contains("viewBox"), "{path}");
            // Tinting relies on currentColor strokes; a hard-coded color in
            // an icon would bypass the theme.
            assert!(text.contains("currentColor"), "{path}");
        }
    }

    #[test]
    fn unknown_paths_and_prefixes_are_empty() {
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
        assert!(Assets.load("fonts/nope.ttf").unwrap().is_none());
        assert!(Assets.list("sounds/").unwrap().is_empty());
    }

    #[test]
    fn bundled_fonts_are_served_as_truetype() {
        let listed = Assets.list("fonts/").unwrap();
        assert_eq!(listed.len(), crate::fonts::EMBEDDED.len());
        for path in listed {
            let bytes = Assets.load(&path).unwrap().expect("listed font loads");
            // sfnt version 1.0: a TrueType face, not a collection or WOFF.
            assert_eq!(bytes[..4], [0, 1, 0, 0], "{path}");
        }
    }
}
