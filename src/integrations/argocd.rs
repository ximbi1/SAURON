//! M9.3: Argo CD read-only views. Pure rendering only -- no network, no
//! mutation, no `argocd` CLI shell-out. Discovery reuses M9.0's
//! `discover()`; listing/browsing `Application`/`ApplicationSet`/
//! `AppProject` reuses the existing generic resource table exactly like
//! M9.1's Flux support does. This module renders each `Application`'s
//! own reported status as evidence -- it never computes a second health
//! score and never claims sync/health beyond what Argo CD itself
//! reports.
use super::ExpectedKind;
use serde_json::Value;

pub const KINDS: &[ExpectedKind] = &[
    ExpectedKind::new("Application", "argoproj.io", "Application"),
    ExpectedKind::new("ApplicationSet", "argoproj.io", "ApplicationSet"),
    ExpectedKind::new("AppProject", "argoproj.io", "AppProject"),
];

/// Kinds this module renders a dedicated status report for.
/// `ApplicationSet`/`AppProject` are discovered (M9.0) and generically
/// browsable, but their own shape (a template generating many
/// Applications; a project's RBAC/source-restriction policy) is
/// different enough from `Application`'s own sync/health status that a
/// dedicated renderer is deferred -- a bounded, documented limitation,
/// matching M9.1's own treatment of Flux's Image* kinds.
pub const STATUS_KINDS: &[&str] = &["Application"];

pub fn is_status_kind(kind: &str) -> bool {
    STATUS_KINDS.contains(&kind)
}

fn sources(v: &Value) -> Vec<&Value> {
    if let Some(list) = v.pointer("/spec/sources").and_then(Value::as_array) {
        list.iter().collect()
    } else if let Some(one) = v.pointer("/spec/source") {
        vec![one]
    } else {
        vec![]
    }
}

/// Verbatim rendering of one `Application`'s own reported status: every
/// source (single or multi-source), destination, sync status + revision,
/// health status, operation state (if a sync is running/finished),
/// conditions, automated sync policy, project, and a bounded managed-
/// resource summary. Every line traces to a field this object actually
/// has; an absent field is reported as absent, never defaulted.
pub fn status_report(kind: &str, name: &str, v: &Value) -> String {
    let mut out = format!("ARGO CD STATUS: {kind}/{name}\n\n");
    let project = v
        .pointer("/spec/project")
        .and_then(Value::as_str)
        .unwrap_or("(not reported)");
    out.push_str(&format!("project: {project}\n\n"));

    out.push_str("SOURCE(S):\n");
    let srcs = sources(v);
    if srcs.is_empty() {
        out.push_str("  (none reported)\n");
    }
    for s in &srcs {
        let repo = s.get("repoURL").and_then(Value::as_str).unwrap_or("?");
        let path = s.get("path").and_then(Value::as_str);
        let chart = s.get("chart").and_then(Value::as_str);
        let revision = s
            .get("targetRevision")
            .and_then(Value::as_str)
            .unwrap_or("(default)");
        match (path, chart) {
            (Some(path), _) => out.push_str(&format!(
                "  - {repo} path={path} targetRevision={revision}\n"
            )),
            (None, Some(chart)) => out.push_str(&format!(
                "  - {repo} chart={chart} targetRevision={revision}\n"
            )),
            (None, None) => out.push_str(&format!("  - {repo} targetRevision={revision}\n")),
        }
    }

    if let Some(destination) = v.pointer("/spec/destination") {
        let server = destination
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("(not reported)");
        let namespace = destination
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or("(not reported)");
        out.push_str(&format!("\nDESTINATION: {server} / {namespace}\n"));
    }

    let sync_status = v
        .pointer("/status/sync/status")
        .and_then(Value::as_str)
        .unwrap_or("(not reported)");
    let synced_revision = v.pointer("/status/sync/revision").and_then(Value::as_str);
    out.push_str(&format!("\nSYNC STATUS: {sync_status}"));
    if let Some(rev) = synced_revision {
        out.push_str(&format!(" (revision: {rev})"));
    }
    out.push('\n');

    let health_status = v
        .pointer("/status/health/status")
        .and_then(Value::as_str)
        .unwrap_or("(not reported)");
    out.push_str(&format!("HEALTH STATUS: {health_status}\n"));

    let automated = v.pointer("/spec/syncPolicy/automated").is_some();
    out.push_str(&format!("automated sync policy: {automated}\n"));

    if let Some(op) = v.pointer("/status/operationState") {
        let phase = op.get("phase").and_then(Value::as_str).unwrap_or("?");
        let message = op.get("message").and_then(Value::as_str).unwrap_or("");
        out.push_str(&format!("\nOPERATION STATE: {phase} -- {message}\n"));
    } else {
        out.push_str("\nOPERATION STATE: none reported (no sync attempted yet)\n");
    }

    let conditions = v
        .pointer("/status/conditions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    out.push_str("\nCONDITIONS (Argo CD's own, rendered verbatim -- not a second health score):\n");
    if conditions.is_empty() {
        out.push_str("  (none reported)\n");
    }
    for c in conditions {
        out.push_str(&format!(
            "  {}: {}\n",
            c.get("type").and_then(Value::as_str).unwrap_or("?"),
            c.get("message").and_then(Value::as_str).unwrap_or(""),
        ));
    }

    let resources = v
        .pointer("/status/resources")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    out.push_str(&format!("\nMANAGED RESOURCES ({}):\n", resources.len()));
    for r in resources.iter().take(20) {
        let kind = r.get("kind").and_then(Value::as_str).unwrap_or("?");
        let name = r.get("name").and_then(Value::as_str).unwrap_or("?");
        let namespace = r.get("namespace").and_then(Value::as_str).unwrap_or("");
        let status = r.get("status").and_then(Value::as_str).unwrap_or("?");
        out.push_str(&format!("  - {kind} {namespace}/{name}: {status}\n"));
    }
    if resources.len() > 20 {
        out.push_str(&format!(
            "  ... {} more (bounded preview)\n",
            resources.len() - 20
        ));
    }

    out.push_str(
        "\nThis is Argo CD's own reported status, rendered verbatim as evidence -- SAURON \
         does not compute a second health score here and does not claim sync/health beyond \
         what Argo CD itself reports. Missing/absent fields are reported as such, never \
         inferred.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn synced_healthy_application_shows_source_destination_and_status() {
        let v = json!({
            "spec": {
                "project": "default",
                "source": {"repoURL": "https://github.com/argoproj/argocd-example-apps.git", "path": "guestbook", "targetRevision": "master"},
                "destination": {"server": "https://kubernetes.default.svc", "namespace": "sauron-m9"},
                "syncPolicy": {},
            },
            "status": {
                "sync": {"status": "Synced", "revision": "abc123"},
                "health": {"status": "Healthy"},
                "operationState": {"phase": "Succeeded", "message": "successfully synced (all tasks run)"},
                "resources": [{"kind": "Service", "name": "guestbook-ui", "namespace": "sauron-m9", "status": "Synced", "version": "v1"}],
            },
        });
        let report = status_report("Application", "guestbook", &v);
        assert!(report.contains("project: default"));
        assert!(report.contains("https://github.com/argoproj/argocd-example-apps.git"));
        assert!(report.contains("path=guestbook targetRevision=master"));
        assert!(report.contains("DESTINATION: https://kubernetes.default.svc / sauron-m9"));
        assert!(report.contains("SYNC STATUS: Synced (revision: abc123)"));
        assert!(report.contains("HEALTH STATUS: Healthy"));
        assert!(report.contains("automated sync policy: false"));
        assert!(report.contains("OPERATION STATE: Succeeded -- successfully synced"));
        assert!(report.contains("MANAGED RESOURCES (1):"));
        assert!(report.contains("Service sauron-m9/guestbook-ui: Synced"));
    }

    #[test]
    fn broken_application_shows_unknown_sync_and_comparison_error_condition() {
        let v = json!({
            "spec": {
                "project": "default",
                "source": {"repoURL": "https://example.com/repo.git", "path": "guestbook", "targetRevision": "no-such-branch"},
                "destination": {"server": "https://kubernetes.default.svc", "namespace": "sauron-m9"},
            },
            "status": {
                "sync": {"status": "Unknown"},
                "health": {"status": "Healthy"},
                "conditions": [{"type": "ComparisonError", "message": "unable to resolve 'no-such-branch' to a commit SHA"}],
            },
        });
        let report = status_report("Application", "guestbook-broken", &v);
        assert!(report.contains("SYNC STATUS: Unknown"));
        assert!(!report.contains("(revision:"));
        assert!(
            report.contains("ComparisonError: unable to resolve 'no-such-branch' to a commit SHA")
        );
        assert!(report.contains("OPERATION STATE: none reported (no sync attempted yet)"));
        assert!(report.contains("MANAGED RESOURCES (0):"));
    }

    #[test]
    fn multi_source_application_lists_every_source() {
        let v = json!({
            "spec": {
                "project": "default",
                "sources": [
                    {"repoURL": "https://github.com/a/a.git", "path": ".", "targetRevision": "main"},
                    {"repoURL": "https://charts.example.com", "chart": "app", "targetRevision": "1.2.3"},
                ],
            },
            "status": {},
        });
        let report = status_report("Application", "multi", &v);
        assert!(report.contains("path=. targetRevision=main"));
        assert!(report.contains("chart=app targetRevision=1.2.3"));
    }

    #[test]
    fn automated_sync_policy_presence_is_shown_distinctly() {
        let v = json!({
            "spec": {"project": "default", "syncPolicy": {"automated": {"prune": true, "selfHeal": true}}},
            "status": {},
        });
        let report = status_report("Application", "auto", &v);
        assert!(report.contains("automated sync policy: true"));
    }

    #[test]
    fn is_status_kind_recognizes_application_only() {
        assert!(is_status_kind("Application"));
        assert!(!is_status_kind("ApplicationSet"));
        assert!(!is_status_kind("AppProject"));
    }
}
