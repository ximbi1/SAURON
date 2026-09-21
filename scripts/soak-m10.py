#!/usr/bin/env python3
"""M10.9 soak: rotates through bounded, self-restoring selection/bulk/
workspace/bookmark operations against the dedicated kind-sauron-test
cluster's sauron-m10 fixtures, sampling RSS/fd/thread counts --
"observed stability only, no leak-freedom claims", matching every other
soak-*.py script's own established honesty. Fresh binary and the
explicit isolated test kubeconfig only.

Scoped down proportionally from soak-m8b.py's own multi-hour run, same
reasoning soak-m9.py already gives for its own scoping: M8B.7's soak
already proved the shared mutation gateway (preflight/commit/verify,
journal, policy) is stable under sustained load. This soak's own job is
narrower -- prove M10's own additions (bounded multi-select, bulk
workflows, workspace/bookmark save-open-delete, config persistence)
don't regress that stability, not re-derive the gateway's own stability
a second time.

bulk_label is throttled (every 10 cycles, always to the same idempotent
value) for the same reason soak-m9.py throttles flux_reconcile/
argocd_rollback -- a real write every cycle would be indistinguishable
from a single repeated write, not a meaningful soak signal. Selection
toggling and workspace/bookmark save-open-delete are pure in-process
state (no Kubernetes write at all for selection; workspace/bookmark
writes go to the local config file, not the cluster), so they run every
cycle with no throttling concern. Each cycle's sections have independent
error handling from the start, per soak-m8.py's own hard-won lesson."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak10-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m10-soak-operational.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 300


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak10', *args)


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


def main():
    subprocess.run(['tmux', '-L', 'sauron-soak10', 'kill-server'], capture_output=True)
    run('bash', 'scripts/test-cluster.sh', 'check')
    run('bash', 'scripts/test-cluster.sh', 'm10-reset', timeout=60)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text('readonly = false\n[contexts.kind-sauron-test]\nreadonly = false\n')
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--mutation-test-cluster-verified '
         '--context kind-sauron-test -n sauron-m10 v1/configmaps')
    expect('configmaps [', 'list synchronized')
    print('soak: session up')

    stats = dict(
        cycle=0, selection_cycles=0, bulk_labels=0,
        workspace_round_trips=0, bookmark_round_trips=0,
        reconnects=0, transient_errors=0,
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

        def selection_step():
            keys('V')
            expect('total)')
            keys('i')
            keys('i')
            keys('C')
            expect('configmaps [')
            stats['selection_cycles'] += 1

        def bulk_label_step():
            if cycle % 10 != 0:
                return
            keys('C')
            keys('V')
            expect('total)')
            command('bulk_label team=soak')
            expect('ELIGIBLE:')
            keys('y')
            expect('RESULT:', 'committed')
            keys('Escape')
            expect('configmaps [')
            keys('C')
            value = kubectl('get', 'cm', 'm10-bulk-a', '-n', 'sauron-m10',
                             '-o', 'jsonpath={.metadata.labels.team}').strip()
            assert value == 'soak', f'm10-bulk-a must carry team=soak: {value!r}'
            stats['bulk_labels'] += 1

        def workspace_step():
            command('workspace_save soak10-ws')
            expect('saved')
            command('workspace_open soak10-ws')
            expect('configmaps [')
            command('workspace_delete soak10-ws')
            expect('deleted')
            stats['workspace_round_trips'] += 1

        def bookmark_step():
            keys('g')
            command('bookmark_save soak10-bm')
            expect('saved')
            command('bookmark_open soak10-bm')
            expect('configmaps [')
            command('bookmark_delete soak10-bm')
            expect('deleted')
            stats['bookmark_round_trips'] += 1

        step('selection', selection_step)
        step('bulk_label', bulk_label_step)
        step('workspace', workspace_step)
        step('bookmark', bookmark_step)

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
    run('bash', 'scripts/test-cluster.sh', 'm10-reset', timeout=60)


if __name__ == '__main__':
    main()
