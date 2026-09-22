//! Context diff = a read-only, bounded, on-demand comparison between the
//! selected object's current context and one explicitly named other
//! context, for the same logical target. This is deliberately the smallest
//! slice in M11 (P3/RESEARCH-only in `docs/SOFKA_PARITY.md`, "scoring
//! optional" even at RESEARCH time) -- see `docs/M11_ACCEPTANCE.md`'s
//! M11.6 section for the investigation that scoped it down to exactly
//! this.
//!
//! Identity discipline: within one connection, `Object.uid` remains the
//! sole identity authority elsewhere in this codebase, unchanged. ACROSS
//! two independent connections there is no such thing as "the same UID" --
//! two different clusters can only ever be compared by an explicit
//! **comparison key** (kind/namespace/name), which this module labels as
//! exactly that, never as identity. No mutation. No new watch: one bounded,
//! timed-out GET on a temporary, independent second `Connection` that is
//! never stored anywhere and is dropped the instant this comparison
//! finishes -- never a background cross-cluster watcher.
use crate::resources::Object;

pub struct ComparisonKey {
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

/// The deliberately small, explicit projection this comparison actually
/// looks at -- never a raw full-object diff. Absent fields compare as
/// absent, never as a default/zero value.
#[derive(Debug, PartialEq, Eq)]
struct Projection {
    health_status: String,
    images: Vec<String>,
    replicas_desired: Option<i64>,
    replicas_available: Option<i64>,
}
fn project(object: &Object) -> Projection {
    let images: Vec<String> = object
        .value
        .pointer("/spec/template/spec/containers")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("image").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect();
    Projection {
        health_status: object.health.status.clone(),
        images,
        replicas_desired: object
            .value
            .pointer("/spec/replicas")
            .and_then(serde_json::Value::as_i64),
        replicas_available: object
            .value
            .pointer("/status/availableReplicas")
            .and_then(serde_json::Value::as_i64),
    }
}

pub enum RightSide {
    Found(Box<Object>),
    NotFound,
    Unsupported,
    Unknown(String),
}

pub fn report(
    left_context: &str,
    right_context: &str,
    key: &ComparisonKey,
    left: &Object,
    right: RightSide,
) -> String {
    let mut out = format!(
        "CONTEXT DIFF: {} {}/{}\nComparison key: kind/namespace/name (NOT identity -- UID \
         is never compared across contexts)\nLeft: {left_context}\nRight: {right_context}\n\n",
        key.kind, key.namespace, key.name
    );
    match right {
        RightSide::NotFound => {
            out.push_str("ONLY-LEFT: present in the left context, not found in the right.\n");
        }
        RightSide::Unsupported => {
            out.push_str(
                "UNKNOWN: the right context does not support this resource kind \
                 (CRD/API not registered there) -- comparison could not be attempted.\n",
            );
        }
        RightSide::Unknown(reason) => {
            out.push_str(&format!(
                "UNKNOWN: comparison could not be completed ({reason}). This is not \
                 evidence of absence or equivalence.\n"
            ));
        }
        RightSide::Found(right_object) => {
            let left_projection = project(left);
            let right_projection = project(&right_object);
            if left_projection == right_projection {
                out.push_str("EQUIVALENT under the declared projection (health status, container images, replica counts).\n");
            } else {
                out.push_str("DIFFERENT:\n");
                if left_projection.health_status != right_projection.health_status {
                    out.push_str(&format!(
                        "  health: {} vs {}\n",
                        left_projection.health_status, right_projection.health_status
                    ));
                }
                if left_projection.images != right_projection.images {
                    out.push_str(&format!(
                        "  images: {:?} vs {:?}\n",
                        left_projection.images, right_projection.images
                    ));
                }
                if left_projection.replicas_desired != right_projection.replicas_desired {
                    out.push_str(&format!(
                        "  desired replicas: {:?} vs {:?}\n",
                        left_projection.replicas_desired, right_projection.replicas_desired
                    ));
                }
                if left_projection.replicas_available != right_projection.replicas_available {
                    out.push_str(&format!(
                        "  available replicas: {:?} vs {:?}\n",
                        left_projection.replicas_available, right_projection.replicas_available
                    ));
                }
            }
        }
    }
    out.push_str(
        "\nProjection only: image/replica/health fields, not a full object diff. \
         Never a mutation; both sides are read-only observations.\n",
    );
    crate::safety::text(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn object(replicas: i64, image: &str, available: i64) -> Object {
        Object::new(json!({
            "apiVersion": "apps/v1", "kind": "Deployment",
            "metadata": {"namespace": "default", "name": "web", "uid": "u"},
            "spec": {"replicas": replicas, "template": {"spec": {"containers": [{"image": image}]}}},
            "status": {"availableReplicas": available},
        }))
    }
    fn key() -> ComparisonKey {
        ComparisonKey {
            kind: "Deployment".into(),
            namespace: "default".into(),
            name: "web".into(),
        }
    }

    #[test]
    fn identical_projection_is_reported_equivalent() {
        let left = object(3, "app:v1", 3);
        let right = object(3, "app:v1", 3);
        let text = report(
            "prod",
            "staging",
            &key(),
            &left,
            RightSide::Found(Box::new(right)),
        );
        assert!(text.contains("EQUIVALENT"));
        assert!(!text.contains("DIFFERENT"));
    }

    #[test]
    fn differing_image_is_reported_never_silently_equivalent() {
        let left = object(3, "app:v1", 3);
        let right = object(3, "app:v2", 3);
        let text = report(
            "prod",
            "staging",
            &key(),
            &left,
            RightSide::Found(Box::new(right)),
        );
        assert!(text.contains("DIFFERENT"));
        assert!(text.contains("app:v1"));
        assert!(text.contains("app:v2"));
    }

    #[test]
    fn only_left_is_explicit_never_silently_equivalent_or_absent() {
        let left = object(3, "app:v1", 3);
        let text = report("prod", "staging", &key(), &left, RightSide::NotFound);
        assert!(text.contains("ONLY-LEFT"));
        assert!(!text.contains("EQUIVALENT"));
    }

    #[test]
    fn unsupported_kind_is_explicit_never_treated_as_absent() {
        let left = object(3, "app:v1", 3);
        let text = report("prod", "staging", &key(), &left, RightSide::Unsupported);
        assert!(text.contains("UNKNOWN"));
        assert!(text.contains("does not support"));
        assert!(
            !text.contains("ONLY-LEFT"),
            "unsupported must never be confused with absent"
        );
    }

    #[test]
    fn comparison_key_is_labeled_explicitly_not_identity() {
        let left = object(3, "app:v1", 3);
        let text = report("prod", "staging", &key(), &left, RightSide::NotFound);
        assert!(text.contains("Comparison key"));
        assert!(text.contains("NOT identity"));
    }

    #[test]
    fn partial_failure_is_unknown_never_a_false_equivalent() {
        let left = object(3, "app:v1", 3);
        let text = report(
            "prod",
            "staging",
            &key(),
            &left,
            RightSide::Unknown("Forbidden: the current identity is not permitted".into()),
        );
        assert!(text.contains("UNKNOWN"));
        assert!(!text.contains("EQUIVALENT"));
    }
}
