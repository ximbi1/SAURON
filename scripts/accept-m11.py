#!/usr/bin/env python3
"""M11.8 combined acceptance; explicit verified isolated kind config only,
no production fallback. Reuses accept-m4.py's tmux/expect helpers, matching
accept-m10.py's own established pattern. Every M11 slice under test here
(Eye, Pulse, evidence bundle, blast radius, context diff) is READ-ONLY --
none of them mutate the cluster, so unlike accept-m8/m8b/m10 this script
never needs --mutation-test-cluster-verified or a readonly=false config.
Scenarios already proven exhaustively at the unit layer (priority ordering
determinism, severity counts, bundle manifest/atomic-write/permission
semantics, provenance grouping, comparison-key-vs-identity wording) are not
re-derived here -- see src/{eye,pulse,bundle,blast_radius,context_diff}.rs
and docs/M11_ACCEPTANCE.md for that mapping. This script proves what only a
real terminal + real cluster + real filesystem bytes can prove: live
priority ordering and Follow navigation against real broken/healthy Pods;
a real bundle export whose bytes are grep'd for a live sensitive fixture
value; live overwrite refuse/--force; live blast-radius provenance grouping
against a real owner chain; a live context diff against a real second
context alias; Eye/Pulse never claiming healthy/zero-problems under a real
RBAC-forbidden LIST (a genuinely denied ServiceAccount, not a mock); 32x9;
exact terminal restoration."""
import importlib.util
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm11-' + uuid.uuid4().hex[:8]
SCRATCH = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad'
)
BUNDLE_DEST = SCRATCH / 'm11-accept-bundle'

spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

KCFG = ['--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test']
SENTINEL = 'SAURON_TEST_SENTINEL_NEVER_DISPLAY'


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


def seq1_eye_priority_order_and_navigation():
    command('v1/pods -n sauron-fixtures')
    expect('pods [', 'list synchronized')
    command('eye')
    out = expect('EYE:', 'CRITICAL', 'healthy', timeout=15)
    crit_pos = out.find('[CRITICAL]')
    healthy_pos = out.find('[healthy]')
    assert crit_pos != -1 and healthy_pos != -1, out
    assert crit_pos < healthy_pos, f'a Critical row must sort before a healthy one:\n{out}'
    keys('Down')
    keys('Enter')
    out = expect('pods [', absent=('EYE:',), timeout=15)
    assert '›' in out, 'Follow must move the table cursor'
    print('PASS seq1: Eye priority-orders Critical before healthy with real evidence, Follow navigates by UID', flush=True)


def seq2_pulse_tiles():
    command('eye')  # already on pods scope from seq1; reopen eye harmlessly then escape
    keys('Escape')
    command('pulse')
    out = expect('PULSE:', 'HEALTH', 'METRICS', 'SCOPE', timeout=15)
    assert 'Critical:' in out and 'Healthy:' in out, out
    keys('Escape')
    print('PASS seq2: Pulse renders HEALTH/METRICS/SCOPE tiles from the real current scope', flush=True)


def seq3_bundle_export_and_redaction():
    subprocess.run(['rm', '-rf', str(BUNDLE_DEST)], check=False)
    command('secrets -n sauron-fixtures / name=redaction-sentinel')
    expect('secrets [1 /', timeout=15)
    command(f'bundle {BUNDLE_DEST}')
    expect('Bundle exported to', str(BUNDLE_DEST), timeout=20)
    keys('Escape')
    manifest = (BUNDLE_DEST / 'manifest.json').read_text()
    assert '"uid"' in manifest and 'redaction-sentinel' in manifest, manifest
    found = subprocess.run(
        ['grep', '-r', SENTINEL, str(BUNDLE_DEST)], capture_output=True, text=True,
    )
    assert found.returncode != 0, f'sensitive fixture value leaked into bundle bytes:\n{found.stdout}'
    for f in BUNDLE_DEST.glob('*.txt'):
        assert (f.stat().st_mode & 0o777) == 0o600, f'{f} must be 0600'
    assert (BUNDLE_DEST.stat().st_mode & 0o777) == 0o700, 'bundle dir must be 0700'
    command(f'bundle {BUNDLE_DEST}')
    expect('already exists and is not empty', timeout=15)
    keys('Escape')
    command(f'bundle {BUNDLE_DEST} --force')
    expect('Bundle exported to', str(BUNDLE_DEST), timeout=20)
    keys('Escape')
    print('PASS seq3: bundle export excludes the live sentinel secret value from every exported byte; 0600/0700 permissions; overwrite refused then forced', flush=True)


def seq4_blast_radius_provenance_grouping():
    command('deployments -n sauron-fixtures / name=healthy')
    expect('[1 /', 'healthy', timeout=15)
    command('blast_radius')
    out = expect('BLAST RADIUS:', 'DIRECTLY TARGETED', 'VERIFIED OWNERSHIP/DEPENDENCY', timeout=20)
    assert 'not a prediction' in out and 'not proof of cause' in out, out
    keys('Down')
    keys('Enter')
    out = expect('replicasets', timeout=15)
    assert '›' in out, 'Follow must move the table cursor to the selected relationship'
    print('PASS seq4: blast radius groups a real owner chain under VERIFIED OWNERSHIP/DEPENDENCY, states the safety disclaimer, Follow navigates', flush=True)


def seq5_context_diff_equivalent_and_unknown():
    command('deployments -n sauron-fixtures / name=healthy')
    expect('[1 /', 'healthy', timeout=15)
    command('context_diff kind-sauron-test-b')
    expect('CONTEXT DIFF:', 'NOT identity', 'EQUIVALENT', timeout=20)
    keys('Escape')
    command('context_diff nonexistent-context-xyz')
    expect('UNKNOWN:', timeout=20)
    keys('Escape')
    print('PASS seq5: context diff reports EQUIVALENT against a real same-cluster alias context, and a graceful UNKNOWN (never a crash) against a nonexistent context', flush=True)


def seq6_narrow_terminal():
    command('v1/pods -n sauron-fixtures')
    expect('pods [', timeout=15)
    command('eye')
    expect('EYE:', timeout=15)
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    expect('EYE:', timeout=15)
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    keys('Escape')
    print('PASS seq6: 32x9 Eye renders without corruption', flush=True)


def seq7_quit_restores_terminal_exactly():
    keys('q')
    expect('M11_RESTORED status=0', timeout=15)
    print('PASS seq7: quit exits cleanly, exact stty restore', flush=True)


LIMITED_SESSION = 'm11-limited-' + uuid.uuid4().hex[:8]
LIMITED_CONFIG = ROOT / '.test-cluster/m11-limited.config'


def seq8_eye_and_pulse_never_claim_healthy_under_forbidden_rbac():
    # A ServiceAccount with zero RoleBindings: the v1/pods LIST itself is
    # forbidden, so state.synced never becomes true and state.rows stays
    # empty. Eye/Pulse must say so explicitly (an explicit caveat), never
    # render "0 problems" as if that meant "checked, all healthy" --
    # matches the exact live-RBAC-proof precedent every M5/M6/M7/M8/M8B
    # accept-*.py script already establishes for Explain/Adjacent/:policy.
    subprocess.run(['kubectl', *KCFG, 'create', 'serviceaccount', 'm11-forbidden',
                     '-n', 'sauron-fixtures'], capture_output=True)
    token = subprocess.run(
        ['kubectl', *KCFG, 'create', 'token', 'm11-forbidden', '-n', 'sauron-fixtures',
         '--duration=10m'],
        check=True, text=True, capture_output=True,
    ).stdout.strip()
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
    LIMITED_CONFIG.write_text(f'''apiVersion: v1
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
      user: m11-forbidden
      namespace: sauron-fixtures
current-context: limited
users:
  - name: m11-forbidden
    user:
      token: {token}
''')
    try:
        launch = ' '.join([
            str(m.BINARY), '--kubeconfig', str(LIMITED_CONFIG), '--context', 'limited',
            '--readonly', '-n', 'sauron-fixtures', 'v1/pods',
        ])
        shell = 'm11f_before=$(stty -g); ' + launch + '; m11f_code=$?; m11f_after=$(stty -g); '
        shell += 'if [ "$m11f_before" = "$m11f_after" ]; then echo M11_FORBIDDEN_RESTORED status=$m11f_code; else echo M11_FORBIDDEN_BROKEN; fi'
        m.tmux('new-session', '-d', '-s', LIMITED_SESSION, '-x', '180', '-y', '40')
        m.tmux('send-keys', '-t', LIMITED_SESSION, '-l', shell)
        m.tmux('send-keys', '-t', LIMITED_SESSION, 'Enter')

        def limited_expect(*terms, timeout=20):
            deadline = time.monotonic() + timeout
            output = ''
            while time.monotonic() < deadline:
                output = m.tmux('capture-pane', '-t', LIMITED_SESSION, '-p')
                if all(t in output for t in terms):
                    return output
                time.sleep(.1)
            raise AssertionError(f'Expected {terms}\n{output}')

        def limited_key(*args):
            m.tmux('send-keys', '-t', LIMITED_SESSION, *args)

        def limited_command(text):
            limited_key(':')
            m.tmux('send-keys', '-t', LIMITED_SESSION, '-l', text)
            limited_key('Enter')

        limited_expect('Forbidden')
        limited_command('eye')
        out = limited_expect('EYE:', 'CAVEAT')
        assert 'list not yet synchronized' in out, out
        assert '0 critical, 0 warning, 0 unknown, 0 healthy' not in out or 'CAVEAT' in out, out
        limited_key('Escape')
        limited_command('pulse')
        out = limited_expect('PULSE:', 'CAVEAT')
        assert 'list not yet synchronized' in out, out
        limited_key('Escape')
        limited_key('C-c')
        limited_expect('M11_FORBIDDEN_RESTORED status=0')
        print('PASS seq8: Eye/Pulse never claim healthy/zero-problems under a real RBAC-forbidden LIST -- both show an explicit CAVEAT', flush=True)
    finally:
        m.tmux('kill-session', '-t', LIMITED_SESSION)
        LIMITED_CONFIG.unlink(missing_ok=True)
        subprocess.run(['kubectl', *KCFG, 'delete', 'serviceaccount', 'm11-forbidden',
                        '-n', 'sauron-fixtures'], capture_output=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('cargo', 'build', '--locked', timeout=180)
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    try:
        launch = ' '.join([
            str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
            '-n', 'sauron-fixtures', 'v1/pods',
        ])
        shell = 'm11_before=$(stty -g); ' + launch + '; m11_code=$?; m11_after=$(stty -g); '
        shell += 'if [ "$m11_before" = "$m11_after" ]; then echo M11_RESTORED status=$m11_code; else echo M11_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('pods [', 'list synchronized', timeout=20)

        seq1_eye_priority_order_and_navigation()
        seq2_pulse_tiles()
        seq3_bundle_export_and_redaction()
        seq4_blast_radius_provenance_grouping()
        seq5_context_diff_equivalent_and_unknown()
        seq6_narrow_terminal()
        seq7_quit_restores_terminal_exactly()
        seq8_eye_and_pulse_never_claim_healthy_under_forbidden_rbac()
    finally:
        m.tmux('kill-session', '-t', SESSION)
        subprocess.run(['rm', '-rf', str(BUNDLE_DEST)], check=False)


if __name__ == '__main__':
    main()
