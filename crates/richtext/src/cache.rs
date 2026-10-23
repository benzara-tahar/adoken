//! FR-2: parsed documents cached per `(id, rev)` (or any key), so
//! reselecting an item paints without reparsing.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use crate::model::Doc;

/// A bounded map; when full, the least recently used entry goes.
pub struct DocCache<K> {
    capacity: usize,
    tick: u64,
    entries: HashMap<K, (Arc<Doc>, u64)>,
}

impl<K: Hash + Eq + Clone> DocCache<K> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            tick: 0,
            entries: HashMap::new(),
        }
    }

    pub fn get(&mut self, key: &K) -> Option<Arc<Doc>> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(doc, used)| {
            *used = tick;
            doc.clone()
        })
    }

    pub fn insert(&mut self, key: K, doc: Arc<Doc>) {
        self.tick += 1;
        if self.entries.len() >= self.capacity
            && !self.entries.contains_key(&key)
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
        self.entries.insert(key, (doc, self.tick));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn least_recently_used_entry_is_evicted() {
        let doc = Arc::new(Doc::default());
        let mut cache = DocCache::new(2);
        cache.insert((1, 1), doc.clone());
        cache.insert((2, 1), doc.clone());
        assert!(cache.get(&(1, 1)).is_some());
        cache.insert((3, 1), doc.clone());
        assert!(cache.get(&(2, 1)).is_none(), "2 was least recently used");
        assert!(cache.get(&(1, 1)).is_some());
        assert_eq!(cache.len(), 2);
    }
}
