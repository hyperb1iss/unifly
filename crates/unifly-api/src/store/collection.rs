// ── Generic reactive entity collection ──
//
// Lock-free concurrent storage with O(1) lookups and push-based
// change notification via `watch` channels.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use dashmap::DashMap;
use tokio::sync::watch;

use crate::model::EntityId;

/// A lock-free, reactive collection for a single entity type.
///
/// Uses `DashMap` for O(1) concurrent lookups and `watch` channels
/// for push-based change notification. Every mutation bumps a version
/// counter and rebuilds the snapshot that subscribers receive.
pub(crate) struct EntityCollection<T: Clone + Send + Sync + 'static> {
    /// Primary storage: key string -> entity.
    /// Keys are MAC addresses for devices/clients, synthetic prefixed IDs
    /// (e.g. `"net:{id}"`) for other entities.
    by_key: DashMap<String, Arc<T>>,

    /// Secondary index: EntityId -> key string.
    id_to_key: DashMap<EntityId, String>,

    /// Reverse of `id_to_key` for efficient removal.
    key_to_id: DashMap<String, EntityId>,

    /// Version counter, bumped on every mutation.
    version: watch::Sender<u64>,

    /// Full snapshot, rebuilt on mutation for efficient subscription.
    snapshot: watch::Sender<Arc<Vec<Arc<T>>>>,

    /// Nested mutation batch depth. Non-zero means snapshot updates are deferred.
    batch_depth: AtomicUsize,

    /// Marks that at least one mutation happened while batching was active.
    pending_snapshot: AtomicBool,
}

impl<T: Clone + Send + Sync + 'static> EntityCollection<T> {
    pub(crate) fn new() -> Self {
        let (version, _) = watch::channel(0u64);
        let (snapshot, _) = watch::channel(Arc::new(Vec::new()));

        Self {
            by_key: DashMap::new(),
            id_to_key: DashMap::new(),
            key_to_id: DashMap::new(),
            version,
            snapshot,
            batch_depth: AtomicUsize::new(0),
            pending_snapshot: AtomicBool::new(false),
        }
    }

    /// Begin a mutation batch.
    ///
    /// While the returned guard is alive, mutations update the collection
    /// state immediately but defer snapshot/version broadcasts until the
    /// outermost guard is dropped.
    pub(crate) fn begin_batch(&self) -> MutationBatch<'_, T> {
        self.batch_depth.fetch_add(1, Ordering::AcqRel);
        MutationBatch { collection: self }
    }

    /// Insert or update an entity. Returns `true` if the key was new.
    pub(crate) fn upsert(&self, key: String, id: EntityId, entity: T) -> bool {
        // Clean up stale id mapping if the key already existed with a different id.
        if let Some(old_id) = self.key_to_id.get(&key)
            && *old_id != id
        {
            self.id_to_key.remove(&*old_id);
        }

        let is_new = !self.by_key.contains_key(&key);
        self.by_key.insert(key.clone(), Arc::new(entity));
        self.id_to_key.insert(id.clone(), key.clone());
        self.key_to_id.insert(key, id);

        self.mark_mutated();

        is_new
    }

    /// Rewrite an existing entity while holding its key's shard lock, so a
    /// concurrent upsert or removal cannot land between the read and the
    /// write. Returns `false` and changes nothing when the key is absent.
    ///
    /// `update` must keep the entity's id and must not touch this
    /// collection: the shard lock is held while it runs.
    pub(crate) fn update_existing(&self, key: &str, update: impl FnOnce(&T) -> T) -> bool {
        let Some(mut entry) = self.by_key.get_mut(key) else {
            return false;
        };
        let next = update(entry.value());
        *entry.value_mut() = Arc::new(next);
        drop(entry);
        self.mark_mutated();
        true
    }

    /// Remove an entity by key. Returns the removed entity if it existed.
    pub(crate) fn remove(&self, key: &str) -> Option<Arc<T>> {
        let removed = self.by_key.remove(key).map(|(_, v)| v);
        if removed.is_some() {
            if let Some((_, id)) = self.key_to_id.remove(key) {
                self.id_to_key.remove(&id);
            }
            self.mark_mutated();
        }
        removed
    }

    /// Look up an entity by its primary key string.
    pub(crate) fn get_by_key(&self, key: &str) -> Option<Arc<T>> {
        self.by_key.get(key).map(|r| Arc::clone(r.value()))
    }

    /// Look up an entity by its `EntityId` (secondary index).
    pub(crate) fn get_by_id(&self, id: &EntityId) -> Option<Arc<T>> {
        let key = self.id_to_key.get(id)?;
        self.by_key
            .get(key.value().as_str())
            .map(|r| Arc::clone(r.value()))
    }

    /// Get the current snapshot (cheap `Arc` clone).
    pub(crate) fn snapshot(&self) -> Arc<Vec<Arc<T>>> {
        self.snapshot.borrow().clone()
    }

    /// Subscribe to snapshot changes via a `watch::Receiver`.
    pub(crate) fn subscribe(&self) -> watch::Receiver<Arc<Vec<Arc<T>>>> {
        self.snapshot.subscribe()
    }

    /// Remove all entities.
    #[allow(dead_code)]
    pub(crate) fn clear(&self) {
        self.by_key.clear();
        self.id_to_key.clear();
        self.key_to_id.clear();
        self.mark_mutated();
    }

    pub(crate) fn len(&self) -> usize {
        self.by_key.len()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Return all current primary keys in the collection.
    pub(crate) fn keys(&self) -> Vec<String> {
        self.by_key.iter().map(|r| r.key().clone()).collect()
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn mark_mutated(&self) {
        if self.batch_depth.load(Ordering::Acquire) > 0 {
            self.pending_snapshot.store(true, Ordering::Release);
        } else {
            self.flush_snapshot();
        }
    }

    /// Collect all values into a snapshot vec and broadcast to subscribers.
    fn rebuild_snapshot(&self) {
        let values: Vec<Arc<T>> = self.by_key.iter().map(|r| Arc::clone(r.value())).collect();
        // `send_modify` updates unconditionally, even with zero receivers.
        self.snapshot.send_modify(|snap| *snap = Arc::new(values));
    }

    fn flush_snapshot(&self) {
        self.rebuild_snapshot();
        self.bump_version();
    }

    /// Increment the version counter.
    fn bump_version(&self) {
        self.version.send_modify(|v| *v += 1);
    }

    fn finish_batch(&self) {
        if self.batch_depth.fetch_sub(1, Ordering::AcqRel) == 1
            && self.pending_snapshot.swap(false, Ordering::AcqRel)
        {
            self.flush_snapshot();
        }
    }

    #[cfg(test)]
    pub(crate) fn version_receiver(&self) -> watch::Receiver<u64> {
        self.version.subscribe()
    }
}

#[must_use]
pub(crate) struct MutationBatch<'a, T: Clone + Send + Sync + 'static> {
    collection: &'a EntityCollection<T>,
}

impl<T: Clone + Send + Sync + 'static> Drop for MutationBatch<'_, T> {
    fn drop(&mut self) {
        self.collection.finish_batch();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::EntityId;
    use uuid::Uuid;

    #[test]
    fn update_existing_rewrites_present_keys_and_skips_absent_ones() {
        let col: EntityCollection<String> = EntityCollection::new();
        col.upsert("key1".into(), EntityId::from("id-1"), "old".into());

        assert!(col.update_existing("key1", |current| format!("{current}+stats")));
        assert_eq!(
            col.get_by_key("key1").as_deref().map(String::as_str),
            Some("old+stats")
        );

        assert!(!col.update_existing("gone", |_| "resurrected".into()));
        assert!(col.get_by_key("gone").is_none());
        assert_eq!(col.len(), 1);
    }

    #[test]
    fn upsert_returns_true_for_new_key() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        assert!(col.upsert("key1".into(), id, "hello".into()));
    }

    #[test]
    fn upsert_returns_false_for_existing_key() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        col.upsert("key1".into(), id.clone(), "hello".into());
        assert!(!col.upsert("key1".into(), id, "world".into()));
    }

    #[test]
    fn get_by_key_and_id() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::Uuid(Uuid::new_v4());
        col.upsert("key1".into(), id.clone(), "hello".into());

        assert_eq!(*col.get_by_key("key1").unwrap(), "hello");
        assert_eq!(*col.get_by_id(&id).unwrap(), "hello");
    }

    #[test]
    fn remove_cleans_up_indexes() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id = EntityId::from("test-id");
        col.upsert("key1".into(), id.clone(), "hello".into());

        let removed = col.remove("key1");
        assert_eq!(*removed.unwrap(), "hello");
        assert!(col.get_by_key("key1").is_none());
        assert!(col.get_by_id(&id).is_none());
        assert!(col.is_empty());
    }

    #[test]
    fn clear_empties_everything() {
        let col: EntityCollection<String> = EntityCollection::new();
        col.upsert("a".into(), EntityId::from("1"), "x".into());
        col.upsert("b".into(), EntityId::from("2"), "y".into());
        assert_eq!(col.len(), 2);

        col.clear();
        assert!(col.is_empty());
        assert!(col.snapshot().is_empty());
    }

    #[test]
    fn snapshot_reflects_current_state() {
        let col: EntityCollection<String> = EntityCollection::new();
        assert!(col.snapshot().is_empty());

        col.upsert("a".into(), EntityId::from("1"), "x".into());
        col.upsert("b".into(), EntityId::from("2"), "y".into());

        let snap = col.snapshot();
        assert_eq!(snap.len(), 2);
    }

    #[test]
    fn upsert_with_changed_id_cleans_old_mapping() {
        let col: EntityCollection<String> = EntityCollection::new();
        let id1 = EntityId::from("old-id");
        let id2 = EntityId::from("new-id");

        col.upsert("key1".into(), id1.clone(), "v1".into());
        assert!(col.get_by_id(&id1).is_some());

        // Re-upsert same key with different id
        col.upsert("key1".into(), id2.clone(), "v2".into());
        assert!(col.get_by_id(&id1).is_none()); // old id cleaned up
        assert_eq!(*col.get_by_id(&id2).unwrap(), "v2");
    }

    #[test]
    fn batch_defers_snapshot_broadcast_until_outer_guard_drops() {
        let col: EntityCollection<String> = EntityCollection::new();
        let mut snapshot_rx = col.subscribe();
        let version_rx = col.version_receiver();
        let start_version = *version_rx.borrow();

        {
            let _outer = col.begin_batch();
            col.upsert("a".into(), EntityId::from("1"), "x".into());

            {
                let _inner = col.begin_batch();
                col.upsert("b".into(), EntityId::from("2"), "y".into());
                col.remove("a");
            }

            assert!(!snapshot_rx.has_changed().unwrap());
            assert_eq!(*version_rx.borrow(), start_version);
            assert_eq!(*col.get_by_key("b").unwrap(), "y");
            assert!(col.snapshot().is_empty());
        }

        assert!(snapshot_rx.has_changed().unwrap());
        assert_eq!(*version_rx.borrow(), start_version + 1);

        let snap = snapshot_rx.borrow_and_update().clone();
        assert_eq!(snap.len(), 1);
        assert_eq!(*snap[0], "y");
    }
}
