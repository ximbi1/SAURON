//! Pulse = a refreshed tile summary of the currently watched scope, not a
//! second continuous watch architecture. v1 is 100% derived from state
//! already loaded by the existing watch (`state.rows`/`state.store`/
//! `state.metrics`) -- zero new Kubernetes requests, so "bounded
//! asynchronous refreshed" is satisfied by the existing render loop:
//! opening/refreshing Pulse simply recomputes these pure tiles against
//! current state. See `docs/M11_ACCEPTANCE.md`'s M11.3 section.
//!
//! Deliberately distinct from `:info` (connection/task/session plumbing):
//! Pulse is about the current resource scope's health/metrics evidence,
//! which `:info` does not cover -- composing, not duplicating.
use crate::eye::ScopeCaveat;
use crate::resources::SharedObject;

pub fn report(rows: &[SharedObject], caveats: &[ScopeCaveat], metrics_summary: &str) -> String {
    let counts = crate::resources::priority::severity_counts(rows);
    let mut text = format!(
        "PULSE: {} row(s) in current scope\n\n\
         HEALTH\n  Critical: {}\n  Warning:  {}\n  Unknown:  {}\n  Healthy:  {}\n\n\
         METRICS\n  {metrics_summary}\n\n",
        counts.total(),
        counts.critical,
        counts.warning,
        counts.unknown,
        counts.healthy,
    );
    text.push_str("SCOPE\n");
    if caveats.is_empty() {
        text.push_str("  No caveats: this view reflects the full current selection.\n");
    } else {
        for caveat in caveats {
            // Reuses Eye's own `ScopeCaveat::text` verbatim -- one shared
            // vocabulary between the two views, never a second wording.
            text.push_str(&format!("  CAVEAT: {}\n", caveat.text()));
        }
    }
    text
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
    fn zero_rows_is_explicit_not_an_all_healthy_claim() {
        let text = report(&[], &[], "Metrics UNKNOWN: Unavailable");
        assert!(text.contains("0 row(s)"));
        assert!(text.contains("Critical: 0"));
        // Zero known problems must never be presented the same as "checked
        // and found healthy" when metrics themselves are unavailable --
        // both facts stay visible side by side, never collapsed.
        assert!(text.contains("Metrics UNKNOWN"));
    }

    #[test]
    fn metrics_unavailable_tile_is_distinct_from_metrics_healthy() {
        let unavailable = report(&[pod("a", "Running")], &[], "Metrics UNKNOWN: Unavailable");
        let sampled = report(
            &[pod("a", "Running")],
            &[],
            "Metrics sampled: 1/1 fresh; omitted=0 malformed=0",
        );
        assert!(unavailable.contains("UNKNOWN"));
        assert!(!sampled.contains("UNKNOWN"));
        assert_ne!(unavailable, sampled);
    }

    #[test]
    fn counts_are_exhaustive_and_match_severity_counts() {
        let rows = vec![pod("a", "Running"), pod("b", "Failed")];
        let text = report(&rows, &[], "");
        assert!(text.contains("2 row(s)"));
        assert!(text.contains("Critical: 1"));
        assert!(text.contains("Healthy:  1"));
    }

    #[test]
    fn caveats_never_silently_dropped_and_absence_is_stated() {
        let with = report(&[], &[ScopeCaveat::Incomplete], "");
        assert!(with.contains("PARTIAL: object cache bound reached"));
        let without = report(&[], &[], "");
        assert!(without.contains("No caveats"));
    }
}
