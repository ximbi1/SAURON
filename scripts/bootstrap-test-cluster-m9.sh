#!/usr/bin/env bash
# M9: creates only the named, local Docker M9 GitOps test cluster; never
# edits default kubeconfig, never touches kind-sauron-test (the M1-M8B
# regression cluster stays untouched -- see docs/M9_ACCEPTANCE.md's
# resolved "dedicated cluster" decision for why this is a second cluster
# rather than installing Flux/Argo CD into the existing one).
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_config="$repo_dir/.test-cluster-m9/config"
test_endpoint="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
[[ "$test_endpoint" == unix://* ]] || { echo 'Refusing non-local Docker endpoint.' >&2; exit 1; }
if docker container inspect sauron-m9-control-plane >/dev/null 2>&1; then
  bash "$repo_dir/scripts/test-cluster-m9.sh" check
  exit
fi
umask 077
mkdir -p "$repo_dir/.test-cluster-m9"
if test -f "$test_config"; then
  backup_dir="$(mktemp -d "$repo_dir/.test-cluster-m9/previous.XXXXXX")"
  cp -- "$test_config" "$backup_dir/config"
  echo 'Preserved previous dedicated kubeconfig under ignored .test-cluster-m9/previous.*.'
fi
kind create cluster --name sauron-m9 --image kindest/node:v1.33.1 \
  --config "$repo_dir/tests/fixtures/kind.yaml" --kubeconfig "$test_config" --wait 60s
chmod 600 "$test_config"
bash "$repo_dir/scripts/test-cluster-m9.sh" check
