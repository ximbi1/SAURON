#!/usr/bin/env python3
"""M8B.7 soak: normal navigation, namespace switches, :policy/:mutations,
cordon+uncordon, set_image round-trip, CronJob trigger (throttled, Job
cleaned up each time), Evict (PDB-denied every cycle -- safe, no-drift;
PDB-free actually evicted and recreated on a throttle), Force delete
(throttled, disposable Pod recreated each time), Drain preview/arm/cancel
(every cycle -- never the second confirm, since kind-sauron-test is
single-node and a real drain would evict every fixture Pod at once, the
same permanent bounded limitation recorded in docs/M8B_ACCEPTANCE.md),
Adjacent/Xray/Explain/Timeline regression touchpoints, one M4 forward
held throughout. Fresh binary and explicit isolated kubeconfig only,
matching every other soak-*.py script.

Mutation cadence is deliberately conservative and self-restoring: cordon/
uncordon and set_image round-trip back to the exact same state every
cycle (real writes, but zero drift across an arbitrarily long run);
trigger/evict-free/force_delete are throttled and immediately self-heal
their disposable object, per the master prompt's own "avoid continuously
deleting/recreating" guidance and soak-m8.py's own established
precedent. Each cycle's sections have independent error handling from the
start, per soak-m8.py's own hard-won lesson (see its own journal entry)."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak8b-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m8b-soak-operational.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 4500


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak8b', *args)


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


def kubectl(*args, timeout=30):
    return subprocess.run(
        ['kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-test', *args],
        cwd=ROOT, check=True, text=True, capture_output=True, timeout=timeout,
    ).stdout


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


def ensure_pod(name, fixture_apply):
    exists = subprocess.run(
        ['kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-test',
         'get', 'pod', name, '-n', 'sauron-m8b'],
        capture_output=True, text=True,
    ).returncode == 0
    if exists:
        return True
    for attempt in range(3):
        try:
            fixture_apply()
            kubectl('wait', '--for=condition=Ready', f'pod/{name}', '-n', 'sauron-m8b',
                    '--timeout=45s', timeout=50)
            return True
        except Exception as e:
            print(f'soak: {name} recreation attempt {attempt + 1} failed: {e}')
    print(f'soak: {name} could not be recreated after 3 attempts; its scenario will be skipped this cycle')
    return False


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak8b', 'kill-server'], capture_output=True)
    run('bash', 'scripts/test-cluster.sh', 'm8b-fixtures', timeout=60)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n'
    )
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--mutation-test-cluster-verified '
         '--context kind-sauron-test -n sauron-m8b apps/v1/deployments')
    expect('deployments.apps [', 'list synchronized')
    print('soak: session up')
    forward_port = start_forward()
    print(f'soak: M4 forward held on 127.0.0.1:{forward_port}')

    stats = dict(
        cycle=0, cordon_uncordon=0, set_image_commits=0, trigger_commits=0,
        evict_denied_checks=0, evict_free_commits=0, force_delete_commits=0,
        drain_previews=0, drain_armed=0, policy_checks=0, journal_checks=0,
        adjacent_checks=0, xray_checks=0, explain_checks=0, timeline_checks=0,
        reconnects=0, transient_errors=0, forward_checks=0, forward_failures=0,
    )
    samples = []
    start = time.monotonic()
    first_requests = metrics_requests_started()

    def step(name, fn):
        """Each section of a cycle is independent: a failure here must
        never skip the rest of the cycle's checks, per soak-m8.py's own
        hard-won lesson (a bug in ITS harness once let this degrade
        silently for the rest of a run)."""
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

    while time.monotonic() - start < DURATION_SECONDS:
        stats['cycle'] += 1
        cycle = stats['cycle']
        elapsed = int(time.monotonic() - start)

        def nav_policy_journal():
            command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
            expect('deployments.apps [')
            command('ns kube-system')
            expect('ns:kube-system')
            command('ns sauron-m8b')
            expect('ns:sauron-m8b')
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

        def cordon_uncordon_step():
            # Self-restoring every cycle -- brief, immediate, matching the
            # explicit sign-off already established for M8B.1's own live
            # evidence, just repeated many times under sustained load.
            command('v1/nodes')
            expect('nodes [')
            command('cordon')
            expect('CONFIRMATION: strong -- press confirm twice to commit')
            keys('y')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('nodes [')
            command('uncordon')
            expect('CONFIRMATION: strong -- press confirm twice to commit')
            keys('y')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('nodes [')
            stats['cordon_uncordon'] += 1

        def set_image_step():
            # Round-trips to the same image within the same cycle -- no drift.
            command('apps/v1/deployments -n sauron-m8b / name=m8b-multi')
            expect('deployments.apps [')
            command('set_image web=registry.k8s.io/pause:3.10')
            expect('CHANGE:', 'web')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            expect('deployments.apps [')
            command('set_image web=registry.k8s.io/pause:3.9')
            expect('CHANGE:', 'web')
            keys('y')
            expect('COMMIT RESULT:', 'VERIFICATION:')
            keys('Escape')
            expect('deployments.apps [')
            stats['set_image_commits'] += 1

        def trigger_step():
            # Throttled: each run creates a real Job; clean it up the same
            # cycle so Jobs never accumulate across a long soak.
            if cycle % 10 != 0:
                return
            command('batch/v1/cronjobs -n sauron-m8b / name=m8b-nightly')
            expect('cronjobs.batch [')
            command('trigger')
            expect('ACTION: trigger', 'CONFIRMATION: required')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('cronjobs.batch [')
            stats['trigger_commits'] += 1
            kubectl('delete', 'job', '-n', 'sauron-m8b', '-l',
                    'sauron.io/triggered-from=m8b-nightly', '--ignore-not-found')

        def evict_step():
            # PDB-denied path: safe, no-drift, every cycle -- the Pod is
            # never actually removed. The PDB-free path genuinely evicts
            # and must self-heal, so it is throttled.
            if not ensure_pod('m8b-evict-blocked',
                              lambda: kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml')):
                return
            command('v1/pods -n sauron-m8b / name=m8b-evict-blocked')
            expect('pods [', 'list synchronized')
            command('evict')
            expect('CONFIRMATION: strong -- press confirm twice to commit')
            keys('y')
            keys('y')
            expect('COMMIT RESULT:')
            keys('Escape')
            expect('pods [')
            stats['evict_denied_checks'] += 1
            if cycle % 10 != 0:
                return
            if not ensure_pod('m8b-evict-free',
                              lambda: kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml')):
                return
            command('v1/pods -n sauron-m8b / name=m8b-evict-free')
            expect('pods [', 'list synchronized')
            command('evict')
            expect('CONFIRMATION: strong -- press confirm twice to commit')
            keys('y')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('pods [')
            stats['evict_free_commits'] += 1
            ensure_pod('m8b-evict-free', lambda: kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml'))

        def force_delete_step():
            # Throttled -- genuinely removes the Pod, must self-heal.
            if cycle % 15 != 0:
                return
            if not ensure_pod('m8b-force-delete',
                              lambda: kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml')):
                return
            command('v1/pods -n sauron-m8b / name=m8b-force-delete')
            expect('pods [', 'list synchronized')
            command('force_delete')
            expect('CONFIRMATION: strong -- press confirm twice to commit')
            keys('y')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('pods [')
            stats['force_delete_commits'] += 1
            ensure_pod('m8b-force-delete', lambda: kubectl('apply', '-f', 'tests/fixtures/m8b-evict.yaml'))

        def drain_step():
            # Preview + arm + cancel ONLY, every cycle -- the second
            # confirm that would run the real orchestrator is never
            # pressed; see this script's own docstring for why.
            command('v1/nodes')
            expect('nodes [')
            command('drain')
            expect('TARGET NODE', 'CORDON STEP')
            expect('PODS PLANNED FOR EVICTION', timeout=15)
            keys('G')
            expect('CONFIRMATION: strong -- press confirm twice to start draining')
            stats['drain_previews'] += 1
            keys('y')
            expect('CONFIRMATION: strong -- press confirm again to start draining')
            stats['drain_armed'] += 1
            keys('Escape')
            expect('nodes [')

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
        step('cordon_uncordon', cordon_uncordon_step)
        step('set_image', set_image_step)
        step('trigger', trigger_step)
        step('evict', evict_step)
        step('force_delete', force_delete_step)
        step('drain', drain_step)
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
