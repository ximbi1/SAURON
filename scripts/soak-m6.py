#!/usr/bin/env python3
"""M6.6 soak: metrics collector + view changes + periodic Explain/Timeline/
Adjacent/Xray, sampling RSS/fd/thread counts and metrics request cadence.
Fresh binary and explicit isolated kubeconfig only, matching every other
accept-*.py/soak-*.py script."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak6-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m6-operational.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 4500


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak6', *args)


def keys(*args):
    tmux('send-keys', '-t', SESSION, *args)


def command(text):
    keys(':')
    tmux('send-keys', '-t', SESSION, '-l', text)
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


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak6', 'kill-server'], capture_output=True)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n'
    )
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--context kind-sauron-test -n sauron-m6 apps/v1/deployments')
    expect('deployments.apps [', 'list synchronized')
    print('soak: session up')

    samples = []
    start = time.monotonic()
    cycle = 0
    reconnects = 0
    explain_checks = 0
    timeline_checks = 0
    adjacent_checks = 0
    xray_checks = 0
    first_requests = metrics_requests_started()

    while time.monotonic() - start < DURATION_SECONDS:
        cycle += 1
        elapsed = int(time.monotonic() - start)
        try:
            # Rotate scope: ns/ctx round trip, plus a metrics-supported
            # resource switch, matching the M4/M5 soak pattern.
            command('apps/v1/deployments -n sauron-m6')
            expect('deployments.apps [')
            command('ns kube-system')
            expect('ns:kube-system')
            command('ns sauron-m6')
            expect('ns:sauron-m6')
            command('v1/pods -n sauron-m6')
            expect('pods [1 / 1;')

            # Periodic Explain (M5): fresh GET + UID check + health.
            keys('X')
            expect('WHY:')
            keys('Escape')
            expect('pods [1 / 1;')
            explain_checks += 1

            # Periodic Timeline (M5): local render, no network.
            keys('T')
            expect('Timeline')
            keys('Escape')
            expect('pods [1 / 1;')
            timeline_checks += 1

            # Periodic Adjacent (M6.4): bounded single-hop relationship graph.
            keys('a')
            expect('ADJACENT')
            keys('Escape')
            expect('pods [1 / 1;')
            adjacent_checks += 1

            # Periodic Xray (M6.5): bounded 2-hop cycle-safe traversal.
            keys('x')
            expect('XRAY')
            keys('Escape')
            expect('pods [1 / 1;')
            xray_checks += 1

            command('v1/nodes')
            expect('nodes [')
            command('v1/pods -n sauron-m6')
            expect('pods [')
        except AssertionError as e:
            reconnects += 1
            print(f'soak: recoverable assertion at cycle {cycle} ({elapsed}s): {e}')
            keys('Escape')
            time.sleep(1)

        s = sample()
        if s:
            s['elapsed_s'] = elapsed
            s['cycle'] = cycle
            s['metrics_requests'] = metrics_requests_started()
            samples.append(s)
            if cycle % 5 == 0 or cycle == 1:
                print(f'soak: cycle {cycle} @ {elapsed}s: {s}')

    last_requests = metrics_requests_started()
    print(f'soak: done. cycles={cycle} explain_checks={explain_checks} '
          f'timeline_checks={timeline_checks} adjacent_checks={adjacent_checks} '
          f'xray_checks={xray_checks} reconnects={reconnects} '
          f'metrics_requests={first_requests}->{last_requests}')
    if samples:
        print(f'soak: first sample: {samples[0]}')
        print(f'soak: last sample: {samples[-1]}')
    tmux('kill-server')


if __name__ == '__main__':
    main()
