//! M10.3: bulk guarded mutations. BULK != BYPASS -- this composes the
//! exact same per-target gateway every other mutation already goes
//! through (`policy::evaluate` -> `Workflow` -> `kube::mutation::
//! commit`/`verify` -> journal), once per selected target, never a
//! parallel executor. There is no second policy engine, no second
//! confirmation model, no second journal here: a `BulkWorkflow` is a
//! bounded collection of ordinary `Workflow`s, each with its own
//! immutable `PolicyEvaluation`/`Confirmation`/outcome/verification.
//!
//! A single UI "confirm" gesture authorizes proceeding with the already-
//! previewed set, but it is sugar for "confirm N individually evaluated
//! operations" -- each target's own `Confirmation` is still built fresh
//! from its own intent and independently checked by `commit()` via
//! `Confirmation::authorizes`, exactly as for a single-target mutation.
//! No target is ever authorized merely because another target in the
//! same bulk operation was authorized.
use super::workflow::{Built, Workflow};
use super::{ConfirmationRequirement, MutationOutcome, PolicyDecision, Verification};

/// One selected target's own workflow attempt. `Err` means the pure
/// builder itself rejected this target's kind (e.g. `:bulk_scale`
/// against a Pod) -- recorded individually as `Unsupported`, never
/// silently dropped from the set the user sees.
pub struct BulkItem {
    pub uid: String,
    pub namespace: String,
    pub name: String,
    pub workflow: Result<Workflow, String>,
}
impl BulkItem {
    pub fn from_built(
        uid: String,
        namespace: String,
        name: String,
        built: Result<Built, String>,
        evaluate: impl FnOnce(&Built) -> super::PolicyEvaluation,
    ) -> Self {
        let workflow = built.map(|b| {
            let evaluation = evaluate(&b);
            Workflow::new(b, evaluation)
        });
        Self {
            uid,
            namespace,
            name,
            workflow,
        }
    }
}

/// The bulk-scoped UI state every M10.3 action carries through preview
/// -> confirm -> commit -> verify. `armed` mirrors `Workflow::armed`
/// exactly (a double-press gate, UX state only, never authorization by
/// itself) but is bulk-scoped: if ANY eligible item requires Strong
/// confirmation, the WHOLE bulk gesture requires the double-press --
/// this never lowers any individual target's own requirement, it only
/// adds a bulk-level acknowledgement on top.
pub struct BulkWorkflow {
    pub source_action: String,
    pub items: Vec<BulkItem>,
    pub armed: bool,
    pub results: Option<Vec<BulkOutcome>>,
}
impl BulkWorkflow {
    pub fn new(source_action: String, items: Vec<BulkItem>) -> Self {
        Self {
            source_action,
            items,
            armed: false,
            results: None,
        }
    }
    /// The strongest confirmation requirement among every eligible item
    /// -- never computed by lowering any single item's own requirement.
    pub fn requirement(&self) -> ConfirmationRequirement {
        self.eligible()
            .map(Workflow::requirement)
            .max_by_key(|r| match r {
                ConfirmationRequirement::None => 0,
                ConfirmationRequirement::Standard => 1,
                ConfirmationRequirement::Strong => 2,
            })
            .unwrap_or(ConfirmationRequirement::None)
    }
    /// Targets that will actually be attempted at commit time: the
    /// builder succeeded AND policy does not already refuse it outright
    /// (`Deny`/`Unsupported` targets are shown in preview but never
    /// executed -- see `denied_or_unsupported`).
    pub fn eligible(&self) -> impl Iterator<Item = &Workflow> {
        self.items
            .iter()
            .filter_map(|i| i.workflow.as_ref().ok())
            .filter(|w| {
                !matches!(
                    w.evaluation.decision,
                    PolicyDecision::Deny | PolicyDecision::Unsupported
                )
            })
    }
    pub fn eligible_count(&self) -> usize {
        self.eligible().count()
    }
    /// Items that will never be attempted: build-time `Unsupported`
    /// (wrong kind) and policy-time `Deny`/`Unsupported`, each with its
    /// own reason -- rendered explicitly in preview, never silently
    /// excluded with no trace.
    pub fn excluded(&self) -> impl Iterator<Item = (&BulkItem, String)> {
        self.items.iter().filter_map(|item| match &item.workflow {
            Err(reason) => Some((item, reason.clone())),
            Ok(w) if matches!(w.evaluation.decision, PolicyDecision::Deny) => {
                Some((item, format!("DENIED: {:?}", w.evaluation.reasons)))
            }
            Ok(w) if matches!(w.evaluation.decision, PolicyDecision::Unsupported) => {
                Some((item, "Unsupported by policy".to_string()))
            }
            Ok(_) => None,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// One target's post-commit record -- reuses `MutationOutcome`/
/// `Verification` verbatim, never a parallel success/failure vocabulary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BulkOutcome {
    pub uid: String,
    pub namespace: String,
    pub name: String,
    pub commit: MutationOutcome,
    pub verification: Option<Verification>,
}

/// Exact aggregate counts for a finished (or partially finished, if
/// cancelled) bulk operation -- every field independently countable
/// from `results`, never a single collapsed "success" bit.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BulkSummary {
    pub total_selected: usize,
    pub excluded: usize,
    pub attempted: usize,
    pub committed: usize,
    pub verified: usize,
    pub cancelled: usize,
    pub other: usize,
}
pub fn summarize(workflow: &BulkWorkflow) -> BulkSummary {
    let mut s = BulkSummary {
        total_selected: workflow.items.len(),
        excluded: workflow.excluded().count(),
        ..Default::default()
    };
    if let Some(results) = &workflow.results {
        s.attempted = results.len();
        for r in results {
            match r.commit {
                MutationOutcome::Committed | MutationOutcome::CommittedButJournalIncomplete => {
                    s.committed += 1;
                    if matches!(r.verification, Some(Verification::Verified)) {
                        s.verified += 1;
                    }
                }
                MutationOutcome::Cancelled => s.cancelled += 1,
                _ => s.other += 1,
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::session::Scope;
    use crate::kube::discovery::Resource;
    use crate::mutation::{MutationEffect, MutationRisk, PolicyEvaluation};

    fn resource(kind: &str) -> Resource {
        Resource {
            api: ::kube::core::ApiResource {
                group: "".into(),
                version: "v1".into(),
                api_version: "v1".into(),
                kind: kind.into(),
                plural: "pods".into(),
            },
            namespaced: true,
            short_names: vec![],
            verbs: vec!["delete".into()],
        }
    }
    fn scope(uid: &str) -> Scope {
        Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/pods".into(),
            namespace: "ns".into(),
            name: format!("pod-{uid}"),
            uid: uid.into(),
        }
    }
    fn built(uid: &str, effect: MutationEffect, risk: MutationRisk) -> Built {
        Built {
            intent: crate::mutation::MutationIntent {
                request_id: 1,
                target: crate::mutation::MutationTarget {
                    scope: scope(uid),
                    resource: resource("Pod"),
                    expected_resource_version: None,
                },
                effect,
                risk,
                summary: "delete".into(),
                payload_sha256: None,
                source_action: "delete".into(),
                create_resource: None,
            },
            payload: None,
            change: "delete pod".into(),
        }
    }
    fn allow() -> PolicyEvaluation {
        PolicyEvaluation {
            decision: PolicyDecision::Allow,
            reasons: vec![],
        }
    }
    fn require_confirmation() -> PolicyEvaluation {
        PolicyEvaluation {
            decision: PolicyDecision::RequireConfirmation,
            reasons: vec![super::super::PolicyReason::DestructiveEffect],
        }
    }
    fn require_stronger() -> PolicyEvaluation {
        PolicyEvaluation {
            decision: PolicyDecision::RequireStrongerConfirmation,
            reasons: vec![super::super::PolicyReason::ForceSemantics],
        }
    }
    fn deny() -> PolicyEvaluation {
        PolicyEvaluation {
            decision: PolicyDecision::Deny,
            reasons: vec![super::super::PolicyReason::ReadonlyMode],
        }
    }

    #[test]
    fn eligible_excludes_deny_and_unsupported_never_silently() {
        let items = vec![
            BulkItem::from_built(
                "a".into(),
                "ns".into(),
                "pod-a".into(),
                Ok(built(
                    "a",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| allow(),
            ),
            BulkItem::from_built(
                "b".into(),
                "ns".into(),
                "pod-b".into(),
                Ok(built(
                    "b",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| deny(),
            ),
            BulkItem::from_built(
                "c".into(),
                "ns".into(),
                "pod-c".into(),
                Err("delete is not supported for ConfigMap".into()),
                |_| allow(),
            ),
        ];
        let workflow = BulkWorkflow::new("delete".into(), items);
        assert_eq!(workflow.eligible_count(), 1);
        assert_eq!(workflow.excluded().count(), 2);
        let reasons: Vec<String> = workflow.excluded().map(|(_, r)| r).collect();
        assert!(reasons.iter().any(|r| r.contains("DENIED")));
        assert!(reasons.iter().any(|r| r.contains("not supported")));
    }

    #[test]
    fn requirement_is_the_strongest_among_eligible_never_lowered() {
        let items = vec![
            BulkItem::from_built(
                "a".into(),
                "ns".into(),
                "pod-a".into(),
                Ok(built(
                    "a",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| require_confirmation(),
            ),
            BulkItem::from_built(
                "b".into(),
                "ns".into(),
                "pod-b".into(),
                Ok(built(
                    "b",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| require_stronger(),
            ),
        ];
        let workflow = BulkWorkflow::new("delete".into(), items);
        assert_eq!(workflow.requirement(), ConfirmationRequirement::Strong);
    }

    #[test]
    fn requirement_is_none_when_every_eligible_item_is_plain_allow() {
        let items = vec![BulkItem::from_built(
            "a".into(),
            "ns".into(),
            "pod-a".into(),
            Ok(built("a", MutationEffect::Modify, MutationRisk::Routine)),
            |_| allow(),
        )];
        let workflow = BulkWorkflow::new("label".into(), items);
        assert_eq!(workflow.requirement(), ConfirmationRequirement::None);
    }

    #[test]
    fn summarize_counts_every_category_exactly_never_a_collapsed_bit() {
        let items = vec![
            BulkItem::from_built(
                "a".into(),
                "ns".into(),
                "pod-a".into(),
                Ok(built(
                    "a",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| allow(),
            ),
            BulkItem::from_built(
                "b".into(),
                "ns".into(),
                "pod-b".into(),
                Ok(built(
                    "b",
                    MutationEffect::Delete,
                    MutationRisk::Destructive,
                )),
                |_| deny(),
            ),
        ];
        let mut workflow = BulkWorkflow::new("delete".into(), items);
        workflow.results = Some(vec![BulkOutcome {
            uid: "a".into(),
            namespace: "ns".into(),
            name: "pod-a".into(),
            commit: MutationOutcome::Committed,
            verification: Some(Verification::Verified),
        }]);
        let s = summarize(&workflow);
        assert_eq!(s.total_selected, 2);
        assert_eq!(s.excluded, 1);
        assert_eq!(s.attempted, 1);
        assert_eq!(s.committed, 1);
        assert_eq!(s.verified, 1);
        assert_eq!(s.cancelled, 0);
    }
}
