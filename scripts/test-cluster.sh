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
  m5-metrics-install)
    # KIND ONLY: self-signed kubelet serving certs need this fixture-only exception.
    # SAURON's API client still verifies TLS; nothing changes in production.
    kube_test apply -k "$repo_dir/tests/fixtures/metrics-server"
    kube_test rollout status deployment/metrics-server -n kube-system --timeout=120s
    ;;
  m4-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m4-sessions.yaml"
    kube_test wait --for=condition=Ready pod/m4-sessions pod/m4-logburst -n sauron-fixtures --timeout=60s
    ;;
  m5-health-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m5-health.yaml"
    ;;
  m6-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m6-relationships.yaml"
    kube_test rollout status deployment/m6-web -n sauron-m6 --timeout=120s
    ;;
  m6-test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test relationships_live -- --ignored --nocapture
    ;;
  m7-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m7-mutation.yaml"
    ;;
  m7-reset)
    # The live proof mutation patches an annotation; restore the exact
    # pristine fixture state so repeated live/soak runs stay deterministic.
    kube_test apply -f "$repo_dir/tests/fixtures/m7-mutation.yaml"
    kube_test annotate configmap m7-target -n sauron-m7 m7-proof- --overwrite 2>/dev/null || true
    ;;
  m7-test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test mutation_live -- --ignored --nocapture
    ;;
  m8-fixtures)
    kube_test apply -f "$repo_dir/tests/fixtures/m8-mutation.yaml"
    kube_test rollout status deployment/m8-deploy -n sauron-m8 --timeout=120s
    kube_test wait --for=condition=Ready pod/m8-pod -n sauron-m8 --timeout=60s
    ;;
  m8-reset)
    # M8.5's live proof scales/restarts/relabels/deletes disposable objects;
    # restore exact pristine fixture state so repeated live/soak runs stay
    # deterministic, matching m7-reset's own convention.
    kube_test apply -f "$repo_dir/tests/fixtures/m8-mutation.yaml"
    kube_test scale deployment/m8-deploy -n sauron-m8 --replicas=1
    kube_test rollout status deployment/m8-deploy -n sauron-m8 --timeout=120s
    kube_test wait --for=condition=Ready pod/m8-pod -n sauron-m8 --timeout=60s
    ;;
  m8-test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test mutation_workflows_live -- --ignored --nocapture
    ;;
  m8b-fixtures)
    # M8B.1 (Cordon/Uncordon) targets the Node directly; ensure it starts
    # schedulable so live tests begin from a known state.
    for node in $(kube_test get nodes -o jsonpath='{.items[*].metadata.name}'); do
      kube_test uncordon "$node" 2>/dev/null || true
    done
    # M8B.2 (Set Image): a dedicated two-container Deployment so a live
    # test can prove the unrelated sidecar container is never touched.
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-set-image.yaml"
    kube_test rollout status deployment/m8b-multi -n sauron-m8b --timeout=120s
    # M8B.3 (CronJob trigger): a dedicated CronJob whose own schedule never
    # fires during a test run (once a year), so any Job present was
    # created only by a live trigger test, never the controller itself.
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-cronjob.yaml"
    # M8B.4 (Evict): a Pod behind a tight PDB (minAvailable=1, the Pod is
    # its only member) proves a real 429 denial; a second, PDB-free Pod
    # proves a real successful eviction.
    # M8B.6 (Force delete): m8b-force-delete is a disposable Pod added to
    # the same fixture file for the mechanism proof (not the specific
    # stuck-on-an-unreachable-node scenario, which is out of proportion to
    # reproduce live).
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-evict.yaml"
    kube_test wait --for=condition=Ready pod/m8b-evict-blocked pod/m8b-evict-free pod/m8b-force-delete -n sauron-m8b --timeout=60s
    ;;
  m8b-reset)
    # Uncordon every node -- idempotent, safe even if nothing is cordoned.
    for node in $(kube_test get nodes -o jsonpath='{.items[*].metadata.name}'); do
      kube_test uncordon "$node"
    done
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-set-image.yaml"
    kube_test rollout status deployment/m8b-multi -n sauron-m8b --timeout=120s
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-cronjob.yaml"
    # Remove every Job a live trigger test created, keeping repeated runs
    # deterministic.
    kube_test delete job -n sauron-m8b -l sauron.io/triggered-from=m8b-nightly --ignore-not-found
    # m8b-evict-free is genuinely evicted by the live test; recreate it.
    kube_test apply -f "$repo_dir/tests/fixtures/m8b-evict.yaml"
    kube_test wait --for=condition=Ready pod/m8b-evict-blocked pod/m8b-evict-free pod/m8b-force-delete -n sauron-m8b --timeout=60s
    ;;
  m8b-test)
    cd "$repo_dir"
    SAURON_TEST_KUBECONFIG="$test_kubeconfig" cargo test --locked --test mutation_m8b_live -- --ignored --nocapture
    ;;
  m10-fixtures)
    # M10.3 (bulk guarded mutations): inert ConfigMaps -- no PDB/rollout
    # concerns, safe to bulk-label/bulk-delete repeatedly.
    kube_test apply -f "$repo_dir/tests/fixtures/m10-bulk.yaml"
    ;;
  m10-reset)
    kube_test apply -f "$repo_dir/tests/fixtures/m10-bulk.yaml"
    # bulk_label may have added a label; re-apply doesn't strip labels
    # added out-of-band, so explicitly clear it on every fixture name for
    # a deterministic re-run, even one interrupted mid-way.
    # kube-root-ca.crt is cluster-injected, not one of this file's own
    # fixtures, but a bulk "select all visible" test legitimately selects
    # it too -- included here so a real select-all-visible demonstration
    # doesn't leave a stray label on it between runs.
    kube_test label configmap m10-bulk-a m10-bulk-b m10-bulk-c m10-bulk-delete-1 m10-bulk-delete-2 \
      kube-root-ca.crt -n sauron-m10 --overwrite team- 2>/dev/null || true
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
