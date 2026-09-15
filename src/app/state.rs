use crate::{
    command::{Action, Keymap},
    config::Settings,
    filters::{Expr, quantity},
    kube::{discovery::Resource, watch::Query},
    resources::{SharedObject, store::Store},
};
use chrono::Utc;
use std::collections::VecDeque;

pub enum Mode {
    Table,
    Command(String),
    Filter(String),
    Document(Document),
    Search(Document, String),
    Picker(Picker),
    Loading,
}
#[derive(Clone, Copy, PartialEq)]
pub enum PickerKind {
    Context,
    Namespace,
}
pub struct Picker {
    pub kind: PickerKind,
    pub title: String,
    pub items: Vec<String>,
    pub active: Option<usize>,
    pub cursor: usize,
}
/// A back/forward-stack entry: semantic navigation intent, not a state snapshot. Never
/// carries store/rows/watch data -- restoring replays the intent (reconnect if needed,
/// re-resolve `resource` against whatever catalog is current, restart the watch) and
/// lets the normal watch/rebuild pipeline repopulate real data, rather than reviving
/// anything old as if it were current. `resource` is the canonical qualified name
/// (`Resource::qualified()`, e.g. "pods" or "widgets.a.sauron.test"), never a raw
/// human alias, so restoring re-resolves deterministically even if aliases/CRDs changed.
/// `selected` is a UID, not a name: same-name-different-UID must not reselect (matches
/// the existing rebuild() invariant that replacement clears selection).
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub context: String,
    pub namespace: Option<String>,
    pub resource: String,
    pub filter_text: String,
    pub sort: String,
    pub descending: bool,
    pub selected: Option<String>,
}
pub struct Document {
    pub title: String,
    pub lines: VecDeque<String>,
    pub scroll: usize,
    pub horizontal: u16,
    pub wrap: bool,
    pub fullscreen: bool,
    pub search: String,
    pub streaming: bool,
    pub follow: bool,
    bytes: usize,
}
impl Document {
    pub fn new(title: String, text: String) -> Self {
        let bytes = text.len();
        Self {
            title,
            lines: text.lines().map(str::to_owned).collect(),
            scroll: 0,
            horizontal: 0,
            wrap: true,
            fullscreen: false,
            search: String::new(),
            streaming: false,
            follow: false,
            bytes,
        }
    }
    pub fn append(&mut self, line: String) {
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.lines.len() > 5000 || self.bytes > 4 * 1024 * 1024 {
            if let Some(old) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(old.len());
                self.scroll = self.scroll.saturating_sub(1);
            } else {
                break;
            }
        }
    }
    pub fn search_next(&mut self, reverse: bool) {
        if self.search.is_empty() || self.lines.is_empty() {
            return;
        }
        let len = self.lines.len();
        let query = self.search.to_lowercase();
        for i in 1..=len {
            let at = if reverse {
                (self.scroll + len - (i % len)) % len
            } else {
                (self.scroll + i) % len
            };
            if self.lines[at].to_lowercase().contains(&query) {
                self.scroll = at;
                self.follow = false;
                break;
            }
        }
    }
}
pub struct State {
    pub epoch: u64,
    pub request: u64,
    pub context: String,
    pub query: Query,
    pub resource: Option<Resource>,
    pub settings: Settings,
    pub keymap: Keymap,
    pub store: Store,
    pub rows: Vec<SharedObject>,
    pub selected: Option<String>,
    pub table: ratatui::widgets::TableState,
    pub filter: Expr,
    pub filter_text: String,
    pub sort: String,
    pub descending: bool,
    pub wide: bool,
    pub mode: Mode,
    pub status: String,
    pub error: Option<String>,
    pub synced: bool,
    pub dirty: bool,
    pub quit: bool,
    pub watch_errors: u64,
    pub page_size: usize,
    prepared: Option<(u64, u64, String, String, bool, Option<i64>)>,
}
impl State {
    pub fn new(query: Query, settings: &Settings) -> anyhow::Result<Self> {
        Ok(Self {
            epoch: 0,
            request: 0,
            context: "Connecting".into(),
            query,
            resource: None,
            settings: settings.clone(),
            keymap: Keymap::compile(&settings.keys)?,
            store: Store::new(settings.max_objects, settings.max_bytes),
            rows: vec![],
            selected: None,
            table: Default::default(),
            filter: Expr::All,
            filter_text: String::new(),
            sort: "NAME".into(),
            descending: false,
            wide: false,
            mode: Mode::Table,
            status: "Starting".into(),
            error: None,
            synced: false,
            dirty: true,
            quit: false,
            watch_errors: 0,
            page_size: 20,
            prepared: None,
        })
    }
    pub fn columns(&self) -> Vec<String> {
        let mut columns = vec!["NAME".into()];
        let namespaced = self.resource.as_ref().is_none_or(|r| r.namespaced);
        if self.query.namespace.is_none() && namespaced {
            columns.insert(0, "NAMESPACE".into());
        }
        if let Some(object) = self
            .rows
            .first()
            .or_else(|| self.store.objects.values().next())
        {
            columns.extend(
                object
                    .cells
                    .iter()
                    .filter(|(k, _)| self.wide || k != "NODE")
                    .map(|(k, _)| k.clone()),
            );
        }
        columns.push("AGE".into());
        columns
    }
    pub fn selected_object(&self) -> Option<SharedObject> {
        self.selected
            .as_ref()
            .and_then(|uid| self.rows.iter().find(|o| &o.uid == uid))
            .cloned()
    }
    pub fn rebuild(&mut self) {
        let now = Utc::now();
        let was_empty = self.rows.is_empty();
        self.rows = self
            .store
            .objects
            .values()
            .filter(|o| self.filter.matches(o, now))
            .cloned()
            .collect();
        let column = &self.sort;
        let desc = self.descending;
        self.rows.sort_by_cached_key(|o| {
            let value = o.field(column, now);
            let numeric = if column == "AGE"
                || [
                    "RESTARTS",
                    "UPDATED",
                    "AVAILABLE",
                    "FAILED",
                    "SUCCEEDED",
                    "ACTIVE",
                    "DATA",
                    "SUBSETS",
                ]
                .contains(&column.as_str())
            {
                value.as_deref().and_then(quantity).map(|n| n.to_bits())
            } else {
                None
            };
            (
                numeric,
                value,
                o.namespace.clone(),
                o.name.clone(),
                o.uid.clone(),
            )
        });
        if desc {
            self.rows.reverse();
        }
        if let Some(uid) = &self.selected {
            if !self.rows.iter().any(|o| &o.uid == uid) {
                self.selected = None;
            }
        } else if was_empty && !self.rows.is_empty() {
            self.selected = Some(self.rows[0].uid.clone());
        }
        self.table.select(
            self.selected
                .as_ref()
                .and_then(|uid| self.rows.iter().position(|o| &o.uid == uid)),
        );
    }
    /// Recompute row order only when inputs change. Cursor/log/prompt redraws do not resort.
    /// The current second is only part of the cache key while the filter has an `age`
    /// comparison, since only that predicate's membership can change from time alone;
    /// AGE column values and sort order stay correct without it (see `rebuild`/`ui::render`).
    /// `epoch` must be part of the key alongside `store.revision`: every reconnect/refresh
    /// replaces `store` with a fresh one whose revision restarts at 0, so a bare revision
    /// can alias a previous watch's revision even though `rows` was already cleared for the
    /// new one — epoch is bumped on every such restart and never resets, so the pair is
    /// unique across watch generations.
    pub fn prepare(&mut self) {
        let tick = self
            .filter
            .has_time_predicate()
            .then(|| Utc::now().timestamp());
        let key = (
            self.epoch,
            self.store.revision,
            self.filter_text.clone(),
            self.sort.clone(),
            self.descending,
            tick,
        );
        if self.prepared.as_ref() != Some(&key) {
            self.rebuild();
            self.prepared = Some(key);
        } else {
            self.table.select(
                self.selected
                    .as_ref()
                    .and_then(|uid| self.rows.iter().position(|o| &o.uid == uid)),
            );
        }
    }
    pub fn move_selection(&mut self, action: Action) {
        use Action::*;
        if let Mode::Document(doc) = &mut self.mode {
            doc.follow = false;
            doc.scroll = match action {
                Down => doc.scroll.saturating_add(1),
                Up => doc.scroll.saturating_sub(1),
                First => 0,
                Last => {
                    doc.follow = doc.streaming;
                    doc.lines.len().saturating_sub(self.page_size)
                }
                PageDown => doc.scroll.saturating_add(self.page_size),
                PageUp => doc.scroll.saturating_sub(self.page_size),
                _ => doc.scroll,
            }
            .min(doc.lines.len().saturating_sub(1));
            return;
        }
        if self.rows.is_empty() {
            self.selected = None;
            return;
        }
        let position = self
            .selected
            .as_ref()
            .and_then(|uid| self.rows.iter().position(|o| &o.uid == uid))
            .unwrap_or(0);
        let next = match action {
            Down => position + 1,
            Up => position.saturating_sub(1),
            First => 0,
            Last => self.rows.len() - 1,
            PageDown => position.saturating_add(self.page_size),
            PageUp => position.saturating_sub(self.page_size),
            _ => position,
        }
        .min(self.rows.len() - 1);
        self.selected = Some(self.rows[next].uid.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_uid_not_row_index() {
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"a","uid":"old","resourceVersion":"1"}}),
            ),
            false,
        );
        s.rebuild();
        assert_eq!(s.selected.as_deref(), Some("old"));
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"a","uid":"new","resourceVersion":"2"}}),
            ),
            false,
        );
        s.rebuild();
        assert!(s.selected.is_none());
    }
    #[test]
    fn prepare_without_relevant_change_does_not_resort_across_a_clock_tick() {
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"a","uid":"1","resourceVersion":"1"}}),
            ),
            false,
        );
        s.prepare();
        assert_eq!(s.rows.len(), 1);
        // A sentinel not present in the store: only rebuild() would strip it back out.
        s.rows.push(s.rows[0].clone());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        s.prepare();
        assert_eq!(
            s.rows.len(),
            2,
            "prepare() must not rebuild from the clock alone when there is no age filter"
        );
    }
    #[test]
    fn prepare_rebuilds_after_a_reconnect_even_when_revision_number_repeats() {
        // Regression: found while driving the app interactively against a live cluster.
        // Every reconnect/refresh replaces `store` with a fresh Store whose revision
        // restarts at 0, so two independent watches can each land on the same revision
        // number (e.g. both finish their initial list at revision 1). Without `epoch` in
        // the key, the second watch's rebuild was skipped as "unchanged" even though rows
        // had just been cleared for it, leaving the table permanently empty after refresh.
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"a","uid":"1","resourceVersion":"1"}}),
            ),
            false,
        );
        s.prepare();
        assert_eq!(s.rows.len(), 1);
        // Simulate cancel_scope(): bump epoch, clear rows, and start a fresh Store that
        // lands on the exact same revision (1) as the one just cached.
        s.epoch += 1;
        s.rows.clear();
        s.store = Store::new(s.settings.max_objects, s.settings.max_bytes);
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"b","uid":"2","resourceVersion":"1"}}),
            ),
            false,
        );
        assert_eq!(
            s.store.revision, 1,
            "second watch coincidentally reaches the same revision"
        );
        s.prepare();
        assert_eq!(
            s.rows.len(),
            1,
            "a new watch generation must always rebuild, even if revision aliases the last one"
        );
        assert_eq!(s.rows[0].uid, "2");
    }
    #[test]
    fn prepare_with_age_filter_rechecks_membership_across_a_clock_tick() {
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        s.filter = Expr::parse("age>0s").expect("valid filter");
        s.filter_text = "age>0s".into();
        s.store.apply(
            crate::resources::Object::new(serde_json::json!({
                "metadata": {"name": "a", "uid": "1", "resourceVersion": "1",
                    "creationTimestamp": Utc::now().to_rfc3339()},
            })),
            false,
        );
        s.prepare();
        let before = s.prepared.clone();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        s.prepare();
        assert_ne!(
            before, s.prepared,
            "an age-based filter must be reconsidered as the clock advances"
        );
    }
    #[test]
    fn age_display_value_is_independent_of_the_prepare_cache() {
        let object = crate::resources::Object::new(serde_json::json!({
            "metadata": {"name": "a", "uid": "1", "resourceVersion": "1",
                "creationTimestamp": (Utc::now() - chrono::Duration::seconds(5)).to_rfc3339()},
        }));
        let early = object.age(Utc::now()).expect("age");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let later = object.age(Utc::now()).expect("age");
        assert!(
            later > early,
            "AGE must keep advancing on every render regardless of prepare()'s memo"
        );
    }
}
