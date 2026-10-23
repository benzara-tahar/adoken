//! Component stories. Each story renders one component in its meaningful
//! states; the gallery binary shows them live, `tests/snapshots.rs` captures
//! each one per built-in theme.
//!
//! Adding a component: add a `Story` to [`STORIES`], run
//! `UPDATE_SNAPSHOTS=1 cargo test -p adoken-gallery --test snapshots`, review
//! the new PNGs in `crates/gallery/snapshots/`.

mod activity;
mod avatar;
mod background;
mod celebrate;
mod chrome;
mod global_shortcut;
mod inbox;
mod ken;
mod onboarding;
mod os_notify;
mod pickers;
mod pins;
mod pipelines;
mod resource_meter;
mod review;
mod rich_text;
mod settings;
mod shell;
mod theme_picker;
pub mod today;
mod tours;
mod workitems;
mod workitems_paths;

use adoken_theme::{radius, size, space, typography};
use adoken_ui::{
    ActiveTheme, Button, ButtonSize, ButtonVariant, Card, Checkbox, Chip, ChipTone, Icon, IconName,
    Kbd, ListRow, Notice, OptionCard, Segmented, Skeleton, Spinner, Status, StatusIcon,
    StepIndicator, TextInput, TypeStyleExt, match_ranges,
};
use gpui::{AnyElement, App, Context, IntoElement, Render, Window, div, prelude::*, px};

pub struct Story {
    /// File-name safe: used in snapshot names.
    pub name: &'static str,
    /// Logical size the snapshot is taken at.
    pub size: (f32, f32),
    pub render: fn(&mut Window, &mut App) -> AnyElement,
}

pub static STORIES: &[Story] = &[
    Story {
        name: "pins-expanded",
        size: (700.0, 600.0),
        render: pins::expanded,
    },
    Story {
        name: "pins-folded",
        size: (700.0, 600.0),
        render: pins::folded,
    },
    Story {
        name: "pins-rail",
        size: (700.0, 600.0),
        render: pins::rail,
    },
    Story {
        name: "pins-peek",
        size: (1100.0, 600.0),
        render: pins::peek,
    },
    Story {
        name: "pins-empty-hint",
        size: (700.0, 600.0),
        render: pins::empty,
    },
    Story {
        name: "pins-twenty-scrolling",
        size: (700.0, 600.0),
        render: pins::twenty,
    },
    Story {
        name: "pins-not-available",
        size: (700.0, 600.0),
        render: pins::unavailable,
    },
    Story {
        name: "pins-stale",
        size: (1100.0, 600.0),
        render: pins::stale,
    },
    Story {
        name: "toast-stack-action",
        size: (560.0, 220.0),
        render: pins::toast_action,
    },
    Story {
        name: "toast-stack-limit",
        size: (560.0, 220.0),
        render: pins::toast_limit,
    },
    Story {
        name: "toast-stack-three",
        size: (560.0, 300.0),
        render: pins::toast_three,
    },
    Story {
        name: "row-action-menu",
        size: (420.0, 340.0),
        render: pins::action_menu,
    },
    Story {
        name: "button",
        size: (560.0, 200.0),
        render: button_story,
    },
    Story {
        name: "kbd",
        size: (360.0, 70.0),
        render: kbd_story,
    },
    Story {
        name: "card",
        size: (400.0, 190.0),
        render: card_story,
    },
    Story {
        name: "text-input",
        size: (400.0, 190.0),
        render: text_input_story,
    },
    Story {
        name: "list-row",
        size: (560.0, 190.0),
        render: list_row_story,
    },
    Story {
        name: "icon",
        size: (420.0, 110.0),
        render: icon_story,
    },
    Story {
        name: "status",
        size: (560.0, 110.0),
        render: status_story,
    },
    Story {
        name: "step-indicator",
        size: (240.0, 120.0),
        render: step_indicator_story,
    },
    Story {
        name: "option-card",
        size: (520.0, 330.0),
        render: option_card_story,
    },
    Story {
        name: "segmented",
        size: (420.0, 110.0),
        render: segmented_story,
    },
    Story {
        name: "chip",
        size: (480.0, 100.0),
        render: chip_story,
    },
    Story {
        name: "notice",
        size: (520.0, 300.0),
        render: notice_story,
    },
    Story {
        name: "empty-state",
        size: (420.0, 220.0),
        render: shell::empty_state_story,
    },
    Story {
        name: "sync-status",
        size: (420.0, 300.0),
        render: shell::sync_status_story,
    },
    Story {
        name: "titlebar-history",
        size: (520.0, 120.0),
        render: shell::history_buttons_story,
    },
    Story {
        name: "shell-search",
        size: (1100.0, 680.0),
        render: shell::shell_search_story,
    },
    Story {
        name: "palette-my-work",
        size: (760.0, 560.0),
        render: shell::palette_my_work_story,
    },
    Story {
        name: "palette-prs",
        size: (760.0, 560.0),
        render: shell::palette_prs_story,
    },
    Story {
        name: "palette-filters",
        size: (760.0, 560.0),
        render: shell::palette_filters_story,
    },
    Story {
        name: "palette-link",
        size: (760.0, 360.0),
        render: shell::palette_link_story,
    },
    Story {
        name: "palette-commands",
        size: (760.0, 520.0),
        render: shell::palette_commands_story,
    },
    Story {
        name: "today-mood-fresh-start",
        size: (1180.0, 900.0),
        render: today::today_mood_fresh_start_story,
    },
    Story {
        name: "today-mood-finding-flow",
        size: (1180.0, 900.0),
        render: today::today_mood_finding_flow_story,
    },
    Story {
        name: "today-mood-helping-grow",
        size: (1180.0, 900.0),
        render: today::today_mood_helping_grow_story,
    },
    Story {
        name: "today-mood-waiting",
        size: (1180.0, 900.0),
        render: today::today_mood_waiting_story,
    },
    Story {
        name: "today-mood-blocked",
        size: (1180.0, 900.0),
        render: today::today_mood_blocked_story,
    },
    Story {
        name: "today-mood-shipped",
        size: (1180.0, 900.0),
        render: today::today_mood_shipped_story,
    },
    Story {
        name: "today-mood-clear",
        size: (1180.0, 900.0),
        render: today::today_mood_clear_story,
    },
    Story {
        name: "today-mood-stalled",
        size: (1180.0, 900.0),
        render: today::today_mood_stalled_story,
    },
    Story {
        name: "today-busy-day",
        size: (1180.0, 900.0),
        render: today::today_busy_day_story,
    },
    Story {
        name: "today-first-sync",
        size: (1180.0, 900.0),
        render: today::today_first_sync_story,
    },
    Story {
        name: "today-stale",
        size: (1180.0, 900.0),
        render: today::today_stale_story,
    },
    Story {
        name: "today-loading",
        size: (1180.0, 900.0),
        render: today::today_loading_story,
    },
    Story {
        name: "today-no-account",
        size: (1180.0, 900.0),
        render: today::today_no_account_story,
    },
    Story {
        name: "today-empty-org",
        size: (1180.0, 900.0),
        render: today::today_empty_org_story,
    },
    Story {
        name: "today-error",
        size: (1180.0, 900.0),
        render: today::today_error_story,
    },
    Story {
        name: "today-narrow",
        size: (720.0, 1300.0),
        render: today::today_narrow_story,
    },
    Story {
        name: "today-up-next-hidden",
        size: (1180.0, 900.0),
        render: today::today_up_next_hidden_story,
    },
    Story {
        name: "today-group-reviewers-shown",
        size: (1180.0, 900.0),
        render: today::today_group_reviewers_shown_story,
    },
    Story {
        name: "today-long-titles",
        size: (1180.0, 900.0),
        render: today::today_long_titles_story,
    },
    Story {
        name: "stage-bar",
        size: (480.0, 360.0),
        render: today::stage_bar_story,
    },
    Story {
        name: "shell-sidebar-expanded",
        size: (1100.0, 680.0),
        render: chrome::sidebar_expanded_story,
    },
    Story {
        name: "shell-sidebar-collapsed",
        size: (1100.0, 680.0),
        render: chrome::sidebar_collapsed_story,
    },
    Story {
        name: "loaders",
        size: (820.0, 440.0),
        render: chrome::loaders_story,
    },
    Story {
        name: "cheat-sheet",
        size: (760.0, 720.0),
        render: chrome::cheat_sheet_story,
    },
    Story {
        name: "cheat-sheet-search",
        size: (760.0, 640.0),
        render: chrome::cheat_sheet_search_story,
    },
    Story {
        name: "review-header",
        size: (1000.0, 200.0),
        render: review::header_story,
    },
    Story {
        name: "review-file-tree",
        size: (340.0, 420.0),
        render: review::file_tree_story,
    },
    Story {
        name: "review-diff-unified",
        size: (1100.0, 700.0),
        render: review::diff_unified_story,
    },
    Story {
        name: "review-diff-split",
        size: (1440.0, 700.0),
        render: review::diff_split_story,
    },
    Story {
        name: "review-file-tree-loading",
        size: (340.0, 300.0),
        render: review::file_tree_loading_story,
    },
    Story {
        name: "review-file-tree-failed",
        size: (340.0, 200.0),
        render: review::file_tree_failed_story,
    },
    Story {
        name: "review-diff-loading",
        size: (1100.0, 200.0),
        render: review::diff_loading_story,
    },
    Story {
        name: "review-diff-loading-slow",
        size: (1100.0, 200.0),
        render: review::diff_loading_slow_story,
    },
    Story {
        name: "review-list",
        size: (1100.0, 640.0),
        render: review::list_story,
    },
    Story {
        name: "review-list-badges",
        size: (1100.0, 640.0),
        render: review::list_badges_story,
    },
    Story {
        name: "review-open",
        size: (1440.0, 860.0),
        render: review::open_story,
    },
    Story {
        name: "review-list-repo-filter",
        size: (900.0, 560.0),
        render: review::list_repo_filter_story,
    },
    Story {
        name: "review-list-repo-picker",
        size: (900.0, 560.0),
        render: review::list_repo_picker_story,
    },
    Story {
        name: "review-list-repo-empty",
        size: (900.0, 560.0),
        render: review::list_repo_empty_story,
    },
    Story {
        name: "review-composer",
        size: (1100.0, 560.0),
        render: review::composer_story,
    },
    Story {
        name: "review-composer-narrow",
        size: (360.0, 560.0),
        render: review::composer_narrow_story,
    },
    Story {
        name: "review-composer-posting",
        size: (1100.0, 560.0),
        render: review::composer_posting_story,
    },
    Story {
        name: "review-composer-failed",
        size: (1100.0, 560.0),
        render: review::composer_failed_story,
    },
    Story {
        name: "review-vote-menu",
        size: (1000.0, 320.0),
        render: review::vote_menu_story,
    },
    Story {
        name: "review-completing",
        size: (1000.0, 200.0),
        render: review::completing_story,
    },
    Story {
        name: "review-complete-failed",
        size: (1000.0, 200.0),
        render: review::complete_failed_story,
    },
    Story {
        name: "review-complete-sheet",
        size: (1100.0, 720.0),
        render: review::complete_sheet_story,
    },
    Story {
        name: "review-complete-new-commits",
        size: (1100.0, 720.0),
        render: review::complete_new_commits_story,
    },
    Story {
        name: "review-read-only",
        size: (1000.0, 200.0),
        render: review::read_only_story,
    },
    Story {
        name: "review-status-menu",
        size: (1000.0, 360.0),
        render: review::status_menu_story,
    },
    Story {
        name: "review-abandon-confirm",
        size: (1100.0, 720.0),
        render: review::abandon_confirm_story,
    },
    Story {
        name: "review-work-items",
        size: (1000.0, 240.0),
        render: review::work_items_story,
    },
    Story {
        name: "review-work-item-picker",
        size: (1100.0, 720.0),
        render: review::work_item_picker_story,
    },
    Story {
        name: "review-link-scope-missing",
        size: (1000.0, 240.0),
        render: review::link_scope_missing_story,
    },
    Story {
        name: "review-copied",
        size: (1200.0, 760.0),
        render: review::copied_story,
    },
    Story {
        name: "review-tree-filter",
        size: (340.0, 420.0),
        render: review::tree_filter_story,
    },
    Story {
        name: "review-tree-filter-empty",
        size: (340.0, 420.0),
        render: review::tree_filter_empty_story,
    },
    Story {
        name: "review-tree-threads-only",
        size: (340.0, 420.0),
        render: review::tree_threads_only_story,
    },
    Story {
        name: "review-threads",
        size: (1300.0, 860.0),
        render: review::threads_story,
    },
    Story {
        name: "review-threads-all",
        size: (1300.0, 860.0),
        render: review::threads_all_story,
    },
    Story {
        name: "review-threads-empty-active",
        size: (1300.0, 640.0),
        render: review::threads_empty_active_story,
    },
    Story {
        name: "review-threads-loading",
        size: (1300.0, 860.0),
        render: review::threads_loading_story,
    },
    Story {
        name: "review-threads-notes",
        size: (1300.0, 860.0),
        render: review::threads_notes_story,
    },
    Story {
        name: "review-threads-read-only",
        size: (1300.0, 860.0),
        render: review::threads_read_only_story,
    },
    Story {
        name: "review-header-groups",
        size: (1000.0, 240.0),
        render: review::header_groups_story,
    },
    Story {
        name: "review-header-groups-shown",
        size: (1000.0, 240.0),
        render: review::header_groups_shown_story,
    },
    Story {
        name: "review-header-draft",
        size: (1000.0, 240.0),
        render: review::header_draft_story,
    },
    Story {
        name: "review-list-groups",
        size: (1100.0, 640.0),
        render: review::list_groups_story,
    },
    Story {
        name: "review-list-groups-shown",
        size: (1100.0, 640.0),
        render: review::list_groups_shown_story,
    },
    Story {
        name: "review-list-row-menu",
        size: (1100.0, 640.0),
        render: review::list_row_menu_story,
    },
    Story {
        name: "review-list-row-menu-draft",
        size: (1100.0, 640.0),
        render: review::list_row_menu_draft_story,
    },
    Story {
        name: "review-list-row-menu-abandoned",
        size: (1100.0, 640.0),
        render: review::list_row_menu_abandoned_story,
    },
    Story {
        name: "review-list-abandon-confirm",
        size: (1100.0, 640.0),
        render: review::list_abandon_confirm_story,
    },
    Story {
        name: "review-list-sending",
        size: (1100.0, 640.0),
        render: review::list_sending_story,
    },
    Story {
        name: "review-copied-title",
        size: (1200.0, 760.0),
        render: review::copied_title_story,
    },
    Story {
        name: "inbox",
        size: (1100.0, 720.0),
        render: inbox::inbox_story,
    },
    Story {
        name: "inbox-snooze",
        size: (1100.0, 520.0),
        render: inbox::inbox_snooze_story,
    },
    Story {
        name: "inbox-filtered",
        size: (1100.0, 360.0),
        render: inbox::inbox_filtered_story,
    },
    Story {
        name: "inbox-zero",
        size: (1100.0, 360.0),
        render: inbox::inbox_zero_story,
    },
    Story {
        name: "inbox-auto-complete-guard",
        size: (1100.0, 520.0),
        render: inbox::inbox_auto_complete_guard_story,
    },
    Story {
        name: "background-dusk",
        size: (1100.0, 680.0),
        render: background::dusk_story,
    },
    Story {
        name: "background-lantern",
        size: (1100.0, 680.0),
        render: background::lantern_story,
    },
    Story {
        name: "background-ink",
        size: (1100.0, 680.0),
        render: background::ink_story,
    },
    Story {
        name: "background-illustration",
        size: (1440.0, 860.0),
        render: background::illustration_story,
    },
    Story {
        name: "os-notify-permission",
        size: (640.0, 380.0),
        render: os_notify::permission_row_story,
    },
    Story {
        name: "work-items",
        size: (1440.0, 900.0),
        render: workitems::my_work_story,
    },
    Story {
        name: "work-items-area",
        size: (1100.0, 720.0),
        render: workitems::by_area_story,
    },
    Story {
        name: "work-items-empty",
        size: (1100.0, 420.0),
        render: workitems::empty_story,
    },
    Story {
        name: "work-items-area-dropdown",
        size: (1100.0, 720.0),
        render: workitems_paths::area_story,
    },
    Story {
        name: "work-items-area-dropdown-projects",
        size: (1100.0, 720.0),
        render: workitems_paths::area_projects_story,
    },
    Story {
        name: "work-items-area-dropdown-filtered",
        size: (1100.0, 720.0),
        render: workitems_paths::area_filtered_story,
    },
    Story {
        name: "work-items-area-dropdown-loading",
        size: (1100.0, 720.0),
        render: workitems_paths::area_loading_story,
    },
    Story {
        name: "work-items-iteration-dropdown",
        size: (1100.0, 720.0),
        render: workitems_paths::iteration_story,
    },
    Story {
        name: "work-item-detail",
        size: (1208.0, 1500.0),
        render: workitems::detail_story,
    },
    Story {
        name: "work-item-detail-wide",
        size: (1208.0, 1100.0),
        render: workitems::wide_story,
    },
    Story {
        name: "work-item-detail-stacked",
        size: (1024.0, 900.0),
        render: workitems::stacked_story,
    },
    Story {
        name: "work-item-detail-narrow-scale",
        size: (900.0, 900.0),
        render: workitems::narrow_scale_story,
    },
    Story {
        name: "work-item-detail-state-menu",
        size: (1208.0, 760.0),
        render: workitems::state_menu_story,
    },
    Story {
        name: "work-item-detail-assign-picker",
        size: (1208.0, 760.0),
        render: workitems::assign_picker_story,
    },
    Story {
        name: "work-item-detail-area-picker",
        size: (1208.0, 760.0),
        render: workitems::area_picker_story,
    },
    Story {
        name: "work-item-detail-iteration-picker",
        size: (1208.0, 760.0),
        render: workitems::iteration_picker_story,
    },
    Story {
        name: "work-item-detail-editing-title",
        size: (1208.0, 560.0),
        render: workitems::editing_title_story,
    },
    Story {
        name: "work-item-detail-description-editor",
        size: (1208.0, 1000.0),
        render: workitems::description_editor_story,
    },
    Story {
        name: "work-item-detail-description-preview",
        size: (1208.0, 1000.0),
        render: workitems::description_preview_story,
    },
    Story {
        name: "work-item-detail-description-html",
        size: (1208.0, 1000.0),
        render: workitems::description_html_story,
    },
    Story {
        name: "work-item-detail-description-blocked",
        size: (1208.0, 1000.0),
        render: workitems::description_blocked_story,
    },
    Story {
        name: "work-item-detail-draft",
        size: (1208.0, 1300.0),
        render: workitems::draft_story,
    },
    Story {
        name: "work-item-detail-links",
        size: (1208.0, 1500.0),
        render: workitems::links_story,
    },
    Story {
        name: "work-item-detail-link-menu",
        size: (1208.0, 1500.0),
        render: workitems::link_menu_story,
    },
    Story {
        name: "work-item-detail-link-picker",
        size: (1208.0, 900.0),
        render: workitems::link_picker_story,
    },
    Story {
        name: "work-item-detail-remove-link",
        size: (1208.0, 900.0),
        render: workitems::remove_link_story,
    },
    Story {
        name: "work-item-detail-composer-mention",
        size: (1208.0, 1500.0),
        render: workitems::composer_mention_story,
    },
    Story {
        name: "work-item-detail-pending",
        size: (1208.0, 1600.0),
        render: workitems::pending_story,
    },
    Story {
        name: "work-item-detail-failed",
        size: (1208.0, 1600.0),
        render: workitems::failed_story,
    },
    Story {
        name: "work-item-detail-conflict",
        size: (1208.0, 760.0),
        render: workitems::conflict_story,
    },
    Story {
        name: "work-item-detail-rule",
        size: (1208.0, 760.0),
        render: workitems::rule_story,
    },
    Story {
        name: "work-item-detail-read-only",
        size: (1208.0, 1500.0),
        render: workitems::read_only_story,
    },
    Story {
        name: "work-item-detail-freshness",
        size: (420.0, 380.0),
        render: workitems::freshness_story,
    },
    Story {
        name: "work-item-detail-partly-synced",
        size: (1208.0, 900.0),
        render: workitems::partly_synced_story,
    },
    Story {
        name: "work-item-detail-offline",
        size: (1208.0, 1500.0),
        render: workitems::offline_story,
    },
    Story {
        name: "work-item-detail-sticky",
        size: (1208.0, 700.0),
        render: workitems::sticky_story,
    },
    Story {
        name: "work-item-detail-not-available",
        size: (1208.0, 900.0),
        render: workitems::not_available_story,
    },
    Story {
        name: "pipelines",
        size: (1440.0, 960.0),
        render: pipelines::definitions_story,
    },
    Story {
        name: "pipelines-run",
        size: (1440.0, 900.0),
        render: pipelines::failed_run_story,
    },
    Story {
        name: "theme-picker",
        size: (900.0, 760.0),
        render: theme_picker::picker_story,
    },
    Story {
        name: "theme-picker-search",
        size: (900.0, 620.0),
        render: theme_picker::picker_search_story,
    },
    Story {
        name: "theme-strip",
        size: (1100.0, 800.0),
        render: theme_picker::strip_story,
    },
    Story {
        name: "settings-general",
        size: (940.0, 680.0),
        render: settings::general_story,
    },
    Story {
        name: "settings-appearance",
        size: (940.0, 680.0),
        render: settings::appearance_story,
    },
    Story {
        name: "settings-code-font",
        size: (940.0, 680.0),
        render: settings::code_font_story,
    },
    Story {
        name: "settings-code-font-missing",
        size: (940.0, 680.0),
        render: settings::code_font_missing_story,
    },
    Story {
        name: "settings-backgrounds",
        size: (940.0, 680.0),
        render: settings::backgrounds_story,
    },
    Story {
        name: "settings-sounds",
        size: (940.0, 680.0),
        render: settings::sounds_story,
    },
    Story {
        name: "settings-notifications",
        size: (940.0, 680.0),
        render: settings::notifications_story,
    },
    Story {
        name: "settings-nudges",
        size: (940.0, 680.0),
        render: settings::nudges_story,
    },
    Story {
        name: "settings-sync",
        size: (940.0, 1240.0),
        render: settings::sync_story,
    },
    Story {
        name: "settings-sync-paused",
        size: (940.0, 1240.0),
        render: settings::sync_paused_story,
    },
    Story {
        name: "settings-sync-manual",
        size: (940.0, 1240.0),
        render: settings::sync_manual_story,
    },
    Story {
        name: "settings-keyboard",
        size: (940.0, 680.0),
        render: settings::keyboard_story,
    },
    Story {
        name: "settings-ken",
        size: (940.0, 680.0),
        render: settings::ken_story,
    },
    Story {
        name: "settings-account-demo",
        size: (940.0, 680.0),
        render: settings::account_demo_story,
    },
    Story {
        name: "settings-account",
        size: (940.0, 900.0),
        render: settings::account_story,
    },
    Story {
        name: "settings-account-pasted",
        size: (940.0, 900.0),
        render: settings::account_pasted_story,
    },
    Story {
        name: "settings-account-rejected",
        size: (940.0, 900.0),
        render: settings::account_rejected_story,
    },
    Story {
        name: "settings-account-expired",
        size: (940.0, 900.0),
        render: settings::account_expired_story,
    },
    Story {
        name: "settings-account-replaced",
        size: (940.0, 900.0),
        render: settings::account_replaced_story,
    },
    Story {
        name: "onboarding-welcome",
        size: (1200.0, 760.0),
        render: onboarding::welcome_story,
    },
    Story {
        name: "onboarding-review-access",
        size: (1200.0, 1100.0),
        render: onboarding::review_access_story,
    },
    Story {
        name: "onboarding-create-in-browser",
        size: (1200.0, 1200.0),
        render: onboarding::create_in_browser_story,
    },
    Story {
        name: "onboarding-paste-verify",
        size: (1200.0, 1100.0),
        render: onboarding::paste_verify_story,
    },
    Story {
        name: "settings-about",
        size: (940.0, 680.0),
        render: settings::about_story,
    },
    Story {
        name: "settings-broken",
        size: (940.0, 680.0),
        render: settings::broken_story,
    },
    Story {
        name: "settings-controls",
        size: (620.0, 260.0),
        render: settings::controls_story,
    },
    Story {
        name: "ken-combo-hud",
        size: (1240.0, 460.0),
        render: ken::combo_hud_story,
    },
    Story {
        name: "ken-ko",
        size: (1000.0, 560.0),
        render: ken::ko_story,
    },
    Story {
        name: "ken-ko-double",
        size: (1000.0, 560.0),
        render: ken::ko_double_story,
    },
    Story {
        name: "ken-ko-reduced-motion",
        size: (420.0, 170.0),
        render: ken::ko_reduced_story,
    },
    Story {
        name: "ken-fight-card",
        size: (760.0, 620.0),
        render: ken::fight_card_story,
    },
    Story {
        name: "ken-fight-card-png",
        size: (640.0, 400.0),
        render: ken::fight_card_png_story,
    },
    Story {
        name: "ken-settings",
        size: (760.0, 1180.0),
        render: ken::settings_story,
    },
    Story {
        name: "celebrate-step",
        size: (1000.0, 300.0),
        render: celebrate::step_story,
    },
    Story {
        name: "celebrate-milestone",
        size: (1000.0, 300.0),
        render: celebrate::milestone_story,
    },
    Story {
        name: "celebrate-pr",
        size: (1000.0, 300.0),
        render: celebrate::pr_story,
    },
    Story {
        name: "celebrate-arcade-milestone",
        size: (1000.0, 300.0),
        render: celebrate::arcade_milestone_story,
    },
    Story {
        name: "celebrate-reduced",
        size: (1000.0, 280.0),
        render: celebrate::reduced_story,
    },
    Story {
        name: "settings-celebrations",
        size: (940.0, 760.0),
        render: celebrate::settings_story,
    },
    Story {
        name: "avatar",
        size: (520.0, 240.0),
        render: avatar::avatar_story,
    },
    Story {
        name: "picker-state-menu",
        size: (1240.0, 320.0),
        render: pickers::state_menu,
    },
    Story {
        name: "picker-people",
        size: (1744.0, 420.0),
        render: pickers::people_picker,
    },
    Story {
        name: "picker-area-tree",
        size: (1400.0, 420.0),
        render: pickers::area_picker,
    },
    Story {
        name: "picker-iteration-tree",
        size: (400.0, 420.0),
        render: pickers::iteration_picker,
    },
    Story {
        name: "picker-link-type",
        size: (632.0, 340.0),
        render: pickers::link_type_menu,
    },
    Story {
        name: "rich-text",
        size: (1000.0, 640.0),
        render: rich_text::story,
    },
    // Statusbar activity and the Activity drawer (spec 0016 AC-S5).
    Story {
        name: "statusbar-activity",
        size: (720.0, 600.0),
        render: activity::statusbar_states,
    },
    Story {
        name: "statusbar-activity-narrow",
        size: (900.0, 600.0),
        render: activity::statusbar_900,
    },
    Story {
        name: "activity-sync",
        size: (1200.0, 760.0),
        render: activity::sync_idle,
    },
    Story {
        name: "activity-sync-syncing",
        size: (1200.0, 760.0),
        render: activity::sync_syncing,
    },
    Story {
        name: "activity-sync-first",
        size: (1200.0, 760.0),
        render: activity::sync_first,
    },
    Story {
        name: "activity-sync-throttled",
        size: (1200.0, 760.0),
        render: activity::sync_throttled,
    },
    Story {
        name: "activity-sync-failed",
        size: (1200.0, 760.0),
        render: activity::sync_failed,
    },
    Story {
        name: "activity-sync-paused",
        size: (1200.0, 760.0),
        render: activity::sync_paused,
    },
    Story {
        name: "activity-sync-scope",
        size: (1200.0, 760.0),
        render: activity::sync_missing_scope,
    },
    Story {
        name: "activity-sync-demo",
        size: (1200.0, 760.0),
        render: activity::sync_demo,
    },
    Story {
        name: "activity-sync-empty",
        size: (1200.0, 760.0),
        render: activity::sync_empty,
    },
    Story {
        name: "activity-sync-no-token",
        size: (1200.0, 760.0),
        render: activity::sync_no_token,
    },
    Story {
        name: "activity-sync-narrow",
        size: (900.0, 600.0),
        render: activity::sync_narrow,
    },
    Story {
        name: "activity-drawer-rail",
        size: (900.0, 600.0),
        render: activity::drawer_rail,
    },
    Story {
        name: "activity-drawer-min",
        size: (900.0, 600.0),
        render: activity::drawer_min,
    },
    Story {
        name: "activity-network",
        size: (1200.0, 760.0),
        render: activity::network,
    },
    Story {
        name: "activity-network-failed",
        size: (1200.0, 760.0),
        render: activity::network_failed,
    },
    Story {
        name: "activity-network-detail",
        size: (1200.0, 760.0),
        render: activity::network_detail,
    },
    Story {
        name: "activity-network-totals",
        size: (1200.0, 760.0),
        render: activity::network_totals,
    },
    Story {
        name: "activity-network-empty",
        size: (1200.0, 760.0),
        render: activity::network_empty,
    },
    Story {
        name: "activity-network-filtered",
        size: (1200.0, 760.0),
        render: activity::network_filtered,
    },
    Story {
        name: "activity-network-demo",
        size: (1200.0, 760.0),
        render: activity::network_demo,
    },
    Story {
        name: "activity-network-ten-thousand",
        size: (900.0, 600.0),
        render: activity::network_10k,
    },
    Story {
        name: "activity-network-ten-thousand-wide",
        size: (1440.0, 900.0),
        render: activity::network_10k_wide,
    },
    // Resource meter (spec 0023 AC-11).
    Story {
        name: "statusbar-resource-meter",
        size: (1200.0, 760.0),
        render: resource_meter::on,
    },
    Story {
        name: "statusbar-resource-meter-narrow",
        size: (660.0, 600.0),
        render: resource_meter::narrow,
    },
    Story {
        name: "statusbar-resource-meter-gigabytes",
        size: (1200.0, 760.0),
        render: resource_meter::gigabytes,
    },
    Story {
        name: "statusbar-resource-meter-menu",
        size: (1200.0, 760.0),
        render: resource_meter::menu,
    },
    // Settings › Keyboard › Global shortcut (spec 0021 AC-14).
    Story {
        name: "settings-keyboard-global-untested",
        size: (940.0, 480.0),
        render: global_shortcut::untested,
    },
    Story {
        name: "settings-keyboard-global-works",
        size: (940.0, 480.0),
        render: global_shortcut::works,
    },
    Story {
        name: "settings-keyboard-global-recording",
        size: (940.0, 480.0),
        render: global_shortcut::recording,
    },
    Story {
        name: "settings-keyboard-global-refused",
        size: (940.0, 480.0),
        render: global_shortcut::refused,
    },
    Story {
        name: "settings-keyboard-global-failed",
        size: (940.0, 480.0),
        render: global_shortcut::failed,
    },
    Story {
        name: "settings-keyboard-global-off",
        size: (940.0, 480.0),
        render: global_shortcut::off,
    },
    // Guided tours (spec 0020 AC-12, tours.md §8).
    Story {
        name: "tour-review-files",
        size: (1440.0, 860.0),
        render: tours::review_files,
    },
    Story {
        name: "tour-review-done",
        size: (1440.0, 860.0),
        render: tours::review_done,
    },
    Story {
        name: "tour-palette",
        size: (760.0, 560.0),
        render: tours::palette,
    },
    Story {
        name: "tour-inbox-filters",
        size: (1100.0, 720.0),
        render: tours::inbox_filters,
    },
    Story {
        name: "tour-work-item-state",
        size: (1208.0, 900.0),
        render: tours::work_item_state,
    },
    Story {
        name: "tour-today-summary",
        size: (1180.0, 900.0),
        render: tours::today_summary,
    },
    Story {
        name: "tour-narrow",
        size: (900.0, 600.0),
        render: tours::narrow,
    },
    Story {
        name: "tour-font-scale",
        size: (1440.0, 860.0),
        render: tours::font_scale,
    },
];

/// App-wide state every story may read, so a story never depends on which
/// ran before it (the snapshot harness shares one app across stories): the
/// bundled code font (JetBrains Mono, as the app registers it), demo
/// avatars (spec 0012) and the demo org's review access (writes apply to
/// the local store, spec 0004). Call once after the theme is installed,
/// before opening story windows.
pub fn init(cx: &mut App) {
    adoken_ui::fonts::register(cx).expect("bundled fonts register");
    avatar::init(cx);
    adoken_app::review::writes::set_access(adoken_app::review::writes::Access::Local, cx);
    cx.bind_keys(adoken_app::pins::bindings());
    cx.bind_keys(adoken_ui::action_menu::bindings());
    cx.bind_keys(adoken_ui::picker::bindings());
}

/// Where story `key` keeps its demo store. Emptied on first use in each
/// process: a store left by an older build (other demo data or schema, a
/// draft typed in the live gallery) never leaks into a snapshot. Later
/// windows of the same story (one per theme) reuse the fresh seed.
pub(crate) fn story_dir(key: &str) -> std::path::PathBuf {
    static FRESH: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let dir = std::env::temp_dir().join(format!("adoken-gallery-{key}"));
    let mut fresh = FRESH.lock().unwrap_or_else(|e| e.into_inner());
    if !fresh.iter().any(|k| k == key) {
        let _ = std::fs::remove_dir_all(&dir);
        fresh.push(key.to_owned());
    }
    dir
}

fn row() -> gpui::Div {
    div().flex().items_center().gap(px(space::S8))
}

fn button_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    let buttons = |disabled: bool| {
        row().children(ButtonVariant::ALL.map(|variant| {
            Button::new(
                format!("{variant:?}-{disabled}").to_lowercase(),
                format!("{variant:?}"),
            )
            .variant(variant)
            .disabled(disabled)
        }))
    };
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(buttons(false))
        .child(buttons(true))
        .child(
            row().child(
                Button::new("with-shortcut", "Open palette")
                    .variant(ButtonVariant::Primary)
                    .shortcut(Kbd::parse("cmd-p").expect("static binding")),
            ),
        )
        .child(
            row()
                .child(
                    Button::new("large", "Create token")
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Lg)
                        .shortcut(Kbd::parse("enter").expect("static binding")),
                )
                .child(
                    Button::new("loading", "Creating…")
                        .variant(ButtonVariant::Primary)
                        .size(ButtonSize::Lg)
                        .loading(true),
                )
                .child(Button::new("with-icon", "Open token page").icon(IconName::ExternalLink)),
        )
        .into_any_element()
}

fn kbd_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    row()
        .children(
            [
                "cmd-p",
                "cmd-shift-l",
                "ctrl-alt-delete",
                "cmd-k cmd-t",
                "enter",
            ]
            .map(|keys| Kbd::parse(keys).expect("static binding")),
        )
        .into_any_element()
}

fn card_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    Card::new()
        .title("Pull request !4821")
        .child(
            div()
                .text_color(theme.colors.text_secondary)
                .type_style(typography::SMALL, theme)
                .child("Refactor sync governor · 3 reviewers · updated 5m ago"),
        )
        .child(
            row()
                .child(Button::new("approve", "Approve").variant(ButtonVariant::Primary))
                .child(Button::new("open", "Open").shortcut(Kbd::parse("enter").expect("static"))),
        )
        .into_any_element()
}

fn text_input_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let field = |text: &'static str, cx: &mut App| {
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Search pull requests, work items…", cx);
            input.set_text(text, cx);
            input
        });
        let theme = cx.theme();
        div()
            .w_full()
            .px(px(space::S8))
            .py(px(space::S4))
            .rounded(px(radius::SM))
            .border_1()
            .border_color(theme.colors.border_control)
            .bg(theme.colors.bg_input)
            .text_color(theme.colors.text_primary)
            .type_style(typography::BODY, theme)
            .child(input)
    };
    let masked = cx.new(|cx| {
        let mut input = TextInput::new("Paste token (⌘V)", cx);
        input.set_masked(true, cx);
        input.set_text("not-a-real-token-just-a-story", cx);
        input
    });
    let empty = field("", cx);
    let filled = field("retry idem", cx);
    // Longer than the box, caret at the end: scrolled to the caret, clipped
    // at the border.
    let overflowing = field(
        "retry idempotent charge on 409 conflict with capped exponential backoff",
        cx,
    );
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(empty)
        .child(filled)
        .child(overflowing)
        .child(
            div()
                .w_full()
                .px(px(space::S8))
                .py(px(space::S4))
                .rounded(px(radius::SM))
                .border_1()
                .border_color(theme.colors.border_control)
                .bg(theme.colors.bg_input)
                .text_color(theme.colors.text_primary)
                .type_style(typography::CODE, theme)
                .child(masked),
        )
        .into_any_element()
}

fn list_row_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let glyph = |glyph: &'static str, color| {
        div()
            .text_color(color)
            .type_style(typography::BODY_STRONG, theme)
            .child(glyph)
    };
    let title = "Retry idempotency keys on payment capture";
    div()
        .w_full()
        .py(px(space::S4))
        .rounded(px(radius::LG))
        .bg(theme.colors.bg_overlay)
        .child(
            ListRow::new("selected", title)
                .highlights(match_ranges(title, &[0, 1, 2, 3, 6, 7, 8, 9]))
                .subtitle("!4821 · payments-api · feature/retry → main")
                .leading(glyph("⇄", theme.colors.pr_active))
                .trailing("Pull request")
                .selected(true),
        )
        .child(
            ListRow::new("plain", "Retry budget for webhook dispatch")
                .highlights(match_ranges(
                    "Retry budget for webhook dispatch",
                    &[0, 1, 2],
                ))
                .subtitle("#4799 · Task · webhooks")
                .leading(glyph("▣", theme.colors.wi_story))
                .trailing("Work item"),
        )
        .child(
            ListRow::new("command", "Toggle theme")
                .height(size::PALETTE_ROW_COMPACT)
                .leading(glyph("›", theme.colors.text_secondary))
                .trailing(Kbd::parse("cmd-shift-l").expect("static binding")),
        )
        .into_any_element()
}

fn icon_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_wrap()
        .gap(px(space::S12))
        .children(IconName::ALL.iter().map(|icon| {
            Icon::new(*icon)
                .size(size::ICON_LG)
                .color(theme.colors.text_secondary)
        }))
        .into_any_element()
}

fn status_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(
            row().children(Status::ALL.iter().enumerate().map(|(ix, status)| {
                row()
                    .gap(px(space::S4))
                    .child(StatusIcon::new(("status", ix), *status))
                    .child(
                        div()
                            .text_color(theme.colors.text_secondary)
                            .type_style(typography::SMALL, theme)
                            .child(format!("{status:?}")),
                    )
            })),
        )
        .child(
            row().child(Spinner::new()).child(
                div()
                    .text_color(theme.colors.text_secondary)
                    .type_style(typography::SMALL, theme)
                    .child("Looking for az…"),
            ),
        )
        .into_any_element()
}

fn step_indicator_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .children((0..4).map(|active| StepIndicator::new(("steps", active as u64), 3, active)))
        .into_any_element()
}

fn option_card_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    let status = |status: Status, id: &'static str, text: &'static str| {
        row()
            .gap(px(space::S6))
            .child(StatusIcon::new(id, status))
            .child(text)
    };
    div()
        .w_full()
        .p(px(space::S12))
        .rounded(px(radius::LG))
        .bg(theme.colors.bg_overlay)
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(
            OptionCard::new("ready", "Use Azure CLI")
                .description("adoken creates a token for you. Nothing to copy.")
                .badge(Chip::new("Recommended").tone(ChipTone::Accent))
                .status(status(
                    Status::Done,
                    "ready-status",
                    "az 2.83.0 · signed in as ada@contoso.com",
                ))
                .shortcut(Kbd::parse("1").expect("static binding"))
                .selected(true),
        )
        .child(
            OptionCard::new("missing", "Use Azure CLI")
                .description("adoken creates a token for you. Nothing to copy.")
                .status(status(
                    Status::Unavailable,
                    "missing-status",
                    "Azure CLI not found",
                ))
                .shortcut(Kbd::parse("1").expect("static binding"))
                .disabled(true),
        )
        .child(
            OptionCard::new("paste", "Paste a token")
                .description("Create one in the browser, paste it here.")
                .shortcut(Kbd::parse("2").expect("static binding")),
        )
        .into_any_element()
}

fn segmented_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(Segmented::new("level", ["Review", "Read-only"], 0))
        .child(Segmented::new(
            "lifetime",
            ["30 days", "90 days", "180 days"],
            1,
        ))
        .into_any_element()
}

fn chip_story(_window: &mut Window, _cx: &mut App) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(space::S12))
        .child(row().children(ChipTone::ALL.map(|tone| Chip::new(format!("{tone:?}")).tone(tone))))
        .child(
            row()
                .child(Checkbox::new(true))
                .child(Checkbox::new(false))
                .child(Chip::new("Write untested")),
        )
        .into_any_element()
}

fn notice_story(_window: &mut Window, cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    div()
        .w_full()
        .p(px(space::S12))
        .rounded(px(radius::LG))
        .bg(theme.colors.bg_overlay)
        .flex()
        .flex_col()
        .gap(px(space::S8))
        .child(
            Notice::new(ChipTone::Danger, "That token didn't work.")
                .detail("It may be expired, revoked, or for another organization.")
                .child(Button::new("retry", "Try again").icon(IconName::RefreshCw)),
        )
        .child(
            Notice::new(
                ChipTone::Warning,
                "You're offline. Check your connection, then try again.",
            )
            .icon(IconName::WifiOff),
        )
        .child(Notice::new(
            ChipTone::Success,
            "Token works. Signed in as Dana Kim.",
        ))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(space::S6))
                .child(Skeleton::new("skeleton-a").width(180.0))
                .child(Skeleton::new("skeleton-b"))
                .child(Skeleton::new("skeleton-c").width(240.0)),
        )
        .into_any_element()
}

/// Paints one story on the theme's app background.
pub struct StoryView {
    story: &'static Story,
}

impl StoryView {
    pub fn new(story: &'static Story) -> Self {
        Self { story }
    }
}

impl Render for StoryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg) = (theme.colors.bg_app, theme.colors.text_primary);
        div()
            .size_full()
            .p(px(space::S16))
            .bg(bg)
            .text_color(fg)
            .child((self.story.render)(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_names_are_unique_and_file_name_safe() {
        let mut names: Vec<_> = STORIES.iter().map(|story| story.name).collect();
        for name in &names {
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{name}"
            );
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), STORIES.len());
    }
}
