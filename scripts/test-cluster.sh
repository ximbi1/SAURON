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
    # Deliberately ambiguous/colliding CRDs for resource-resolution acceptance.
    kube_test apply -f "$repo_dir/tests/fixtures/ambiguous.yaml"
    for crd in portals.a.sauron.test widgets.a.sauron.test widgets.b.sauron.test probes.a.sauron.test; do
      kube_test wait --for=condition=Established "crd/$crd" --timeout=30s
    done
    kube_test apply -f "$repo_dir/tests/fixtures/ambiguous-instances.yaml"
    # Second context alias on the SAME isolated cluster, for context-switching
    # acceptance (picker, namespace-per-context memory). Not a second cluster.
    kubectl --kubeconfig "$test_kubeconfig" config set-context kind-sauron-test-b \
      --cluster=kind-sauron-test --user=kind-sauron-test --namespace=kube-system >/dev/null
    ;;
  test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test cluster -- --ignored --nocapture
    ;;
  m3-sort-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m3-sort.yaml"
    ;;
  m4-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m4-sessions.yaml"
    kube_test wait --for=condition=Ready pod/m4-sessions pod/m4-logburst -n sauron-fixtures --timeout=60s
    ;;
  m4-recreate)
    kube_test delete pod m4-sessions -n sauron-fixtures --wait=true --timeout=45s
    kube_test apply -f "$repo_dir/tests/fixtures/m4-sessions.yaml"
    kube_test wait --for=condition=Ready pod/m4-sessions -n sauron-fixtures --timeout=60s
    ;;
  m4-ephemeral)
    kube_test patch pod m4-sessions -n sauron-fixtures --subresource=ephemeralcontainers --type=merge \
      -p '{"spec":{"ephemeralContainers":[{"name":"observer","image":"busybox:1.37","command":["sh","-c","echo M4_EPHEMERAL_READY; sleep 3600"]}]}}'
    # No condition= wait target exists for ephemeral containers; poll their status
    # directly so callers never race a log request against a not-yet-running one.
    for _ in $(seq 1 30); do
      state="$(kube_test get pod m4-sessions -n sauron-fixtures \
        -o jsonpath='{.status.ephemeralContainerStatuses[?(@.name=="observer")].state.running}')"
      [ -n "$state" ] && break
      sleep 1
    done
    [ -n "$state" ] || { echo 'Ephemeral container observer never reached Running.' >&2; exit 1; }
    ;;
  m3-sort-update)
    kube_test patch configmap m3-sort-a -n sauron-fixtures --type merge -p '{"data":{"rank":"20"}}'
    ;;
  m3-sort-delete)
    kube_test delete configmap m3-sort-a -n sauron-fixtures --wait=true --timeout=15s
    ;;
  *) echo 'Usage: bash scripts/test-cluster.sh [check|fixtures|test]' >&2; exit 2 ;;
esac
