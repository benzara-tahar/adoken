//! adoken base UI: the gpui theme bridge and shared components.
//!
//! Every component reads colors from [`theme::Theme`] and sizes from
//! `adoken_theme` constants, has unit tests for its logic, a story in
//! `crates/gallery`, and a snapshot per built-in theme.

pub mod action_menu;
mod button;
mod card;
mod chip;
pub mod diff;
mod empty_state;
pub mod file_tree;
pub mod fonts;
pub mod icon;
pub mod images;
mod kbd;
mod list_row;
pub mod loaders;
pub mod motion;
mod notice;
mod option_card;
pub mod pin_row;
pub mod popover;
mod scroll;
mod segmented;
mod stage_bar;
mod status;
mod step_indicator;
pub mod text_input;
pub mod theme;
pub mod toast;
pub mod tooltip;
// Guided tours: anchors, placement, coach mark (spec 0020).
pub mod tour;

pub use button::{Button, ButtonPalette, ButtonSize, ButtonVariant};
pub use card::Card;
pub use chip::{Checkbox, Chip, ChipTone};
pub use empty_state::EmptyState;
pub use icon::{Assets, Icon, IconName};
pub use kbd::{Kbd, KeyPlatform, format_keystroke};
pub use list_row::{ListRow, match_ranges};
pub use loaders::{fist_loader, progress_ring, splash_overlay};
pub use notice::{Notice, Skeleton};
pub use option_card::{OPTION_CARD_HEIGHT, OptionCard};
pub use pin_row::{
    ICON_INSET, PinRailIcon, PinRow, PinRowState, PinTrailing, change_dot, focus_ring, pin_glyph,
};
pub use scroll::ContainScroll;
pub use segmented::Segmented;
pub use stage_bar::StageBar;
pub use status::{Spinner, Status, StatusIcon};
pub use step_indicator::StepIndicator;
pub use text_input::{TextInput, TextInputEvent};
pub use theme::{ActiveTheme, CodeFontStyle, Theme, TypeStyleExt};
pub use toast::{Toast, ToastAction, ToastEvent, ToastStack, ToastTone};
pub use tooltip::Tooltip;

// Settings controls.
mod select;
mod slider;
mod stepper;
mod switch;
pub use select::{Select, SelectOption};
pub use slider::Slider;
pub use stepper::Stepper;
pub use switch::Switch;

// Multi-line editor (work item description and composer, spec 0015).
pub mod text_area;
pub use text_area::{TextArea, TextAreaEvent};

// Anchored pickers for work item editing (spec 0015 FR-11/13/14/21).
pub mod picker;

// Rich text for work item descriptions and comments (spec 0015 FR-3).
pub mod rich_text;
pub use rich_text::{MentionName, RichText};
