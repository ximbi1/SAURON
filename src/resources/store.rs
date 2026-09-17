use super::{Object, SharedObject, timeline};
use chrono::{DateTime, Utc};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Seen through a live, uninterrupted watch stream.
    WatchObserved,
    /// Reconstructed by diffing a fresh relist against the last known state
    /// after a watch reconnect -- the delta is real, but anything that
    /// happened *between* the two observations was never seen and is not
    /// synthesized here.
    RelistObserved,
}
#[derive(Clone, Debug)]
pub struct Change {
    pub at: DateTime<Utc>,
    pub summary: String,
    pub source: Source,
}

pub struct Store {
    pub objects: BTreeMap<String, SharedObject>,
    staging: Option<BTreeMap<String, SharedObject>>,
    pub histories: BTreeMap<String, VecDeque<Change>>,
    history_order: VecDeque<String>,
    pub incomplete: bool,
    pub revision: u64,
    bytes: usize,
    staging_bytes: usize,
    max_objects: usize,
    max_bytes: usize,
}
impl Store {
    pub fn new(max_objects: usize, max_bytes: usize) -> Self {
        Self {
            objects: BTreeMap::new(),
            staging: None,
            histories: BTreeMap::new(),
            history_order: VecDeque::new(),
            incomplete: false,
            revision: 0,
            bytes: 0,
            staging_bytes: 0,
            max_objects,
            max_bytes,
        }
    }
    pub fn begin(&mut self) {
        self.staging = Some(BTreeMap::new());
        self.staging_bytes = 0;
        self.incomplete = false;
    }
    pub fn is_syncing(&self) -> bool {
        self.staging.is_some()
    }
    /// Diffs every slot present before or after the relist against its prior
    /// state (`Source::RelistObserved`) before replacing `objects` wholesale.
    /// A relist that reconnects to unchanged state must diff to nothing; one
    /// that missed real changes while disconnected gets exactly one delta
    /// per object, never a synthesized sequence of intermediate states.
    pub fn finish(&mut self) {
        if let Some(staged) = self.staging.take() {
            let mut keys: std::collections::BTreeSet<String> =
                self.objects.keys().cloned().collect();
            keys.extend(staged.keys().cloned());
            for key in keys {
                let old = self.objects.get(&key).cloned();
                let new = staged.get(&key).cloned();
                self.diff_and_record(old.as_ref(), new.as_ref(), Source::RelistObserved);
            }
            self.objects = staged;
            self.bytes = self.staging_bytes;
            self.staging_bytes = 0;
            self.revision += 1;
        }
    }
    fn diff_and_record(
        &mut self,
        old: Option<&SharedObject>,
        new: Option<&SharedObject>,
        source: Source,
    ) {
        match (old, new) {
            (Some(old), Some(new)) if old.uid == new.uid => {
                if old.version == new.version {
                    return;
                }
                let changes = timeline::meaningful_diff(old, new);
                if !changes.is_empty() {
                    self.record(&new.uid, changes.join("; "), source);
                }
            }
            (Some(old), Some(_new)) => {
                self.record(
                    &old.uid,
                    "Object replaced by a different UID".into(),
                    source,
                );
            }
            (Some(old), None) => {
                self.record(&old.uid, "Deleted (observed by watch)".into(), source);
            }
            (None, Some(_)) | (None, None) => {}
        }
    }
    pub fn apply(&mut self, object: Object, initial: bool) {
        let key = object.slot();
        if initial {
            if let Some(staging) = self.staging.as_mut() {
                let old = staging.get(&key).map_or(0, |o| o.bytes);
                if (!staging.contains_key(&key) && staging.len() >= self.max_objects)
                    || self.staging_bytes.saturating_sub(old) + object.bytes > self.max_bytes
                {
                    self.incomplete = true;
                    return;
                }
                self.staging_bytes = self.staging_bytes.saturating_sub(old) + object.bytes;
                staging.insert(key, Arc::new(object));
            }
            return;
        }
        let old = self.objects.get(&key).cloned();
        if old
            .as_ref()
            .is_some_and(|o| o.uid == object.uid && o.version == object.version)
        {
            return;
        }
        let old_bytes = old.as_ref().map_or(0, |o| o.bytes);
        if (!self.objects.contains_key(&key) && self.objects.len() >= self.max_objects)
            || self.bytes.saturating_sub(old_bytes) + object.bytes > self.max_bytes
        {
            self.incomplete = true;
            return;
        }
        let new = Arc::new(object);
        self.diff_and_record(old.as_ref(), Some(&new), Source::WatchObserved);
        self.bytes = self.bytes.saturating_sub(old_bytes) + new.bytes;
        self.objects.insert(key, new);
        self.revision += 1;
    }
    pub fn delete(&mut self, object: &Object) {
        let key = object.slot();
        if self.objects.get(&key).is_some_and(|o| o.uid == object.uid) {
            if let Some(old) = self.objects.remove(&key) {
                self.bytes = self.bytes.saturating_sub(old.bytes);
            }
            self.record(
                &object.uid,
                "Deleted (observed by watch)".into(),
                Source::WatchObserved,
            );
            self.revision += 1;
        }
    }
    fn record(&mut self, uid: &str, summary: String, source: Source) {
        if !self.histories.contains_key(uid) {
            if self.history_order.len() >= 256
                && let Some(old) = self.history_order.pop_front()
            {
                self.histories.remove(&old);
            }
            self.history_order.push_back(uid.to_owned());
        }
        let h = self.histories.entry(uid.to_owned()).or_default();
        if h.len() >= 64 {
            h.pop_front();
        }
        h.push_back(Change {
            at: Utc::now(),
            summary,
            source,
        });
    }
    pub fn bytes(&self) -> usize {
        self.bytes + self.staging_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obj(uid: &str, rv: &str) -> Object {
        Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"a","uid":uid,"resourceVersion":rv}}),
        )
    }
    fn obj_phase(uid: &str, rv: &str, phase: &str) -> Object {
        Object::new(
            serde_json::json!({"kind":"Pod","apiVersion":"v1","metadata":{"name":"a","uid":uid,"resourceVersion":rv},"status":{"phase":phase}}),
        )
    }
    #[test]
    fn relist_is_atomic_and_old_delete_cannot_remove_replacement() {
        let mut s = Store::new(10, 100_000);
        s.apply(obj("old", "1"), false);
        s.begin();
        s.apply(obj("new", "2"), true);
        assert_eq!(s.objects["/a"].uid, "old");
        s.finish();
        s.delete(&obj("old", "1"));
        assert_eq!(s.objects["/a"].uid, "new");
    }
    #[test]
    fn unchanged_relist_records_nothing() {
        let mut s = Store::new(10, 100_000);
        s.apply(obj_phase("u", "1", "Running"), false);
        s.begin();
        s.apply(obj_phase("u", "1", "Running"), true);
        s.finish();
        assert!(s.histories.get("u").is_none_or(|h| h.is_empty()));
    }
    #[test]
    fn changed_while_disconnected_relist_records_exactly_one_delta() {
        let mut s = Store::new(10, 100_000);
        s.apply(obj_phase("u", "1", "Pending"), false);
        s.begin();
        // The relist never saw the intermediate state -- only the final one.
        s.apply(obj_phase("u", "5", "Running"), true);
        s.finish();
        let h = s.histories.get("u").expect("one delta recorded");
        assert_eq!(h.len(), 1);
        assert!(h[0].summary.contains("Pending → Running"));
        assert_eq!(h[0].source, Source::RelistObserved);
    }
    #[test]
    fn uid_replacement_and_deletion_are_both_observed_through_a_relist() {
        let mut s = Store::new(10, 100_000);
        s.apply(obj_phase("old", "1", "Running"), false);
        s.begin();
        s.apply(obj_phase("new", "1", "Running"), true);
        s.finish();
        let h = s.histories.get("old").expect("replacement recorded");
        assert!(h.iter().any(|c| c.summary.contains("replaced")));

        let mut gone = Store::new(10, 100_000);
        gone.apply(obj_phase("u2", "1", "Running"), false);
        gone.begin();
        // Nothing staged for this slot at all -- the object vanished while disconnected.
        gone.finish();
        let h = gone.histories.get("u2").expect("deletion recorded");
        assert!(h.iter().any(|c| c.summary.contains("Deleted")));
    }
    #[test]
    fn budget_is_visible() {
        let mut s = Store::new(0, 10);
        s.begin();
        s.apply(obj("a", "1"), true);
        s.finish();
        assert!(s.incomplete);
        assert!(s.objects.is_empty());
    }
}
