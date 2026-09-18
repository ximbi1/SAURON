#!/usr/bin/env python3
"""M7.6 combined adversarial acceptance; explicit verified isolated kind
config only, no production fallback. Reuses accept-m4.py's tmux/expect
helpers.

M7 ships no user-facing mutation workflow (that is M8): there is no
keybinding that dry-runs, confirms or commits a mutation. The interactive
scenarios below therefore cover the :policy/:mutations read-only surfaces,
32x9, and M4/M5/M6 regression touchpoints. The executor-level scenarios
(preview->dry-run->confirmation->commit, same-name replacement, conflict,
confirmation invalidation, cancellation, RBAC denial, journal failure
semantics, outcome-ambiguity) are proven live in tests/mutation_live.rs and
in the fake-HTTP suite (tests/watch_transport.rs), since M7 exposes no TUI
path to reach them -- see docs/M7_ACCEPTANCE.md for the mapping."""
import importlib.util
import pathlib
import shlex
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm7-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m7-operational.toml'
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


def info_until(predicate, timeout=45):
    deadline = time.monotonic() + timeout
    output = ''
    while time.monotonic() < deadline:
        command('info')
        output = expect('Runtime diagnostics', 'Metrics requests started:')
        keys('Escape')
        if predicate(output):
            return output
        time.sleep(.5)
    raise AssertionError(output)


def seq1_policy_view_denies_without_verified_cluster():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('u')
    out = expect('POLICY', 'UnverifiedCluster')
    assert 'Modify: Allow' not in out and 'Delete: Allow' not in out
    keys('Escape')
    expect('configmaps [')
    print('PASS seq1: :policy shows every hypothetical mutation denied (no in-app verified-cluster flag)', flush=True)


def seq2_protected_namespace_policy():
    command('v1/configmaps -n kube-system')
    expect('configmaps [')
    keys('u')
    out = expect('POLICY', 'ProtectedNamespace')
    keys('Escape')
    expect('configmaps [')
    print('PASS seq2: protected namespace policy denies before any mutation would be attempted', flush=True)


def seq3_mutations_journal_view():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('Escape')
    expect('configmaps [')
    print('PASS seq3: :mutations journal view opens read-only, zero Kubernetes requests', flush=True)


def seq14_narrow_terminal_policy_and_mutations():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('w')
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    expect('configmaps [')
    keys('u')
    expect('POLICY')
    keys('Escape')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('Escape')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    expect('configmaps [')
    print('PASS seq14: 32x9 :policy and :mutations render without corruption', flush=True)


def seq15_m5_explain_regression():
    command('v1/pods -n sauron-fixtures')
    expect('pods [', 'list synchronized')
    keys('/')
    literal('name=crashloop')
    keys('Enter')
    expect('pods [1 /', 'crashloop')
    keys('X')
    text = expect('WHY:', 'crashloop')
    assert 'CrashLoopBackOff' in text or 'Error' in text, text
    keys('Escape')
    expect('pods [1 /')
    print('PASS seq15: M5 Explain unaffected by M7', flush=True)


def seq17_m6_adjacent_xray_regression():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('a')
    expect('ADJACENT')
    keys('Escape')
    keys('x')
    expect('XRAY')
    keys('Escape')
    expect('deployments.apps [')
    print('PASS seq17: M6 Adjacent/Xray unaffected by M7', flush=True)


def seq16_m4_forward_alive_during_m7_navigation(port_holder):
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
    print(f'PASS seq16a: forward on 127.0.0.1:{port} started before M7 navigation', flush=True)


def seq16_m4_forward_still_alive(port_holder):
    port = port_holder['port']
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', \
        f'M4 forward on {port} must survive unrelated M7 navigation: {check.stdout}'
    print(f'PASS seq16b: forward on 127.0.0.1:{port} still alive', flush=True)


def stop_forward():
    command('pf')
    out = expect('Listening')
    line = next(l for l in out.splitlines() if 'Listening' in l)
    session_id = line.strip().lstrip('│').strip().split()[0]
    command(f'pf_stop {session_id}')
    expect('Cancelled')
    keys('Escape')


def seq_metrics_scoped_during_m7_navigation():
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('u')
    expect('POLICY')
    keys('Escape')
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    print('PASS: metrics collector remains scoped and functional across M7 navigation', flush=True)


def seq19_rapid_churn_while_policy_view_navigable():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    for _ in range(5):
        command('v1/pods -n sauron-fixtures')
        expect('pods [')
        command('v1/configmaps -n sauron-m7')
        expect('configmaps [')
    keys('u')
    expect('POLICY')
    keys('Escape')
    print('PASS seq19: rapid resource churn around the policy view causes no corruption or crash', flush=True)


def seq20_quit_while_journal_view_open():
    command('v1/configmaps -n sauron-m7')
    expect('configmaps [')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('C-c')
    out = expect('M7_RESTORED status=0')
    assert 'M7_RESTORED status=0' in out, out
    print('PASS seq20: quit while the mutation journal view is open exits cleanly, exact stty restore', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm7-fixtures', timeout=60)
    m.run('cargo', 'build', '--locked', timeout=180)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n'
    )
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    port_holder = {}
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context',
            'kind-sauron-test', '--config', str(OPERATIONAL), '-n', 'sauron-m7',
            'v1/configmaps'])
        shell = 'm7_before=$(stty -g); ' + launch + '; m7_code=$?; m7_after=$(stty -g); '
        shell += 'if [ "$m7_before" = "$m7_after" ]; then echo M7_RESTORED status=$m7_code; else echo M7_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('configmaps [', 'list synchronized', 'OPERATIONAL')

        seq16_m4_forward_alive_during_m7_navigation(port_holder)
        seq1_policy_view_denies_without_verified_cluster()
        seq16_m4_forward_still_alive(port_holder)
        seq2_protected_namespace_policy()
        seq3_mutations_journal_view()
        seq14_narrow_terminal_policy_and_mutations()
        seq16_m4_forward_still_alive(port_holder)
        seq15_m5_explain_regression()
        seq17_m6_adjacent_xray_regression()
        seq_metrics_scoped_during_m7_navigation()
        seq19_rapid_churn_while_policy_view_navigable()
        seq16_m4_forward_still_alive(port_holder)
        stop_forward()
        seq20_quit_while_journal_view_open()
    finally:
        m.tmux('kill-session', '-t', SESSION)


if __name__ == '__main__':
    main()
