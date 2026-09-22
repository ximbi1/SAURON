#!/usr/bin/env python3
"""M12.8 soak: plugin churn (approve/run/timeout/cancel cycles) alongside
M11's own already-soaked Eye/Pulse/blast-radius churn and normal
navigation, plus a bounded batch of real headless `--output json`
invocations every few cycles -- "observed stability only, no
leak-freedom claims", the same honesty every soak-*.py script in this
project already establishes. Samples RSS/fd/thread counts every cycle
and the app's own `:info` active-session count periodically; at the end,
explicitly confirms via a real `pgrep`/`ps` sweep that zero plugin child
processes (matching this run's own unique markers) remain -- the exact
question M12's GroupKillGuard fix exists to answer under sustained churn,
not just a single scripted repro. Fresh binary and the explicit isolated
test kubeconfig only."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak12-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
SCRATCH = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad'
)
CONFIG_HOME = SCRATCH / f'soak12-config-{uuid.uuid4().hex[:8]}'
MARKER = uuid.uuid4().hex[:8]
NUMERIC_MARKER = str(uuid.uuid4().int)[:3]
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 240


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak12', *args)


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


def child_count(pid):
    result = subprocess.run(['pgrep', '-P', pid], capture_output=True, text=True)
    return len([l for l in result.stdout.splitlines() if l.strip()])


def sample():
    pid = sauron_pid()
    if not pid:
        return None
    rss = int(run('bash', '-c', f'awk "/VmRSS/{{print \\$2}}" /proc/{pid}/status').strip() or 0)
    fds = int(run('bash', '-c', f'ls /proc/{pid}/fd | wc -l').strip())
    threads = int(run('bash', '-c', f'ls /proc/{pid}/task | wc -l').strip())
    children = child_count(pid)
    return {'rss_kib': rss, 'fds': fds, 'threads': threads, 'children': children}


def write_config():
    (CONFIG_HOME / 'sauron').mkdir(parents=True, exist_ok=True)
    (CONFIG_HOME / 'sauron' / 'config.toml').write_text(f'''
[plugins.churn_fast]
executable = "/bin/echo"
args = ["soak-{MARKER}"]
trust = "approved"
timeout_secs = 5

[plugins.churn_timeout]
executable = "/bin/sleep"
args = ["1.5{NUMERIC_MARKER}"]
trust = "approved"
timeout_secs = 1

[plugins.churn_cancel]
executable = "/bin/sleep"
args = ["30.{NUMERIC_MARKER}"]
trust = "approved"
timeout_secs = 20
''')


def pgrep_marker(text):
    found = subprocess.run(['pgrep', '-f', text], capture_output=True, text=True)
    return [l for l in found.stdout.splitlines() if l.strip()]


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak12', 'kill-server'], capture_output=True)
    run('bash', 'scripts/test-cluster.sh', 'check')
    write_config()
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'XDG_CONFIG_HOME={CONFIG_HOME} {BINARY} --kubeconfig {CONFIG} '
         '--context kind-sauron-test -n sauron-fixtures v1/pods')
    expect('pods [', 'list synchronized')
    print('soak: session up')

    stats = dict(
        cycle=0, eye_opens=0, pulse_opens=0, blast_radius_opens=0,
        plugin_completed=0, plugin_timedout=0, plugin_cancelled=0,
        headless_invocations=0, reconnects=0, transient_errors=0,
    )
    samples = []
    info_samples = []
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
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 /')
            command('eye')
            expect('EYE:')
            keys('Escape')
            expect('pods [', absent=('EYE:',))
            stats['eye_opens'] += 1

        def pulse_step():
            command('pulse')
            expect('PULSE:')
            keys('Escape')
            expect('pods [', absent=('PULSE:',))
            stats['pulse_opens'] += 1

        def blast_radius_step():
            command('deployments -n sauron-fixtures / name=healthy')
            expect('[1 /')
            command('blast_radius')
            expect('BLAST RADIUS:')
            keys('Escape')
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 /')
            stats['blast_radius_opens'] += 1

        def plugin_churn_step():
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 /')
            command('plugin churn_fast')
            expect('PLUGIN: churn_fast', 'Exit code: 0')
            keys('Escape')
            stats['plugin_completed'] += 1
            if cycle % 3 == 0:
                command('plugin churn_timeout')
                expect('PLUGIN: churn_timeout', 'TIMED OUT')
                keys('Escape')
                stats['plugin_timedout'] += 1
            if cycle % 4 == 0:
                command('plugin churn_cancel')
                expect('Running plugin')
                keys('Escape')
                stats['plugin_cancelled'] += 1
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 /')

        def headless_step():
            if cycle % 6 != 0:
                return
            out = run(str(BINARY), '--kubeconfig', str(CONFIG), '--context',
                      'kind-sauron-test', 'v1/pods', '-n', 'sauron-fixtures',
                      '--snapshot', '--output', 'json')
            assert '"schemaVersion"' in out, out
            stats['headless_invocations'] += 1

        step('eye', eye_step)
        step('pulse', pulse_step)
        step('blast_radius', blast_radius_step)
        step('plugin_churn', plugin_churn_step)
        step('headless', headless_step)

        s = sample()
        if s:
            s['elapsed_s'] = elapsed
            s['cycle'] = cycle
            samples.append(s)
            if cycle % 5 == 0 or cycle == 1:
                print(f'soak: cycle {cycle} @ {elapsed}s: {s} stats={stats}')

        if cycle % 10 == 0:
            command('info')
            out = expect('Active sessions:')
            for line in out.splitlines():
                if 'Active sessions:' in line:
                    info_samples.append((cycle, line.strip()))
                    print(f'soak: cycle {cycle} {line.strip()}')
            keys('Escape')
            command('v1/pods -n sauron-fixtures -l app=healthy')
            expect('pods [1 /')

    print(f'soak: done. duration={int(time.monotonic() - start)}s stats={stats}')
    if samples:
        print(f'soak: first sample: {samples[0]}')
        print(f'soak: last sample: {samples[-1]}')
    if info_samples:
        print(f'soak: first info: {info_samples[0]}')
        print(f'soak: last info: {info_samples[-1]}')

    # sauron is the pane's own direct command (not wrapped in a shell), so
    # a graceful quit already ends the session/server on its own; only
    # force kill-server if the server is still up (e.g. a stuck cycle).
    keys('C-c')
    time.sleep(2)
    subprocess.run(['tmux', '-L', 'sauron-soak12', 'kill-server'], capture_output=True)
    subprocess.run(['rm', '-rf', str(CONFIG_HOME)], check=False)

    time.sleep(1)
    orphans = pgrep_marker(MARKER) + pgrep_marker(f'sleep.*{NUMERIC_MARKER}')
    print(f'soak: post-shutdown pgrep for markers {MARKER}/{NUMERIC_MARKER}: {orphans or "none"}')
    assert not orphans, f'orphan plugin process(es) survived soak shutdown: {orphans}'
    print('soak: zero orphan plugin processes after shutdown -- confirmed')


if __name__ == '__main__':
    main()
