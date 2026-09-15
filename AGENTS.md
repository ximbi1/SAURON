# Repository instructions

Read HANDBOOK.md at every major phase and keep it current. Current operations and
verification checkpoint are in docs/RUNBOOK.md; parity in docs/SOFKA_PARITY.md.

The user's pre-existing Kubernetes cluster is PRODUCTION. Read-only inspection is
authorized. Never create, edit, patch, delete, scale, restart, evict, drain, exec, attach,
upload, launch debug/helper resources, or invoke potentially mutating plugins there.
Never use the default kubeconfig for fixture setup or cleanup.

All development fixture mutations must target the separate Docker kind cluster
sauron-test with `.test-cluster/config` and `--context kind-sauron-test`. Use
`bash scripts/test-cluster.sh` to verify the Docker label and loopback API endpoint
before applying fixtures. Never print/commit raw kubeconfig or Secret values.

Keep buildable slices, implement real behavior, record actual test results and failures.
User permits installing needed development tools. No deployment or publishing authorized.
