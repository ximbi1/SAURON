//! Pure typed reference extraction. These are unresolved claims, not graph edges.
pub mod network;
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
}
