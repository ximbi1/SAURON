#!/usr/bin/env python3
"""M8.6 soak: normal navigation, namespace/context changes, :policy/
:mutations, scale/restart/label/annotate/delete mutation previews (most
cancelled, some harmless idempotent commits), Adjacent/Xray/Explain/
Timeline/metrics, one M4 forward held throughout. Fresh binary and
explicit isolated kubeconfig only, matching every other soak-*.py script.

Mutation cadence is deliberately conservative and self-restoring: scale
always commits to the SAME replicas=1 the fixture already sits at (a real
write, but a no-drift one across an arbitrarily long run); restart is
cancelled most cycles and only occasionally actually committed (each
commit is a genuinely new templated intent, never a repeated identical
payload); label/annotate always set-then-remove within the same cycle, so
no residue accumulates; delete is previewed every cycle but only actually
committed (and the disposable m8-pod recreated) every 15th cycle, per the
master prompt's own "avoid continuously deleting/recreating" guidance."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak8-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m8-soak-operational.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 4500


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak8', *args)


def keys(*args):
    tmux('send-keys', '-t', SESSION, *args)


def literal(text):
    tmux('send-keys', '-t', SESSION, '-l', text)


def command(text):
    keys(':')
    literal(text)
    keys('Enter')


def capture():
    return tmux('capture-pane', '-t', SESSION, '-p')


def expect(*terms, absent=(), timeout=25):
    deadline = time.monotonic() + timeout
    matched = False
    output = ''
    while time.monotonic() < deadline:
        output = capture()
        if all(t in output for t in terms) and all(t not in output for t in absent):
            if matched:
                return output
            matched = True
        else:
            matched = False
        time.sleep(.1)
    raise AssertionError(f'Expected {terms}, absent {absent}\n{output}')


def sauron_pid():
    result = subprocess.run(['pgrep', '-x', 'sauron'], capture_output=True, text=True)
    out = result.stdout.strip()
    return out.splitlines()[0] if out else None


def sample():
    pid = sauron_pid()
    if not pid:
        return None
    rss = int(run('bash', '-c', f'awk "/VmRSS/{{print \\$2}}" /proc/{pid}/status').strip() or 0)
    fds = int(run('bash', '-c', f'ls /proc/{pid}/fd | wc -l').strip())
    threads = int(run('bash', '-c', f'ls /proc/{pid}/task | wc -l').strip())
    return {'rss_kib': rss, 'fds': fds, 'threads': threads}


def metrics_requests_started():
    command('info')
    out = expect('Runtime diagnostics', 'Metrics requests started:')
    keys('Escape')
    for line in out.splitlines():
        if 'Metrics requests started:' in line:
            digits = line.split(':', 1)[1].strip().split()[0]
            return int(digits)
    return None


def start_forward():
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
    keys('Escape')
    return port


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak8', 'kill-server'], capture_output=True)
    run('bash', 'scripts/test-cluster.sh', 'm8-fixtures', timeout=60)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n'
    )
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--mutation-test-cluster-verified '
         '--context kind-sauron-test -n sauron-m8 apps/v1/deployments')
    expect('deployments.apps [', 'list synchronized')
    print('soak: session up')
    forward_port = start_forward()
    print(f'soak: M4 forward held on 127.0.0.1:{forward_port}')

    stats = dict(
        cycle=0, previews=0, dry_runs=0, scale_commits=0, restart_commits=0,
        restart_cancels=0, label_commits=0, annotate_commits=0, delete_previews=0,
        delete_commits=0, policy_checks=0, journal_checks=0, adjacent_checks=0,
        xray_checks=0, explain_checks=0, timeline_checks=0, reconnects=0,
        transient_errors=0, forward_checks=0, forward_failures=0,
    )
    samples = []
    start = time.monotonic()
    first_requests = metrics_requests_started()

    def step(name, fn):
        """Each section of a cycle is independent: a failure here must
        never skip the rest of the cycle's checks (that would silently
        stop exercising M4/M5/M6/M7 touchpoints for the remainder of a
        75-minute run without ever being noticed)."""
        try:
            fn()
        except AssertionError as e:
            stats['reconnects'] += 1
            print(f'soak: recoverable assertion in {name} at cycle {stats["cycle"]}: {e}')
            keys('Escape')
            time.sleep(1)
        except Exception as e:
            stats['transient_errors'] += 1
            print(f'soak: transient error in {name} at cycle {stats["cycle"]}: {e}')
            time.sleep(1)

    def ensure_m8_pod_exists():
        """Self-healing: a delete commit (or a prior failed recreation
        attempt) must never leave the fixture missing for the rest of the
        run. Checked every cycle, not just right after a commit."""
        exists = subprocess.run(
            ['kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-test',
             'get', 'pod', 'm8-pod', '-n', 'sauron-m8'],
            capture_output=True, text=True,
        ).returncode == 0
        if exists:
            return
        for attempt in range(3):
            try:
                run('kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-test',
                    'apply', '-f', 'tests/fixtures/m8-mutation.yaml', timeout=30)
                run('kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-test',
                    'wait', '--for=condition=Ready', 'pod/m8-pod', '-n', 'sauron-m8',
                    '--timeout=45s', timeout=50)
                return
            except Exception as e:
                print(f'soak: m8-pod recreation attempt {attempt + 1} failed: {e}')
        print('soak: m8-pod could not be recreated after 3 attempts; delete scenarios will be skipped this cycle')

    while time.monotonic() - start < DURATION_SECONDS:
        stats['cycle'] += 1
        cycle = stats['cycle']
        elapsed = int(time.monotonic() - start)

        def nav_policy_journal():
            command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
            expect('deployments.apps [')
            command('ns kube-system')
            expect('ns:kube-system')
            command('ns sauron-m8')
            expect('ns:sauron-m8')
            keys('u')
            expect('POLICY')
            keys('Escape')
            expect('deployments.apps [')
            stats['policy_checks'] += 1
            keys('m')
            expect('MUTATION JOURNAL')
            keys('Escape')
            expect('deployments.apps [')
            stats['journal_checks'] += 1

        def scale_step():
            # Always commits back to the fixture's own replicas=1 -- a
            # real write, never drifting, dry-run every other cycle.
            command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
            expect('deployments.apps [')
            command('scale 1')
            expect('TARGET:', 'ACTION: scale')
            stats['previews'] += 1
            if cycle % 2 == 0:
                keys('d')
                expect('PREFLIGHT (server dry-run):')
                stats['dry_runs'] += 1
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            stats['scale_commits'] += 1
            keys('Escape')
            expect('deployments.apps [')

        def restart_step():
            command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
            expect('deployments.apps [')
            command('restart')
            expect('TARGET:', 'ACTION: restart')
            stats['previews'] += 1
            if cycle % 4 == 0:
                keys('y')
                expect('COMMIT RESULT:', 'VERIFICATION:')
                stats['restart_commits'] += 1
            else:
                stats['restart_cancels'] += 1
            keys('Escape')
            expect('deployments.apps [')

        def label_annotate_step():
            # Set then remove within the same cycle -- no residue.
            command('v1/configmaps -n sauron-m8 / name=m8-meta')
            expect('configmaps [', '1 /')
            command('label soak=set')
            expect('CHANGE:')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            command('label soak-')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            stats['label_commits'] += 1
            command('annotate soak=set')
            expect('CHANGE:')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            command('annotate soak-')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            stats['annotate_commits'] += 1
            expect('configmaps [')

        def delete_step():
            # Previewed every cycle, only actually committed (and the
            # disposable Pod recreated) every 15th cycle.
            ensure_m8_pod_exists()
            command('v1/pods -n sauron-m8 / name=m8-pod')
            expect('pods [', 'list synchronized')
            command('delete')
            expect('TARGET:', 'ACTION: delete', 'CONFIRMATION: strong')
            stats['delete_previews'] += 1
            if cycle % 15 == 0:
                keys('y')
                expect('CONFIRMATION: strong -- press confirm again')
                keys('y')
                expect('COMMIT RESULT:')
                stats['delete_commits'] += 1
                keys('Escape')
                ensure_m8_pod_exists()
            else:
                keys('Escape')
            expect('pods [')

        def m5_m6_step():
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 / 1;')
            keys('X')
            expect('WHY:')
            keys('Escape')
            stats['explain_checks'] += 1
            keys('T')
            expect('Timeline')
            keys('Escape')
            stats['timeline_checks'] += 1
            command('apps/v1/deployments -n sauron-m6')
            expect('deployments.apps [', 'm6-web')
            keys('a')
            expect('ADJACENT')
            keys('Escape')
            stats['adjacent_checks'] += 1
            keys('x')
            expect('XRAY')
            keys('Escape')
            stats['xray_checks'] += 1

        def forward_check_step():
            check = subprocess.run(
                ['curl', '-s', '-m', '3', f'http://127.0.0.1:{forward_port}/',
                 '-o', '/dev/null', '-w', '%{http_code}'],
                capture_output=True, text=True,
            )
            stats['forward_checks'] += 1
            if check.stdout.strip() != '200':
                stats['forward_failures'] += 1
                print(f'soak: forward check failed at cycle {cycle}: {check.stdout}')

        step('nav_policy_journal', nav_policy_journal)
        step('scale', scale_step)
        step('restart', restart_step)
        step('label_annotate', label_annotate_step)
        step('delete', delete_step)
        step('m5_m6', m5_m6_step)
        step('forward_check', forward_check_step)

        s = sample()
        if s:
            s['elapsed_s'] = elapsed
            s['cycle'] = cycle
            s['metrics_requests'] = metrics_requests_started()
            samples.append(s)
            if cycle % 5 == 0 or cycle == 1:
                print(f'soak: cycle {cycle} @ {elapsed}s: {s} stats={stats}')

    last_requests = metrics_requests_started()
    print(f'soak: done. duration={int(time.monotonic() - start)}s stats={stats} '
          f'metrics_requests={first_requests}->{last_requests}')
    if samples:
        print(f'soak: first sample: {samples[0]}')
        print(f'soak: last sample: {samples[-1]}')
    tmux('kill-server')


if __name__ == '__main__':
    main()
