//! Settings (spec 0009): the ⌘, window and the app-wide `settings.toml`
//! plumbing every module uses.
//!
//! File API (all no-ops without a settings path, e.g. gallery stories):
//! - [`write_table`] / [`remove_table`]: replace one top-level table. Writes
//!   are queued, coalesced per table (a dragging slider writes its latest
//!   value once), and applied by [`adoken_settings::update`] on the
//!   background executor: format-preserving, serialized, atomic.
//! - [`observe`]: re-read your table after any write completes or after the
//!   file changes on disk (polled every [`POLL`]). [`generation`] counts
//!   those changes; [`path`] is the file.
//!
//! Cold start: [`init`] only registers globals; the first stat runs one
//! [`POLL`] after launch and nothing is read until a view asks.

pub mod model;
pub mod notify_rules;
mod pages;
pub mod sync_prefs;
mod view;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use adoken_theme::background::BackgroundSettings;
use gpui::{
    App, AppContext as _, BorrowAppContext, Bounds, Context, Global, KeyBinding, Subscription,
    TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, actions, point, px, size,
};

use crate::palette::PaletteCommand;
pub use model::{Motion, Tab};
pub use view::{AccountStory, SettingsView};

actions!(
    settings,
    [
        /// Open Settings (spec 0009).
        OpenSettings,
        /// Settings: next tab.
        NextTab,
        /// Settings: previous tab.
        PreviousTab,
        /// Settings: focus the search box.
        FocusSearch,
        /// Settings: close the window.
        CloseSettings,
    ]
);

pub const OPEN_KEYS: &str = "secondary-,";
pub const NEXT_TAB_KEYS: &str = "secondary-]";
pub const PREVIOUS_TAB_KEYS: &str = "secondary-[";
pub const SEARCH_KEYS: &str = "secondary-f";
pub const CLOSE_KEYS: &str = "secondary-w";
/// Key context of the Settings window.
pub const CONTEXT: &str = "Settings";
/// How often the file is checked for outside edits.
pub const POLL: Duration = Duration::from_secs(2);
/// Debug/screenshots: open Settings on this tab at launch (`general`,
/// `sounds`, …, see [`Tab::id`]).
pub const OPEN_ENV: &str = "ADOKEN_SETTINGS";
const WINDOW_SIZE: (f32, f32) = (940.0, 680.0);
const WINDOW_MIN_SIZE: (f32, f32) = (720.0, 520.0);

pub fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new(OPEN_KEYS, OpenSettings, None),
        KeyBinding::new(NEXT_TAB_KEYS, NextTab, Some(CONTEXT)),
        KeyBinding::new(PREVIOUS_TAB_KEYS, PreviousTab, Some(CONTEXT)),
        KeyBinding::new(SEARCH_KEYS, FocusSearch, Some(CONTEXT)),
        KeyBinding::new(CLOSE_KEYS, CloseSettings, Some(CONTEXT)),
    ]
}

/// Commands Settings contributes to the palette.
pub fn commands() -> Vec<PaletteCommand> {
    vec![PaletteCommand {
        title: "Settings…",
        keys: Some(OPEN_KEYS),
        action: Box::new(OpenSettings),
    }]
}

/// Bumped after every completed write and every outside edit.
#[derive(Default)]
struct Generation(u64);

impl Global for Generation {}

/// Where settings live and what the writer is doing.
struct SettingsFile {
    path: Option<PathBuf>,
    demo: bool,
    writer: Rc<RefCell<Writer>>,
}

impl Global for SettingsFile {}

#[derive(Default)]
struct Writer {
    queue: Vec<Op>,
    writing: bool,
    /// Modification time after our last write or last seen outside edit.
    mtime: Option<SystemTime>,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Set(String, toml::Table),
    Remove(String),
}

impl Op {
    fn table(&self) -> &str {
        match self {
            Op::Set(name, _) | Op::Remove(name) => name,
        }
    }
}

/// Queue `op`, dropping any queued op for the same table (latest wins).
fn enqueue(queue: &mut Vec<Op>, op: Op) {
    queue.retain(|queued| queued.table() != op.table());
    queue.push(op);
}

/// The open Settings window and the tab to reopen on.
#[derive(Default)]
struct SettingsWindow {
    handle: Option<WindowHandle<SettingsView>>,
    last_tab: Tab,
}

impl Global for SettingsWindow {}

/// `[general] motion`, once read (`None` = follow the system).
#[derive(Default)]
struct MotionOverride(Option<bool>);

impl Global for MotionOverride {}

/// Register the settings globals, the ⌘, handler, and the outside-edit
/// poll. `path` is `settings.toml` (demo: inside the demo directory). No
/// blocking IO: the one read happens on the background executor.
pub fn init(path: PathBuf, demo: bool, cx: &mut App) {
    cx.set_global(Generation::default());
    cx.set_global(SettingsWindow::default());
    cx.set_global(MotionOverride::default());
    cx.set_global(SettingsFile {
        path: Some(path),
        demo,
        writer: Rc::default(),
    });
    cx.on_action(|_: &OpenSettings, cx| {
        let tab = cx.global::<SettingsWindow>().last_tab;
        open(tab, cx);
    });
    // Outside edits and other modules' writes reach the parts of the app
    // no view owns: motion, the sound engine, the background controller.
    cx.observe_global::<Generation>(|cx| reload_live(false, cx))
        .detach();
    reload_live(true, cx);
    poll(cx);
}

/// Open Settings on `tab`, or bring the open window forward on it.
pub fn open(tab: Tab, cx: &mut App) {
    remember_tab(tab, cx);
    let existing = cx
        .try_global::<SettingsWindow>()
        .and_then(|window| window.handle);
    if let Some(handle) = existing
        && handle
            .update(cx, |view, window, cx| {
                view.show_tab(tab, window, cx);
                window.activate_window();
            })
            .is_ok()
    {
        return;
    }
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(WINDOW_MIN_SIZE.0), px(WINDOW_MIN_SIZE.1))),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            app_id: Some("dev.adoken.app".into()),
            ..Default::default()
        },
        |window, cx| {
            let view = cx.new(|cx| SettingsView::new(tab, window, cx));
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
            view
        },
    );
    match opened {
        Ok(handle) => {
            if cx.has_global::<SettingsWindow>() {
                cx.global_mut::<SettingsWindow>().handle = Some(handle);
            }
        }
        Err(error) => log::warn!("settings: couldn't open the window: {error}"),
    }
}

/// Open Settings on `tab` with `query` in its search box (a palette command
/// jumping to one setting, e.g. `> Code font…`).
pub fn open_with_query(tab: Tab, query: &str, cx: &mut App) {
    open(tab, cx);
    let Some(handle) = cx
        .try_global::<SettingsWindow>()
        .and_then(|window| window.handle)
    else {
        return;
    };
    handle
        .update(cx, |view, window, cx| view.search_for(query, window, cx))
        .ok();
}

/// `ADOKEN_SETTINGS=<tab>`: open Settings at launch (screenshots, demos).
pub fn open_from_env(cx: &mut App) {
    if let Some(tab) = std::env::var(OPEN_ENV).ok().as_deref().and_then(Tab::parse) {
        open(tab, cx);
    }
}

pub(crate) fn remember_tab(tab: Tab, cx: &mut App) {
    if cx.has_global::<SettingsWindow>() {
        cx.global_mut::<SettingsWindow>().last_tab = tab;
    }
}

/// The tab the Settings window shows while it is the key window (the
/// global shortcut's test-from-Settings case, spec 0021 FR-8).
pub(crate) fn key_tab(cx: &mut App) -> Option<Tab> {
    let handle = cx.try_global::<SettingsWindow>()?.handle?;
    handle
        .update(cx, |view, window, _| {
            window.is_window_active().then_some(view.tab)
        })
        .ok()
        .flatten()
}

/// `[general] motion` as read last; `None` follows the system.
pub fn reduce_motion_override(cx: &App) -> Option<bool> {
    cx.try_global::<MotionOverride>().and_then(|m| m.0)
}

/// Apply a motion choice now (env `ADOKEN_REDUCE_MOTION` still wins).
pub fn apply_motion(motion: Motion, cx: &mut App) {
    if cx.has_global::<MotionOverride>() {
        cx.global_mut::<MotionOverride>().0 = motion.reduce();
    }
    let reduce = crate::reduce_motion_preference(cx);
    if cx.reduce_motion() != reduce {
        cx.set_reduce_motion(reduce);
    }
}

/// Read the file off the UI thread and apply the app-wide parts. `first`:
/// the launch read, which only applies motion and `[sync]` (sounds and
/// backgrounds load with the store, see `shell`).
fn reload_live(first: bool, cx: &mut App) {
    let Some(path) = path(cx) else {
        return;
    };
    let demo = is_demo(cx);
    let job = cx.background_executor().spawn(async move {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str::<toml::Table>(&text).ok()
    });
    cx.spawn(async move |cx| {
        let Some(doc) = job.await else {
            return;
        };
        cx.update(|cx| {
            apply_motion(Motion::from_document(&doc).0, cx);
            crate::sync::set_prefs(sync_prefs::parse(&doc).0, cx);
            // The code font paints from the first frame on; skipped while
            // our own write is in flight (the file is behind the screen).
            if !is_writing(cx) {
                let code_font = adoken_theme::CodeFontSettings::from_document(&doc).0;
                crate::code_font::apply(code_font, cx);
            }
            if first {
                return;
            }
            let sound = model::table_of(&doc, adoken_sound::SETTINGS_TABLE);
            crate::sound::install(adoken_sound::SoundSettings::from_table(sound).0, cx);
            // Demo mode without a table shows the generated illustration.
            if let Some(background) = doc
                .get(adoken_theme::background::SETTINGS_TABLE)
                .and_then(toml::Value::as_table)
            {
                crate::background::set_settings(
                    BackgroundSettings::from_table(background.clone()).0,
                    cx,
                );
            } else if !demo {
                crate::background::set_settings(BackgroundSettings::default(), cx);
            }
        });
    })
    .detach();
}

/// The settings file, when the app has one.
pub fn path(cx: &App) -> Option<PathBuf> {
    cx.try_global::<SettingsFile>()
        .and_then(|file| file.path.clone())
}

/// Demo mode (`ADOKEN_DEMO=1`): fake data, no account.
pub fn is_demo(cx: &App) -> bool {
    cx.try_global::<SettingsFile>()
        .is_some_and(|file| file.demo)
}

/// Changes so far (writes and outside edits).
pub fn generation(cx: &App) -> u64 {
    cx.try_global::<Generation>().map_or(0, |g| g.0)
}

/// A write is queued or running: in-memory state is ahead of the file.
pub fn is_writing(cx: &App) -> bool {
    cx.try_global::<SettingsFile>().is_some_and(|file| {
        let writer = file.writer.borrow();
        writer.writing || !writer.queue.is_empty()
    })
}

/// The last write's error (syntax error in the file, IO), if it failed.
pub fn write_error(cx: &App) -> Option<String> {
    cx.try_global::<SettingsFile>()
        .and_then(|file| file.writer.borrow().error.clone())
}

/// Call `f` after every completed write and every outside edit.
pub fn observe<T: 'static>(
    cx: &mut Context<T>,
    f: impl FnMut(&mut T, &mut Context<T>) + 'static,
) -> Subscription {
    cx.observe_global::<Generation>(f)
}

/// Replace the top-level `[name]` table (key-wise merge; see module docs).
pub fn write_table(cx: &mut App, name: &str, table: toml::Table) {
    submit(Op::Set(name.to_owned(), table), cx);
}

/// Remove the top-level `[name]` table.
pub fn remove_table(cx: &mut App, name: &str) {
    submit(Op::Remove(name.to_owned()), cx);
}

fn submit(op: Op, cx: &mut App) {
    let Some(file) = cx.try_global::<SettingsFile>() else {
        return;
    };
    let Some(path) = file.path.clone() else {
        return;
    };
    let writer = file.writer.clone();
    {
        let mut state = writer.borrow_mut();
        enqueue(&mut state.queue, op);
        if state.writing {
            return;
        }
        state.writing = true;
    }
    cx.spawn(async move |cx| {
        loop {
            let ops = std::mem::take(&mut writer.borrow_mut().queue);
            if ops.is_empty() {
                break;
            }
            let path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    adoken_settings::update(&path, |document| {
                        for op in &ops {
                            match op {
                                Op::Set(name, table) => {
                                    adoken_settings::set_table(document, name, table)?
                                }
                                Op::Remove(name) => adoken_settings::remove_table(document, name),
                            }
                        }
                        Ok(())
                    })
                    .map(|_| modified(&path))
                    .map_err(|error| error.to_string())
                })
                .await;
            let mut state = writer.borrow_mut();
            match result {
                Ok(mtime) => {
                    state.mtime = mtime;
                    state.error = None;
                }
                Err(error) => {
                    log::warn!("settings: {error}");
                    state.error = Some(error);
                }
            }
        }
        writer.borrow_mut().writing = false;
        cx.update(bump);
    })
    .detach();
}

fn modified(path: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn bump(cx: &mut App) {
    cx.update_global::<Generation, _>(|generation, _| generation.0 += 1);
}

/// Every [`POLL`], stat the file off the UI thread; an mtime we didn't
/// write means an outside edit.
fn poll(cx: &mut App) {
    let Some(file) = cx.try_global::<SettingsFile>() else {
        return;
    };
    let Some(path) = file.path.clone() else {
        return;
    };
    let writer = file.writer.clone();
    cx.spawn(async move |cx| {
        // Baseline: the file as it is at launch is not an edit.
        let stat_path = path.clone();
        let baseline = cx
            .background_executor()
            .spawn(async move { modified(&stat_path) })
            .await;
        {
            let mut state = writer.borrow_mut();
            if !state.writing && state.mtime.is_none() {
                state.mtime = baseline;
            }
        }
        loop {
            cx.background_executor().timer(POLL).await;
            if writer.borrow().writing {
                continue;
            }
            let stat_path = path.clone();
            let mtime = cx
                .background_executor()
                .spawn(async move { modified(&stat_path) })
                .await;
            let changed = {
                let mut state = writer.borrow_mut();
                let changed = !state.writing && state.mtime != mtime;
                if changed {
                    state.mtime = mtime;
                }
                changed
            };
            if changed {
                cx.update(bump);
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, n: i64) -> Op {
        let mut table = toml::Table::new();
        table.insert("n".into(), n.into());
        Op::Set(name.into(), table)
    }

    #[test]
    fn queued_writes_coalesce_per_table() {
        let mut queue = Vec::new();
        enqueue(&mut queue, set("sound", 1));
        enqueue(&mut queue, set("background", 1));
        enqueue(&mut queue, set("sound", 2));
        enqueue(&mut queue, Op::Remove("background".into()));
        assert_eq!(
            queue,
            vec![set("sound", 2), Op::Remove("background".into())]
        );
    }

    #[test]
    fn demo_background_table_matches_what_demo_mode_shows() {
        // The first Backgrounds edit in demo mode starts from this table;
        // it must equal the default the app paints, or the image would jump.
        let path = "/tmp/adoken-demo/backgrounds/demo-illustration-v1.png";
        assert_eq!(
            BackgroundSettings::from_table(model::demo_background_table(path)).0,
            crate::background::demo_settings(std::path::Path::new(path))
        );
    }

    #[test]
    fn bindings_parse() {
        for keys in [
            OPEN_KEYS,
            NEXT_TAB_KEYS,
            PREVIOUS_TAB_KEYS,
            SEARCH_KEYS,
            CLOSE_KEYS,
        ] {
            assert!(adoken_ui::Kbd::parse(keys).is_ok(), "{keys}");
        }
    }
}
