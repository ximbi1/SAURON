//! Storage references: PVC<->PV binding and StorageClass, never a capacity/usage claim.
use super::*;

pub(super) fn extract(object: &Object, out: &mut References) {
    match (object.api_version.as_str(), object.kind.as_str()) {
        ("v1", "PersistentVolumeClaim") => {
            // The API server sets volumeName only once bound; it carries no UID to verify.
            out.field(
                &object.value,
                "",
                "/spec/volumeName",
                "PersistentVolume",
                "",
            );
            storage_class(out, &object.value);
        }
        ("v1", "PersistentVolume") => {
            if let Some(claim) = object.value.pointer("/spec/claimRef")
                && !claim.is_null()
            {
                claim_ref(out, claim);
            }
            storage_class(out, &object.value);
        }
        _ => {}
    }
}

// claimRef is a schema-fixed field: it can only ever name a PersistentVolumeClaim,
// so hardcoding kind/apiVersion here is not a guess, unlike a generic status field.
fn claim_ref(out: &mut References, claim: &Value) {
    let (Some(name), Some(namespace)) = (claim["name"].as_str(), claim["namespace"].as_str())
    else {
        out.malformed = true;
        return;
    };
    out.add(
        Target {
            api_version: "v1".into(),
            kind: "PersistentVolumeClaim".into(),
            namespace: namespace.into(),
            name: name.into(),
            expected_uid: claim["uid"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            provenance: Provenance::StatusReference,
        },
        "/spec/claimRef".into(),
    );
}

fn storage_class(out: &mut References, value: &Value) {
    let Some(name) = value
        .pointer("/spec/storageClassName")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return;
    };
    out.add(
        Target {
            api_version: "storage.k8s.io/v1".into(),
            kind: "StorageClass".into(),
            namespace: "".into(),
            name: name.into(),
            expected_uid: None,
            provenance: Provenance::ExplicitReference,
        },
        "/spec/storageClassName".into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pvc_references_volume_and_storage_class_without_guessing_uid() {
        let o = Object::new(json!({
            "apiVersion":"v1","kind":"PersistentVolumeClaim",
            "metadata":{"namespace":"n","name":"data"},
            "spec":{"volumeName":"pv-1","storageClassName":"fast"}
        }));
        let refs = super::super::extract(&o);
        assert_eq!(refs.targets.len(), 2);
        let pv = refs
            .targets
            .keys()
            .find(|t| t.kind == "PersistentVolume")
            .unwrap();
        assert_eq!(pv.api_version, "v1");
        assert_eq!(pv.expected_uid, None);
        let sc = refs
            .targets
            .keys()
            .find(|t| t.kind == "StorageClass")
            .unwrap();
        assert_eq!(sc.api_version, "storage.k8s.io/v1");
        assert_eq!(sc.namespace, "");
        assert!(!refs.partial && !refs.malformed);
    }

    #[test]
    fn pv_claim_ref_carries_uid_and_storage_class_is_cluster_scoped() {
        let o = Object::new(json!({
            "apiVersion":"v1","kind":"PersistentVolume",
            "metadata":{"name":"pv-1"},
            "spec":{
                "storageClassName":"fast",
                "claimRef":{"apiVersion":"v1","kind":"PersistentVolumeClaim","name":"data","namespace":"n","uid":"claim-uid"}
            }
        }));
        let refs = super::super::extract(&o);
        assert_eq!(refs.targets.len(), 2);
        let claim = refs
            .targets
            .keys()
            .find(|t| t.kind == "PersistentVolumeClaim")
            .unwrap();
        assert_eq!(claim.namespace, "n");
        assert_eq!(claim.expected_uid.as_deref(), Some("claim-uid"));
        assert_eq!(claim.provenance, Provenance::StatusReference);
    }

    #[test]
    fn pv_without_claim_ref_or_storage_class_extracts_nothing() {
        let o = Object::new(
            json!({"apiVersion":"v1","kind":"PersistentVolume","metadata":{"name":"pv-1"},"spec":{"claimRef":null}}),
        );
        let refs = super::super::extract(&o);
        assert!(refs.targets.is_empty());
        assert!(!refs.malformed);
    }
}
