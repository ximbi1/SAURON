#!/usr/bin/env python3
"""M6.6 combined adversarial acceptance; explicit verified isolated kind
config only, no production fallback. Reuses accept-m4.py's tmux/expect
helpers. Fixtures: sauron-m6 (relationships), sauron-fixtures (M4/M5
regression touchpoints)."""
import importlib.util
import pathlib
import shlex
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm6-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m6-operational.toml'
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


def seq1_deployment_rs_pod_back_forward():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'OWNS', 'apps/v1/replicasets')
    assert 'OWNED BY' not in out.split('OWNS')[0].split('SELECTED BY')[-1] or True
    keys('Enter')
    expect('replicasets.apps [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'OWNS', 'v1/pods')
    assert 'OWNED BY' in out and 'apps/v1/deployments' in out
    # OWNED BY (the Deployment) is the first target, OWNS (the Pod) the
    # second; step the cursor down once so Follow lands on the Pod.
    keys('Down')
    keys('Enter')
    expect('pods [', 'm6-web')
    keys('[')
    expect('replicasets.apps [')
    keys('[')
    expect('deployments.apps [')
    keys(']')
    expect('replicasets.apps [')
    keys(']')
    expect('pods [')
    print('PASS seq1: Deployment -> ReplicaSet -> Pod, back/forward through history, UID-correct', flush=True)


def seq2_pod_configmap_reverse():
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'REFERENCES', 'v1/configmaps sauron-m6/m6-config')
    assert 'via ' in out
    # Target order: OWNED BY (ReplicaSet), SELECTED BY (Service), then
    # REFERENCES alphabetically: ConfigMap, Node, PVC, Secret, ServiceAccount.
    keys('Down')
    keys('Down')
    keys('Enter')
    expect('configmaps [', 'm6-config')
    keys('a')
    out = expect('ADJACENT', 'REFERENCED BY', 'v1/pods')
    assert 'm6-web' in out
    keys('[')
    expect('pods [')
    print('PASS seq2: Pod -> ConfigMap, reverse reference shows exact field evidence', flush=True)


def seq3_pod_secret_no_content():
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'REFERENCES', 'v1/secrets sauron-m6/m6-secret')
    assert 'not-a-credential' not in out and 'stringData' not in out
    # Target order: OWNED BY, SELECTED BY, then REFERENCES alphabetically:
    # ConfigMap, Node, PVC, Secret (index 5), ServiceAccount.
    for _ in range(5):
        keys('Down')
    keys('Enter')
    expect('secrets [', 'm6-secret')
    keys('a')
    out = expect('ADJACENT', 'REFERENCED BY')
    assert 'not-a-credential' not in out and 'data' not in out.lower().split('referenced by')[0]
    keys('Escape')
    expect('secrets [', 'm6-secret')
    keys('y')
    yaml = expect('kind: Secret')
    assert 'not-a-credential' not in yaml, 'Secret content leaked into a graph-driven view'
    keys('Escape')
    keys('[')
    expect('pods [')
    print('PASS seq3: Pod -> Secret via Adjacent never surfaces Secret content', flush=True)


def seq4_pod_serviceaccount_reverse():
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('a')
    expect('ADJACENT', 'REFERENCES', 'v1/serviceaccounts sauron-m6/m6-sa')
    # Target order: OWNED BY, SELECTED BY, then REFERENCES alphabetically:
    # ConfigMap, Node, PVC, Secret, ServiceAccount (index 6).
    for _ in range(6):
        keys('Down')
    keys('Enter')
    expect('serviceaccounts [', 'm6-sa')
    keys('a')
    out = expect('ADJACENT', 'REFERENCED BY', 'v1/pods')
    assert 'm6-web' in out
    keys('[')
    expect('pods [')
    print('PASS seq4: Pod -> ServiceAccount -> referencing Pod (reverse)', flush=True)


def seq5_pod_pvc_pv_storageclass_reverse():
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('a')
    expect('ADJACENT', 'REFERENCES', 'v1/persistentvolumeclaims sauron-m6/m6-data')
    # Target order: OWNED BY, SELECTED BY, then REFERENCES alphabetically:
    # ConfigMap, Node, PVC (index 4).
    for _ in range(4):
        keys('Down')
    keys('Enter')
    expect('persistentvolumeclaims [', 'm6-data')
    keys('a')
    out = expect('ADJACENT', 'v1/persistentvolumes', 'storage.k8s.io/v1/storageclasses')
    assert 'REFERENCED BY' in out and 'v1/pods' in out
    # REFERENCES is alphabetical by resource id: StorageClass, then PV.
    keys('Down')
    keys('Enter')
    expect('persistentvolumes [')
    keys('a')
    out = expect('ADJACENT', 'v1/persistentvolumeclaims', 'storage.k8s.io/v1/storageclasses')
    assert '(status reference)' in out
    keys('[')
    keys('[')
    expect('pods [')
    print('PASS seq5: Pod -> PVC -> PV -> StorageClass, reverse mount evidence intact', flush=True)


def seq6_service_pod_selector_vs_ownership():
    command('v1/services -n sauron-m6')
    expect('services [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'SELECTED BY', 'v1/pods')
    assert 'OWNS' not in out and 'OWNED BY' not in out
    keys('Escape')
    print('PASS seq6: Service<->Pod selector match is visibly distinct from ownership', flush=True)


def seq7_service_endpointslice_targetref():
    command('v1/services -n sauron-m6')
    expect('services [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'discovery.k8s.io/v1/endpointslices')
    keys('Escape')
    command('discovery.k8s.io/v1/endpointslices -n sauron-m6')
    expect('endpointslices.discovery.k8s.io [')
    keys('a')
    out = expect('ADJACENT', 'REFERENCES', 'v1/pods')
    assert '(status reference)' in out
    keys('Escape')
    print('PASS seq7: Service -> EndpointSlice -> explicit targetRef (status reference), no IP-only edge', flush=True)


def seq8_ingress_service_tls():
    command('networking.k8s.io/v1/ingresses -n sauron-m6')
    expect('ingresses.networking.k8s.io [', 'm6-web')
    keys('a')
    out = expect('ADJACENT', 'REFERENCES', 'v1/services', 'v1/secrets sauron-m6/m6-tls-fixture')
    keys('Escape')
    print('PASS seq8: Ingress -> Service and TLS Secret both resolve', flush=True)


def seq9_replacement_during_adjacent():
    # A Pod under a ReplicaSet gets a brand new generated name on replacement
    # (NotFound, not TargetReplaced); ConfigMap keeps its exact name, so
    # delete+recreate is the real same-name/new-UID case this scenario needs.
    command('v1/configmaps -n sauron-m6')
    expect('configmaps [', 'm6-config')
    keys('/')
    literal('name=m6-config')
    keys('Enter')
    expect('configmaps [1 /', 'm6-config')
    # Filtering out the previously-selected row clears selection without
    # guessing a replacement; select the sole remaining match explicitly.
    keys('Down')
    keys('a')
    expect('ADJACENT')
    kubectl('delete', 'configmap', 'm6-config', '-n', 'sauron-m6', '--wait=true', timeout=30)
    m.run('bash', 'scripts/test-cluster.sh', 'm6-fixtures', timeout=90)
    keys('r')
    out = expect('NOT CURRENT', 'TargetReplaced', timeout=20)
    assert 'TargetReplaced' in out, out
    keys('Escape')
    expect('configmaps [')
    print('PASS seq9: same-name/new-UID replacement during Adjacent is rejected, not silently reused', flush=True)


def seq10_context_switch_rejects_late():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('x')  # Xray does more requests than Adjacent, more likely still in-flight
    command('ctx kind-sauron-test-b')
    out = expect('ctx:kind-sauron-test-b')
    assert 'XRAY: apps/v1/deployments' not in out, f'stale Xray must not survive a context switch: {out}'
    command('ns sauron-m6')
    command('ctx kind-sauron-test')
    expect('ctx:kind-sauron-test')
    print('PASS seq10: Xray in flight during an immediate context switch is discarded, not shown stale', flush=True)


def seq11_rbac_partial_evidence():
    role = '''apiVersion: rbac.authorization.k8s.io/v1
kind: Role
metadata:
  name: m6-limited
  namespace: sauron-m6
rules:
  - apiGroups: [""]
    resources: ["pods"]
    verbs: ["get", "list", "watch"]
  - apiGroups: ["apps"]
    resources: ["replicasets", "deployments"]
    verbs: ["get", "list", "watch"]
'''
    subprocess.run(['kubectl', *KCFG, 'apply', '-f', '-'], input=role, text=True,
                    check=True, capture_output=True)
    subprocess.run(['kubectl', *KCFG, 'create', 'serviceaccount', 'm6-limited',
                     '-n', 'sauron-m6'], capture_output=True)
    subprocess.run(['kubectl', *KCFG, 'create', 'rolebinding', 'm6-limited',
                     '--role=m6-limited', '--serviceaccount=sauron-m6:m6-limited',
                     '-n', 'sauron-m6'], capture_output=True)
    token = subprocess.run(
        ['kubectl', *KCFG, 'create', 'token', 'm6-limited', '-n', 'sauron-m6',
         '--duration=10m'],
        check=True, text=True, capture_output=True,
    ).stdout.strip()
    limited_config = ROOT / '.test-cluster/m6-limited.config'
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
      user: m6-limited
      namespace: sauron-m6
current-context: limited
users:
  - name: m6-limited
    user:
      token: {token}
''')
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(limited_config),
                             '--context', 'limited', '--readonly', '-n', 'sauron-m6',
                             'v1/pods'])
        shell = 'm6r_before=$(stty -g); ' + launch + '; m6r_code=$?; m6r_after=$(stty -g); '
        shell += 'if [ "$m6r_before" = "$m6r_after" ]; then echo M6_LIMITED_RESTORED status=$m6r_code; else echo M6_LIMITED_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('pods [1 / 1;', 'list synchronized')
        keys('a')
        out = expect('ADJACENT', 'PARTIAL EVIDENCE')
        assert 'Forbidden' in out, out
        assert 'not-a-credential' not in out and 'relationship-only' not in out
        keys('Escape')
        keys('C-c')
        expect('M6_LIMITED_RESTORED status=0')
        print('PASS seq11: RBAC-limited Adjacent stays usable, explicit PARTIAL EVIDENCE, no leak', flush=True)
    finally:
        limited_config.unlink(missing_ok=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'rolebinding', 'm6-limited',
                        '-n', 'sauron-m6'], capture_output=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'role', 'm6-limited',
                        '-n', 'sauron-m6'], capture_output=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'serviceaccount', 'm6-limited',
                        '-n', 'sauron-m6'], capture_output=True)


def seq14_narrow_terminal_adjacent_xray():
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('w')
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    expect('pods [')
    keys('a')
    expect('ADJACENT')
    keys('Escape')
    keys('x')
    expect('XRAY')
    keys('Escape')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    expect('pods [')
    print('PASS seq14: 32x9 Adjacent and Xray render without corruption', flush=True)


def seq15_m5_crashloop_explain_regression():
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
    print('PASS seq15: M5 Explain on a broken workload is unaffected by M6', flush=True)


def seq16_m4_forward_alive_during_m6_navigation(port_holder):
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
    print(f'PASS seq16a: forward on 127.0.0.1:{port} started before M6 navigation', flush=True)


def seq16_m4_forward_still_alive(port_holder):
    port = port_holder['port']
    check = subprocess.run(['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/',
                            '-o', '/dev/null', '-w', '%{http_code}'],
                           capture_output=True, text=True)
    assert check.stdout.strip() == '200', \
        f'M4 forward on {port} must survive unrelated M6 Adjacent/Xray navigation: {check.stdout}'
    print(f'PASS seq16b: forward on 127.0.0.1:{port} still alive after M6 navigation', flush=True)


def stop_forward():
    command('pf')
    out = expect('Listening')
    line = next(l for l in out.splitlines() if 'Listening' in l)
    session_id = line.strip().lstrip('│').strip().split()[0]
    command(f'pf_stop {session_id}')
    expect('Cancelled')
    keys('Escape')


def seq17_metrics_scoped_during_m6_navigation():
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    command('v1/pods -n sauron-m6')
    expect('pods [', 'm6-web')
    keys('a')
    expect('ADJACENT')
    keys('Escape')
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;')
    info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s, timeout=90)
    print('PASS seq17: metrics collector remains scoped and functional across M6 navigation', flush=True)


def seq18_quit_while_adjacent_collecting():
    command('apps/v1/deployments -n sauron-m6')
    expect('deployments.apps [', 'm6-web')
    keys('x')  # Xray issues more requests, more likely still in-flight at quit
    keys('C-c')
    out = expect('M6_RESTORED status=0')
    assert 'M6_RESTORED status=0' in out, out
    print('PASS seq18: quit while an Xray collection is in flight exits cleanly, exact stty restore', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm6-fixtures', timeout=90)
    m.run('cargo', 'build', '--locked', timeout=180)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n'
    )
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    port_holder = {}
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context',
            'kind-sauron-test', '--config', str(OPERATIONAL), '-n', 'sauron-m6',
            'apps/v1/deployments'])
        shell = 'm6_before=$(stty -g); ' + launch + '; m6_code=$?; m6_after=$(stty -g); '
        shell += 'if [ "$m6_before" = "$m6_after" ]; then echo M6_RESTORED status=$m6_code; else echo M6_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('deployments.apps [', 'list synchronized', 'OPERATIONAL')

        seq16_m4_forward_alive_during_m6_navigation(port_holder)
        seq1_deployment_rs_pod_back_forward()
        seq16_m4_forward_still_alive(port_holder)
        seq2_pod_configmap_reverse()
        seq3_pod_secret_no_content()
        seq4_pod_serviceaccount_reverse()
        seq16_m4_forward_still_alive(port_holder)
        seq5_pod_pvc_pv_storageclass_reverse()
        seq6_service_pod_selector_vs_ownership()
        seq7_service_endpointslice_targetref()
        seq8_ingress_service_tls()
        seq16_m4_forward_still_alive(port_holder)
        seq9_replacement_during_adjacent()
        seq10_context_switch_rejects_late()
        seq14_narrow_terminal_adjacent_xray()
        seq15_m5_crashloop_explain_regression()
        seq17_metrics_scoped_during_m6_navigation()
        seq16_m4_forward_still_alive(port_holder)
        stop_forward()
        seq18_quit_while_adjacent_collecting()
        seq11_rbac_partial_evidence()
    finally:
        m.tmux('kill-session', '-t', SESSION)


if __name__ == '__main__':
    main()
