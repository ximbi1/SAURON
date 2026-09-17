pub use super::document::Document;
use crate::{
    command::{Action, Keymap},
    config::Settings,
    filters::{Expr, Truth, value::Field},
    kube::{discovery::Resource, watch::Query},
    resources::{SharedObject, store::Store},
};
use chrono::Utc;

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
    Port,
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
/// resolve canonical GVR only when crossing catalogs, restart the watch) and
/// lets the normal watch/rebuild pipeline repopulate real data, rather than reviving
/// anything old as if it were current. `resource` is resolved metadata, never a raw
/// human alias. Explicit server selectors are part of the intent, not global state.
/// `selected` is a UID, not a name: same-name-different-UID must not reselect (matches
/// the existing rebuild() invariant that replacement clears selection).
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub context: String,
    pub namespace: Option<String>,
    pub resource: Resource,
    pub labels: Option<String>,
    pub fields: Option<String>,
    pub filter_text: String,
    pub sort: String,
    pub descending: bool,
    pub selected: Option<String>,
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
    pub input_error: Option<String>,
    pub synced: bool,
    pub dirty: bool,
    pub quit: bool,
    pub watch_errors: u64,
    pub forward_count: usize,
    pub filter_unknown: usize,
    pub autoselect: bool,
    pub page_size: usize,
    /// Live-extracted CRD `additionalPrinterColumns`, fetched once per resource view
    /// (never per object); empty for curated kinds and non-CRD resources alike. See
    /// `kube::printer` for why this is preferred over server Table conversion.
    pub printer_columns: Vec<crate::kube::printer::PrinterColumn>,
    pub metrics: super::metrics::Cache,
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
            input_error: None,
            synced: false,
            dirty: true,
            quit: false,
            watch_errors: 0,
            forward_count: 0,
            filter_unknown: 0,
            autoselect: true,
            page_size: 20,
            printer_columns: vec![],
            metrics: super::metrics::Cache::default(),
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
        for column in &self.printer_columns {
            if (column.priority == 0 || self.wide) && !columns.contains(&column.name) {
                columns.push(column.name.clone());
            }
        }
        columns.push("AGE".into());
        columns
    }
    /// A column's display value for `object`: a live CRD printer column first (by
    /// name), falling back to the object's own curated/generic field lookup. Printer
    /// columns are checked first since their names are resource-declared and could in
    /// principle shadow a generic field name; in practice authors avoid the common
    /// ones (`NAME`/`AGE`/`STATUS`), so this ordering is a safety default, not a
    /// scenario expected to matter often.
    pub fn cell(
        &self,
        object: &crate::resources::Object,
        column: &str,
        now: chrono::DateTime<Utc>,
    ) -> Option<String> {
        self.printer_columns
            .iter()
            .find(|c| c.name == column)
            .and_then(|c| c.field.read(object, now))
            .map(|s| s.display())
            .or_else(|| object.field(column, now))
    }
    pub fn set_filter(&mut self, text: &str) -> anyhow::Result<()> {
        let expression = Expr::parse(text)?;
        self.filter = expression;
        self.filter_text = text.to_owned();
        self.input_error = None;
        self.dirty = true;
        Ok(())
    }
    pub fn selected_object(&self) -> Option<SharedObject> {
        self.selected
            .as_ref()
            .and_then(|uid| self.rows.iter().find(|o| &o.uid == uid))
            .cloned()
    }
    pub fn rebuild(&mut self) {
        let now = Utc::now();
        self.filter_unknown = 0;
        self.rows = self
            .store
            .objects
            .values()
            .filter(|o| match self.filter.evaluate(o, now) {
                Truth::Yes => true,
                Truth::No => false,
                Truth::Unknown => {
                    self.filter_unknown += 1;
                    false
                }
            })
            .cloned()
            .collect();
        if let Ok(field) = Field::parse(&self.sort) {
            crate::resources::sort::rows(&mut self.rows, &field, self.descending, now);
        }
        // A pending list has not disproved the history's selected UID. Conversely,
        // after live deletion an empty→nonempty transition must not select a replacement.
        if self.synced || !self.store.objects.is_empty() {
            if let Some(uid) = &self.selected {
                if !self.rows.iter().any(|o| &o.uid == uid) {
                    self.selected = None;
                }
            } else if self.autoselect {
                self.selected = self.rows.first().map(|o| o.uid.clone());
            }
            self.autoselect = false;
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
            doc.navigate(action);
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
    fn sorting_preserves_uid_and_does_not_reselect_after_delete_recreate() {
        use crate::resources::Object;
        use serde_json::json;
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        let object = |name: &str, uid: &str, rv: &str, n: i64| {
            Object::new(json!({
                "metadata":{"name":name,"uid":uid,"resourceVersion":rv,"creationTimestamp":format!("2026-09-15T10:00:0{n}Z")},
                "kind":"Pod","apiVersion":"v1","spec":{"containers":[{"name":"c"}]},
                "status":{"containerStatuses":[{"name":"c","ready":true,"restartCount":n}]}
            }))
        };
        s.store.apply(object("a", "a", "1", 2), false);
        s.store.apply(object("b", "b", "1", 1), false);
        s.synced = true;
        s.prepare();
        assert_eq!(s.selected.as_deref(), Some("a"));
        s.sort = "RESTARTS".into();
        s.prepare();
        assert_eq!(s.rows[0].name, "b");
        assert_eq!(s.selected.as_deref(), Some("a"));
        s.descending = true;
        s.prepare();
        assert_eq!(s.rows[0].name, "a");
        s.sort = "AGE".into();
        s.prepare();
        assert_eq!(s.rows[0].name, "b");
        for i in 3..9 {
            s.store.apply(object("a", "a", &i.to_string(), i), false);
            s.prepare();
            assert_eq!(s.selected.as_deref(), Some("a"));
        }
        s.store.delete(&object("a", "a", "9", 8));
        s.prepare();
        assert!(s.selected.is_none());
        s.store.delete(&object("b", "b", "1", 1));
        s.prepare();
        s.store.apply(object("a", "replacement", "10", 2), false);
        s.prepare();
        assert!(
            s.selected.is_none(),
            "a new incarnation must not become selected"
        );
    }
    #[test]
    fn history_selection_waits_for_initial_list_before_validation() {
        let mut s = State::new(Query::default(), &Settings::default()).expect("state");
        s.selected = Some("history-uid".into());
        s.prepare();
        assert_eq!(s.selected.as_deref(), Some("history-uid"));
        s.store.begin();
        s.prepare();
        assert_eq!(s.selected.as_deref(), Some("history-uid"));
        s.store.apply(
            crate::resources::Object::new(
                serde_json::json!({"metadata":{"name":"target","uid":"history-uid"}}),
            ),
            true,
        );
        s.store.finish();
        s.synced = true;
        s.prepare();
        assert_eq!(s.selected_object().expect("selected").uid, "history-uid");
    }
    #[test]
    fn repaired_filter_clears_input_error_but_preserves_transport_error() {
        let mut state = State::new(Query::default(), &Settings::default()).expect("state");
        state.set_filter("name=api").expect("valid");
        state.error = Some("Forbidden API".into());
        state.input_error = state.set_filter("/[/").err().map(|e| e.to_string());
        assert!(state.input_error.is_some());
        assert_eq!(state.filter_text, "name=api");
        state.set_filter("/^api/").expect("repaired");
        assert!(state.input_error.is_none());
        assert_eq!(state.error.as_deref(), Some("Forbidden API"));
    }
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
