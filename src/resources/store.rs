use super::{Object, SharedObject};
use chrono::{DateTime, Utc};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Change {
    pub at: DateTime<Utc>,
    pub summary: String,
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
    pub fn finish(&mut self) {
        if let Some(objects) = self.staging.take() {
            self.objects = objects;
            self.bytes = self.staging_bytes;
            self.staging_bytes = 0;
            self.revision += 1;
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
        let old_bytes = old.as_ref().map_or(0, |o| o.bytes);
        if (!self.objects.contains_key(&key) && self.objects.len() >= self.max_objects)
            || self.bytes.saturating_sub(old_bytes) + object.bytes > self.max_bytes
        {
            self.incomplete = true;
            return;
        }
        if let Some(old) = old {
            if old.uid == object.uid && old.version == object.version {
                return;
            }
            if old.uid == object.uid {
                let mut changes = Vec::new();
                if old.health.status != object.health.status {
                    changes.push(format!(
                        "status: {} → {}",
                        old.health.status, object.health.status
                    ));
                }
                for path in [
                    "/metadata/generation",
                    "/spec/replicas",
                    "/status/readyReplicas",
                    "/status/conditions",
                    "/status/containerStatuses",
                    "/status/initContainerStatuses",
                ] {
                    if old.value.pointer(path) != object.value.pointer(path) {
                        changes.push(format!("{path} changed"));
                    }
                }
                if !changes.is_empty() {
                    self.record(&object.uid, changes.join("; "));
                }
            } else {
                self.record(&old.uid, "Object replaced by a different UID".into());
            }
        }
        self.bytes = self.bytes.saturating_sub(old_bytes) + object.bytes;
        self.objects.insert(key, Arc::new(object));
        self.revision += 1;
    }
    pub fn delete(&mut self, object: &Object) {
        let key = object.slot();
        if self.objects.get(&key).is_some_and(|o| o.uid == object.uid) {
            if let Some(old) = self.objects.remove(&key) {
                self.bytes = self.bytes.saturating_sub(old.bytes);
            }
            self.record(&object.uid, "Deleted (observed by watch)".into());
            self.revision += 1;
        }
    }
    fn record(&mut self, uid: &str, summary: String) {
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
    fn budget_is_visible() {
        let mut s = Store::new(0, 10);
        s.begin();
        s.apply(obj("a", "1"), true);
        s.finish();
        assert!(s.incomplete);
        assert!(s.objects.is_empty());
    }
}
