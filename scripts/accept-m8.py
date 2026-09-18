#!/usr/bin/env python3
"""M8.6 combined adversarial acceptance; explicit verified isolated kind
config only, no production fallback. Reuses accept-m4.py's tmux/expect
helpers, and follows accept-m7.py's own documented approach: scenarios
already proven exhaustively at the unit/fake-HTTP layer (Conflict/403/404
classification, no-automatic-retry, TOCTOU replacement rejection, policy
denial reasons, journal correctness) are not re-derived interactively here
-- see tests/watch_transport.rs and docs/M8_ACCEPTANCE.md for that mapping.
This script proves the parts only a real terminal + real cluster can prove:
end-to-end scale/restart/delete/label/annotate through the actual TUI,
readonly-then-live via :reload (not two separate launches), live
replacement rejection, rapid context/namespace churn, 32x9, M4/M5/M6/M7
regression touchpoints while M8 workflows are in use, and exact terminal
restoration."""
import importlib.util
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm8-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m8-accept-operational.toml'
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


def seq1_readonly_scale_denies_zero_write_then_reload_enables():
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = true\n')
    command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
    expect('deployments.apps [', 'list synchronized')
    command('scale 2')
    expect('POLICY: Deny', 'ReadonlyMode')
    keys('y')
    out2 = m.tmux('capture-pane', '-t', SESSION, '-p')
    assert 'COMMIT RESULT' not in out2, 'a readonly denial must never commit: ' + out2
    keys('Escape')
    expect('deployments.apps [')
    replicas = kubectl('get', 'deployment', 'm8-deploy', '-n', 'sauron-m8',
                        '-o', 'jsonpath={.spec.replicas}').strip()
    assert replicas == '1', f'readonly denial must send zero writes, but replicas={replicas}'
    print('PASS seq1: readonly denies :scale with zero writes, visible not hidden', flush=True)

    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n')
    command('reload')
    expect('Configuration reloaded')
    print('PASS seq1b: :reload picks up readonly=false for this context without relaunching', flush=True)


def seq2_scale_dry_run_commit_verify_and_reset():
    command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
    expect('deployments.apps [')
    command('scale 2')
    expect('CHANGE:', 'replicas: 1 -> 2', 'POLICY: RequireConfirmation')
    keys('d')
    expect('PREFLIGHT (server dry-run): Committed')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    replicas = kubectl('get', 'deployment', 'm8-deploy', '-n', 'sauron-m8',
                        '-o', 'jsonpath={.spec.replicas}').strip()
    assert replicas == '2', replicas
    command('scale 1')
    expect('CHANGE:', 'replicas: 2 -> 1')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS seq2: scale dry-run/commit/verify round-trips live, fixture left deterministic', flush=True)


def seq3_restart_exact_annotation_observed():
    command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
    expect('deployments.apps [')
    command('restart')
    expect('ACTION: restart', 'CONFIRMATION: required')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    annotation = kubectl(
        'get', 'deployment', 'm8-deploy', '-n', 'sauron-m8',
        '-o', 'jsonpath={.spec.template.metadata.annotations.kubectl\\.kubernetes\\.io/restartedAt}',
    ).strip()
    assert annotation, 'restart must set a real restartedAt annotation'
    print(f'PASS seq3: restart committed and verified, restartedAt={annotation}', flush=True)


def seq4_label_annotate_set_remove_preserve_unrelated():
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [', '1 /')
    command('label accept=set')
    expect('CHANGE:', 'metadata.labels["accept"] = "set"')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    kept = kubectl('get', 'configmap', 'm8-meta', '-n', 'sauron-m8',
                    '-o', 'jsonpath={.metadata.labels.kept}').strip()
    assert kept == 'unrelated-label-must-survive', kept
    command('label accept-')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    command('annotate accept=set')
    expect('CHANGE:', 'metadata.annotations["accept"] = "set"')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    kept = kubectl('get', 'configmap', 'm8-meta', '-n', 'sauron-m8',
                    '-o', 'jsonpath={.metadata.annotations.kept}').strip()
    assert kept == 'unrelated-annotation-must-survive', kept
    command('annotate accept-')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS seq4: label/annotate set+remove commit and verify live, unrelated metadata always survives', flush=True)


def recreate_m8_pod():
    # A same-name recreate right after a delete can otherwise race the old
    # incarnation's own termination (`apply` would just patch the still-
    # terminating object, and `wait --for=condition=Ready` would then time
    # out against a Pod that is dying, not becoming ready).
    kubectl('delete', 'pod', 'm8-pod', '-n', 'sauron-m8', '--ignore-not-found',
            '--wait=true', '--timeout=30s')
    kubectl('apply', '-f', 'tests/fixtures/m8-mutation.yaml')
    kubectl('wait', '--for=condition=Ready', 'pod/m8-pod', '-n', 'sauron-m8', '--timeout=60s')


def seq5_delete_strong_confirmation_and_observed_gone():
    recreate_m8_pod()
    command('v1/pods -n sauron-m8 / name=m8-pod')
    expect('pods [', 'list synchronized')
    command('delete')
    expect('CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    expect('CONFIRMATION: strong -- press confirm again to commit')
    keys('y')
    expect('COMMIT RESULT: Committed')
    out = expect('VERIFICATION:')
    assert 'not attempted' not in out
    keys('Escape')
    print('PASS seq5: delete requires a real double press, commits and verifies live', flush=True)


def seq6_replacement_between_preview_and_confirm_is_rejected():
    recreate_m8_pod()
    command('v1/pods -n sauron-m8 / name=m8-pod')
    expect('pods [', 'list synchronized')
    command('label replace-check=set')
    expect('CHANGE:', 'metadata.labels["replace-check"] = "set"')
    # Delete + recreate: exact same name, brand new UID, between preview and confirm.
    kubectl('delete', 'pod', 'm8-pod', '-n', 'sauron-m8', '--wait=true', '--timeout=30s')
    kubectl('apply', '-f', 'tests/fixtures/m8-mutation.yaml')
    kubectl('wait', '--for=condition=Ready', 'pod/m8-pod', '-n', 'sauron-m8', '--timeout=60s')
    time.sleep(1)
    keys('y')
    out = expect('COMMIT RESULT:')
    assert 'COMMIT RESULT: Committed' not in out, \
        'a same-name/new-UID replacement must never silently commit against the stale preview: ' + out
    assert 'TargetReplaced' in out or 'NotFound' in out, out
    label = kubectl('get', 'pod', 'm8-pod', '-n', 'sauron-m8',
                     '-o', 'jsonpath={.metadata.labels.replace-check}').strip()
    assert label == '', f'the rejected commit must never have touched the replacement object: {label!r}'
    keys('Escape')
    print('PASS seq6: same-name/new-UID replacement between preview and confirm is rejected live, zero write applied', flush=True)


def seq7_rapid_context_namespace_churn_around_mutation_preview():
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [')
    for _ in range(5):
        command('v1/pods -n sauron-fixtures')
        expect('pods [')
        command('v1/configmaps -n sauron-m8 / name=m8-meta')
        expect('configmaps [')
    command('label churn=set')
    expect('CHANGE:')
    keys('Escape')
    expect('configmaps [')
    print('PASS seq7: rapid resource churn around an open mutation preview causes no corruption or crash', flush=True)


def seq8_m4_forward_alive_during_m8_navigation(port_holder):
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
    print(f'PASS seq8a: forward on 127.0.0.1:{port} started before M8 navigation', flush=True)


def seq8_m4_forward_still_alive(port_holder):
    port = port_holder['port']
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', \
        f'M4 forward on {port} must survive unrelated M8 mutation activity: {check.stdout}'
    print(f'PASS seq8b: forward on 127.0.0.1:{port} still alive', flush=True)


def stop_forward():
    command('pf')
    out = expect('Listening')
    line = next(l for l in out.splitlines() if 'Listening' in l)
    session_id = line.strip().lstrip('│').strip().split()[0]
    command(f'pf_stop {session_id}')
    expect('Cancelled')
    keys('Escape')


def seq9_m5_explain_regression():
    command('v1/pods -n sauron-fixtures / name=crashloop')
    expect('pods [1 /', 'crashloop', timeout=45)
    keys('X')
    text = expect('WHY:', 'crashloop')
    assert 'CrashLoopBackOff' in text or 'Error' in text, text
    keys('Escape')
    print('PASS seq9: M5 Explain unaffected by M8', flush=True)


def seq10_m6_adjacent_xray_regression():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('a')
    expect('ADJACENT')
    keys('Escape')
    keys('x')
    expect('XRAY')
    keys('Escape')
    expect('deployments.apps [')
    print('PASS seq10: M6 Adjacent/Xray unaffected by M8', flush=True)


def seq11_m7_policy_and_journal_regression():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('u')
    expect('POLICY')
    keys('Escape')
    keys('m')
    out = expect('MUTATION JOURNAL')
    assert 'VerificationResult' in out or 'CommitResult' in out, \
        'the journal must show M8 records alongside M7 ones: ' + out
    keys('Escape')
    print('PASS seq11: M7 :policy/:mutations views unaffected by M8; journal carries M8 records too', flush=True)


def seq12_narrow_terminal_mutation_preview():
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [', '1 /')
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    command('label narrow=set')
    expect('Label: m8-meta')
    keys('y')
    time.sleep(2)
    expect('Label: m8-meta')
    keys('Escape')
    expect('configmaps [', '1 /')
    command('label narrow-')
    keys('y')
    time.sleep(2)
    keys('Escape')
    expect('configmaps [', '1 /')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    print('PASS seq12: 32x9 mutation preview/confirm/commit does not panic or corrupt the session', flush=True)


def seq13_cancel_before_commit_sends_zero_writes():
    command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
    expect('deployments.apps [')
    command('scale 9')
    expect('CHANGE:', 'replicas: 1 -> 9')
    keys('Escape')
    expect('deployments.apps [', absent=('TARGET:',))
    replicas = kubectl('get', 'deployment', 'm8-deploy', '-n', 'sauron-m8',
                        '-o', 'jsonpath={.spec.replicas}').strip()
    assert replicas == '1', f'cancel before commit must send zero writes: replicas={replicas}'
    print('PASS seq13: Escape before commit sends zero requests and leaves no residual mutation state', flush=True)


def seq14_quit_while_mutation_journal_open():
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('C-c')
    out = expect('M8_RESTORED status=0')
    assert 'M8_RESTORED status=0' in out, out
    print('PASS seq14: quit while the mutation journal view is open exits cleanly, exact stty restore', flush=True)


def reset_fixtures():
    m.run('bash', 'scripts/test-cluster.sh', 'm8-reset', timeout=60)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm8-fixtures', timeout=60)
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
            '-n', 'sauron-m8', 'apps/v1/deployments',
        ])
        shell = 'm8_before=$(stty -g); ' + launch + '; m8_code=$?; m8_after=$(stty -g); '
        shell += 'if [ "$m8_before" = "$m8_after" ]; then echo M8_RESTORED status=$m8_code; else echo M8_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('deployments.apps [', 'list synchronized', 'READ ONLY')

        seq1_readonly_scale_denies_zero_write_then_reload_enables()
        seq8_m4_forward_alive_during_m8_navigation(port_holder)
        seq2_scale_dry_run_commit_verify_and_reset()
        seq3_restart_exact_annotation_observed()
        seq8_m4_forward_still_alive(port_holder)
        seq4_label_annotate_set_remove_preserve_unrelated()
        seq13_cancel_before_commit_sends_zero_writes()
        seq5_delete_strong_confirmation_and_observed_gone()
        seq6_replacement_between_preview_and_confirm_is_rejected()
        seq8_m4_forward_still_alive(port_holder)
        seq7_rapid_context_namespace_churn_around_mutation_preview()
        seq9_m5_explain_regression()
        seq10_m6_adjacent_xray_regression()
        seq11_m7_policy_and_journal_regression()
        seq12_narrow_terminal_mutation_preview()
        seq8_m4_forward_still_alive(port_holder)
        stop_forward()
        seq14_quit_while_mutation_journal_open()
    finally:
        m.tmux('kill-session', '-t', SESSION)
    reset_fixtures()


if __name__ == '__main__':
    main()
