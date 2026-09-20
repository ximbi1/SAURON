//! Pure typed reference extraction. These are unresolved claims, not graph edges.
pub mod network;
pub mod storage;
use super::Provenance;
use crate::resources::Object;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub api_version: String,
    pub kind: String,
    /// Owner references inherit namespace only after discovery determines scope.
    pub namespace: String,
    pub name: String,
    pub expected_uid: Option<String>,
    pub provenance: Provenance,
}

#[derive(Default)]
pub struct References {
    pub targets: BTreeMap<Target, BTreeSet<String>>,
    pub partial: bool,
    pub malformed: bool,
    inspected: usize,
}
impl References {
    fn add(&mut self, target: Target, path: String) {
        if target.name.is_empty()
            || target.name.len() > 253
            || target.kind.is_empty()
            || (target.api_version.is_empty() && target.provenance != Provenance::StatusReference)
            || path.len() > 1024
        {
            self.malformed = true;
            return;
        }
        if !self.targets.contains_key(&target) && self.targets.len() >= 128 {
            self.partial = true;
            return;
        }
        let paths = self.targets.entry(target).or_default();
        if paths.len() >= 16 && !paths.contains(&path) {
            self.partial = true;
        } else {
            paths.insert(path);
        }
    }
    fn field(&mut self, root: &Value, base: &str, relative: &str, kind: &str, namespace: &str) {
        let Some(value) = root.pointer(relative) else {
            return;
        };
        let Some(name) = value.as_str().filter(|s| !s.is_empty()) else {
            if !value.is_null() {
                self.malformed = true;
            }
            return;
        };
        self.add(
            Target {
                api_version: "v1".into(),
                kind: kind.into(),
                namespace: namespace.into(),
                name: name.into(),
                expected_uid: None,
                provenance: Provenance::ExplicitReference,
            },
            format!("{base}{relative}"),
        );
    }
    fn budget(&mut self) -> bool {
        self.inspected += 1;
        if self.inspected > 4096 {
            self.partial = true;
            false
        } else {
            true
        }
    }
}

fn array<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = (usize, &'a Value)> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
}

pub fn extract(object: &Object) -> References {
    let mut out = References::default();
    network::extract(object, &mut out);
    storage::extract(object, &mut out);
    for (i, owner) in object
        .value
        .pointer("/metadata/ownerReferences")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        if !out.budget() {
            break;
        }
        let (Some(version), Some(kind), Some(name), Some(uid)) = (
            owner["apiVersion"].as_str(),
            owner["kind"].as_str(),
            owner["name"].as_str(),
            owner["uid"].as_str().filter(|s| !s.is_empty()),
        ) else {
            out.malformed = true;
            continue;
        };
        out.add(
            Target {
                api_version: version.into(),
                kind: kind.into(),
                namespace: object.namespace.clone(),
                name: name.into(),
                expected_uid: Some(uid.into()),
                provenance: Provenance::OwnerReference,
            },
            format!("/metadata/ownerReferences/{i}"),
        );
    }
    // Matched by API group only, never the exact version -- Flux's own
    // sourceRef/dependsOn schema is stable across its v1beta2->v1
    // migrations, and group+kind is exactly what M9.0's own capability
    // discovery already keys on for the same reason.
    match (group(&object.api_version), object.kind.as_str()) {
        ("kustomize.toolkit.fluxcd.io", "Kustomization") => {
            flux_source_ref(
                &mut out,
                &object.value,
                &object.namespace,
                "/spec/sourceRef",
            );
            flux_depends_on(&mut out, object);
            return out;
        }
        ("helm.toolkit.fluxcd.io", "HelmRelease") => {
            flux_source_ref(
                &mut out,
                &object.value,
                &object.namespace,
                "/spec/chart/spec/sourceRef",
            );
            flux_depends_on(&mut out, object);
            return out;
        }
        ("argoproj.io", "Application") => {
            argocd_managed_resources(&mut out, object);
            argocd_project_ref(&mut out, object);
            return out;
        }
        _ => {}
    }
    let base = match (object.api_version.as_str(), object.kind.as_str()) {
        ("v1", "Pod") => "/spec",
        ("apps/v1", "Deployment" | "StatefulSet" | "DaemonSet" | "ReplicaSet")
        | ("batch/v1", "Job") => "/spec/template/spec",
        ("batch/v1", "CronJob") => "/spec/jobTemplate/spec/template/spec",
        _ => return out,
    };
    if let Some(spec) = object.value.pointer(base) {
        pod_spec(&mut out, spec, base, &object.namespace);
    }
    out
}

fn group(api_version: &str) -> &str {
    api_version.split_once('/').map(|(g, _)| g).unwrap_or("")
}

// Flux does not persist a resolved apiVersion inside sourceRef (only
// kind/name/namespace), and its source kinds' own API version has
// migrated over Flux's history (v1beta2 -> v1) -- StatusReference is the
// existing, already-designed escape hatch for "the kind is known, the
// exact served version is not", exactly like PV's own claimRef
// (storage.rs) already uses it for the same reason, not a new provenance
// meaning invented for Flux.
fn flux_source_ref(out: &mut References, root: &Value, ns: &str, pointer: &str) {
    let Some(source) = root.pointer(pointer) else {
        return;
    };
    let (Some(kind), Some(name)) = (
        source.get("kind").and_then(Value::as_str),
        source
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty()),
    ) else {
        out.malformed = true;
        return;
    };
    let namespace = source
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or(ns);
    out.add(
        Target {
            api_version: String::new(),
            kind: kind.into(),
            namespace: namespace.into(),
            name: name.into(),
            expected_uid: None,
            provenance: Provenance::StatusReference,
        },
        pointer.into(),
    );
}

// dependsOn always names another object of the SAME kind/apiVersion as
// the referencing one -- unlike sourceRef, there is no version ambiguity
// here at all, so this is a plain ExplicitReference.
fn flux_depends_on(out: &mut References, object: &Object) {
    let Some(deps) = object
        .value
        .pointer("/spec/dependsOn")
        .and_then(Value::as_array)
    else {
        return;
    };
    for (i, dep) in deps.iter().enumerate() {
        if !out.budget() {
            return;
        }
        let Some(name) = dep
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            out.malformed = true;
            continue;
        };
        let namespace = dep
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or(&object.namespace);
        out.add(
            Target {
                api_version: object.api_version.clone(),
                kind: object.kind.clone(),
                namespace: namespace.into(),
                name: name.into(),
                expected_uid: None,
                provenance: Provenance::ExplicitReference,
            },
            format!("/spec/dependsOn/{i}"),
        );
    }
}

// `status.resources[]` entries are Argo CD's own reconciled inventory --
// each already carries an explicit `group`/`version` (unlike Flux's
// sourceRef), so the target's `api_version` is real reported data, never
// guessed. No `expected_uid`: Argo CD's own inventory does not carry
// one, matching `Provenance::StatusReference`'s existing "kind/version
// known, identity not UID-verified" contract.
fn argocd_managed_resources(out: &mut References, object: &Object) {
    let Some(resources) = object
        .value
        .pointer("/status/resources")
        .and_then(Value::as_array)
    else {
        return;
    };
    for (i, r) in resources.iter().enumerate() {
        if !out.budget() {
            return;
        }
        let (Some(kind), Some(version), Some(name)) = (
            r.get("kind").and_then(Value::as_str),
            r.get("version").and_then(Value::as_str),
            r.get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty()),
        ) else {
            out.malformed = true;
            continue;
        };
        let group = r.get("group").and_then(Value::as_str).unwrap_or("");
        let api_version = if group.is_empty() {
            version.to_string()
        } else {
            format!("{group}/{version}")
        };
        let namespace = r
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or(&object.namespace);
        out.add(
            Target {
                api_version,
                kind: kind.into(),
                namespace: namespace.into(),
                name: name.into(),
                expected_uid: None,
                provenance: Provenance::StatusReference,
            },
            format!("/status/resources/{i}"),
        );
    }
}

// `spec.project` names an `AppProject` -- schema-fixed to that one kind,
// same "kind known, version not persisted" shape as Flux's sourceRef.
fn argocd_project_ref(out: &mut References, object: &Object) {
    let Some(project) = object
        .value
        .pointer("/spec/project")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    out.add(
        Target {
            api_version: String::new(),
            kind: "AppProject".into(),
            namespace: object.namespace.clone(),
            name: project.into(),
            expected_uid: None,
            provenance: Provenance::StatusReference,
        },
        "/spec/project".into(),
    );
}

fn pod_spec(out: &mut References, spec: &Value, base: &str, ns: &str) {
    out.field(spec, base, "/nodeName", "Node", "");
    out.field(spec, base, "/serviceAccountName", "ServiceAccount", ns);
    for (i, secret) in array(spec, "imagePullSecrets") {
        if !out.budget() {
            return;
        }
        out.field(
            secret,
            &format!("{base}/imagePullSecrets/{i}"),
            "/name",
            "Secret",
            ns,
        );
    }
    for (i, volume) in array(spec, "volumes") {
        if !out.budget() {
            return;
        }
        let path = format!("{base}/volumes/{i}");
        for (field, kind) in [
            ("/persistentVolumeClaim/claimName", "PersistentVolumeClaim"),
            ("/configMap/name", "ConfigMap"),
            ("/secret/secretName", "Secret"),
        ] {
            out.field(volume, &path, field, kind, ns);
        }
        if let Some(projected) = volume.get("projected") {
            for (j, source) in array(projected, "sources") {
                if !out.budget() {
                    return;
                }
                let path = format!("{path}/projected/sources/{j}");
                out.field(source, &path, "/configMap/name", "ConfigMap", ns);
                out.field(source, &path, "/secret/name", "Secret", ns);
            }
        }
    }
    for category in ["containers", "initContainers", "ephemeralContainers"] {
        for (i, container) in array(spec, category) {
            if !out.budget() {
                return;
            }
            let path = format!("{base}/{category}/{i}");
            for (j, env) in array(container, "env") {
                if !out.budget() {
                    return;
                }
                let path = format!("{path}/env/{j}");
                out.field(
                    env,
                    &path,
                    "/valueFrom/configMapKeyRef/name",
                    "ConfigMap",
                    ns,
                );
                out.field(env, &path, "/valueFrom/secretKeyRef/name", "Secret", ns);
            }
            for (j, env) in array(container, "envFrom") {
                if !out.budget() {
                    return;
                }
                let path = format!("{path}/envFrom/{j}");
                out.field(env, &path, "/configMapRef/name", "ConfigMap", ns);
                out.field(env, &path, "/secretRef/name", "Secret", ns);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn pod(spec: Value) -> Object {
        Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"name":"p","namespace":"n","uid":"u"},"spec":spec}),
        )
    }
    #[test]
    fn pod_references_are_exact_and_deduplicate_paths() {
        let o = pod(
            json!({"nodeName":"node","serviceAccountName":"sa","imagePullSecrets":[{"name":"s"}],"volumes":[{"persistentVolumeClaim":{"claimName":"data"}},{"configMap":{"name":"cm"}},{"secret":{"secretName":"s"}},{"projected":{"sources":[{"configMap":{"name":"cm"}},{"secret":{"name":"s"}}]}}],"containers":[{"env":[{"valueFrom":{"configMapKeyRef":{"name":"cm","key":"k"}}}],"envFrom":[{"secretRef":{"name":"s"}}]}],"initContainers":[{"envFrom":[{"configMapRef":{"name":"cm"}}]}],"ephemeralContainers":[{"env":[{"valueFrom":{"secretKeyRef":{"name":"s","key":"k"}}}]}]}),
        );
        let refs = extract(&o);
        assert_eq!(refs.targets.len(), 5);
        for (target, paths) in &refs.targets {
            assert_eq!(
                target.namespace,
                if target.kind == "Node" { "" } else { "n" }
            );
            for path in paths {
                assert_eq!(
                    o.value.pointer(path).and_then(Value::as_str),
                    Some(target.name.as_str())
                );
            }
        }
        assert_eq!(
            refs.targets
                .iter()
                .find(|(t, _)| t.kind == "Secret")
                .unwrap()
                .1
                .len(),
            5
        );
        assert!(!refs.partial && !refs.malformed);
    }
    #[test]
    fn templates_keep_paths_and_do_not_guess_crd_schema() {
        for (version, kind, prefix) in [
            ("apps/v1", "Deployment", "/spec/template/spec"),
            ("apps/v1", "StatefulSet", "/spec/template/spec"),
            ("apps/v1", "DaemonSet", "/spec/template/spec"),
            ("apps/v1", "ReplicaSet", "/spec/template/spec"),
            ("batch/v1", "Job", "/spec/template/spec"),
            (
                "batch/v1",
                "CronJob",
                "/spec/jobTemplate/spec/template/spec",
            ),
        ] {
            let spec = json!({"serviceAccountName":"sa"});
            let value = if kind == "CronJob" {
                json!({"jobTemplate":{"spec":{"template":{"spec":spec}}}})
            } else {
                json!({"template":{"spec":spec}})
            };
            let o = Object::new(
                json!({"apiVersion":version,"kind":kind,"metadata":{"namespace":"n"},"spec":value}),
            );
            let refs = extract(&o);
            assert_eq!(
                refs.targets.values().next().unwrap(),
                &BTreeSet::from([format!("{prefix}/serviceAccountName")])
            );
            let mut crd = o;
            crd.api_version = "example.io/v1".into();
            assert!(extract(&crd).targets.is_empty());
        }
    }
    #[test]
    fn generic_owners_require_uid_and_keep_duplicate_evidence() {
        let o = Object::new(
            json!({"apiVersion":"example.io/v1","kind":"Thing","metadata":{"ownerReferences":[{"apiVersion":"example.io/v1","kind":"Parent","name":"p","uid":"old"},{"apiVersion":"example.io/v1","kind":"Parent","name":"p","uid":"old"},{"apiVersion":"v1","kind":"Pod","name":"bad"}]}}),
        );
        let refs = extract(&o);
        assert!(refs.malformed);
        assert_eq!(refs.targets.len(), 1);
        let (target, paths) = refs.targets.iter().next().unwrap();
        assert_eq!(target.expected_uid.as_deref(), Some("old"));
        assert_eq!(paths.len(), 2);
    }
    #[test]
    fn extraction_bounds_are_visible() {
        let secrets: Vec<_> = (0..200).map(|n| json!({"name":format!("s{n}")})).collect();
        let refs = extract(&pod(json!({"imagePullSecrets":secrets})));
        assert_eq!(refs.targets.len(), 128);
        assert!(refs.partial);
    }

    #[test]
    fn kustomization_source_ref_is_a_status_reference_with_no_guessed_api_version() {
        let o = Object::new(json!({
            "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
            "kind": "Kustomization",
            "metadata": {"namespace": "sauron-m9", "name": "podinfo-kustomize"},
            "spec": {"sourceRef": {"kind": "GitRepository", "name": "podinfo"}},
        }));
        let refs = extract(&o);
        assert_eq!(refs.targets.len(), 1);
        let (target, paths) = refs.targets.iter().next().unwrap();
        assert_eq!(target.kind, "GitRepository");
        assert_eq!(target.name, "podinfo");
        assert_eq!(
            target.namespace, "sauron-m9",
            "sourceRef.namespace defaults to the referencing object's own namespace"
        );
        assert_eq!(
            target.api_version, "",
            "Flux does not persist a resolved apiVersion in sourceRef -- never guessed"
        );
        assert_eq!(target.provenance, Provenance::StatusReference);
        assert_eq!(paths.iter().next().unwrap(), "/spec/sourceRef");
        assert!(!refs.malformed);
    }

    #[test]
    fn kustomization_depends_on_targets_the_same_kind_and_version_unambiguously() {
        let o = Object::new(json!({
            "apiVersion": "kustomize.toolkit.fluxcd.io/v1",
            "kind": "Kustomization",
            "metadata": {"namespace": "sauron-m9", "name": "podinfo-dependent"},
            "spec": {
                "sourceRef": {"kind": "GitRepository", "name": "podinfo-missing"},
                "dependsOn": [{"name": "podinfo-kustomize"}, {"name": "other", "namespace": "ns2"}],
            },
        }));
        let refs = extract(&o);
        assert_eq!(refs.targets.len(), 3);
        let dep = refs
            .targets
            .keys()
            .find(|t| t.name == "podinfo-kustomize")
            .expect("dependsOn target");
        assert_eq!(dep.kind, "Kustomization");
        assert_eq!(dep.api_version, "kustomize.toolkit.fluxcd.io/v1");
        assert_eq!(dep.namespace, "sauron-m9");
        assert_eq!(dep.provenance, Provenance::ExplicitReference);
        let other = refs
            .targets
            .keys()
            .find(|t| t.name == "other")
            .expect("explicit-namespace dependsOn target");
        assert_eq!(other.namespace, "ns2");
    }

    #[test]
    fn helm_release_chart_source_ref_is_extracted_from_its_own_nested_path() {
        let o = Object::new(json!({
            "apiVersion": "helm.toolkit.fluxcd.io/v2",
            "kind": "HelmRelease",
            "metadata": {"namespace": "sauron-m9", "name": "podinfo-helm"},
            "spec": {"chart": {"spec": {"sourceRef": {"kind": "HelmRepository", "name": "podinfo", "namespace": "sauron-m9"}}}},
        }));
        let refs = extract(&o);
        assert_eq!(refs.targets.len(), 1);
        let target = refs.targets.keys().next().unwrap();
        assert_eq!(target.kind, "HelmRepository");
        assert_eq!(target.name, "podinfo");
        assert_eq!(target.provenance, Provenance::StatusReference);
        assert!(!refs.malformed);
    }

    #[test]
    fn a_flux_crd_with_an_unrelated_group_never_matches_the_flux_dispatch() {
        // A foreign CRD that happens to reuse the Kind "Kustomization" in a
        // different group must never be mistaken for Flux's own -- dispatch
        // is by (group, kind), never kind alone, matching M9.0's own
        // discovery precedent.
        let o = Object::new(json!({
            "apiVersion": "other.example.com/v1",
            "kind": "Kustomization",
            "metadata": {"namespace": "n"},
            "spec": {"sourceRef": {"kind": "GitRepository", "name": "x"}},
        }));
        assert!(extract(&o).targets.is_empty());
    }

    #[test]
    fn argocd_application_managed_resources_carry_real_group_version_never_guessed() {
        let o = Object::new(json!({
            "apiVersion": "argoproj.io/v1alpha1",
            "kind": "Application",
            "metadata": {"namespace": "argocd", "name": "guestbook"},
            "spec": {"project": "default"},
            "status": {
                "resources": [
                    {"kind": "Service", "name": "guestbook-ui", "namespace": "sauron-m9", "version": "v1", "status": "Synced"},
                    {"group": "apps", "kind": "Deployment", "name": "guestbook-ui", "namespace": "sauron-m9", "version": "v1", "status": "Synced"},
                ],
            },
        }));
        let refs = extract(&o);
        assert!(!refs.malformed);
        assert_eq!(
            refs.targets.len(),
            3,
            "2 managed resources + 1 AppProject reference"
        );
        let service = refs
            .targets
            .keys()
            .find(|t| t.kind == "Service")
            .expect("Service target");
        assert_eq!(service.api_version, "v1");
        assert_eq!(service.namespace, "sauron-m9");
        assert_eq!(service.provenance, Provenance::StatusReference);
        let deployment = refs
            .targets
            .keys()
            .find(|t| t.kind == "Deployment")
            .expect("Deployment target");
        assert_eq!(deployment.api_version, "apps/v1");
    }

    #[test]
    fn argocd_application_project_reference_defaults_to_the_applications_own_namespace() {
        let o = Object::new(json!({
            "apiVersion": "argoproj.io/v1alpha1",
            "kind": "Application",
            "metadata": {"namespace": "argocd", "name": "guestbook"},
            "spec": {"project": "default"},
            "status": {},
        }));
        let refs = extract(&o);
        assert_eq!(refs.targets.len(), 1);
        let (target, _) = refs.targets.iter().next().unwrap();
        assert_eq!(target.kind, "AppProject");
        assert_eq!(target.name, "default");
        assert_eq!(target.namespace, "argocd");
        assert_eq!(target.api_version, "");
        assert_eq!(target.provenance, Provenance::StatusReference);
    }

    #[test]
    fn an_unrelated_crd_sharing_the_application_kind_name_never_matches_argocd_dispatch() {
        let o = Object::new(json!({
            "apiVersion": "other.example.com/v1",
            "kind": "Application",
            "metadata": {"namespace": "n"},
            "spec": {"project": "default"},
        }));
        assert!(extract(&o).targets.is_empty());
    }
}
