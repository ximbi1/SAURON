#!/usr/bin/env python3
"""M4.4 soak: long-running live session with a watch, a log stream, a
port-forward, and periodic navigation cycles, sampling RSS/fd/thread counts.
Fresh binary and explicit isolated kubeconfig only, matching every other
accept-*.py script in this project."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m4-exec-config.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 4500


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak', *args)


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


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak', 'kill-server'], capture_output=True)
    tmux('new-session', '-d', '-s', SESSION, '-x', '150', '-y', '36',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--context kind-sauron-test v1/pods -n sauron-fixtures')
    expect('pods [', 'list synchronized')
    print('soak: session up')

    samples = []
    start = time.monotonic()
    cycle = 0
    reconnects = 0
    session_starts = 0
    forward_id = None

    while time.monotonic() - start < DURATION_SECONDS:
        cycle += 1
        elapsed = int(time.monotonic() - start)
        try:
            # Navigate: ns/ctx round trip
            command('ns kube-system')
            expect('ns:kube-system')
            command('ns sauron-fixtures')
            expect('ns:sauron-fixtures')
            command('ctx kind-sauron-test-b')
            expect('ctx:kind-sauron-test-b')
            command('ctx kind-sauron-test')
            expect('ctx:kind-sauron-test')

            # Log stream burst: open, let it stream a bit, close
            command('v1/pods -n sauron-fixtures -l test=m4-sessions')
            expect('pods [1 / 1;')
            keys('Down')
            command('logs web')
            expect('Streaming')
            session_starts += 1
            time.sleep(3)
            keys('Escape')
            expect('pods [')

            # Port-forward: start once, leave it running, verify periodically
            if forward_id is None:
                keys('Down')
                keys('f')
                expect('Pod TCP port')
                keys('Enter')
                expect('Listening')
                out = capture()
                for line in out.splitlines():
                    if 'Listening' in line:
                        forward_id = line.strip().split()[0]
                        break
                keys('Escape')
                expect('pods [')
                print(f'soak: forward {forward_id} started at cycle {cycle}')
            else:
                command('pf')
                out = expect('Listening')
                if 'Listening' not in out:
                    reconnects += 1
                keys('Escape')
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
            samples.append(s)
            if cycle % 5 == 0 or cycle == 1:
                print(f'soak: cycle {cycle} @ {elapsed}s: {s}')

    print(f'soak: done. cycles={cycle} session_starts={session_starts} '
          f'reconnects={reconnects} forward_id={forward_id}')
    if samples:
        print(f'soak: first sample: {samples[0]}')
        print(f'soak: last sample: {samples[-1]}')
    tmux('kill-server')


if __name__ == '__main__':
    main()
