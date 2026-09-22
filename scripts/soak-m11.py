#!/usr/bin/env python3
"""M11.8 soak: rotates through bounded, read-only Eye/Pulse/blast-radius/
bundle/context-diff churn plus selection/navigation churn against the
kind-sauron-test cluster's sauron-fixtures namespace, sampling RSS/fd/
thread counts -- "observed stability only, no leak-freedom claims", the
same honesty every soak-*.py script in this project already establishes.
Fresh binary and the explicit isolated test kubeconfig only.

Every M11 view under soak here is read-only -- unlike M8/M8B/M9/M10's own
soaks, there is no mutation cadence to throttle; the only throttling that
matters is I/O: bundle export (real filesystem writes) and context diff
(a real second network connection) run every 5th cycle, not every cycle,
so the soak measures steady-state churn rather than being dominated by
disk/connection setup cost. An M4 port-forward is held alive for the whole
run and checked every cycle, matching every other soak's own precedent of
proving a held background operation survives concurrent churn."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak11-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m11-soak-operational.toml'
)
BUNDLE_DEST = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/soak11-bundle'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 300


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak11', *args)


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


def start_forward():
    # test=m4-sessions is the declared-port fixture pod every other soak/
    # accept script in this project already uses for a held forward --
    # app=healthy has no declared containerPort at all.
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
    command('v1/pods -n sauron-fixtures')
    expect('pods [')
    return port


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak11', 'kill-server'], capture_output=True)
    subprocess.run(['rm', '-rf', str(BUNDLE_DEST)], check=False)
    run('bash', 'scripts/test-cluster.sh', 'check')
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    # Every M11 view soaked here is read-only; the only reason this needs a
    # non-default config at all is that M4's own port-forward (held alive
    # as a background-churn witness, matching every other soak's
    # precedent) requires readonly=false -- nothing M11 added changes that.
    OPERATIONAL.write_text('readonly = false\n[contexts.kind-sauron-test]\nreadonly = false\n')
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--context kind-sauron-test -n sauron-fixtures v1/pods')
    expect('pods [', 'list synchronized')
    print('soak: session up')
    forward_port = start_forward()
    print(f'soak: M4 forward held on 127.0.0.1:{forward_port}')

    stats = dict(
        cycle=0, eye_opens=0, pulse_opens=0, blast_radius_opens=0,
        bundle_exports=0, context_diffs=0, selection_cycles=0,
        reconnects=0, transient_errors=0, forward_checks=0, forward_failures=0,
    )
    samples = []
    start = time.monotonic()

    def step(name, fn):
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

        def eye_step():
            command('v1/pods -n sauron-fixtures')
            expect('pods [')
            command('eye')
            expect('EYE:')
            keys('Down')
            keys('Up')
            keys('Escape')
            expect('pods [', absent=('EYE:',))
            stats['eye_opens'] += 1

        def pulse_step():
            command('pulse')
            expect('PULSE:')
            keys('Escape')
            expect('pods [', absent=('PULSE:',))
            stats['pulse_opens'] += 1

        def selection_step():
            keys('V')
            expect('total)')
            keys('i')
            keys('C')
            expect('pods [')
            stats['selection_cycles'] += 1

        def blast_radius_step():
            command('deployments -n sauron-fixtures / name=healthy')
            expect('[1 /')
            command('blast_radius')
            expect('BLAST RADIUS:')
            keys('Escape')
            expect('[1 /', absent=('BLAST RADIUS:',))
            command('v1/pods -n sauron-fixtures')
            expect('pods [')
            stats['blast_radius_opens'] += 1

        def bundle_step():
            if cycle % 5 != 0:
                return
            command('secrets -n sauron-fixtures / name=redaction-sentinel')
            expect('secrets [1 /')
            command(f'bundle {BUNDLE_DEST} --force')
            expect('Bundle exported to')
            keys('Escape')
            command('v1/pods -n sauron-fixtures')
            expect('pods [')
            stats['bundle_exports'] += 1

        def context_diff_step():
            if (cycle + 2) % 5 != 0:
                return
            command('deployments -n sauron-fixtures / name=healthy')
            expect('[1 /')
            command('context_diff kind-sauron-test-b')
            expect('CONTEXT DIFF:')
            keys('Escape')
            command('v1/pods -n sauron-fixtures')
            expect('pods [')
            stats['context_diffs'] += 1

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

        step('eye', eye_step)
        step('pulse', pulse_step)
        step('selection', selection_step)
        step('blast_radius', blast_radius_step)
        step('bundle', bundle_step)
        step('context_diff', context_diff_step)
        step('forward_check', forward_check_step)

        s = sample()
        if s:
            s['elapsed_s'] = elapsed
            s['cycle'] = cycle
            samples.append(s)
            if cycle % 5 == 0 or cycle == 1:
                print(f'soak: cycle {cycle} @ {elapsed}s: {s} stats={stats}')

    print(f'soak: done. duration={int(time.monotonic() - start)}s stats={stats}')
    if samples:
        print(f'soak: first sample: {samples[0]}')
        print(f'soak: last sample: {samples[-1]}')
    tmux('kill-server')
    subprocess.run(['rm', '-rf', str(BUNDLE_DEST)], check=False)


if __name__ == '__main__':
    main()
