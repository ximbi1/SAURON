//! M11.1 shared foundation: deterministic, evidence-preserving aggregation
//! over an already-known resource scope. Pure functions only -- no network,
//! no new truth, no second health/evidence engine. Reads `Object.health`
//! (M5.3, unchanged) and nothing else. See `docs/M11_ACCEPTANCE.md`'s M11.1
//! section for the reconnaissance this composes rather than reinvents.
//!
//! Callers are responsible for scope/epoch safety: this module receives
//! whatever `&[SharedObject]` the caller already trusts as current (e.g.
//! `State::rows` after `prepare()`), and produces no state of its own that
//! could go stale independently -- there is nothing here to cancel, because
//! there is no I/O and no memoization across calls.
use super::{SharedObject, health::Severity};

/// Highest-attention first: Critical, then Warning, then Unknown (uncertainty
/// deserves attention above a confirmed-healthy result), then Healthy last.
/// This is exactly `Severity`'s own existing `Ord` reversed -- no numeric
/// score, no opaque weighting. Ties are left in the caller's input order
/// (stable sort), matching `resources::sort::rows`'s own convention of
/// relying on the store's canonical namespace/name order for tie-breaking
/// rather than re-deriving one here.
pub fn by_attention(rows: &[SharedObject]) -> Vec<SharedObject> {
    let mut ordered: Vec<_> = rows.to_vec();
    ordered.sort_by_key(|o| std::cmp::Reverse(o.health.severity));
    ordered
}

/// Bounded, exhaustive counts -- every row is in exactly one bucket, so a
/// reader can always reconstruct `rows.len()` from this struct alone. Never
/// silently drops a row into "healthy" for lack of a category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeverityCounts {
    pub healthy: usize,
    pub unknown: usize,
    pub warning: usize,
    pub critical: usize,
}
impl SeverityCounts {
    pub fn total(&self) -> usize {
        self.healthy + self.unknown + self.warning + self.critical
    }
    pub fn problems(&self) -> usize {
        self.warning + self.critical
    }
}
pub fn severity_counts(rows: &[SharedObject]) -> SeverityCounts {
    let mut counts = SeverityCounts::default();
    for row in rows {
        match row.health.severity {
            Severity::Healthy => counts.healthy += 1,
            Severity::Unknown => counts.unknown += 1,
            Severity::Warning => counts.warning += 1,
            Severity::Critical => counts.critical += 1,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Object;
    use serde_json::json;
    use std::sync::Arc;

    fn pod(name: &str, phase: &str) -> SharedObject {
        let mut status = json!({"phase": phase});
        if phase == "Running" {
            status["conditions"] = json!([{"type": "Ready", "status": "True"}]);
        }
        Arc::new(Object::new(json!({
            "apiVersion": "v1", "kind": "Pod",
            "metadata": {"namespace": "default", "name": name, "uid": name},
            "status": status,
        })))
    }

    #[test]
    fn attention_order_is_critical_then_warning_then_unknown_then_healthy() {
        let healthy = pod("a-healthy", "Running");
        let failed = pod("b-failed", "Failed");
        let unknown = Arc::new(Object::new(json!({
            "apiVersion": "v1", "kind": "Widget",
            "metadata": {"namespace": "default", "name": "c-unknown", "uid": "c-unknown"},
        })));
        let rows = vec![healthy.clone(), failed.clone(), unknown.clone()];
        let ordered = by_attention(&rows);
        let names: Vec<_> = ordered.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(healthy.health.severity, Severity::Healthy);
        assert_eq!(failed.health.severity, Severity::Critical);
        assert_eq!(unknown.health.severity, Severity::Unknown);
        assert_eq!(names, vec!["b-failed", "c-unknown", "a-healthy"]);
    }

    #[test]
    fn attention_order_never_reorders_healthy_ahead_of_a_real_problem() {
        // A broader regression guard than the single fixed-input test above:
        // whatever the concrete severities are, every Warning/Critical row
        // must sort strictly before every Healthy row.
        let rows = vec![pod("a", "Running"), pod("b", "Failed"), pod("c", "Running")];
        let ordered = by_attention(&rows);
        let first_healthy = ordered
            .iter()
            .position(|o| o.health.severity == Severity::Healthy);
        let last_problem = ordered
            .iter()
            .rposition(|o| o.health.severity >= Severity::Warning);
        if let (Some(h), Some(p)) = (first_healthy, last_problem) {
            assert!(p < h, "a problem row sorted after a healthy row");
        }
    }

    #[test]
    fn stable_sort_preserves_input_tie_order_same_as_resources_sort() {
        let a = pod("a", "Failed");
        let b = pod("b", "Failed");
        let ordered = by_attention(&[a.clone(), b.clone()]);
        assert_eq!(
            ordered.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"],
            "equal severity must preserve caller order, not reorder arbitrarily"
        );
    }

    #[test]
    fn severity_counts_are_exhaustive_and_reconstruct_total() {
        let rows = vec![pod("a", "Running"), pod("b", "Failed"), pod("c", "Failed")];
        let counts = severity_counts(&rows);
        assert_eq!(counts.healthy, 1);
        assert_eq!(counts.critical, 2);
        assert_eq!(counts.warning, 0);
        assert_eq!(counts.unknown, 0);
        assert_eq!(counts.total(), rows.len());
        assert_eq!(counts.problems(), 2);
    }

    #[test]
    fn empty_scope_is_zero_counts_not_a_panic_or_a_default_healthy_claim() {
        let counts = severity_counts(&[]);
        assert_eq!(counts, SeverityCounts::default());
        assert_eq!(counts.total(), 0);
        assert!(by_attention(&[]).is_empty());
    }

    #[test]
    fn same_name_different_uid_are_never_merged_into_one_row() {
        // Two distinct incarnations that happen to share a namespace/name --
        // priority ordering must carry both through untouched, never
        // collapse them by name (matches this project's UID != NAME
        // invariant; a real merge bug would drop one row silently).
        let old = pod("replaced", "Failed");
        let new = Arc::new(Object::new(json!({
            "apiVersion": "v1", "kind": "Pod",
            "metadata": {"namespace": "default", "name": "replaced", "uid": "new-uid"},
            "status": {"phase": "Running"},
        })));
        let ordered = by_attention(&[old.clone(), new.clone()]);
        assert_eq!(ordered.len(), 2);
        assert!(ordered.iter().any(|o| o.uid == old.uid));
        assert!(ordered.iter().any(|o| o.uid == new.uid));
    }
}
