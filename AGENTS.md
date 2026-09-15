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

When interactive acceptance against kind-sauron-test finds a real bug, before continuing
to the next task: find the root cause (not just the symptom), add a regression test when
reasonable, run the full check suite, and repeat the exact live flow that discovered it to
confirm the fix against the real cluster, not just the unit test. A milestone is not done
because it compiles or because unit tests pass — it is done when it has been observed
working live, including the flows most likely to break it, not just the happy path.

When changing context, namespace, resource scope, or another identity boundary, actively
test for stale asynchronous results from the previous scope. A result may only update
visible state when its epoch/request identity still matches the current view. Test rapid
repeated switching, not only a single clean transition.
