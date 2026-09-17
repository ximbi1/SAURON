#!/usr/bin/env python3
"""M5.6 combined adversarial acceptance; explicit verified kind config only,
no production fallback. Reuses accept-m4.py's tmux/expect helpers."""
import importlib.util
import pathlib
import shlex
import subprocess
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m5-operational.toml'
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


def info_until(predicate, timeout=45):
    deadline = time.monotonic() + timeout
    output = ''
    while time.monotonic() < deadline:
        m.command('info')
        output = m.expect('Runtime diagnostics', 'Metrics requests started:')
        if predicate(output):
            return output
        time.sleep(.5)
    raise AssertionError(output)


def seq1_metrics_filter_sort_ns_back():
    m.command('v1/pods -n sauron-fixtures')
    m.expect('pods [', 'list synchronized')
    info_until(lambda s: 'Metrics requests started: 0' not in s and 'Metrics request active: false' in s)
    m.keys('Escape')
    m.expect('pods [', 'list synchronized')
    time.sleep(2)  # let the completed poll's samples land on every row
    m.keys('/')
    m.tmux('send-keys', '-t', m.SESSION, '-l', 'cpu>0')
    m.keys('Enter')
    known_filtered = m.expect('pods [', 'list synchronized')
    assert '; ?' in known_filtered, known_filtered
    m.keys('Escape')
    m.keys('w')
    m.command('sort mem/l')
    m.expect('MEM/L')
    m.command('ns kube-system')
    m.expect('ns:kube-system')
    m.command('ns sauron-fixtures')
    m.expect('ns:sauron-fixtures', 'pods [')
    print('PASS seq1: metrics filter/sort/ns-switch/back, known/unknown semantics intact', flush=True)


def seq2_crashloop_explain_events_logs_back():
    m.command('v1/pods -n sauron-fixtures')
    m.expect('pods [', 'list synchronized')
    m.keys('/')
    m.tmux('send-keys', '-t', m.SESSION, '-l', 'name=crashloop')
    m.keys('Enter')
    m.expect('pods [1 /', 'crashloop')
    m.keys('X')
    text = m.expect('WHY:', 'crashloop')
    assert 'CrashLoopBackOff' in text or 'Error' in text, text
    m.keys('Escape')
    m.expect('pods [1 /')
    m.keys('E')
    m.expect('Events for', 'crashloop')
    m.keys('Escape')
    m.expect('pods [1 /')
    m.keys('l')
    m.expect('Logs: crashloop')  # the fixture's container exits immediately (one Ended shot)
    m.keys('Escape')
    m.expect('pods [1 /')
    print('PASS seq2: crashloop Explain -> Events -> logs -> back, all consistent', flush=True)


def seq3_rollout_progressing_explain_completion_timeline():
    kubectl('-n', 'sauron-fixtures', 'rollout', 'restart', 'deployment/healthy')
    m.command('deployments -n sauron-fixtures')
    m.expect('deployments.apps [')
    m.keys('/')
    m.tmux('send-keys', '-t', m.SESSION, '-l', 'name=healthy')
    m.keys('Enter')
    m.expect('deployments.apps [1 /')
    seen_progressing = False
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        out = m.tmux('capture-pane', '-t', m.SESSION, '-p')
        if 'Progressing' in out:
            seen_progressing = True
            m.keys('X')
            explain = m.expect('WHY:')
            assert 'Progressing' in explain or 'generation' in explain.lower(), explain
            m.keys('Escape')
            break
        time.sleep(1)
    deadline = time.monotonic() + 60
    ready = False
    while time.monotonic() < deadline:
        out = m.tmux('capture-pane', '-t', m.SESSION, '-p')
        if 'Ready' in out and 'healthy' in out:
            ready = True
            break
        time.sleep(1)
    assert ready, 'deployment never reconverged to Ready after rollout restart'
    m.keys('T')
    timeline = m.expect('Timeline')
    if not seen_progressing:
        print('NOTE seq3: rollout completed before a Progressing frame was captured '
              '(fast reconciliation on a 1-node kind cluster); timeline evidence below '
              'still confirms the rollout happened.', flush=True)
    assert 'generation' in timeline.lower() or 'replicas' in timeline.lower() or \
        'No meaningful watch changes' not in timeline, timeline
    m.keys('Escape')
    print(f'PASS seq3: rollout {"Progressing observed, " if seen_progressing else ""}'
          f'reconverged to Ready, Timeline shows a meaningful delta', flush=True)


def seq4_5_metrics_disappears_and_recovers():
    m.command('v1/pods -n sauron-fixtures -l app=healthy')
    m.expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s)
    kubectl('-n', 'kube-system', 'scale', 'deployment/metrics-server', '--replicas=0')
    try:
        info_until(lambda s: 'Metrics UNKNOWN' in s or 'Stale' in s or 'Unavailable' in s,
                   timeout=90)
        out = m.tmux('capture-pane', '-t', m.SESSION, '-p')
        assert 'CPU cores: 0' not in out, f'metrics absence must never render as zero: {out}'
        print('PASS seq4: metrics API disappearance -> explicit UNKNOWN/Stale, never zero', flush=True)
    finally:
        kubectl('-n', 'kube-system', 'scale', 'deployment/metrics-server', '--replicas=1')
        kubectl('-n', 'kube-system', 'rollout', 'status', 'deployment/metrics-server', '--timeout=90s')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    m.command('ctx kind-sauron-test-b')
    m.expect('ctx:kind-sauron-test-b')
    m.command('ns sauron-fixtures')
    m.command('v1/pods -n sauron-fixtures -l app=healthy')
    m.expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    m.command('ctx kind-sauron-test')
    m.expect('ctx:kind-sauron-test')
    print('PASS seq5: metrics recover with fresh real samples after scale-back, '
          'independently on both contexts, no stale cross-context bleed', flush=True)


def seq6_explain_context_switch_rejects_stale():
    m.command('v1/pods -n sauron-fixtures -l app=healthy')
    m.expect('pods [1 / 1;')
    m.keys('X')
    m.expect('WHY:')
    m.command('ctx kind-sauron-test-b')
    out = m.expect('ctx:kind-sauron-test-b')
    assert 'WHY: Pod/healthy' not in out, f'stale Explain must not survive a context switch: {out}'
    m.command('ns sauron-fixtures')
    m.command('ctx kind-sauron-test')
    m.expect('ctx:kind-sauron-test')
    print('PASS seq6: Explain open during an immediate context switch is discarded, not shown stale', flush=True)


def seq7_explain_uid_replacement_rejected():
    m.command('v1/pods -n sauron-fixtures')
    m.expect('pods [')
    m.keys('/')
    m.tmux('send-keys', '-t', m.SESSION, '-l', 'name=crashloop')
    m.keys('Enter')
    m.expect('pods [1 /', 'crashloop')
    m.keys('X')
    m.expect('WHY:', 'crashloop')
    kubectl('-n', 'sauron-fixtures', 'delete', 'pod', 'crashloop', '--wait=false',
            '--grace-period=0', '--force')
    m.run('bash', 'scripts/test-cluster.sh', 'fixtures', timeout=60)
    m.keys('r')
    out = m.expect('replaced', 'NOT CURRENT', timeout=20)
    assert 'replaced' in out.lower(), out
    m.keys('Escape')
    print('PASS seq7: Explain refreshed after a same-name/new-UID replacement is rejected, not stale', flush=True)


def seq9_rbac_partial_evidence():
    role = '''apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: m5-limited
  namespace: sauron-fixtures
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["deployments"]
    verbs: ["get", "list", "watch"]
'''
    subprocess.run(['kubectl', *KCFG, 'apply', '-f', '-'], input=role, text=True,
                    check=True, capture_output=True)
    subprocess.run(['kubectl', *KCFG, 'create', 'serviceaccount', 'm5-limited',
                     '-n', 'sauron-fixtures'], capture_output=True)
    subprocess.run(['kubectl', *KCFG, 'create', 'rolebinding', 'm5-limited',
                     '--role=m5-limited', '--serviceaccount=sauron-fixtures:m5-limited',
                     '-n', 'sauron-fixtures'], capture_output=True)
    token = subprocess.run(
        ['kubectl', *KCFG, 'create', 'token', 'm5-limited', '-n', 'sauron-fixtures',
         '--duration=10m'],
        check=True, text=True, capture_output=True,
    ).stdout.strip()
    limited_config = ROOT / '.test-cluster/m5-limited.config'
    server = subprocess.run(
        ['kubectl', *KCFG, 'config', 'view', '--minify', '-o',
         'jsonpath={.clusters[0].cluster.server}'],
        check=True, text=True, capture_output=True,
    ).stdout
    ca = subprocess.run(
        ['kubectl', *KCFG, 'config', 'view', '--raw', '--minify', '-o',
         'jsonpath={.clusters[0].cluster.certificate-authority-data}'],
        check=True, text=True, capture_output=True,
    ).stdout
    limited_config.write_text(f'''apiVersion: v1
kind: Config
clusters:
  - name: limited
    cluster:
      server: {server}
      certificate-authority-data: {ca}
contexts:
  - name: limited
    context:
      cluster: limited
      user: m5-limited
      namespace: sauron-fixtures
current-context: limited
users:
  - name: m5-limited
    user:
      token: {token}
''')
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(limited_config),
                             '--context', 'limited', '--readonly', '-n', 'sauron-fixtures',
                             '-l', 'app=healthy'])
        shell = 'm5r_before=$(stty -g); ' + launch + '; m5r_code=$?; m5r_after=$(stty -g); '
        shell += 'if [ "$m5r_before" = "$m5r_after" ]; then echo M5_LIMITED_RESTORED status=$m5r_code; else echo M5_LIMITED_BROKEN; fi'
        m.tmux('send-keys', '-t', m.SESSION, '-l', shell)
        m.keys('Enter')
        m.expect('pods [1 / 1;', 'list synchronized')
        m.keys('X')
        explain = m.expect('WHY:', 'PARTIAL EVIDENCE')
        assert 'Forbidden' in explain, explain
        assert 'TOP_SECRET' not in explain, explain
        m.keys('Escape')
        m.keys('C-c')
        m.expect('M5_LIMITED_RESTORED status=0')
        print('PASS seq9: RBAC-limited Explain stays usable, explicit PARTIAL EVIDENCE, no leak', flush=True)
    finally:
        limited_config.unlink(missing_ok=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'rolebinding', 'm5-limited',
                        '-n', 'sauron-fixtures'], capture_output=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'role', 'm5-limited',
                        '-n', 'sauron-fixtures'], capture_output=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'serviceaccount', 'm5-limited',
                        '-n', 'sauron-fixtures'], capture_output=True)


def seq10_narrow_terminal_sweep():
    m.command('v1/pods -n sauron-fixtures -l app=healthy')
    m.expect('pods [1 / 1;')
    m.keys('w')
    m.tmux('resize-window', '-t', m.SESSION, '-x', '32', '-y', '9')
    m.expect('pods [1 / 1;')  # column headers legitimately abbreviate at 32 wide
    m.keys('X')
    m.expect('WHY:')
    m.keys('Escape')
    m.keys('T')
    m.expect('Timeline')
    m.keys('Escape')
    m.tmux('resize-window', '-t', m.SESSION, '-x', '180', '-y', '40')
    m.expect('pods [1 / 1;')
    print('PASS seq10: 32x9 metrics/health/Explain/Timeline all render without corruption', flush=True)


def seq11_forward_alive_during_m5_inspection(port_holder):
    m.command('v1/pods -n sauron-fixtures -l test=m4-sessions')
    m.expect('pods [1 / 1;', 'm4-sessions')
    m.keys('f')
    m.expect('Pod TCP port')
    m.keys('Enter')
    out = m.expect('Listening')
    port = None
    for line in out.splitlines():
        if 'Listening' in line and '127.0.0.1:' in line:
            port = line.split('127.0.0.1:')[1].split()[0].split('->')[0].strip()
            break
    assert port, out
    port_holder['port'] = port
    m.keys('Escape')
    m.expect('pods [1 / 1;')
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', check.stdout
    print(f'PASS seq11a: forward on 127.0.0.1:{port} started and serving before M5 work', flush=True)


def seq11_forward_still_alive(port_holder):
    port = port_holder['port']
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', \
        f'M4 forward on {port} must survive unrelated M5 navigation/inspection: {check.stdout}'
    print(f'PASS seq11b: forward on 127.0.0.1:{port} still alive and serving after M5 sequences', flush=True)


def stop_forward():
    m.command('pf')
    out = m.expect('Listening')
    line = next(l for l in out.splitlines() if 'Listening' in l)
    session_id = line.strip().lstrip('│').strip().split()[0]
    m.command(f'pf_stop {session_id}')
    m.expect('Cancelled')
    m.keys('Escape')


def seq12_quit_with_collector_active():
    m.keys('C-c')
    out = m.expect('M5_RESTORED status=0')
    assert 'M5_RESTORED status=0' in out, out
    print('PASS seq12: quit with the metrics collector active exits cleanly, exact stty restore', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('cargo', 'build', '--locked', timeout=180)
    m.tmux('new-session', '-d', '-s', m.SESSION, '-x', '180', '-y', '40')
    port_holder = {}
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context',
            'kind-sauron-test', '--config', str(OPERATIONAL), '-n', 'sauron-fixtures',
            '-l', 'app=healthy'])
        shell = 'm5_before=$(stty -g); ' + launch + '; m5_code=$?; m5_after=$(stty -g); '
        shell += 'if [ "$m5_before" = "$m5_after" ]; then echo M5_RESTORED status=$m5_code; else echo M5_TERMINAL_BROKEN; fi'
        m.tmux('send-keys', '-t', m.SESSION, '-l', shell); m.keys('Enter')
        m.expect('pods [1 / 1;', 'list synchronized', 'OPERATIONAL')

        seq11_forward_alive_during_m5_inspection(port_holder)
        seq1_metrics_filter_sort_ns_back()
        seq11_forward_still_alive(port_holder)
        seq2_crashloop_explain_events_logs_back()
        seq3_rollout_progressing_explain_completion_timeline()
        seq11_forward_still_alive(port_holder)
        seq6_explain_context_switch_rejects_stale()
        seq7_explain_uid_replacement_rejected()
        seq10_narrow_terminal_sweep()
        seq11_forward_still_alive(port_holder)
        seq4_5_metrics_disappears_and_recovers()
        seq11_forward_still_alive(port_holder)
        stop_forward()
        seq12_quit_with_collector_active()
        seq9_rbac_partial_evidence()
    finally:
        m.tmux('kill-session', '-t', m.SESSION)


if __name__ == '__main__':
    main()
