//! M7: mutation model and policy contracts. Infrastructure only -- M8 adds
//! user-facing mutation workflows on top of this. Nothing in this module (or
//! `policy`) issues a network request; that lives in `kube::mutation`'s
//! executor. NO MUTATION WITHOUT POLICY. UNKNOWN never silently means Allow.
pub mod bulk;
pub mod drain;
pub mod journal;
pub mod policy;
pub mod view;
pub mod workflow;

use crate::{app::session::Scope, kube::discovery::Resource};

/// Incarnation-safe identity: `scope` carries context/cluster/resource id/
/// namespace/name/UID/epoch/request, matching the same convention already
/// used for owned sessions (logs/exec/forward). Name alone is never identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationTarget {
    pub scope: Scope,
    pub resource: Resource,
    /// Only meaningful when the operation is conditioned on it; absent means
    /// the operation does not use optimistic concurrency, not "any version".
    pub expected_resource_version: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MutationEffect {
    Create,
    Modify,
    Delete,
}

/// Describes guardrail requirements, never a subjective danger score. Never
/// derived from a single low-looking field alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MutationRisk {
    Routine,
    Destructive,
    PrivilegeSensitive,
    ClusterCritical,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationIntent {
    pub request_id: u64,
    pub target: MutationTarget,
    pub effect: MutationEffect,
    pub risk: MutationRisk,
    /// Human-readable description of the requested change, never sensitive
    /// payload content.
    pub summary: String,
    /// Redacted-summary hash of the actual payload, if any; used to bind a
    /// confirmation to the exact requested change without storing it.
    pub payload_sha256: Option<String>,
    pub source_action: String,
    /// M8B.3: only meaningful for `MutationEffect::Create` -- the
    /// resource actually being *created* (e.g. Job), distinct from
    /// `target.resource` (the source object, e.g. CronJob, whose UID is
    /// TOCTOU-revalidated before the create is sent). `None` for every
    /// other effect.
    pub create_resource: Option<Resource>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicyReason {
    ReadonlyMode,
    ReadonlyOverride,
    UnverifiedCluster,
    TargetIdentityIncomplete,
    TargetReplaced,
    ProtectedNamespace(String),
    ClusterScopedSensitiveResource,
    DestructiveEffect,
    PrivilegeSensitiveResource,
    ClusterCriticalResource,
    ConfirmationRequired,
    StrongerConfirmationRequired,
    Unsupported,
    /// M8B.6: this specific intent skips graceful termination
    /// (`grace_period_seconds=0`) -- always accompanies
    /// `StrongerConfirmationRequired`, never a substitute for it, so a
    /// future policy audit can grep for exactly this reason instead of
    /// having to infer it from `source_action`.
    ForceSemantics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny,
    RequireConfirmation,
    RequireStrongerConfirmation,
    Unsupported,
}
impl PolicyDecision {
    /// M8.0: the UI-facing mapping from a policy decision to the confirmation
    /// UX it demands -- pure and total, so no caller can invent a fourth
    /// posture. `Allow`/`Deny`/`Unsupported` need no confirmation UX: `Allow`
    /// proceeds, the other two never reach a confirmable state at all.
    pub fn confirmation_requirement(self) -> ConfirmationRequirement {
        match self {
            PolicyDecision::RequireConfirmation => ConfirmationRequirement::Standard,
            PolicyDecision::RequireStrongerConfirmation => ConfirmationRequirement::Strong,
            PolicyDecision::Allow | PolicyDecision::Deny | PolicyDecision::Unsupported => {
                ConfirmationRequirement::None
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyEvaluation {
    pub decision: PolicyDecision,
    /// Always non-empty for Deny/RequireConfirmation/RequireStrongerConfirmation/
    /// Unsupported: a decision must be explainable, never a bare bool.
    pub reasons: Vec<PolicyReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationRequirement {
    None,
    Standard,
    Strong,
}

/// Session-local, never persisted, never reusable across a changed critical
/// property. Binds to the exact intent: context/cluster, resource id,
/// namespace, name, UID, effect and payload hash. A confirmation granted for
/// one object/effect/payload must never authorize a different one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmation {
    pub request_id: u64,
    pub scope: Scope,
    pub effect: MutationEffect,
    pub payload_sha256: Option<String>,
    pub requirement: ConfirmationRequirement,
}
impl Confirmation {
    pub fn authorizes(&self, intent: &MutationIntent) -> bool {
        self.request_id == intent.request_id
            && self.scope == intent.target.scope
            && self.effect == intent.effect
            && self.payload_sha256 == intent.payload_sha256
    }
}

/// Local, deterministic description of an intended change. No network
/// mutation. Distinct from server dry-run (`kube::mutation::Preflight`) and
/// from commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationPreview {
    pub intent: MutationIntent,
    pub evaluation: PolicyEvaluation,
}

/// Preserves the distinction between "definitely not committed" and "commit
/// outcome cannot be proven" -- collapsing them is the one mistake this type
/// exists to prevent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationOutcome {
    Committed,
    CommittedButJournalIncomplete,
    Denied,
    PreflightRejected,
    TargetReplaced,
    Conflict,
    Forbidden,
    NotFound,
    TimedOutBeforeSend,
    OutcomeUnknown,
    TransportFailure,
    Cancelled,
    Unsupported,
    /// M8B.4: the API server rejected an eviction because it would
    /// violate a PodDisruptionBudget (HTTP 429) -- a distinct fact from
    /// `Conflict` (409), which callers must never blur together, and
    /// which must never trigger an automatic fallback to a plain delete.
    DisruptionBudgetDenied,
}

/// M8.5: a fresh, post-commit observation -- a distinct fact from
/// `MutationOutcome` (what the API server did with the write itself).
/// A verification failure/timeout/unknown NEVER downgrades a `Committed`
/// outcome: the server already confirmed the write; this only describes
/// whether SAURON was able to confirm it too, and how. Deliberately does
/// not include readiness/availability -- that stays M5's Explain/health
/// engine, never duplicated here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verification {
    /// Fresh observation confirms the exact requested change.
    Verified,
    /// Observed, but the resource has not yet reflected the change in any
    /// way that indicates failure (e.g. delete accepted, no
    /// `deletionTimestamp` yet). Not evidence of a problem -- an
    /// eventual-consistency window, never retried automatically.
    Pending,
    /// Verification could not be attempted or completed (cancelled, timed
    /// out, or an ambiguous transport failure) -- never treated as failure.
    Unknown,
    /// A fresh observation was obtained but does not match what was
    /// requested. Redacted, field-path-only description -- never a raw
    /// value dump.
    ObservedDifferent(String),
    /// The object's UID changed between commit and verification -- the
    /// commit itself is unaffected; this only means SAURON cannot vouch for
    /// what the *current* object reflects.
    TargetReplaced,
    /// Delete-specific: `metadata.deletionTimestamp` is now present.
    DeletionInProgress,
    /// Delete-specific: a fresh GET returned 404, or returned 200 with a
    /// different UID under the same name (the previewed object is gone).
    ObservedGone,
    /// Create-specific (M8B.3): a fresh GET confirms the newly created
    /// object exists, correctly traceable back to its source. The String
    /// is a bounded, redacted description of the confirmed identity --
    /// never a raw object dump.
    Created(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(uid: &str) -> Scope {
        Scope {
            epoch: 1,
            request: 1,
            context: "kind-sauron-test".into(),
            cluster: "kind-sauron-test".into(),
            resource: "v1/configmaps".into(),
            namespace: "sauron-m7".into(),
            name: "m7-target".into(),
            uid: uid.into(),
        }
    }
    fn intent(uid: &str, effect: MutationEffect, hash: Option<&str>) -> MutationIntent {
        MutationIntent {
            request_id: 7,
            target: MutationTarget {
                scope: scope(uid),
                resource: crate::kube::discovery::Resource {
                    api: ::kube::core::ApiResource {
                        group: "".into(),
                        version: "v1".into(),
                        api_version: "v1".into(),
                        kind: "ConfigMap".into(),
                        plural: "configmaps".into(),
                    },
                    namespaced: true,
                    short_names: vec![],
                    verbs: vec!["patch".into()],
                },
                expected_resource_version: None,
            },
            effect,
            risk: MutationRisk::Routine,
            summary: "annotate m7-target".into(),
            payload_sha256: hash.map(str::to_owned),
            source_action: "m7_proof".into(),
            create_resource: None,
        }
    }

    #[test]
    fn confirmation_authorizes_only_the_exact_bound_intent() {
        let base = intent("uid-1", MutationEffect::Modify, Some("hash-a"));
        let confirmation = Confirmation {
            request_id: base.request_id,
            scope: base.target.scope.clone(),
            effect: base.effect,
            payload_sha256: base.payload_sha256.clone(),
            requirement: ConfirmationRequirement::Standard,
        };
        assert!(confirmation.authorizes(&base));

        let mut replaced = base.clone();
        replaced.target.scope.uid = "uid-2".into();
        assert!(
            !confirmation.authorizes(&replaced),
            "same-name new-UID replacement must not reuse the confirmation"
        );

        let mut other_namespace = base.clone();
        other_namespace.target.scope.namespace = "other-ns".into();
        assert!(!confirmation.authorizes(&other_namespace));

        let mut other_context = base.clone();
        other_context.target.scope.context = "other-context".into();
        assert!(!confirmation.authorizes(&other_context));

        let mut other_effect = base.clone();
        other_effect.effect = MutationEffect::Delete;
        assert!(!confirmation.authorizes(&other_effect));

        let mut other_payload = base.clone();
        other_payload.payload_sha256 = Some("hash-b".into());
        assert!(!confirmation.authorizes(&other_payload));

        let mut other_request = base.clone();
        other_request.request_id = 8;
        assert!(!confirmation.authorizes(&other_request));

        let mut other_resource = base.clone();
        other_resource.target.scope.resource = "v1/secrets".into();
        assert!(!confirmation.authorizes(&other_resource));
    }
}
