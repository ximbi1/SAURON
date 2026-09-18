//! Kubernetes network references, never traffic/causality or IP→Pod guesses.
use super::*;
use crate::evidence::Unknown;

pub fn selects(service: &Object, pod: &Object) -> Result<bool, Unknown> {
    if service.api_version != "v1"
        || service.kind != "Service"
        || pod.api_version != "v1"
        || pod.kind != "Pod"
    {
        return Err(Unknown::Unsupported);
    }
    if service.namespace.is_empty() || pod.namespace.is_empty() {
        return Err(Unknown::NotReported);
    }
    if service.namespace != pod.namespace {
        return Ok(false);
    }
    let selector = match service.value.pointer("/spec/selector") {
        None | Some(Value::Null) => return Ok(false),
        Some(value) => value.as_object().ok_or(Unknown::Malformed)?,
    };
    if selector.is_empty() {
        return Ok(false);
    }
    let labels = match pod.value.pointer("/metadata/labels") {
        None | Some(Value::Null) => None,
        Some(value) => Some(value.as_object().ok_or(Unknown::Malformed)?),
    };
    for (key, value) in selector {
        let value = value.as_str().ok_or(Unknown::Malformed)?;
        if labels.and_then(|l| l.get(key)).and_then(Value::as_str) != Some(value) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn endpoint(out: &mut References, value: &Value, base: &str, ns: &str) {
    let Some(reference) = value.get("targetRef") else {
        return;
    };
    if reference.is_null() {
        return;
    }
    let (Some(kind), Some(name)) = (reference["kind"].as_str(), reference["name"].as_str()) else {
        out.malformed = true;
        return;
    };
    out.add(
        Target {
            // Kubernetes' EndpointSlice controller normally omits apiVersion.
            // Resolve a missing version only through an unambiguous catalog;
            // never assume core/v1 from a familiar-looking kind name.
            api_version: reference["apiVersion"].as_str().unwrap_or_default().into(),
            kind: kind.into(),
            namespace: reference["namespace"].as_str().unwrap_or(ns).into(),
            name: name.into(),
            expected_uid: reference["uid"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            provenance: Provenance::StatusReference,
        },
        format!("{base}/targetRef"),
    );
}

pub(super) fn extract(object: &Object, out: &mut References) {
    let ns = &object.namespace;
    match (object.api_version.as_str(), object.kind.as_str()) {
        ("networking.k8s.io/v1", "Ingress") => {
            out.field(
                &object.value,
                "",
                "/spec/defaultBackend/service/name",
                "Service",
                ns,
            );
            if let Some(spec) = object.value.get("spec") {
                for (i, tls) in array(spec, "tls") {
                    if !out.budget() {
                        return;
                    }
                    out.field(tls, &format!("/spec/tls/{i}"), "/secretName", "Secret", ns);
                }
                for (i, rule) in array(spec, "rules") {
                    if !out.budget() {
                        return;
                    }
                    if let Some(http) = rule.get("http") {
                        for (j, path) in array(http, "paths") {
                            if !out.budget() {
                                return;
                            }
                            out.field(
                                path,
                                &format!("/spec/rules/{i}/http/paths/{j}"),
                                "/backend/service/name",
                                "Service",
                                ns,
                            );
                        }
                    }
                }
            }
        }
        ("discovery.k8s.io/v1", "EndpointSlice") => {
            out.field(
                &object.value,
                "",
                "/metadata/labels/kubernetes.io~1service-name",
                "Service",
                ns,
            );
            for (i, endpoint_value) in array(&object.value, "endpoints") {
                if !out.budget() {
                    return;
                }
                endpoint(out, endpoint_value, &format!("/endpoints/{i}"), ns);
            }
        }
        ("v1", "Endpoints") => {
            for (i, subset) in array(&object.value, "subsets") {
                if !out.budget() {
                    return;
                }
                for field in ["addresses", "notReadyAddresses"] {
                    for (j, address) in array(subset, field) {
                        if !out.budget() {
                            return;
                        }
                        endpoint(out, address, &format!("/subsets/{i}/{field}/{j}"), ns);
                    }
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn equality_selectors_are_not_ownership_and_obey_namespace() {
        let mut service = Object::new(
            json!({"apiVersion":"v1","kind":"Service","metadata":{"namespace":"n"},"spec":{"selector":{"app":"web","tier":"front"}}}),
        );
        let mut pod = Object::new(
            json!({"apiVersion":"v1","kind":"Pod","metadata":{"namespace":"n","labels":{"app":"web","tier":"front","extra":"ok"}}}),
        );
        assert_eq!(selects(&service, &pod), Ok(true));
        pod.namespace = "other".into();
        assert_eq!(selects(&service, &pod), Ok(false));
        pod.namespace = "n".into();
        pod.value["metadata"]["labels"]["tier"] = "back".into();
        assert_eq!(selects(&service, &pod), Ok(false));
        service.value["spec"]["selector"] = json!({});
        assert_eq!(selects(&service, &pod), Ok(false));
        service.value["spec"]["selector"] = Value::Null;
        assert_eq!(selects(&service, &pod), Ok(false));
    }
    #[test]
    fn endpoint_ips_never_invent_objects_and_uid_is_preserved() {
        let object = Object::new(
            json!({"apiVersion":"discovery.k8s.io/v1","kind":"EndpointSlice","metadata":{"namespace":"n","labels":{"kubernetes.io/service-name":"svc"}},"endpoints":[{"addresses":["10.0.0.1"]},{"addresses":["10.0.0.2"],"targetRef":{"apiVersion":"v1","kind":"Pod","name":"p","uid":"u","namespace":"n"}}]}),
        );
        let refs = super::super::extract(&object);
        assert_eq!(refs.targets.len(), 2);
        let (pod, paths) = refs.targets.iter().find(|(t, _)| t.kind == "Pod").unwrap();
        assert_eq!(pod.expected_uid.as_deref(), Some("u"));
        assert_eq!(pod.provenance, Provenance::StatusReference);
        assert!(paths.contains("/endpoints/1/targetRef"));
        let mut omitted = object.value.clone();
        omitted["endpoints"][1]["targetRef"]
            .as_object_mut()
            .unwrap()
            .remove("apiVersion");
        let refs = super::super::extract(&Object::new(omitted));
        let pod = refs
            .targets
            .keys()
            .find(|t| t.kind == "Pod")
            .expect("real controller omits version");
        assert!(pod.api_version.is_empty());
        assert_eq!(pod.expected_uid.as_deref(), Some("u"));
    }
    #[test]
    fn ingress_default_path_backends_and_tls_keep_evidence() {
        let object = Object::new(
            json!({"apiVersion":"networking.k8s.io/v1","kind":"Ingress","metadata":{"namespace":"n"},"spec":{"defaultBackend":{"service":{"name":"svc"}},"rules":[{"http":{"paths":[{"backend":{"service":{"name":"svc"}}}]}}],"tls":[{"secretName":"tls"}]}}),
        );
        let refs = super::super::extract(&object);
        assert_eq!(refs.targets.len(), 2);
        for (target, paths) in refs.targets {
            for path in paths {
                assert_eq!(
                    object.value.pointer(&path).and_then(Value::as_str),
                    Some(target.name.as_str())
                );
            }
        }
    }
}
