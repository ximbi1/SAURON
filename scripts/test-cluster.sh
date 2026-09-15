#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_kubeconfig="$repo_dir/.test-cluster/config"
test_context=kind-sauron-test
test_node=sauron-test-control-plane

test -f "$test_kubeconfig" || { echo 'Dedicated test kubeconfig missing; refusing default context.' >&2; exit 1; }
test_label="$(docker inspect --format '{{ index .Config.Labels "io.x-k8s.kind.cluster" }}' "$test_node")"
test "$test_label" = sauron-test || { echo 'Unexpected Docker cluster identity.' >&2; exit 1; }
test_binding="$(docker port "$test_node" 6443/tcp)"
[[ "$test_binding" == 127.0.0.1:* && "$test_binding" != *$'\n'* ]] || { echo 'Expected one loopback API binding.' >&2; exit 1; }
test_server="$(kubectl --kubeconfig "$test_kubeconfig" --context "$test_context" config view --minify -o jsonpath='{.clusters[0].cluster.server}')"
test "$test_server" = "https://$test_binding" || { echo 'Kubeconfig API endpoint differs from isolated Docker node; refusing.' >&2; exit 1; }

kube_test() { kubectl --kubeconfig "$test_kubeconfig" --context "$test_context" --request-timeout=15s "$@"; }
case "${1:-check}" in
  check)
    kube_test get nodes
    ;;
  fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/workloads.yaml"
    kube_test wait --for=condition=Established crd/eyes.testing.sauron.local --timeout=30s
    kube_test apply -f "$repo_dir/tests/fixtures/eye.yaml"
    ;;
  test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test cluster -- --ignored --nocapture
    ;;
  *) echo 'Usage: bash scripts/test-cluster.sh [check|fixtures|test]' >&2; exit 2 ;;
esac
