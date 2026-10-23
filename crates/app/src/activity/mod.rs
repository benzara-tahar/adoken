//! Statusbar activity (spec 0016, docs/design/activity.md): the statusbar's
//! sync item and network indicator, and the Activity drawer docked at the
//! bottom of the content column with two tabs, **Sync** (what each kind
//! syncs, recent passes and what they changed) and **Network** (every
//! request adoken sends, filters, totals, copy and export).
//!
//! - [`model::Activity`]: per-window state and the data both tabs read from
//!   the session's [`crate::sync::ActivityHub`] (built by `ActivitySync` in
//!   `crates/sync`: `ledger::NetLog`, `activity::Tracker`).
//! - [`panel::ActivityPanel`]: the drawer's view, created on first open.
//! - [`statusbar`]: the two statusbar buttons and the 900 px collapse.
//! - Pure models: [`net`] (rows, filters, tail), [`indicator`] (sampling,
//!   announcements), [`passes`] (Sync tab copy, folding), [`settings`]
//!   (`[activity]`, drawer geometry).
//!
//! Keys (FR-3): `⌘⇧A` toggles the drawer on its last tab; inside it
//! `⌃Tab`/`⌃⇧Tab` switch tabs, `⌘⇧↩` maximizes, `Esc` closes and returns
//! focus, `F6` cycles content → drawer → toasts. Single letters (`F`, `T`,
//! `j`/`k`) and `/` live in `ActivityPanel && !TextInput`.

pub mod fixtures;
pub mod indicator;
pub mod model;
pub mod net;
mod net_tab;
pub mod panel;
pub mod passes;
pub mod settings;
pub mod statusbar;
mod sync_tab;

use gpui::{KeyBinding, actions};

use crate::palette::PaletteCommand;

pub use model::{Activity, ActivityEvent};
pub use panel::ActivityPanel;
pub use settings::Tab;

actions!(
    activity,
    [
        /// Open or close the Activity drawer on its last tab.
        Toggle,
        /// Open the Activity drawer on Sync.
        ShowSync,
        /// Open the Activity drawer on Network.
        ShowNetwork,
        /// Close the Activity drawer.
        Hide,
        /// Save the network log as JSON Lines.
        ExportNetworkLog,
        /// Clear the network log's filters.
        ClearFilters,
        /// Next drawer tab.
        NextTab,
        /// Previous drawer tab.
        PreviousTab,
        /// Maximize or restore the drawer.
        ToggleMaximize,
        /// Unwind: close a detail, leave the filter, close the drawer.
        Dismiss,
        /// Cycle focus: content, drawer, toasts.
        CycleRegion,
        /// Network: failed requests only.
        ToggleFailedOnly,
        /// Network: Requests ⇄ Totals.
        ToggleTotals,
        /// Network: focus the filter field.
        FocusFilter,
        /// Network: copy the selected request(s).
        CopyRows,
        /// Move the cursor down.
        SelectNext,
        /// Move the cursor up.
        SelectPrevious,
        /// First row.
        SelectFirst,
        /// Last row (Network: resume following the tail).
        SelectLast,
        /// Page down.
        PageDown,
        /// Page up.
        PageUp,
        /// Open, expand, or toggle the row's detail.
        Confirm,
        /// Expand a pass / Network: Totals.
        Right,
        /// Collapse a pass / Network: Requests.
        Left,
        /// Sync: open the selected changed item in Azure DevOps.
        OpenInBrowser,
        /// Splitter: taller by 24 px.
        GrowSmall,
        /// Splitter: shorter by 24 px.
        ShrinkSmall,
        /// Splitter: taller by 96 px.
        GrowLarge,
        /// Splitter: shorter by 96 px.
        ShrinkLarge,
        /// Splitter: tallest.
        Maximize,
        /// Splitter: shortest.
        Minimize,
    ]
);

/// `⌘⇧A` (FR-3, Q-7).
pub const TOGGLE_KEYS: &str = "secondary-shift-a";
pub const CLEAR_FILTERS_KEYS: &str = "secondary-backspace";
pub const MAXIMIZE_KEYS: &str = "secondary-shift-enter";
/// Key context of the drawer.
pub const CONTEXT: &str = "ActivityPanel";
/// Drawer keys that would shadow typing: only without a text field.
const LIST_CONTEXT: &str = "ActivityPanel && !TextInput";
/// The focused resize handle.
const SPLITTER_CONTEXT: &str = "ActivitySplitter";

pub fn bindings() -> Vec<KeyBinding> {
    let list = Some(LIST_CONTEXT);
    let splitter = Some(SPLITTER_CONTEXT);
    vec![
        KeyBinding::new(TOGGLE_KEYS, Toggle, Some("Workspace")),
        KeyBinding::new("f6", CycleRegion, Some("Workspace")),
        KeyBinding::new("ctrl-tab", NextTab, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, Some(CONTEXT)),
        KeyBinding::new(MAXIMIZE_KEYS, ToggleMaximize, Some(CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(CONTEXT)),
        KeyBinding::new(CLEAR_FILTERS_KEYS, ClearFilters, Some(CONTEXT)),
        KeyBinding::new("f", ToggleFailedOnly, list),
        KeyBinding::new("t", ToggleTotals, list),
        KeyBinding::new("/", FocusFilter, list),
        KeyBinding::new("secondary-c", CopyRows, list),
        KeyBinding::new("down", SelectNext, list),
        KeyBinding::new("j", SelectNext, list),
        KeyBinding::new("up", SelectPrevious, list),
        KeyBinding::new("k", SelectPrevious, list),
        KeyBinding::new("home", SelectFirst, list),
        KeyBinding::new("end", SelectLast, list),
        KeyBinding::new("pagedown", PageDown, list),
        KeyBinding::new("pageup", PageUp, list),
        KeyBinding::new("enter", Confirm, list),
        KeyBinding::new("right", Right, list),
        KeyBinding::new("left", Left, list),
        KeyBinding::new("secondary-enter", OpenInBrowser, list),
        KeyBinding::new("up", GrowSmall, splitter),
        KeyBinding::new("down", ShrinkSmall, splitter),
        KeyBinding::new("shift-up", GrowLarge, splitter),
        KeyBinding::new("shift-down", ShrinkLarge, splitter),
        KeyBinding::new("home", Maximize, splitter),
        KeyBinding::new("end", Minimize, splitter),
    ]
}

/// Palette commands (FR-3; command-palette.md §4.5). "Show sync activity"
/// and "Show network log" show `⌘⇧A`, the key that opens the drawer.
pub fn commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand {
            title: "Activity: Show sync activity",
            keys: Some(TOGGLE_KEYS),
            action: Box::new(ShowSync),
        },
        PaletteCommand {
            title: "Activity: Show network log",
            keys: Some(TOGGLE_KEYS),
            action: Box::new(ShowNetwork),
        },
        PaletteCommand {
            title: "Activity: Hide",
            keys: None,
            action: Box::new(Hide),
        },
        PaletteCommand {
            title: "Network log: Export…",
            keys: None,
            action: Box::new(ExportNetworkLog),
        },
        PaletteCommand {
            title: "Network log: Clear filters",
            keys: Some(CLEAR_FILTERS_KEYS),
            action: Box::new(ClearFilters),
        },
    ]
}

/// `adoken-network-2026-10-01-1041.jsonl` (activity.md §5.7).
pub fn export_file_name(now: adoken_model::Millis, offset: time::UtcOffset) -> String {
    let t = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(now) * 1_000_000)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(offset);
    format!(
        "adoken-network-{:04}-{:02}-{:02}-{:02}{:02}.jsonl",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    )
}

/// Write `entries` as the FR-25 JSON Lines export. Blocking: call off the
/// UI thread.
pub fn export_to(
    path: &std::path::Path,
    entries: &[adoken_sync::ledger::Entry],
    now: adoken_model::Millis,
) -> std::io::Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    adoken_sync::ledger::export_jsonl(entries, &mut out, env!("CARGO_PKG_VERSION"), now)?;
    std::io::Write::flush(&mut out)
}

/// The export's toast: "Exported 1,284 requests" [Show in Finder], or the
/// OS reason with [Try again] (activity.md §5.7).
pub fn export_done(result: std::io::Result<usize>, path: std::path::PathBuf, cx: &mut gpui::App) {
    use adoken_ui::{IconName, Toast, ToastAction, ToastStack, ToastTone};
    let toast = match result {
        Ok(count) => Toast::new(
            "activity-export",
            IconName::Download,
            if count == 1 {
                "Exported 1 request".to_owned()
            } else {
                format!("Exported {} requests", net::count_text(count))
            },
        )
        .action(ToastAction::new("Show in Finder", move |_, cx| {
            cx.reveal_path(&path)
        })),
        Err(error) => Toast::new(
            "activity-export",
            IconName::CircleAlert,
            format!("Couldn't save the export: {error}. The log is unchanged."),
        )
        .tone(ToastTone::Warning)
        .action(ToastAction::new("Try again", |window, cx| {
            window.dispatch_action(Box::new(ExportNetworkLog), cx)
        })),
    };
    ToastStack::show(toast, cx);
}

/// Opens the drawer from outside the main window (Settings › Sync's "Open
/// sync activity" link); installed by the shell.
type OpenFn = std::rc::Rc<dyn Fn(Tab, &mut gpui::App)>;

struct Opener(OpenFn);

impl gpui::Global for Opener {}

/// The main window's height as last laid out, as `f32` bits (0: unknown).
/// Settings reports an out-of-range `panel_height` against it (spec 0009
/// FR-5).
static MAIN_WINDOW_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// `[activity]` as Settings just edited it, applied before the file write
/// lands (spec 0009 FR-3: next frame).
pub(crate) struct LiveSettings(pub settings::ActivitySettings);

impl gpui::Global for LiveSettings {}

pub fn apply_live(table: &toml::Table, cx: &mut gpui::App) {
    let mut root = toml::Table::new();
    root.insert(settings::TABLE.into(), toml::Value::Table(table.clone()));
    let (read, _) = settings::parse(&root, main_window_height());
    cx.set_global(LiveSettings(read));
}

pub fn set_opener(open: impl Fn(Tab, &mut gpui::App) + 'static, cx: &mut gpui::App) {
    cx.set_global(Opener(std::rc::Rc::new(open)));
}

/// Bring the main window forward with the drawer open on `tab`.
pub fn request_open(tab: Tab, cx: &mut gpui::App) {
    if let Some(open) = cx.try_global::<Opener>().map(|o| o.0.clone()) {
        open(tab, cx);
    }
}

pub fn remember_window_height(height: f32) {
    MAIN_WINDOW_HEIGHT.store(height.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

pub fn main_window_height() -> Option<f32> {
    let bits = MAIN_WINDOW_HEIGHT.load(std::sync::atomic::Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_file_name_uses_local_time() {
        let offset = time::UtcOffset::from_hms(2, 0, 0).unwrap();
        // 2026-09-25 09:30 UTC.
        assert_eq!(
            export_file_name(fixtures::NOW, offset),
            "adoken-network-2026-09-25-1130.jsonl"
        );
    }
}
