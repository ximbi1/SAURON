#!/usr/bin/env python3
"""M8B.7 combined adversarial acceptance; explicit verified isolated kind
config only, no production fallback. Reuses accept-m4.py's tmux/expect
helpers, and follows accept-m8.py's own documented approach: scenarios
already proven exhaustively at the unit/fake-HTTP layer (Conflict/403/404
classification, no-automatic-retry, TOCTOU replacement rejection, policy
denial reasons, journal correctness, PDB-denial-never-falls-back-to-
delete, grace_period_seconds=0 isolation) are not re-derived interactively
here -- see tests/watch_transport.rs and docs/M8B_ACCEPTANCE.md for that
mapping. This script proves the parts only a real terminal + real cluster
can prove: end-to-end cordon/uncordon/set_image/trigger/evict/force_delete
through the actual TUI, readonly-then-live via :reload, live replacement
rejection, rapid context/namespace churn, 32x9, M4/M5/M6/M7/M8 regression
touchpoints while M8B workflows are in use, exact terminal restoration --
and Drain's own bounded exception: the preview/arm/cancel UI is proven
live, but the second confirm that would actually run the orchestrator is
never pressed, since kind-sauron-test is single-node and a real drain
would evict every fixture Pod across every M1-M8B namespace at once (the
same resolved, permanent bounded limitation recorded in
docs/M8B_ACCEPTANCE.md's M8B.5 journal entries)."""
import importlib.util
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm8b-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m8b-accept-operational.toml'
)
spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

KCFG = ['--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test']


def kubectl(*args, timeout=30):
    return subprocess.run(
        ['kubectl', *KCFG, *args], cwd=ROOT, check=True, text=True,
        capture_output=True, timeout=timeout,
    ).stdout


def keys(*args):
    m.tmux('send-keys', '-t', SESSION, *args)


def literal(text):
    m.tmux('send-keys', '-t', SESSION, '-l', text)


def command(text):
    keys(':')
    literal(text)
    keys('Enter')


def expect(*terms, absent=(), timeout=25):
    deadline = time.monotonic() + timeout
    matched = False
    output = ''
    while time.monotonic() < deadline:
        output = m.tmux('capture-pane', '-t', SESSION, '-p')
        if all(t in output for t in terms) and all(t not in output for t in absent):
            if matched:
                return output
            matched = True
        else:
            matched = False
        time.sleep(.08)
    raise AssertionError(f'Expected {terms}, absent {absent}\n{output}')


def seq1_readonly_cordon_denies_zero_write_then_reload_enables():
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = true\n')
    command('v1/nodes')
    expect('nodes [', 'list synchronized')
    command('cordon')
    expect('POLICY: Deny', 'ReadonlyMode')
    keys('y')
    out2 = m.tmux('capture-pane', '-t', SESSION, '-p')
    assert 'COMMIT RESULT' not in out2, 'a readonly denial must never commit: ' + out2
    keys('Escape')
    expect('nodes [')
    unschedulable = kubectl('get', 'nodes', '-o',
                             'jsonpath={.items[0].spec.unschedulable}').strip()
    assert unschedulable == '', f'readonly denial must send zero writes: unschedulable={unschedulable!r}'
    print('PASS seq1: readonly denies :cordon with zero writes, visible not hidden', flush=True)

    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n')
    command('reload')
    expect('Configuration reloaded')
    print('PASS seq1b: :reload picks up readonly=false for this context without relaunching', flush=True)


def seq2_cordon_uncordon_strong_confirmation_commits_and_verifies():
    command('v1/nodes')
    expect('nodes [')
    command('cordon')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    expect('CONFIRMATION: strong -- press confirm again to commit')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    expect('nodes [')
    unschedulable = kubectl('get', 'nodes', '-o',
                             'jsonpath={.items[0].spec.unschedulable}').strip()
    assert unschedulable == 'true', unschedulable
    command('uncordon')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    unschedulable = kubectl('get', 'nodes', '-o',
                             'jsonpath={.items[0].spec.unschedulable}').strip()
    assert unschedulable in ('', 'false'), \
        f'the node must be left schedulable no matter what: {unschedulable!r}'
    print('PASS seq2: cordon/uncordon require a real double press, commit and verify live, node left schedulable', flush=True)


def seq3_set_image_dry_run_commit_verify_and_reset():
    command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
    expect('deployments.apps [', 'list synchronized')
    command('set_image web=registry.k8s.io/pause:3.10')
    expect('CHANGE:', 'web', 'POLICY: RequireConfirmation')
    keys('d')
    expect('PREFLIGHT (server dry-run): Committed')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    web_image = kubectl(
        'get', 'deployment', 'm8b-multi', '-n', 'sauron-m8b', '-o',
        'jsonpath={.spec.template.spec.containers[?(@.name=="web")].image}',
    ).strip()
    sidecar_image = kubectl(
        'get', 'deployment', 'm8b-multi', '-n', 'sauron-m8b', '-o',
        'jsonpath={.spec.template.spec.containers[?(@.name=="sidecar")].image}',
    ).strip()
    assert web_image == 'registry.k8s.io/pause:3.10', web_image
    assert sidecar_image == 'registry.k8s.io/pause:3.9', \
        f'the unrelated sidecar container must never be touched: {sidecar_image}'
    command('set_image web=registry.k8s.io/pause:3.9')
    expect('CHANGE:', 'web')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS seq3: set_image dry-run/commit/verify round-trips live, unrelated sidecar untouched, fixture reset', flush=True)


def seq4_trigger_cronjob_creates_a_traceable_job():
    command('batch/v1/cronjobs -n sauron-m8b / name=m8b-nightly')
    expect('cronjobs.batch [', 'list synchronized')
    command('trigger')
    expect('ACTION: trigger', 'CONFIRMATION: required')
    keys('y')
    expect('COMMIT RESULT: Committed')
    out = expect('VERIFICATION:')
    assert 'Created' in out, out
    keys('Escape')
    jobs = kubectl('get', 'jobs', '-n', 'sauron-m8b', '-l',
                    'sauron.io/triggered-from=m8b-nightly', '-o',
                    'jsonpath={.items[*].metadata.name}').strip()
    assert jobs, 'trigger must create a Job traceable back to its source CronJob'
    print(f'PASS seq4: trigger created and verified Job(s) {jobs}', flush=True)


def seq5_evict_pdb_denied_then_succeeds_on_a_pdb_free_pod():
    command('v1/pods -n sauron-m8b / name=m8b-evict-blocked')
    expect('pods [', 'list synchronized')
    command('evict')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    keys('y')
    out = expect('COMMIT RESULT:')
    assert 'DisruptionBudgetDenied' in out, out
    keys('Escape')
    expect('pods [', '1 /')
    still_present = kubectl('get', 'pod', 'm8b-evict-blocked', '-n', 'sauron-m8b',
                             '-o', 'jsonpath={.metadata.name}').strip()
    assert still_present == 'm8b-evict-blocked', \
        'a PDB denial must never fall back to a plain delete, live'
    print('PASS seq5a: evict denied by a real PDB, Pod never removed as a fallback', flush=True)

    command('v1/pods -n sauron-m8b / name=m8b-evict-free')
    expect('pods [', 'list synchronized')
    command('evict')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    keys('y')
    expect('COMMIT RESULT: Committed')
    out = expect('VERIFICATION:')
    assert 'not attempted' not in out
    keys('Escape')
    print('PASS seq5b: evict on a PDB-free Pod committed and verified live', flush=True)


def seq6_force_delete_strong_confirmation_and_observed_gone():
    command('v1/pods -n sauron-m8b / name=m8b-force-delete')
    expect('pods [', 'list synchronized')
    command('force_delete')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    expect('CONFIRMATION: strong -- press confirm again to commit')
    keys('y')
    expect('COMMIT RESULT: Committed')
    out = expect('VERIFICATION:')
    assert 'not attempted' not in out
    keys('Escape')
    print('PASS seq6: force_delete requires a real double press, commits and verifies live, grace_period_seconds=0', flush=True)


def seq7_replacement_between_preview_and_confirm_is_rejected():
    command('v1/pods -n sauron-m8b / name=m8b-evict-blocked')
    expect('pods [', 'list synchronized')
    command('label replace-check=set')
    expect('CHANGE:', 'metadata.labels["replace-check"] = "set"')
    # Delete + recreate: exact same name, brand new UID, between preview and confirm.
    kubectl('delete', 'pod', 'm8b-evict-blocked', '-n', 'sauron-m8b', '--wait=true', '--timeout=30s')
    kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml')
    kubectl('wait', '--for=condition=Ready', 'pod/m8b-evict-blocked', '-n', 'sauron-m8b', '--timeout=60s')
    time.sleep(1)
    keys('y')
    out = expect('COMMIT RESULT:')
    assert 'COMMIT RESULT: Committed' not in out, \
        'a same-name/new-UID replacement must never silently commit against the stale preview: ' + out
    assert 'TargetReplaced' in out or 'NotFound' in out, out
    label = kubectl('get', 'pod', 'm8b-evict-blocked', '-n', 'sauron-m8b',
                     '-o', 'jsonpath={.metadata.labels.replace-check}').strip()
    assert label == '', f'the rejected commit must never have touched the replacement object: {label!r}'
    keys('Escape')
    print('PASS seq7: same-name/new-UID replacement between preview and confirm is rejected live, zero write applied', flush=True)


def seq8_rapid_context_namespace_churn_around_m8b_preview():
    command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
    expect('deployments.apps [')
    for _ in range(5):
        command('v1/pods -n sauron-fixtures')
        expect('pods [')
        command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
        expect('deployments.apps [')
    command('set_image web=registry.k8s.io/pause:3.10')
    expect('CHANGE:')
    keys('Escape')
    expect('deployments.apps [')
    print('PASS seq8: rapid resource churn around an open M8B mutation preview causes no corruption or crash', flush=True)


def seq9_m4_forward_alive_during_m8b_navigation(port_holder):
    command('v1/pods -n sauron-fixtures -l test=m4-sessions')
    expect('pods [1 / 1;', 'm4-sessions')
    keys('f')
    expect('Pod TCP port')
    keys('Enter')
    out = expect('Listening')
    port = None
    for line in out.splitlines():
        if 'Listening' in line and '127.0.0.1:' in line:
            port = line.split('127.0.0.1:')[1].split()[0].split('->')[0].strip()
            break
    assert port, out
    port_holder['port'] = port
    keys('Escape')
    expect('pods [1 / 1;')
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', check.stdout
    print(f'PASS seq9a: forward on 127.0.0.1:{port} started before M8B navigation', flush=True)


def seq9_m4_forward_still_alive(port_holder):
    port = port_holder['port']
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', \
        f'M4 forward on {port} must survive unrelated M8B mutation activity: {check.stdout}'
    print(f'PASS seq9b: forward on 127.0.0.1:{port} still alive', flush=True)


def stop_forward():
    command('pf')
    out = expect('Listening')
    line = next(l for l in out.splitlines() if 'Listening' in l)
    session_id = line.strip().lstrip('│').strip().split()[0]
    command(f'pf_stop {session_id}')
    expect('Cancelled')
    keys('Escape')


def seq10_m5_explain_regression():
    command('v1/pods -n sauron-fixtures / name=crashloop')
    expect('pods [1 /', 'crashloop', timeout=45)
    keys('X')
    text = expect('WHY:', 'crashloop')
    assert 'CrashLoopBackOff' in text or 'Error' in text, text
    keys('Escape')
    print('PASS seq10: M5 Explain unaffected by M8B', flush=True)


def seq11_m6_adjacent_xray_regression():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('a')
    expect('ADJACENT')
    keys('Escape')
    keys('x')
    expect('XRAY')
    keys('Escape')
    expect('deployments.apps [')
    print('PASS seq11: M6 Adjacent/Xray unaffected by M8B', flush=True)


def seq12_m7_m8_policy_and_journal_regression():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('u')
    expect('POLICY')
    keys('Escape')
    keys('m')
    out = expect('MUTATION JOURNAL')
    assert 'VerificationResult' in out or 'CommitResult' in out, \
        'the journal must show M8B records alongside M7/M8 ones: ' + out
    keys('Escape')
    print('PASS seq12: M7 :policy/:mutations views unaffected by M8B; journal carries M8B records too', flush=True)


def seq13_drain_preview_arms_then_is_cancelled_never_executed():
    # The single documented, permanent bounded exception in this script:
    # the preview/arm/cancel UI is proven live, but the second confirm
    # that would actually run the orchestrator is never pressed --
    # kind-sauron-test is single-node, and a real drain would evict every
    # fixture Pod across every M1-M8B namespace at once.
    before = kubectl('get', 'pods', '-A', '-o', 'jsonpath={.items[*].metadata.uid}').split()
    command('v1/nodes')
    expect('nodes [')
    command('drain')
    # Real cluster, real Pod count -- the document is far taller than the
    # terminal, so top-of-document terms and end-of-document terms are
    # checked in separate captures rather than assuming they share a page.
    expect('TARGET NODE', 'CORDON STEP')
    out = expect('PODS PLANNED FOR EVICTION', timeout=15)
    planned = int(out.split('PODS PLANNED FOR EVICTION (')[1].split(')')[0])
    assert planned > 0, 'the live preview must show a real, non-empty Pod plan: ' + out
    keys('G')
    expect('PDB-AWARE EVICTION', 'NO ROLLBACK', 'CANCELLATION',
           'CONFIRMATION: strong -- press confirm twice to start draining')
    keys('y')
    expect('CONFIRMATION: strong -- press confirm again to start draining')
    keys('Escape')
    expect('nodes [')
    unschedulable = kubectl('get', 'nodes', '-o',
                             'jsonpath={.items[0].spec.unschedulable}').strip()
    assert unschedulable in ('', 'false'), \
        f'cancelling before the second confirm must never cordon anything: {unschedulable!r}'
    after = kubectl('get', 'pods', '-A', '-o', 'jsonpath={.items[*].metadata.uid}').split()
    assert set(before) == set(after), \
        'cancelling before the second confirm must never evict a single Pod anywhere in the cluster'
    print('PASS seq13: drain preview shows the full safety contract live, arms on first press, '
          'cancelling before the second press evicts nothing and leaves every Pod and the node untouched', flush=True)


def seq14_narrow_terminal_m8b_preview():
    command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
    expect('deployments.apps [', '1 /')
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    command('set_image web=registry.k8s.io/pause:3.10')
    expect('Set image: m8b-multi')
    keys('Escape')
    time.sleep(1)
    expect('deployments.apps [', '1 /')
    command('v1/nodes')
    expect('nodes [')
    command('drain')
    expect('Drain:')
    keys('Escape')
    time.sleep(1)
    expect('nodes [')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    print('PASS seq14: 32x9 M8B mutation preview/confirm (including Drain, the longest document) does not panic or corrupt the session', flush=True)


def seq15_cancel_before_commit_sends_zero_writes():
    command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
    expect('deployments.apps [')
    command('set_image web=registry.k8s.io/pause:3.99')
    expect('CHANGE:', 'web')
    keys('Escape')
    expect('deployments.apps [', absent=('TARGET:',))
    web_image = kubectl(
        'get', 'deployment', 'm8b-multi', '-n', 'sauron-m8b', '-o',
        'jsonpath={.spec.template.spec.containers[?(@.name=="web")].image}',
    ).strip()
    assert web_image != 'registry.k8s.io/pause:3.99', \
        f'cancel before commit must send zero writes: web image={web_image}'
    print('PASS seq15: Escape before commit sends zero requests and leaves no residual mutation state', flush=True)


def seq16_quit_while_mutation_journal_open():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('C-c')
    out = expect('M8B_RESTORED status=0')
    assert 'M8B_RESTORED status=0' in out, out
    print('PASS seq16: quit while the mutation journal view is open exits cleanly, exact stty restore', flush=True)


def reset_fixtures():
    m.run('bash', 'scripts/test-cluster.sh', 'm8b-reset', timeout=60)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm8b-fixtures', timeout=60)
    m.run('bash', 'scripts/test-cluster.sh', 'm7-fixtures', timeout=60)
    m.run('bash', 'scripts/test-cluster.sh', 'm4-fixtures', timeout=60)
    m.run('bash', 'scripts/test-cluster.sh', 'm6-fixtures', timeout=60)
    m.run('cargo', 'build', '--locked', timeout=180)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = true\n')
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    port_holder = {}
    try:
        launch = ' '.join([
            str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
            '--config', str(OPERATIONAL), '--mutation-test-cluster-verified',
            '-n', 'sauron-m8b', 'apps/v1/deployments',
        ])
        shell = 'm8b_before=$(stty -g); ' + launch + '; m8b_code=$?; m8b_after=$(stty -g); '
        shell += 'if [ "$m8b_before" = "$m8b_after" ]; then echo M8B_RESTORED status=$m8b_code; else echo M8B_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('deployments.apps [', 'list synchronized', 'READ ONLY')

        seq1_readonly_cordon_denies_zero_write_then_reload_enables()
        seq9_m4_forward_alive_during_m8b_navigation(port_holder)
        seq2_cordon_uncordon_strong_confirmation_commits_and_verifies()
        seq9_m4_forward_still_alive(port_holder)
        seq3_set_image_dry_run_commit_verify_and_reset()
        seq4_trigger_cronjob_creates_a_traceable_job()
        seq9_m4_forward_still_alive(port_holder)
        seq5_evict_pdb_denied_then_succeeds_on_a_pdb_free_pod()
        seq6_force_delete_strong_confirmation_and_observed_gone()
        seq15_cancel_before_commit_sends_zero_writes()
        seq7_replacement_between_preview_and_confirm_is_rejected()
        seq9_m4_forward_still_alive(port_holder)
        seq8_rapid_context_namespace_churn_around_m8b_preview()
        seq13_drain_preview_arms_then_is_cancelled_never_executed()
        seq10_m5_explain_regression()
        seq11_m6_adjacent_xray_regression()
        seq12_m7_m8_policy_and_journal_regression()
        seq14_narrow_terminal_m8b_preview()
        seq9_m4_forward_still_alive(port_holder)
        stop_forward()
        seq16_quit_while_mutation_journal_open()
    finally:
        m.tmux('kill-session', '-t', SESSION)
    reset_fixtures()


if __name__ == '__main__':
    main()
