#!/usr/bin/env bash
# Creates only the named, local Docker test cluster; never edits default kubeconfig.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_config="$repo_dir/.test-cluster/config"
test_endpoint="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
[[ "$test_endpoint" == unix://* ]] || { echo 'Refusing non-local Docker endpoint.' >&2; exit 1; }
if docker container inspect sauron-test-control-plane >/dev/null 2>&1; then
  bash "$repo_dir/scripts/test-cluster.sh" check
  exit
fi
umask 077
mkdir -p "$repo_dir/.test-cluster"
if test -f "$test_config"; then
  backup_dir="$(mktemp -d "$repo_dir/.test-cluster/previous.XXXXXX")"
  cp -- "$test_config" "$backup_dir/config"
  echo 'Preserved previous dedicated kubeconfig under ignored .test-cluster/previous.*.'
fi
kind create cluster --name sauron-test --image kindest/node:v1.33.1 \
  --config "$repo_dir/tests/fixtures/kind.yaml" --kubeconfig "$test_config" --wait 60s
chmod 600 "$test_config"
bash "$repo_dir/scripts/test-cluster.sh" check
