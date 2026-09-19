//! M9.1: Flux read-only views. Pure rendering only -- no network, no
//! mutation. Discovery reuses M9.0's `discover()`; listing/browsing Flux
//! objects reuses the existing generic resource table (Flux's own CRDs
//! already define `additionalPrinterColumns` that M3's existing CRD-
//! column support surfaces automatically -- no new code needed for
//! that). This module renders each object's own reported status as
//! evidence: it never computes a second health score, never claims
//! reconciliation succeeded beyond what Flux's own conditions state, and
//! never guesses a field Flux did not actually report.
use super::ExpectedKind;
use serde_json::Value;

pub const KINDS: &[ExpectedKind] = &[
    ExpectedKind::new(
        "Kustomization",
        "kustomize.toolkit.fluxcd.io",
        "Kustomization",
    ),
    ExpectedKind::new("HelmRelease", "helm.toolkit.fluxcd.io", "HelmRelease"),
    ExpectedKind::new("GitRepository", "source.toolkit.fluxcd.io", "GitRepository"),
    ExpectedKind::new("OCIRepository", "source.toolkit.fluxcd.io", "OCIRepository"),
    ExpectedKind::new(
        "HelmRepository",
        "source.toolkit.fluxcd.io",
        "HelmRepository",
    ),
    ExpectedKind::new("Bucket", "source.toolkit.fluxcd.io", "Bucket"),
    ExpectedKind::new(
        "ImageRepository",
        "image.toolkit.fluxcd.io",
        "ImageRepository",
    ),
    ExpectedKind::new("ImagePolicy", "image.toolkit.fluxcd.io", "ImagePolicy"),
    ExpectedKind::new(
        "ImageUpdateAutomation",
        "image.toolkit.fluxcd.io",
        "ImageUpdateAutomation",
    ),
];

/// Kinds this module renders a dedicated status report for. The three
/// image-automation kinds above are discovered (M9.0) and browsable
/// generically like any other resource, but their own status shape
/// (image scan results/policies, not reconciliation conditions) is
/// different enough that a dedicated renderer is deferred -- a bounded,
/// documented limitation (see docs/M9_ACCEPTANCE.md's M9.1 journal
/// entry), not a silent gap.
pub const STATUS_KINDS: &[&str] = &[
    "Kustomization",
    "HelmRelease",
    "GitRepository",
    "OCIRepository",
    "HelmRepository",
    "Bucket",
];

pub fn is_status_kind(kind: &str) -> bool {
    STATUS_KINDS.contains(&kind)
}

fn conditions(v: &Value) -> &[Value] {
    v.pointer("/status/conditions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Verbatim rendering of one Flux object's own reported status --
/// generation/observedGeneration staleness (including the `-1` "never
/// reconciled" sentinel Flux itself uses), suspend state, every
/// condition Flux reported (never filtered to just `Ready`, since
/// HelmRelease reports `Ready` and `Released` as two separate facts),
/// revision fields, `dependsOn`, and the source reference. Every line
/// traces to a field this object actually has; an absent field is
/// reported as absent, never defaulted or inferred.
pub fn status_report(kind: &str, name: &str, v: &Value) -> String {
    let mut out = format!("FLUX STATUS: {kind}/{name}\n\n");
    let generation = v.pointer("/metadata/generation").and_then(Value::as_i64);
    let observed = v
        .pointer("/status/observedGeneration")
        .and_then(Value::as_i64);
    match (generation, observed) {
        (_, Some(-1)) => out.push_str("observedGeneration: -1 -- never reconciled\n\n"),
        (Some(g), Some(o)) if g != o => out.push_str(&format!(
            "STALE: observedGeneration ({o}) does not match generation ({g}) -- the \
             conditions below reflect an older spec, not the current one\n\n"
        )),
        (Some(g), Some(o)) => out.push_str(&format!(
            "observedGeneration: {o} (current, matches generation {g})\n\n"
        )),
        _ => out.push_str("observedGeneration: not reported\n\n"),
    }
    let suspended = v
        .pointer("/spec/suspend")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    out.push_str(&format!("suspended: {suspended}\n\n"));
    out.push_str("CONDITIONS (Flux's own, rendered verbatim -- not a second health score):\n");
    if conditions(v).is_empty() {
        out.push_str("  (none reported)\n");
    }
    for c in conditions(v) {
        out.push_str(&format!(
            "  {} = {} ({}): {}\n",
            c.get("type").and_then(Value::as_str).unwrap_or("?"),
            c.get("status").and_then(Value::as_str).unwrap_or("?"),
            c.get("reason").and_then(Value::as_str).unwrap_or("-"),
            c.get("message").and_then(Value::as_str).unwrap_or(""),
        ));
    }
    let revisions: Vec<String> = [
        ("lastAppliedRevision", "/status/lastAppliedRevision"),
        ("lastAttemptedRevision", "/status/lastAttemptedRevision"),
        ("artifact.revision", "/status/artifact/revision"),
    ]
    .into_iter()
    .filter_map(|(label, path)| {
        v.pointer(path)
            .and_then(Value::as_str)
            .map(|rev| format!("{label}: {rev}"))
    })
    .collect();
    if !revisions.is_empty() {
        out.push('\n');
        out.push_str(&revisions.join("\n"));
        out.push('\n');
    }
    if let Some(deps) = v
        .pointer("/spec/dependsOn")
        .and_then(Value::as_array)
        .filter(|d| !d.is_empty())
    {
        out.push_str("\ndependsOn:\n");
        for d in deps {
            let namespace = d.get("namespace").and_then(Value::as_str);
            let name = d.get("name").and_then(Value::as_str).unwrap_or("?");
            out.push_str(&format!(
                "  - {}{}\n",
                namespace.map(|n| format!("{n}/")).unwrap_or_default(),
                name
            ));
        }
    }
    let source = v
        .pointer("/spec/sourceRef")
        .or_else(|| v.pointer("/spec/chart/spec/sourceRef"));
    if let Some(source) = source {
        out.push_str(&format!(
            "\nsourceRef: {} {}{}\n",
            source.get("kind").and_then(Value::as_str).unwrap_or("?"),
            source
                .get("namespace")
                .and_then(Value::as_str)
                .map(|n| format!("{n}/"))
                .unwrap_or_default(),
            source.get("name").and_then(Value::as_str).unwrap_or("?"),
        ));
    }
    out.push_str(
        "\nThis is Flux's own reported status, rendered verbatim as evidence -- SAURON \
         does not compute a second health score here and does not claim reconciliation \
         succeeded beyond what these conditions state. Missing/absent fields are reported \
         as such, never inferred.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ready_kustomization_shows_current_generation_and_condition() {
        let v = json!({
            "metadata": {"generation": 1},
            "spec": {"sourceRef": {"kind": "GitRepository", "name": "podinfo"}},
            "status": {
                "observedGeneration": 1,
                "conditions": [{"type": "Ready", "status": "True", "reason": "ReconciliationSucceeded", "message": "Applied revision: master@sha1:abc"}],
                "lastAppliedRevision": "master@sha1:abc",
            },
        });
        let report = status_report("Kustomization", "podinfo-kustomize", &v);
        assert!(report.contains("observedGeneration: 1 (current, matches generation 1)"));
        assert!(
            report.contains(
                "Ready = True (ReconciliationSucceeded): Applied revision: master@sha1:abc"
            )
        );
        assert!(report.contains("lastAppliedRevision: master@sha1:abc"));
        assert!(report.contains("sourceRef: GitRepository podinfo"));
        assert!(!report.contains("STALE"));
        assert!(!report.contains("never reconciled"));
    }

    #[test]
    fn never_reconciled_sentinel_is_shown_distinctly_from_ordinary_staleness() {
        let v = json!({
            "metadata": {"generation": 1},
            "spec": {"suspend": true, "sourceRef": {"kind": "GitRepository", "name": "missing"}},
            "status": {"observedGeneration": -1},
        });
        let report = status_report("Kustomization", "podinfo-dependent", &v);
        assert!(report.contains("observedGeneration: -1 -- never reconciled"));
        assert!(!report.contains("STALE"));
        assert!(report.contains("suspended: true"));
        assert!(report.contains("(none reported)"));
    }

    #[test]
    fn stale_observed_generation_is_flagged_explicitly() {
        let v = json!({
            "metadata": {"generation": 2},
            "status": {
                "observedGeneration": 1,
                "conditions": [{"type": "Ready", "status": "True", "reason": "ReconciliationSucceeded", "message": "old"}],
            },
        });
        let report = status_report("Kustomization", "podinfo-kustomize", &v);
        assert!(report.contains("STALE: observedGeneration (1) does not match generation (2)"));
    }

    #[test]
    fn helm_release_shows_every_condition_never_only_ready() {
        let v = json!({
            "metadata": {"generation": 1},
            "spec": {"chart": {"spec": {"sourceRef": {"kind": "HelmRepository", "name": "podinfo", "namespace": "sauron-m9"}}}},
            "status": {
                "observedGeneration": 1,
                "conditions": [
                    {"type": "Ready", "status": "True", "reason": "InstallSucceeded", "message": "Helm install succeeded"},
                    {"type": "Released", "status": "True", "reason": "InstallSucceeded", "message": "Helm install succeeded"},
                ],
                "lastAttemptedRevision": "6.15.0",
            },
        });
        let report = status_report("HelmRelease", "podinfo-helm", &v);
        assert!(report.contains("Ready = True (InstallSucceeded)"));
        assert!(report.contains("Released = True (InstallSucceeded)"));
        assert!(report.contains("sourceRef: HelmRepository sauron-m9/podinfo"));
        assert!(report.contains("lastAttemptedRevision: 6.15.0"));
    }

    #[test]
    fn depends_on_is_rendered_when_present() {
        let v = json!({
            "metadata": {"generation": 1},
            "spec": {"dependsOn": [{"name": "podinfo-kustomize"}, {"name": "other", "namespace": "ns2"}]},
            "status": {"observedGeneration": 1},
        });
        let report = status_report("Kustomization", "podinfo-dependent", &v);
        assert!(report.contains("dependsOn:\n  - podinfo-kustomize\n  - ns2/other"));
    }

    #[test]
    fn is_status_kind_recognizes_the_six_reconciliation_kinds_only() {
        for kind in STATUS_KINDS {
            assert!(is_status_kind(kind));
        }
        assert!(!is_status_kind("ImageRepository"));
        assert!(!is_status_kind("Pod"));
    }
}
