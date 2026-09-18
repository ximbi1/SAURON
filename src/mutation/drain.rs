//! M8B.5: Drain's pure planning logic -- deciding which Pods on a Node are
//! eligible for eviction, and which are excluded and why. No network, no
//! policy decision, no mutation: exactly the same "pure builder" role
//! `mutation::workflow`'s functions play for every other action. The
//! actual orchestrated execution (cordon, then evict each eligible Pod
//! through the existing `kube::mutation::commit`/`verify`) lives in
//! `kube::drain`, which is the only place that issues requests.
use super::{MutationIntent, PolicyEvaluation};
use crate::kube::discovery::Resource;
use serde_json::Value;

pub const DRAIN_KINDS: &[&str] = &["Node"];

/// Why a Pod was excluded from eviction -- never silently skipped without
/// a reason the preview can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exclusion {
    /// Owned by a DaemonSet -- expected to keep running on every node;
    /// `kubectl drain`'s own default also never evicts these.
    DaemonSetOwned,
    /// Uses `emptyDir` or `hostPath` storage -- evicting would lose that
    /// data; excluded by default rather than silently discarding it.
    LocalStorage,
}

/// One Pod as planned for a drain run, before any request is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedPod {
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub exclusion: Option<Exclusion>,
}

fn is_daemonset_owned(pod: &Value) -> bool {
    pod.pointer("/metadata/ownerReferences")
        .and_then(Value::as_array)
        .is_some_and(|refs| {
            refs.iter()
                .any(|r| r.get("kind").and_then(Value::as_str) == Some("DaemonSet"))
        })
}

fn uses_local_storage(pod: &Value) -> bool {
    pod.pointer("/spec/volumes")
        .and_then(Value::as_array)
        .is_some_and(|volumes| {
            volumes
                .iter()
                .any(|v| v.get("emptyDir").is_some() || v.get("hostPath").is_some())
        })
}

/// Builds the plan for every Pod currently scheduled on the Node --
/// `pods` must already be scoped to that Node by the caller (this
/// function does no node-affinity filtering itself). DaemonSet-owned
/// Pods are checked before local-storage ones; a Pod could technically
/// be both, but the exclusion reason shown is whichever check fires
/// first, deterministically, never both at once.
pub fn plan(pods: &[Value]) -> Vec<PlannedPod> {
    pods.iter()
        .filter_map(|pod| {
            let namespace = pod.pointer("/metadata/namespace")?.as_str()?.to_owned();
            let name = pod.pointer("/metadata/name")?.as_str()?.to_owned();
            let uid = pod.pointer("/metadata/uid")?.as_str()?.to_owned();
            let exclusion = if is_daemonset_owned(pod) {
                Some(Exclusion::DaemonSetOwned)
            } else if uses_local_storage(pod) {
                Some(Exclusion::LocalStorage)
            } else {
                None
            };
            Some(PlannedPod {
                namespace,
                name,
                uid,
                exclusion,
            })
        })
        .collect()
}

/// What happened to one planned Pod during a drain run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// Never attempted -- excluded during planning, before any request.
    Excluded(Exclusion),
    /// The drain stopped (cancelled, or the cordon itself failed) before
    /// this Pod's eviction was ever attempted -- distinct from `Excluded`:
    /// this Pod WAS eligible, it just never got its turn.
    NotAttempted,
    /// An eviction was actually attempted; this is its real
    /// `MutationOutcome`, exactly as `kube::mutation::commit` returned it
    /// -- never re-interpreted, never retried.
    Attempted(super::MutationOutcome),
}

/// One row of the composite, per-Pod report -- never collapsed into a
/// single aggregate "Drain succeeded" boolean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainStep {
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub outcome: StepOutcome,
    pub verification: Option<super::Verification>,
}

/// The full result of a drain run: the cordon's own outcome, plus one
/// step per planned Pod. `cordon_outcome` failing means every step is
/// `NotAttempted` -- Drain never evicts on a Node it failed to cordon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainReport {
    pub cordon_outcome: super::MutationOutcome,
    pub steps: Vec<DrainStep>,
}

/// M8B.5 UI wiring: what a preview document knows about the Pods on the
/// target Node before Drain runs -- never shows a specific plan until a
/// fresh read has actually returned one. `Loading` and `Failed` are both
/// explicit states, never rendered as an empty (and therefore misleading)
/// eligible-Pods list.
#[derive(Clone, Debug)]
pub enum DrainPreview {
    Loading,
    Ready(Vec<PlannedPod>),
    Failed(String),
}

/// M8B.5 UI wiring: Drain's own composite double-press workflow shell,
/// mirroring `workflow::Workflow`'s exact arm/commit contract (`armed` is
/// UX state only, authorizes nothing by itself) but wrapping a cordon
/// intent plus a Node-wide eviction plan instead of one intent -- Drain's
/// one documented exception to "one user action, one intent". Nothing
/// here issues a request; the actual orchestrated commit happens only in
/// `kube::drain::drain`, invoked on the second confirm press.
pub struct DrainWorkflow {
    pub cordon_intent: MutationIntent,
    pub cordon_payload: Option<Value>,
    pub cordon_change: String,
    pub pod_resource: Resource,
    pub evaluation: PolicyEvaluation,
    pub preview: DrainPreview,
    pub armed: bool,
    pub report: Option<DrainReport>,
}

impl DrainWorkflow {
    pub fn new(
        cordon_intent: MutationIntent,
        cordon_payload: Option<Value>,
        cordon_change: String,
        pod_resource: Resource,
        evaluation: PolicyEvaluation,
    ) -> Self {
        Self {
            cordon_intent,
            cordon_payload,
            cordon_change,
            pod_resource,
            evaluation,
            preview: DrainPreview::Loading,
            armed: false,
            report: None,
        }
    }
    pub fn requirement(&self) -> super::ConfirmationRequirement {
        self.evaluation.decision.confirmation_requirement()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pod(namespace: &str, name: &str, uid: &str) -> Value {
        serde_json::json!({
            "metadata": {"namespace": namespace, "name": name, "uid": uid},
            "spec": {"volumes": []},
        })
    }

    #[test]
    fn plan_includes_an_ordinary_pod_with_no_exclusion() {
        let planned = plan(&[pod("default", "web-1", "uid-1")]);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].exclusion, None);
    }

    #[test]
    fn plan_excludes_a_daemonset_owned_pod() {
        let mut p = pod("kube-system", "node-exporter-abc", "uid-1");
        p["metadata"]["ownerReferences"] =
            serde_json::json!([{"kind": "DaemonSet", "name": "node-exporter", "uid": "ds-uid"}]);
        let planned = plan(&[p]);
        assert_eq!(planned[0].exclusion, Some(Exclusion::DaemonSetOwned));
    }

    #[test]
    fn plan_excludes_a_pod_using_empty_dir() {
        let mut p = pod("default", "cache-1", "uid-1");
        p["spec"]["volumes"] = serde_json::json!([{"name": "scratch", "emptyDir": {}}]);
        let planned = plan(&[p]);
        assert_eq!(planned[0].exclusion, Some(Exclusion::LocalStorage));
    }

    #[test]
    fn plan_excludes_a_pod_using_host_path() {
        let mut p = pod("default", "logs-1", "uid-1");
        p["spec"]["volumes"] =
            serde_json::json!([{"name": "hostlogs", "hostPath": {"path": "/var/log"}}]);
        let planned = plan(&[p]);
        assert_eq!(planned[0].exclusion, Some(Exclusion::LocalStorage));
    }

    #[test]
    fn plan_never_excludes_a_pod_using_only_a_configmap_or_secret_volume() {
        let mut p = pod("default", "app-1", "uid-1");
        p["spec"]["volumes"] =
            serde_json::json!([{"name": "cfg", "configMap": {"name": "app-config"}}]);
        let planned = plan(&[p]);
        assert_eq!(planned[0].exclusion, None);
    }

    #[test]
    fn plan_handles_a_mixed_batch_deterministically() {
        let mut daemonset_pod = pod("kube-system", "ds-pod", "uid-1");
        daemonset_pod["metadata"]["ownerReferences"] =
            serde_json::json!([{"kind": "DaemonSet", "name": "x", "uid": "y"}]);
        let mut local_storage_pod = pod("default", "cache-1", "uid-2");
        local_storage_pod["spec"]["volumes"] = serde_json::json!([{"name": "s", "emptyDir": {}}]);
        let ordinary_pod = pod("default", "web-1", "uid-3");
        let planned = plan(&[daemonset_pod, local_storage_pod, ordinary_pod]);
        assert_eq!(planned.len(), 3);
        assert_eq!(planned[0].exclusion, Some(Exclusion::DaemonSetOwned));
        assert_eq!(planned[1].exclusion, Some(Exclusion::LocalStorage));
        assert_eq!(planned[2].exclusion, None);
    }

    #[test]
    fn plan_is_idempotent_on_the_same_input() {
        let pods = [
            pod("default", "web-1", "uid-1"),
            pod("default", "web-2", "uid-2"),
        ];
        assert_eq!(plan(&pods), plan(&pods));
    }

    #[test]
    fn plan_skips_a_malformed_entry_missing_identity_fields_rather_than_panicking() {
        let malformed = serde_json::json!({"metadata": {"namespace": "default"}});
        let planned = plan(&[malformed, pod("default", "web-1", "uid-1")]);
        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].name, "web-1");
    }
}
