//! The app's themes: built-ins plus `<settings dir>/themes/*.adoken-theme.toml`,
//! the `[appearance]` choice (one theme id), and hot reload.
//!
//! Threading: [`init`] only installs the default built-in (no IO, cold
//! start). [`load`] reads `settings.toml` and the themes folder on the
//! background executor, then polls both every [`POLL`] (a directory stat,
//! off the UI thread) so dropped or edited theme files apply within a
//! second (spec 0007 AC-3) and outside edits to `[appearance]` are picked
//! up. A file that stops parsing keeps its last good version and reports
//! the error (tokens.md §9.1).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use adoken_theme::{
    APPEARANCE_TABLE, Appearance, AppearanceSettings, LoadedTheme, THEME_FILE_SUFFIX, ThemeError,
    ThemeFamily, ThemeRegistry, ThemeVariant, load_theme_dir,
};
use gpui::{App, Global, Task};

use crate::{background, settings};

/// How often the themes folder and settings generation are checked.
pub const POLL: Duration = Duration::from_millis(500);
/// Folder name next to `settings.toml`.
pub const THEMES_DIR: &str = "themes";

/// One user theme file and how it loaded.
#[derive(Debug, Clone, PartialEq)]
pub struct UserFile {
    pub path: PathBuf,
    /// The family in use from this file (possibly its last good version).
    pub family_id: Option<String>,
    /// Warnings (contrast, unknown keys) and per-key errors.
    pub issues: Vec<String>,
    /// Why the current file content could not be used at all.
    pub error: Option<String>,
}

pub struct ThemeLibrary {
    registry: ThemeRegistry,
    settings: AppearanceSettings,
    /// `[appearance]` as last read: written back with only the theme keys
    /// changed (the settings writer replaces a table's key set).
    appearance_table: toml::Table,
    settings_issues: Vec<String>,
    /// macOS appearance at launch: the default when no theme was chosen
    /// (or the chosen one is gone), and how an old light/dark pair
    /// resolves. Not followed afterwards (user decision 2026-09-30).
    system: Appearance,
    files: Vec<UserFile>,
    last_good: HashMap<PathBuf, ThemeFamily>,
    themes_dir: Option<PathBuf>,
    /// Bumped whenever the registry or file list changes.
    revision: u64,
    stamp: Vec<Stamp>,
    settings_generation: u64,
}

impl Global for ThemeLibrary {}

impl ThemeLibrary {
    fn new(system: Appearance) -> Self {
        Self {
            registry: ThemeRegistry::with_builtins(),
            settings: AppearanceSettings::default_for(system),
            appearance_table: toml::Table::new(),
            settings_issues: Vec::new(),
            system,
            files: Vec::new(),
            last_good: HashMap::new(),
            themes_dir: None,
            revision: 0,
            stamp: Vec::new(),
            settings_generation: 0,
        }
    }

    pub fn registry(&self) -> &ThemeRegistry {
        &self.registry
    }

    pub fn settings(&self) -> &AppearanceSettings {
        &self.settings
    }

    pub fn settings_issues(&self) -> &[String] {
        &self.settings_issues
    }

    pub fn files(&self) -> &[UserFile] {
        &self.files
    }

    pub fn themes_dir(&self) -> Option<&Path> {
        self.themes_dir.as_deref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The theme painted: the chosen one, or the default when it names a
    /// missing (e.g. deleted user) theme.
    pub fn active(&self) -> &ThemeVariant {
        self.registry.resolve(&self.settings.theme, self.system)
    }
}

/// Install the default built-in for `system` (no IO: first frame budget).
pub fn init(system: Appearance, cx: &mut App) {
    cx.set_global(ThemeLibrary::new(system));
    install_active(cx);
}

/// A library with the given selection, user families and file statuses
/// (stories, tests). Paints nothing: the story's theme stays installed.
pub fn install_for_story(
    settings: AppearanceSettings,
    user: Vec<ThemeFamily>,
    files: Vec<UserFile>,
    cx: &mut App,
) {
    let mut library = ThemeLibrary::new(Appearance::Dark);
    library.settings = settings;
    for family in user {
        let _ = library.registry.insert(family);
    }
    library.files = files;
    cx.set_global(library);
}

/// Read `[appearance]` and the themes folder next to `settings.toml`, then
/// keep watching both. No-op without a settings path.
pub fn load(cx: &mut App) {
    let Some(settings_path) = settings::path(cx) else {
        return;
    };
    let Some(dir) = settings_path.parent().map(|dir| dir.join(THEMES_DIR)) else {
        return;
    };
    if cx.try_global::<ThemeLibrary>().is_none() {
        init(Appearance::Dark, cx);
    }
    cx.global_mut::<ThemeLibrary>().themes_dir = Some(dir);
    cx.spawn(async move |cx| {
        let (settings, themes) = cx.update(|cx| (reload_settings(cx), reload_themes(cx)));
        settings.await;
        themes.await;
        loop {
            cx.background_executor().timer(POLL).await;
            let Some((dir, stamp)) = cx.update(|cx| {
                let library = cx.global::<ThemeLibrary>();
                library
                    .themes_dir
                    .clone()
                    .map(|dir| (dir, library.stamp.clone()))
            }) else {
                break;
            };
            let current = cx
                .background_executor()
                .spawn(async move { stamp_dir(&dir) })
                .await;
            let (settings, themes) = cx.update(|cx| {
                let settings_changed = settings::generation(cx)
                    != cx.global::<ThemeLibrary>().settings_generation
                    && !settings::is_writing(cx);
                (
                    settings_changed.then(|| reload_settings(cx)),
                    (current != stamp).then(|| reload_themes(cx)),
                )
            });
            if let Some(task) = settings {
                task.await;
            }
            if let Some(task) = themes {
                task.await;
            }
        }
    })
    .detach();
}

/// Re-read `[appearance]` from `settings.toml` (off the UI thread).
fn reload_settings(cx: &mut App) -> Task<()> {
    let Some(path) = settings::path(cx) else {
        return Task::ready(());
    };
    let generation = settings::generation(cx);
    let system = cx.global::<ThemeLibrary>().system;
    cx.spawn(async move |cx| {
        let read = cx
            .background_executor()
            .spawn(async move {
                match std::fs::read_to_string(&path) {
                    Ok(text) => AppearanceSettings::from_settings_toml(&text, system)
                        .and_then(|(settings, issues)| {
                            Ok((settings, issues, AppearanceSettings::table_of(&text)?))
                        })
                        .map_err(|error| format!("settings.toml: {}", error.message())),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok((
                        AppearanceSettings::default_for(system),
                        Vec::new(),
                        toml::Table::new(),
                    )),
                    Err(error) => Err(format!("settings.toml: {error}")),
                }
            })
            .await;
        cx.update(|cx| {
            let library = cx.global_mut::<ThemeLibrary>();
            library.settings_generation = generation;
            match read {
                Ok((settings, issues, table)) => {
                    library.settings_issues = issues;
                    library.appearance_table = table;
                    if library.settings != settings {
                        library.settings = settings;
                        library.revision += 1;
                        install_active(cx);
                    }
                }
                // A syntax error elsewhere in the file: keep what we have.
                Err(error) => library.settings_issues = vec![error],
            }
        });
    })
}

/// Reload every user theme file (off the UI thread) and repaint.
pub fn reload_themes(cx: &mut App) -> Task<()> {
    let Some(dir) = cx.global::<ThemeLibrary>().themes_dir.clone() else {
        return Task::ready(());
    };
    cx.spawn(async move |cx| {
        let (stamp, results) = cx
            .background_executor()
            .spawn(async move {
                (
                    stamp_dir(&dir),
                    load_theme_dir(&dir, ThemeRegistry::builtin()),
                )
            })
            .await;
        cx.update(|cx| {
            let library = cx.global_mut::<ThemeLibrary>();
            let (registry, files) = merge_user_themes(results, &mut library.last_good);
            library.registry = registry;
            library.files = files;
            library.stamp = stamp;
            library.revision += 1;
            install_active(cx);
        });
    })
}

/// Built-ins plus the loaded files. A file that fails keeps its last good
/// family (and reports why); a family whose ids clash is rejected.
pub fn merge_user_themes(
    results: Vec<(PathBuf, Result<LoadedTheme, ThemeError>)>,
    last_good: &mut HashMap<PathBuf, ThemeFamily>,
) -> (ThemeRegistry, Vec<UserFile>) {
    let mut registry = ThemeRegistry::with_builtins();
    let mut files = Vec::with_capacity(results.len());
    let present: Vec<PathBuf> = results.iter().map(|(path, _)| path.clone()).collect();
    last_good.retain(|path, _| present.contains(path));
    for (path, result) in results {
        let mut file = UserFile {
            path: path.clone(),
            family_id: None,
            issues: Vec::new(),
            error: None,
        };
        let family = match result {
            Ok(loaded) => {
                file.issues = loaded.issues.iter().map(ToString::to_string).collect();
                Some(loaded.family)
            }
            Err(error) => {
                file.error = Some(error.to_string());
                last_good.get(&path).cloned()
            }
        };
        if let Some(family) = family {
            match registry.insert(family.clone()) {
                Ok(()) => {
                    file.family_id = Some(family.id.clone());
                    last_good.insert(path, family);
                }
                Err(error) => file.error = Some(error.to_string()),
            }
        }
        files.push(file);
    }
    (registry, files)
}

/// `(path, modified, size)` of every theme file, sorted: cheap change check.
type Stamp = (PathBuf, Option<SystemTime>, u64);

fn stamp_dir(dir: &Path) -> Vec<Stamp> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut stamp: Vec<Stamp> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(THEME_FILE_SUFFIX))
        })
        .map(|entry| {
            let meta = entry.metadata().ok();
            (
                entry.path(),
                meta.as_ref().and_then(|m| m.modified().ok()),
                meta.map_or(0, |m| m.len()),
            )
        })
        .collect();
    stamp.sort();
    stamp
}

/// Paint [`ThemeLibrary::active`] (with the background's translucency).
pub fn install_active(cx: &mut App) {
    let Some(library) = cx.try_global::<ThemeLibrary>() else {
        return;
    };
    let variant = library.active().clone();
    let installed = cx
        .try_global::<adoken_ui::Theme>()
        .is_some_and(|theme| theme.id == variant.id.as_str() && theme.raw == variant.colors);
    if !installed || background::is_showing(cx) {
        background::install_theme(&variant, cx);
    }
}

fn persist(cx: &mut App) {
    let library = cx.global_mut::<ThemeLibrary>();
    library.settings.write_into(&mut library.appearance_table);
    let table = library.appearance_table.clone();
    settings::write_table(cx, APPEARANCE_TABLE, table);
}

/// Change other `[appearance]` keys (the code font) in the same table the
/// theme key is written from, then save it: the settings writer replaces a
/// table's key set, so two copies of the table would drop each other's keys.
pub fn update_appearance(cx: &mut App, change: impl FnOnce(&mut toml::Table)) {
    if !cx.has_global::<ThemeLibrary>() {
        return;
    }
    let library = cx.global_mut::<ThemeLibrary>();
    change(&mut library.appearance_table);
    let table = library.appearance_table.clone();
    settings::write_table(cx, APPEARANCE_TABLE, table);
}

/// Paint `id` now and persist it (picker click / Enter).
pub fn commit(id: &str, cx: &mut App) {
    let library = cx.global_mut::<ThemeLibrary>();
    if library.registry.variant(id).is_none() {
        return;
    }
    if library.settings.theme != id {
        library.settings.theme = id.to_owned();
        library.revision += 1;
        install_active(cx);
    }
    persist(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use adoken_theme::load_theme_dir;

    const THEME: &str = "name = \"Mine\"\n[[variant]]\nappearance = \"dark\"\n[variant.colors]\nbg.app = \"#101010\"\n";

    #[test]
    fn failing_reload_keeps_the_last_good_family_and_reports_why() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.adoken-theme.toml");
        std::fs::write(&file, THEME).unwrap();
        let mut last_good = HashMap::new();

        let (registry, files) = merge_user_themes(
            load_theme_dir(dir.path(), ThemeRegistry::builtin()),
            &mut last_good,
        );
        assert!(registry.variant("mine-dark").is_some());
        assert_eq!(files[0].family_id.as_deref(), Some("mine"));

        std::fs::write(&file, "name = \"Mine\"\n[[variant]\n").unwrap();
        let (registry, files) = merge_user_themes(
            load_theme_dir(dir.path(), ThemeRegistry::builtin()),
            &mut last_good,
        );
        assert!(registry.variant("mine-dark").is_some(), "last good kept");
        assert!(files[0].error.as_deref().unwrap().contains("TOML"));

        std::fs::remove_file(&file).unwrap();
        let (registry, files) = merge_user_themes(
            load_theme_dir(dir.path(), ThemeRegistry::builtin()),
            &mut last_good,
        );
        assert!(registry.variant("mine-dark").is_none());
        assert!(files.is_empty() && last_good.is_empty());
    }

    #[test]
    fn a_user_file_cannot_shadow_a_built_in() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("dracula.adoken-theme.toml"),
            THEME.replace("appearance = \"dark\"", "appearance = \"dark\"\nid = \"x\""),
        )
        .unwrap();
        let (registry, files) = merge_user_themes(
            load_theme_dir(dir.path(), ThemeRegistry::builtin()),
            &mut HashMap::new(),
        );
        assert_eq!(
            registry.families().len(),
            ThemeRegistry::builtin().families().len()
        );
        assert!(
            files[0]
                .error
                .as_deref()
                .unwrap()
                .contains("already in use")
        );
    }

    #[test]
    fn stamps_change_when_a_theme_file_is_added_or_edited() {
        let dir = tempfile::tempdir().unwrap();
        assert!(stamp_dir(&dir.path().join("missing")).is_empty());
        let before = stamp_dir(dir.path());
        std::fs::write(dir.path().join("notes.txt"), "ignored").unwrap();
        assert_eq!(stamp_dir(dir.path()), before);
        let file = dir.path().join("mine.adoken-theme.toml");
        std::fs::write(&file, THEME).unwrap();
        let added = stamp_dir(dir.path());
        assert_ne!(added, before);
        std::fs::write(&file, format!("{THEME}bg.sidebar = \"#111111\"\n")).unwrap();
        assert_ne!(stamp_dir(dir.path()), added, "size or mtime moved");
    }
}
