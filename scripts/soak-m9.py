#!/usr/bin/env python3
"""M9.7 soak: rotates through bounded, self-restoring Flux/Argo CD/Helm
integration operations against the dedicated sauron-m9 cluster, sampling
RSS/fd/thread counts and metrics-request progress -- "observed stability
only, no leak-freedom claims", exactly the honesty soak-m7.py/soak-m8b.py
already established. Fresh binary and the explicit isolated M9 kubeconfig
only, matching every other soak-*.py script.

Scoped down proportionally from soak-m8b.py's own multi-hour run: M8B.7's
soak already proved the shared mutation gateway (preflight/commit/verify,
journal, policy) is stable under sustained load across many operation
kinds. This soak's own job is narrower -- prove M9's own additions
(Flux/Argo CD guarded actions, Helm's dedicated Secret-body reader) don't
regress that stability, not re-derive the gateway's own stability a
second time. A shorter default duration reflects that scoping, not a
lower bar of evidence.

Mutation cadence is self-restoring every cycle (flux_suspend/resume and
argocd_sync/refresh round-trip back to the same state), matching
soak-m8b.py's own precedent. flux_reconcile and argocd_rollback are
throttled (bump the generation/write an operation field every cycle
would be indistinguishable from a single repeated write, not a
meaningful soak signal -- same reasoning soak-m7.py's own docstring
already gives for its own throttled mutation). Helm inspection is
read-only, so it runs every cycle with no throttling concern. Each
cycle's sections have independent error handling from the start, per
soak-m8.py's own hard-won lesson."""
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'soak9-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster-m9/config'
BINARY = ROOT / 'target/debug/sauron'
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m9-soak-operational.toml'
)
DURATION_SECONDS = int(sys.argv[1]) if len(sys.argv) > 1 else 600


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-soak9', *args)


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
        ['kubectl', '--kubeconfig', str(CONFIG), '--context', 'kind-sauron-m9', *args],
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
    command('v1/pods -n sauron-m9 -l app=podinfo')
    expect('pods [')
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
    subprocess.run(['tmux', '-L', 'sauron-soak9', 'kill-server'], capture_output=True)
    run('bash', 'scripts/test-cluster-m9.sh', 'check')
    run('bash', 'scripts/test-cluster-m9.sh', 'flux-reset', timeout=60)
    run('bash', 'scripts/test-cluster-m9.sh', 'argocd-reset', timeout=60)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text(
        'readonly = true\n[contexts.kind-sauron-m9]\nreadonly = false\n'
    )
    tmux('new-session', '-d', '-s', SESSION, '-x', '160', '-y', '38',
         f'{BINARY} --kubeconfig {CONFIG} --config {OPERATIONAL} '
         '--mutation-test-cluster-verified '
         '--context kind-sauron-m9 -n sauron-m9 kustomizations.kustomize.toolkit.fluxcd.io')
    expect('kustomizations.kustomize', 'list synchronized')
    print('soak: session up')
    forward_port = start_forward()
    print(f'soak: M4 forward held on 127.0.0.1:{forward_port}')

    stats = dict(
        cycle=0, flux_status_checks=0, flux_suspend_resume=0, flux_reconciles=0,
        argocd_status_checks=0, argocd_sync_refresh=0, argocd_rollbacks=0,
        helm_checks=0, reconnects=0, transient_errors=0,
        forward_checks=0, forward_failures=0,
    )
    samples = []
    start = time.monotonic()
    first_requests = metrics_requests_started()

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

        def flux_status_step():
            command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
            expect('kustomizations.kustomize', '1 /')
            command('flux')
            expect('Flux: podinfo-kustomize')
            keys('Escape')
            expect('kustomizations.kustomize')
            stats['flux_status_checks'] += 1

        def flux_suspend_resume_step():
            command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
            expect('kustomizations.kustomize', '1 /')
            command('flux_suspend')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            command('flux_resume')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('kustomizations.kustomize')
            stats['flux_suspend_resume'] += 1

        def flux_reconcile_step():
            if cycle % 10 != 0:
                return
            command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
            expect('kustomizations.kustomize', '1 /')
            command('flux_reconcile')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('kustomizations.kustomize')
            stats['flux_reconciles'] += 1

        def argocd_status_step():
            command('applications.argoproj.io -n argocd / name=guestbook')
            expect('applications.argoproj', '1 /')
            command('argocd')
            expect('Argo CD: guestbook')
            keys('Escape')
            expect('applications.argoproj')
            stats['argocd_status_checks'] += 1

        def argocd_sync_refresh_step():
            command('applications.argoproj.io -n argocd / name=guestbook')
            expect('applications.argoproj', '1 /')
            command('argocd_sync')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            command('argocd_refresh')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('applications.argoproj')
            stats['argocd_sync_refresh'] += 1

        def argocd_rollback_step():
            if cycle % 10 != 0:
                return
            revision = kubectl(
                'get', 'application', 'guestbook', '-n', 'argocd', '-o',
                'jsonpath={.status.sync.revision}',
            ).strip()
            if not revision:
                return
            command('applications.argoproj.io -n argocd / name=guestbook')
            expect('applications.argoproj', '1 /')
            command(f'argocd_rollback {revision}')
            expect('CONFIRMATION: required (press confirm once)')
            keys('y')
            expect('COMMIT RESULT: Committed')
            keys('Escape')
            expect('applications.argoproj')
            stats['argocd_rollbacks'] += 1

        def helm_step():
            command('secrets -n sauron-m9 / name=sh.helm.release.v1.demo-release.v1')
            expect('secrets [1 /')
            command('helm')
            expect('HELM RELEASE: demo-release', absent=('hunter2',))
            keys('Escape')
            expect('secrets [')
            stats['helm_checks'] += 1

        def forward_check_step():
            check = subprocess.run(
                ['curl', '-s', '-m', '3', f'http://127.0.0.1:{forward_port}/healthz',
                 '-o', '/dev/null', '-w', '%{http_code}'],
                capture_output=True, text=True,
            )
            stats['forward_checks'] += 1
            if check.stdout.strip() != '200':
                stats['forward_failures'] += 1
                print(f'soak: forward check failed at cycle {cycle}: {check.stdout}')

        step('flux_status', flux_status_step)
        step('flux_suspend_resume', flux_suspend_resume_step)
        step('flux_reconcile', flux_reconcile_step)
        step('argocd_status', argocd_status_step)
        step('argocd_sync_refresh', argocd_sync_refresh_step)
        step('argocd_rollback', argocd_rollback_step)
        step('helm', helm_step)
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
