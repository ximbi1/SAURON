//! M10.1: bounded, ordered, identity-based multi-select foundation.
//! `State.selected: Option<String>` (single cursor-bound UID) stays
//! untouched -- this is a genuinely separate, additive concept: cursor
//! focus (which row the arrow keys move) and the selected set (which
//! rows a bulk action would target) are independent, exactly like a
//! file manager's or vim visual-block's own cursor-vs-mark-set split.
//!
//! Scoped implicitly by living inside `State`: `App::cancel_scope()`
//! (called on every context/resource-kind/namespace switch, already the
//! single place that clears the single-select `selected` field) also
//! clears this -- so a selection never silently survives a view change
//! into a context/resource/namespace it was never made against. This
//! reuses that existing invariant rather than teaching `Selection` its
//! own copy of context/resource/namespace to compare against.
use crate::resources::SharedObject;

/// A concrete, deliberately small bound -- well under `Store`'s own
/// `max_objects` (thousands), because a bulk operation's own preview/
/// journal/report rendering needs to stay human-scannable, not just
/// memory-bounded. Exceeding it is an explicit refusal, never silent
/// truncation.
pub const MAX_SELECTION: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedTarget {
    pub uid: String,
    pub namespace: String,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionError {
    /// Already at `MAX_SELECTION`; carries the bound so callers can
    /// render an exact, honest refusal message rather than a vague one.
    BoundExceeded { max: usize },
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionError::BoundExceeded { max } => {
                write!(
                    f,
                    "selection is already at its bound ({max}); deselect something first"
                )
            }
        }
    }
}

/// Ordered by first-selected -- a plain `Vec` rather than a set type:
/// bounded at `MAX_SELECTION`, so linear membership/removal is cheap,
/// and insertion order is exactly what a bulk preview should render
/// (the order the user built the selection in), not an arbitrary hash
/// or sort order.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    targets: Vec<SelectedTarget>,
}
impl Selection {
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
    pub fn len(&self) -> usize {
        self.targets.len()
    }
    pub fn contains(&self, uid: &str) -> bool {
        self.targets.iter().any(|t| t.uid == uid)
    }
    /// Adds `object` if absent, removes it if already present -- the one
    /// primitive the "toggle current row" keybinding needs. Returns
    /// whether the target ended up selected (`true`) or deselected
    /// (`false`) after the call, so callers can render distinct status
    /// text without re-querying `contains`.
    pub fn toggle(&mut self, object: &SharedObject) -> Result<bool, SelectionError> {
        if let Some(pos) = self.targets.iter().position(|t| t.uid == object.uid) {
            self.targets.remove(pos);
            return Ok(false);
        }
        if self.targets.len() >= MAX_SELECTION {
            return Err(SelectionError::BoundExceeded { max: MAX_SELECTION });
        }
        self.targets.push(SelectedTarget {
            uid: object.uid.clone(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
        });
        Ok(true)
    }
    /// Idempotent add (unlike `toggle`, never removes an already-present
    /// target) -- the primitive "select visible" needs, since re-running
    /// it over an already-partially-selected view must not deselect
    /// anything. Returns `true` if this call actually added it.
    pub fn add(&mut self, object: &SharedObject) -> Result<bool, SelectionError> {
        if self.contains(&object.uid) {
            return Ok(false);
        }
        if self.targets.len() >= MAX_SELECTION {
            return Err(SelectionError::BoundExceeded { max: MAX_SELECTION });
        }
        self.targets.push(SelectedTarget {
            uid: object.uid.clone(),
            namespace: object.namespace.clone(),
            name: object.name.clone(),
        });
        Ok(true)
    }
    pub fn clear(&mut self) {
        self.targets.clear();
    }
    pub fn iter(&self) -> impl Iterator<Item = &SelectedTarget> {
        self.targets.iter()
    }
    /// Splits the selection against a fresh `rows` snapshot into what is
    /// still genuinely present versus stale (selected but no longer in
    /// the current row set -- deleted, or the view relisted without it).
    /// Never mutates `self`: a stale target stays selected (visible,
    /// explicit) until the user deliberately clears/deselects it, per
    /// M10.1's own "never silently drop, never silently rebind to a
    /// same-name new-UID row" contract. Matching is by UID alone here
    /// because `rows` is always the current single view's own row set
    /// (one context/resource/namespace-scope at a time), so UID already
    /// disambiguates without needing namespace/name in the comparison.
    pub fn status<'a>(&'a self, rows: &[SharedObject]) -> SelectionStatus<'a> {
        let mut present = Vec::new();
        let mut stale = Vec::new();
        for target in &self.targets {
            if rows.iter().any(|o| o.uid == target.uid) {
                present.push(target);
            } else {
                stale.push(target);
            }
        }
        SelectionStatus { present, stale }
    }
}
pub struct SelectionStatus<'a> {
    pub present: Vec<&'a SelectedTarget>,
    pub stale: Vec<&'a SelectedTarget>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;

    fn object(uid: &str, ns: &str, name: &str) -> SharedObject {
        std::sync::Arc::new(Object::new(json!({
            "apiVersion":"v1","kind":"Pod",
            "metadata":{"namespace":ns,"name":name,"uid":uid}
        })))
    }

    #[test]
    fn toggle_adds_then_removes_by_identity() {
        let mut s = Selection::default();
        let a = object("a", "ns", "pod-a");
        assert_eq!(s.toggle(&a), Ok(true));
        assert!(s.contains("a"));
        assert_eq!(s.len(), 1);
        assert_eq!(s.toggle(&a), Ok(false));
        assert!(!s.contains("a"));
        assert!(s.is_empty());
    }

    #[test]
    fn add_is_idempotent_never_deselects() {
        let mut s = Selection::default();
        let a = object("a", "ns", "pod-a");
        assert_eq!(s.add(&a), Ok(true));
        assert_eq!(
            s.add(&a),
            Ok(false),
            "re-adding an already-selected target is a no-op, not a toggle-off"
        );
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn bound_is_refused_explicitly_never_silently_truncated() {
        let mut s = Selection::default();
        for i in 0..MAX_SELECTION {
            s.add(&object(&i.to_string(), "ns", "pod"))
                .expect("under bound");
        }
        assert_eq!(s.len(), MAX_SELECTION);
        let over = object("over", "ns", "pod");
        assert_eq!(
            s.add(&over),
            Err(SelectionError::BoundExceeded { max: MAX_SELECTION })
        );
        assert_eq!(
            s.toggle(&over),
            Err(SelectionError::BoundExceeded { max: MAX_SELECTION })
        );
        assert_eq!(
            s.len(),
            MAX_SELECTION,
            "a refused add/toggle must never partially mutate the set"
        );
    }

    #[test]
    fn status_marks_missing_targets_stale_never_drops_or_rebinds_them() {
        let mut s = Selection::default();
        let a = object("uid-1", "ns", "pod-a");
        s.add(&a).unwrap();
        // Same name, different UID -- simulates delete+recreate. Rows no
        // longer contain "uid-1" at all.
        let replacement = object("uid-2", "ns", "pod-a");
        let rows = [replacement];
        let status = s.status(&rows);
        assert!(status.present.is_empty());
        assert_eq!(status.stale.len(), 1);
        assert_eq!(
            status.stale[0].uid, "uid-1",
            "the stale entry is still the ORIGINAL uid, never silently rebound to the new one"
        );
        assert_eq!(s.len(), 1, "status() never mutates the selection itself");
    }

    #[test]
    fn status_present_when_row_still_there() {
        let mut s = Selection::default();
        let a = object("uid-1", "ns", "pod-a");
        s.add(&a).unwrap();
        let rows = [a.clone()];
        let status = s.status(&rows);
        assert_eq!(status.present.len(), 1);
        assert!(status.stale.is_empty());
    }

    #[test]
    fn clear_empties_regardless_of_stale_or_present() {
        let mut s = Selection::default();
        s.add(&object("a", "ns", "x")).unwrap();
        s.add(&object("b", "ns", "y")).unwrap();
        s.clear();
        assert!(s.is_empty());
    }

    #[test]
    fn iter_preserves_insertion_order() {
        let mut s = Selection::default();
        s.add(&object("c", "ns", "c")).unwrap();
        s.add(&object("a", "ns", "a")).unwrap();
        s.add(&object("b", "ns", "b")).unwrap();
        let order: Vec<_> = s.iter().map(|t| t.uid.as_str()).collect();
        assert_eq!(order, ["c", "a", "b"]);
    }
}
