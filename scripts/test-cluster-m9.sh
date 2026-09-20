#!/usr/bin/env bash
# M9: dedicated GitOps integration test cluster, isolated from
# kind-sauron-test (the M1-M8B regression cluster is never touched by
# this script or by Flux/Argo CD installation). Same guarded Docker/API
# identity verification model as scripts/test-cluster.sh, pointed at a
# different cluster/context/kubeconfig -- see docs/M9_ACCEPTANCE.md's
# "M9 dedicated cluster lifecycle" section for the full rationale.
set -euo pipefail

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_kubeconfig="$repo_dir/.test-cluster-m9/config"
test_context=kind-sauron-m9
test_node=sauron-m9-control-plane
flux_version=v2.9.5
argocd_version=v3.5.3

test -f "$test_kubeconfig" || { echo 'Dedicated M9 kubeconfig missing; refusing default context.' >&2; exit 1; }
test_label="$(docker inspect --format '{{ index .Config.Labels "io.x-k8s.kind.cluster" }}' "$test_node")"
test "$test_label" = sauron-m9 || { echo 'Unexpected Docker cluster identity.' >&2; exit 1; }
test_binding="$(docker port "$test_node" 6443/tcp)"
[[ "$test_binding" == 127.0.0.1:* && "$test_binding" != *$'\n'* ]] || { echo 'Expected one loopback API binding.' >&2; exit 1; }
test_server="$(kubectl --kubeconfig "$test_kubeconfig" --context "$test_context" config view --minify -o jsonpath='{.clusters[0].cluster.server}')"
test "$test_server" = "https://$test_binding" || { echo 'Kubeconfig API endpoint differs from isolated Docker node; refusing.' >&2; exit 1; }

kube_m9() { kubectl --kubeconfig "$test_kubeconfig" --context "$test_context" --request-timeout=15s "$@"; }
case "${1:-check}" in
  check)
    kube_m9 get nodes
    ;;
  flux-install)
    # Pinned version, fetched once and applied via plain kubectl -- no
    # `flux` CLI involved, matching every other test-cluster.sh install
    # case (e.g. m5-metrics-install's own kustomize/kubectl-only pattern).
    manifest="$repo_dir/.test-cluster-m9/flux-install-$flux_version.yaml"
    if [ ! -f "$manifest" ]; then
      curl -sL "https://github.com/fluxcd/flux2/releases/download/$flux_version/install.yaml" -o "$manifest"
    fi
    kube_m9 apply -f "$manifest"
    for deployment in source-controller kustomize-controller helm-controller notification-controller; do
      kube_m9 rollout status "deployment/$deployment" -n flux-system --timeout=120s
    done
    ;;
  flux-fixtures)
    kube_m9 apply -f "$repo_dir/tests/fixtures/m9-flux.yaml"
    ;;
  flux-reset)
    kube_m9 apply -f "$repo_dir/tests/fixtures/m9-flux.yaml"
    ;;
  flux-test)
    cd "$repo_dir"
    SAURON_TEST_M9_KUBECONFIG="$test_kubeconfig" cargo test --locked --test mutation_m9_live -- --ignored --nocapture
    ;;
  argocd-install)
    # Pinned version, fetched once and applied via plain kubectl -- no
    # `argocd` CLI involved, matching flux-install's own precedent.
    # --server-side is required: the ApplicationSet CRD's schema exceeds
    # kubectl client-side apply's 262144-byte last-applied-configuration
    # annotation limit (a real, documented Argo CD installation quirk,
    # not specific to this script).
    manifest="$repo_dir/.test-cluster-m9/argocd-install-$argocd_version.yaml"
    if [ ! -f "$manifest" ]; then
      curl -sL "https://raw.githubusercontent.com/argoproj/argo-cd/$argocd_version/manifests/install.yaml" -o "$manifest"
    fi
    kube_m9 get namespace argocd >/dev/null 2>&1 || kube_m9 create namespace argocd
    kube_m9 apply -n argocd --server-side --force-conflicts -f "$manifest"
    for deployment in argocd-repo-server argocd-server argocd-applicationset-controller; do
      kube_m9 rollout status "deployment/$deployment" -n argocd --timeout=120s
    done
    kube_m9 rollout status statefulset/argocd-application-controller -n argocd --timeout=120s
    ;;
  argocd-fixtures)
    kube_m9 apply -f "$repo_dir/tests/fixtures/m9-argocd.yaml"
    ;;
  argocd-reset)
    # guestbook is genuinely synced by the live test (real Service+
    # Deployment in sauron-m9); re-apply then re-sync so repeated runs
    # stay deterministic, matching every other *-reset case's own
    # self-restoring convention.
    kube_m9 apply -f "$repo_dir/tests/fixtures/m9-argocd.yaml"
    kube_m9 patch application guestbook -n argocd --type merge -p '{"operation":{"sync":{"revision":"master"}}}'
    ;;
  argocd-test)
    cd "$repo_dir"
    SAURON_TEST_M9_KUBECONFIG="$test_kubeconfig" cargo test --locked --test mutation_m9_argocd_live -- --ignored --nocapture
    ;;
  *) echo 'Usage: bash scripts/test-cluster-m9.sh [check|flux-install|flux-fixtures|flux-reset|flux-test|argocd-install|argocd-fixtures|argocd-reset|argocd-test]' >&2; exit 2 ;;
esac
