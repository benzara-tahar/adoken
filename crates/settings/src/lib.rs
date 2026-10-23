//! `settings.toml`, the one user preferences file (ADR 0001: preferences in
//! TOML, not the DB). Every writer — the Settings window, notify's
//! persistence, the sidebar, themes, Ken — goes through [`update`]:
//!
//! - **Format-preserving.** The file is edited with `toml_edit`, so comments,
//!   key order and tables other modules own survive a write. A section write
//!   merges key by key: unchanged keys keep their formatting and comments,
//!   keys missing from the new section are removed.
//! - **Serialized.** One process-wide lock covers read → edit → write, so two
//!   modules saving different sections at once never lose each other's
//!   changes.
//! - **Atomic.** Written to a temporary file next to the target, then
//!   renamed over it.
//! - **Safe on broken files.** A file with a TOML syntax error is never
//!   overwritten ([`Error::Syntax`]); the user's text stays as it is.
//!
//! Blocking IO: call from a background executor, never the UI thread.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use toml_edit::{DocumentMut, Item, Table};

/// File name inside the app's data directory (demo: the demo directory).
pub const FILE_NAME: &str = "settings.toml";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("settings.toml has a syntax error, so nothing was saved: {0}")]
    Syntax(#[from] toml_edit::TomlError),
    #[error("couldn't encode settings: {0}")]
    Encode(#[from] toml::ser::Error),
    #[error("couldn't save settings: {0}")]
    Io(#[from] io::Error),
}

/// Held for every read-modify-write in this process.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Read `path`, apply `edit`, and write the result back atomically when it
/// changed. A missing file starts empty. Returns whether the file changed.
pub fn update(
    path: &Path,
    edit: impl FnOnce(&mut DocumentMut) -> Result<(), Error>,
) -> Result<bool, Error> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let before = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document: DocumentMut = before.parse()?;
    edit(&mut document)?;
    let after = document.to_string();
    if after == before {
        return Ok(false);
    }
    write_atomic(path, &after)?;
    Ok(true)
}

/// Replace the top-level `[name]` table with `table`, merging key by key.
pub fn set_table(document: &mut DocumentMut, name: &str, table: &toml::Table) -> Result<(), Error> {
    let new = encode(table)?;
    merge_item(document.as_table_mut(), name, Item::Table(new));
    Ok(())
}

/// Remove the top-level `[name]` table (and everything under it).
pub fn remove_table(document: &mut DocumentMut, name: &str) {
    document.as_table_mut().remove(name);
}

/// Set top-level keys owned by one module (notify's settings live at the
/// root): every key in `table` is merged in, and keys in `owned` that
/// `table` lacks are removed. Other root keys and tables are untouched.
pub fn set_root_keys(
    document: &mut DocumentMut,
    table: &toml::Table,
    owned: &[&str],
) -> Result<(), Error> {
    let new = encode(table)?;
    let root = document.as_table_mut();
    for key in owned {
        if !new.contains_key(key) {
            root.remove(key);
        }
    }
    for (key, item) in new {
        merge_item(root, &key, item);
    }
    Ok(())
}

/// `table` as a `toml_edit` table, via its canonical TOML text.
fn encode(table: &toml::Table) -> Result<Table, Error> {
    let text = toml::to_string(table)?;
    let mut table = text.parse::<DocumentMut>()?.as_table().clone();
    clear_positions(&mut table);
    Ok(table)
}

/// Tables parsed from another document carry that document's positions;
/// drop them so new tables render after their parent.
fn clear_positions(table: &mut Table) {
    table.set_position(None);
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(sub) => clear_positions(sub),
            Item::ArrayOfTables(array) => {
                for sub in array.iter_mut() {
                    clear_positions(sub);
                }
            }
            _ => {}
        }
    }
}

fn merge_table(existing: &mut Table, new: Table) {
    let stale: Vec<String> = existing
        .iter()
        .map(|(key, _)| key.to_owned())
        .filter(|key| !new.contains_key(key))
        .collect();
    for key in stale {
        existing.remove(&key);
    }
    for (key, item) in new {
        merge_item(existing, &key, item);
    }
}

fn merge_item(parent: &mut Table, key: &str, new: Item) {
    match (parent.get_mut(key), new) {
        (Some(Item::Table(old)), Item::Table(new)) => merge_table(old, new),
        (Some(Item::Value(old)), Item::Value(mut new)) => {
            if same_value(old, &new) {
                return;
            }
            // Keep the user's spacing and trailing comment on the line.
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        (Some(Item::Value(old)), Item::Table(new)) if old.is_inline_table() => {
            // The user wrote an inline table; keep that style.
            let mut new = toml_edit::Value::InlineTable(new.into_inline_table());
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        (Some(old), new) => *old = new,
        (None, new) => {
            parent.insert(key, new);
        }
    }
}

fn same_value(a: &toml_edit::Value, b: &toml_edit::Value) -> bool {
    let bare = |value: &toml_edit::Value| {
        let mut value = value.clone();
        value.decor_mut().clear();
        value.to_string()
    };
    bare(a) == bare(b)
}

/// Write `text` to a temporary file beside `path`, then rename it over
/// `path`. Creates the parent directory.
pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let tmp = temp_path(path);
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        fs::remove_file(&tmp).ok();
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| FILE_NAME.to_owned());
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(text: &str) -> toml::Table {
        toml::from_str(text).expect("valid test TOML")
    }

    fn apply(before: &str, edit: impl FnOnce(&mut DocumentMut) -> Result<(), Error>) -> String {
        let mut document: DocumentMut = before.parse().expect("valid test TOML");
        edit(&mut document).expect("edit");
        document.to_string()
    }

    #[test]
    fn set_table_keeps_other_tables_and_comments() {
        let before = "# my settings\n\
                      [palette.rows.pr]\n\
                      title = \"{title}\" # custom\n\n\
                      [sound]\n\
                      enabled = false # off for now\n\
                      master = 0.6\n";
        let after = apply(before, |doc| {
            set_table(doc, "sound", &table("enabled = true\nmaster = 0.6\n"))
        });
        assert!(after.contains("# my settings"));
        assert!(after.contains("title = \"{title}\" # custom"));
        assert!(
            after.contains("enabled = true # off for now"),
            "a changed value keeps its line comment: {after}"
        );
        assert!(after.contains("master = 0.6"));
    }

    #[test]
    fn set_table_removes_keys_missing_from_the_new_table() {
        let after = apply("[sidebar]\ncollapsed = true\nwidth = 240\n", |doc| {
            set_table(doc, "sidebar", &table("collapsed = false\n"))
        });
        let parsed = table(&after);
        assert_eq!(parsed["sidebar"].as_table().unwrap().len(), 1);
        assert_eq!(parsed["sidebar"]["collapsed"].as_bool(), Some(false));
    }

    #[test]
    fn set_table_writes_nested_tables() {
        let after = apply("", |doc| {
            set_table(
                doc,
                "background",
                &table("source = \"builtin:dusk\"\n[views.pr_review]\nblur = 48\n"),
            )
        });
        assert_eq!(
            table(&after),
            table(
                "[background]\nsource = \"builtin:dusk\"\n[background.views.pr_review]\nblur = 48\n"
            )
        );
        // Removing the override drops the nested table.
        let after = apply(&after, |doc| {
            set_table(doc, "background", &table("source = \"builtin:dusk\"\n"))
        });
        assert!(!after.contains("views"), "{after}");
    }

    #[test]
    fn inline_tables_stay_inline() {
        let after = apply("sound = { enabled = false }\n", |doc| {
            set_table(doc, "sound", &table("enabled = true\n"))
        });
        assert_eq!(after.trim(), "sound = { enabled = true }");
    }

    #[test]
    fn root_keys_replace_only_the_owned_keys() {
        let before = "show_self_caused = true\nbadge = \"P0\"\n\n[sound]\nenabled = true\n";
        let after = apply(before, |doc| {
            set_root_keys(
                doc,
                &table("badge = \"AllUnread\"\n[delivery]\np0 = \"Banner\"\n"),
                &["show_self_caused", "badge", "delivery"],
            )
        });
        let parsed = table(&after);
        assert!(parsed.get("show_self_caused").is_none());
        assert_eq!(parsed["badge"].as_str(), Some("AllUnread"));
        assert_eq!(parsed["delivery"]["p0"].as_str(), Some("Banner"));
        assert_eq!(parsed["sound"]["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn update_creates_the_file_and_reports_no_op_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(FILE_NAME);
        let write = |path: &Path| {
            update(path, |doc| {
                set_table(doc, "ken", &table("enabled = true\n"))
            })
        };
        assert!(write(&path).unwrap());
        assert!(!write(&path).unwrap(), "same content: no write");
        assert_eq!(
            table(&fs::read_to_string(&path).unwrap()),
            table("[ken]\nenabled = true\n")
        );
    }

    #[test]
    fn update_never_overwrites_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        fs::write(&path, "[sound\nenabled = true\n").unwrap();
        let result = update(&path, |doc| {
            set_table(doc, "ken", &table("enabled = true\n"))
        });
        assert!(matches!(result, Err(Error::Syntax(_))));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[sound\nenabled = true\n"
        );
    }

    #[test]
    fn concurrent_section_writes_never_lose_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let threads: Vec<_> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|name| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        update(&path, |doc| {
                            set_table(doc, name, &table(&format!("n = {i}\n")))
                        })
                        .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let parsed = table(&fs::read_to_string(&path).unwrap());
        for name in ["a", "b", "c", "d"] {
            assert_eq!(parsed[name]["n"].as_integer(), Some(24), "{name}");
        }
    }
}
