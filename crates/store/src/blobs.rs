//! Content-addressed file contents for local diffs (spec 0004 FR-2).
//!
//! Git object ids name immutable content, so a cached blob is never fetched
//! twice. Files live beside the org's store, `<store dir>/blobs/<ab>/<id>`
//! (directories `0700`, files `0600`), not in SQLite: a store commit wakes
//! every view to reload, and blob bookkeeping (a fetch, an LRU touch) must
//! not; nor should a 500 MB cache be copied by the pre-migration backup.
//!
//! LRU: reads touch the file's mtime, so recency survives restarts. The
//! in-memory index (built by one directory scan on the first write) orders
//! entries by a logical tick seeded from those mtimes; a write that pushes
//! the total above the cap evicts least recently used blobs until it fits.
//!
//! Blocking IO: call from a background executor or a blocking pool.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

/// FR-2 cache cap.
pub const DEFAULT_CAP_BYTES: u64 = 500 * 1024 * 1024;

/// Cheap to clone; `Send + Sync`.
#[derive(Clone)]
pub struct BlobCache {
    inner: Arc<Inner>,
}

struct Inner {
    dir: PathBuf,
    cap: u64,
    /// `None` until the first write scans the directory.
    index: Mutex<Option<Index>>,
}

#[derive(Default)]
struct Index {
    /// id → (bytes, tick).
    entries: HashMap<String, (u64, u64)>,
    /// (tick, id), oldest first.
    lru: BTreeSet<(u64, String)>,
    total: u64,
    tick: u64,
}

impl Index {
    fn touch(&mut self, id: &str) {
        self.tick += 1;
        let tick = self.tick;
        if let Some((_, old)) = self.entries.get_mut(id) {
            self.lru.remove(&(*old, id.to_owned()));
            *old = tick;
            self.lru.insert((tick, id.to_owned()));
        }
    }

    fn insert(&mut self, id: String, bytes: u64) {
        if self.entries.contains_key(&id) {
            self.touch(&id);
            return;
        }
        self.tick += 1;
        self.entries.insert(id.clone(), (bytes, self.tick));
        self.lru.insert((self.tick, id));
        self.total += bytes;
    }
}

impl std::fmt::Debug for BlobCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobCache")
            .field("dir", &self.inner.dir)
            .field("cap", &self.inner.cap)
            .finish()
    }
}

/// Git object ids are hex (SHA-1 today, SHA-256 later); anything else is
/// refused so an id can never escape the cache directory.
pub fn is_object_id(id: &str) -> bool {
    (4..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit())
}

impl BlobCache {
    /// The cache in `dir` with the FR-2 cap.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_cap(dir, DEFAULT_CAP_BYTES)
    }

    pub fn with_cap(dir: impl Into<PathBuf>, cap: u64) -> Self {
        BlobCache {
            inner: Arc::new(Inner {
                dir: dir.into(),
                cap,
                index: Mutex::new(None),
            }),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        if !is_object_id(id) {
            return None;
        }
        let id = id.to_ascii_lowercase();
        Some(self.inner.dir.join(&id[..2]).join(id))
    }

    fn index(&self) -> MutexGuard<'_, Option<Index>> {
        self.inner.index.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Cached right now (no read, no touch).
    pub fn contains(&self, id: &str) -> bool {
        self.path(id).is_some_and(|p| p.is_file())
    }

    /// The blob's bytes, marking it recently used.
    pub fn get(&self, id: &str) -> std::io::Result<Option<Vec<u8>>> {
        let Some(path) = self.path(id) else {
            return Ok(None);
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        // Recency for the next launch's scan; best effort.
        if let Ok(file) = std::fs::File::options().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        if let Some(index) = self.index().as_mut() {
            index.touch(&id.to_ascii_lowercase());
        }
        Ok(Some(bytes))
    }

    /// The blob as text (invalid UTF-8 replaced; NULs kept, so binary
    /// content stays recognisable).
    pub fn text(&self, id: &str) -> std::io::Result<Option<String>> {
        Ok(self.get(id)?.map(|bytes| match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }))
    }

    /// Store `bytes` under `id` (a no-op when present: content-addressed),
    /// then evict least recently used blobs above the cap. The new blob is
    /// the most recent, so it is never the one evicted.
    pub fn put(&self, id: &str, bytes: &[u8]) -> std::io::Result<()> {
        let path = self
            .path(id)
            .ok_or_else(|| std::io::Error::other(format!("not an object id: {id}")))?;
        let id = id.to_ascii_lowercase();
        let mut guard = self.index();
        if guard.is_none() {
            *guard = Some(self.scan()?);
        }
        let index = guard.as_mut().expect("index loaded");
        if !path.is_file() {
            let dir = path.parent().expect("blob path has a parent");
            ensure_dir(&self.inner.dir)?;
            ensure_dir(dir)?;
            let partial = path.with_extension("partial");
            std::fs::write(&partial, bytes)?;
            restrict(&partial, 0o600)?;
            std::fs::rename(&partial, &path)?;
        }
        index.insert(id.clone(), bytes.len() as u64);
        while index.total > self.inner.cap {
            let Some((tick, victim)) = index.lru.first().cloned() else {
                break;
            };
            if victim == id {
                break;
            }
            index.lru.remove(&(tick, victim.clone()));
            if let Some((size, _)) = index.entries.remove(&victim) {
                index.total -= size;
            }
            if let Some(path) = self.path(&victim) {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// Bytes on disk as the index sees them (scans on first use).
    pub fn total_bytes(&self) -> std::io::Result<u64> {
        let mut guard = self.index();
        if guard.is_none() {
            *guard = Some(self.scan()?);
        }
        Ok(guard.as_ref().map_or(0, |i| i.total))
    }

    /// Build the index from disk: every blob, ordered by mtime.
    fn scan(&self) -> std::io::Result<Index> {
        let mut found: Vec<(SystemTime, String, u64)> = Vec::new();
        let shards = match std::fs::read_dir(&self.inner.dir) {
            Ok(shards) => shards,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Index::default()),
            Err(e) => return Err(e),
        };
        for shard in shards.flatten() {
            let Ok(files) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                let Ok(meta) = file.metadata() else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                if !is_object_id(&name) {
                    // An interrupted write.
                    if name.ends_with(".partial") {
                        let _ = std::fs::remove_file(file.path());
                    }
                    continue;
                }
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                found.push((modified, name, meta.len()));
            }
        }
        found.sort();
        let mut index = Index::default();
        for (_, id, bytes) in found {
            index.insert(id, bytes);
        }
        Ok(index)
    }
}

fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.is_dir() {
        std::fs::create_dir_all(dir)?;
        restrict(dir, 0o700)?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> String {
        format!("{n:02x}").repeat(20)
    }

    #[test]
    fn round_trips_and_refuses_non_ids() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::new(dir.path().join("blobs"));
        assert_eq!(cache.get(&id(1)).unwrap(), None);
        cache.put(&id(1).to_ascii_uppercase(), b"hello").unwrap();
        assert!(cache.contains(&id(1)));
        assert_eq!(cache.get(&id(1)).unwrap().as_deref(), Some(&b"hello"[..]));
        assert_eq!(cache.text(&id(1)).unwrap().as_deref(), Some("hello"));
        assert!(cache.put("../../etc/passwd", b"x").is_err());
        assert_eq!(cache.get("../x").unwrap(), None);
    }

    #[test]
    fn evicts_least_recently_used_above_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::with_cap(dir.path().join("blobs"), 10);
        cache.put(&id(1), b"aaaa").unwrap();
        cache.put(&id(2), b"bbbb").unwrap();
        // Reading 1 makes 2 the least recently used.
        cache.get(&id(1)).unwrap();
        cache.put(&id(3), b"cccc").unwrap();
        assert!(cache.contains(&id(1)));
        assert!(!cache.contains(&id(2)));
        assert!(cache.contains(&id(3)));
        assert_eq!(cache.total_bytes().unwrap(), 8);
    }

    #[test]
    fn a_blob_larger_than_the_cap_is_kept_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cache = BlobCache::with_cap(dir.path().join("blobs"), 4);
        cache.put(&id(1), b"aa").unwrap();
        cache.put(&id(2), b"bbbbbbbb").unwrap();
        assert!(!cache.contains(&id(1)));
        assert!(cache.contains(&id(2)));
    }

    #[test]
    fn recency_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = dir.path().join("blobs");
        {
            let cache = BlobCache::with_cap(&blobs, 10);
            cache.put(&id(1), b"aaaa").unwrap();
            cache.put(&id(2), b"bbbb").unwrap();
        }
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let path = |n| blobs.join(&id(n)[..2]).join(id(n));
        for n in [1, 2] {
            std::fs::File::options()
                .write(true)
                .open(path(n))
                .unwrap()
                .set_modified(old + std::time::Duration::from_secs(n as u64))
                .unwrap();
        }
        let cache = BlobCache::with_cap(&blobs, 10);
        // 1 is older on disk; reading it now makes 2 the oldest.
        cache.get(&id(1)).unwrap();
        cache.put(&id(3), b"cccc").unwrap();
        assert!(cache.contains(&id(1)));
        assert!(!cache.contains(&id(2)));
    }
}
