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
  *) echo 'Usage: bash scripts/test-cluster-m9.sh [check|flux-install|flux-fixtures|flux-reset|flux-test]' >&2; exit 2 ;;
esac
