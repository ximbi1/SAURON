#!/usr/bin/env python3
"""M9.7 combined adversarial acceptance; explicit verified isolated kind
config only, no production fallback. Two clusters are involved, exactly
as the rest of M9 already established: `kind-sauron-test` (M1-M8B, run
here only as unmodified full regression via the existing accept-*.py
scripts, proving zero drift from every M9 change to shared code such as
src/app/mod.rs, src/graph/references.rs, src/kube/mutation.rs) and the
dedicated `sauron-m9` cluster (Flux/Argo CD/Helm live evidence, guarded
by scripts/test-cluster-m9.sh's own non-heuristic Docker/API identity
check, reused unweakened).

Scope note on what is deliberately NOT re-proven interactively here:
replacement/TOCTOU rejection for Flux/Argo CD targets is already proven
at the fake-HTTP layer (tests/watch_transport.rs's
mutation_uid_mismatch_before_commit_sends_zero_mutation_request and the
M9-specific verify_flux_.../verify_argocd_... tests) and the Helm reader
has its own dedicated TOCTOU tests (fake-HTTP: helm_read_release_fails_
closed_on_uid_mismatch...; live: live_stale_uid_against_a_real_secret_
fails_closed in tests/mutation_m9_helm_live.rs) -- the underlying
mechanism (kube::mutation::commit's UID revalidation, kube::helm::
read_release's own re-verification) is identical for every kind M9
touches, so this script does not re-derive it a third time through a
terminal. M9.6 (Helm rollback/uninstall) is explicitly DEFERRED (see
docs/M9_ACCEPTANCE.md's own M9.6 write-up) -- no Helm mutation exists to
test."""
import importlib.util
import pathlib
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm9-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m9-accept-operational.toml'
)
spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

M9_CONFIG = ROOT / '.test-cluster-m9/config'
KCFG = ['--kubeconfig', str(M9_CONFIG), '--context', 'kind-sauron-m9']


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


def seq1_readonly_denies_flux_and_argocd_actions_zero_writes():
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-m9]\nreadonly = true\n')
    command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
    expect('kustomizations.kustomize', '1 /')
    command('flux_suspend')
    expect('POLICY: Deny', 'ReadonlyMode')
    keys('y')
    out = m.tmux('capture-pane', '-t', SESSION, '-p')
    assert 'COMMIT RESULT' not in out, 'a readonly denial must never commit: ' + out
    keys('Escape')
    suspend = kubectl('get', 'kustomization', 'podinfo-kustomize', '-n', 'sauron-m9', '-o',
                       'jsonpath={.spec.suspend}').strip()
    assert suspend in ('', 'false'), f'readonly denial must send zero writes: suspend={suspend!r}'

    command('applications.argoproj.io -n argocd / name=guestbook')
    expect('applications.argoproj', '1 /')
    command('argocd_sync')
    expect('POLICY: Deny', 'ReadonlyMode')
    keys('y')
    out = m.tmux('capture-pane', '-t', SESSION, '-p')
    assert 'COMMIT RESULT' not in out, 'a readonly denial must never commit: ' + out
    keys('Escape')
    print('PASS seq1: readonly denies flux_suspend/argocd_sync with zero writes', flush=True)

    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-m9]\nreadonly = false\n')
    command('reload')
    expect('Configuration reloaded')
    print('PASS seq1b: :reload picks up readonly=false for this context without relaunching', flush=True)


def seq2_flux_suspend_resume_reconcile_round_trip():
    command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
    expect('kustomizations.kustomize', '1 /')
    command('flux_suspend')
    expect('CONFIRMATION: required (press confirm once)')
    keys('y')
    expect('COMMIT RESULT: Committed')
    keys('Escape')
    suspend = kubectl('get', 'kustomization', 'podinfo-kustomize', '-n', 'sauron-m9', '-o',
                       'jsonpath={.spec.suspend}').strip()
    assert suspend == 'true', suspend

    command('flux_resume')
    expect('CONFIRMATION: required (press confirm once)')
    keys('y')
    expect('COMMIT RESULT: Committed')
    keys('Escape')
    suspend = kubectl('get', 'kustomization', 'podinfo-kustomize', '-n', 'sauron-m9', '-o',
                       'jsonpath={.spec.suspend}').strip()
    assert suspend in ('', 'false'), f'must be left un-suspended: {suspend!r}'

    command('flux_reconcile')
    expect('CONFIRMATION: required (press confirm once)')
    keys('y')
    expect('COMMIT RESULT: Committed')
    keys('Escape')
    print('PASS seq2: Flux suspend/resume/reconcile round-trips live against a real Kustomization, left un-suspended', flush=True)


def seq3_argocd_sync_refresh_rollback_round_trip():
    revision = kubectl(
        'get', 'application', 'guestbook', '-n', 'argocd', '-o',
        'jsonpath={.status.sync.revision}',
    ).strip()
    assert revision, 'guestbook must already have a synced revision'

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

    command(f'argocd_rollback {revision}')
    expect('CONFIRMATION: required (press confirm once)', revision)
    keys('y')
    expect('COMMIT RESULT: Committed')
    keys('Escape')
    print('PASS seq3: Argo CD sync/refresh/rollback round-trips live against a real Application '
          '(rollback to its own already-synced revision -- a mechanism proof, matching '
          "this project's own established Force-delete-style precedent)", flush=True)


def seq4_helm_readonly_inspection():
    command('secrets -n sauron-m9 / name=sh.helm.release.v1.demo-release.v1')
    expect('secrets [1 /')
    command('helm')
    expect('HELM RELEASE: demo-release', 'status: deployed', absent=('hunter2',))
    keys('Escape')
    print('PASS seq4: :helm decodes and renders a real release live, secret-safe', flush=True)


def seq5_crd_absence_shows_unsupported_never_healthy_or_empty():
    other_session = 'm9-crdabsent-' + uuid.uuid4().hex[:8]
    m.tmux('new-session', '-d', '-s', other_session, '-x', '180', '-y', '40')
    try:
        launch = ' '.join([
            str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
            'v1/pods',
        ])
        m.tmux('send-keys', '-t', other_session, '-l', launch)
        m.tmux('send-keys', '-t', other_session, 'Enter')

        def expect_other(*terms, timeout=20):
            deadline = time.monotonic() + timeout
            output = ''
            while time.monotonic() < deadline:
                output = m.tmux('capture-pane', '-t', other_session, '-p')
                if all(t in output for t in terms):
                    return output
                time.sleep(.08)
            raise AssertionError(f'Expected {terms}\n{output}')

        expect_other('pods [', 'list synchronized')
        m.tmux('send-keys', '-t', other_session, ':')
        m.tmux('send-keys', '-t', other_session, '-l', 'flux')
        m.tmux('send-keys', '-t', other_session, 'Enter')
        out = expect_other('STATE: Unsupported')
        assert 'Healthy' not in out and 'Available' not in out, out
        m.tmux('send-keys', '-t', other_session, 'Escape')
        m.tmux('send-keys', '-t', other_session, ':')
        m.tmux('send-keys', '-t', other_session, '-l', 'argocd')
        m.tmux('send-keys', '-t', other_session, 'Enter')
        out = expect_other('STATE: Unsupported')
        assert 'Healthy' not in out and 'Available' not in out, out
        m.tmux('send-keys', '-t', other_session, 'C-c')
        print('PASS seq5: on kind-sauron-test (no Flux/Argo CD installed), :flux and :argocd '
              'both report STATE: Unsupported, never Healthy or empty', flush=True)
    finally:
        m.tmux('kill-session', '-t', other_session)


def seq6_cancel_before_commit_sends_zero_writes():
    command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
    expect('kustomizations.kustomize', '1 /')
    command('flux_suspend')
    expect('CONFIRMATION: required (press confirm once)')
    keys('Escape')
    suspend = kubectl('get', 'kustomization', 'podinfo-kustomize', '-n', 'sauron-m9', '-o',
                       'jsonpath={.spec.suspend}').strip()
    assert suspend in ('', 'false'), \
        f'cancel before commit must send zero writes: suspend={suspend!r}'
    print('PASS seq6: Escape before commit on flux_suspend sends zero requests', flush=True)


def seq6b_never_reconciled_kustomization_shows_partial_never_healthy_or_empty():
    command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-dependent')
    expect('kustomizations.kustomize', '1 /')
    command('flux')
    out = expect('Flux: podinfo-dependent')
    assert 'observedGeneration: -1 -- never reconciled' in out, out
    assert 'suspended: true' in out, out
    assert 'sourceRef: GitRepository podinfo-missing' in out, out
    keys('Escape')
    print('PASS seq6b: a never-reconciled, missing-source Kustomization renders the -1 sentinel '
          'and an unresolvable sourceRef explicitly -- never Healthy, never empty', flush=True)


def seq7_narrow_terminal_m9_document():
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    command('kustomizations.kustomize.toolkit.fluxcd.io -n sauron-m9 / name=podinfo-kustomize')
    expect('kustomizations.kustomize')
    command('flux')
    expect('Flux: podinfo-kustomize')
    keys('Escape')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    expect('kustomizations.kustomize')
    print('PASS seq7: 32x9 with a Flux document open does not panic or corrupt the session', flush=True)


def seq8_m5_m6_m7_regression_touchpoints():
    command('v1/pods -n sauron-m9')
    expect('pods [')
    keys('X')
    expect('Explain:', timeout=30)
    keys('Escape')
    command('v1/configmaps -n sauron-m9')
    expect('configmaps [')
    keys('m')
    expect('MUTATION JOURNAL')
    keys('Escape')
    print('PASS seq8: M5 Explain/M7 mutation journal views unaffected on sauron-m9', flush=True)


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


def check_forward(port):
    check = subprocess.run(
        ['curl', '-s', '-m', '3', f'http://127.0.0.1:{port}/healthz',
         '-o', '/dev/null', '-w', '%{http_code}'],
        capture_output=True, text=True,
    )
    assert check.stdout.strip() == '200', f'forward on 127.0.0.1:{port} not responding: {check.stdout}'


def seq9_quit_while_helm_view_open():
    command('secrets -n sauron-m9 / name=sh.helm.release.v1.demo-release.v1')
    expect('secrets [1 /')
    command('helm')
    expect('HELM RELEASE: demo-release')
    keys('C-c')
    out = expect('M9_RESTORED status=0')
    assert 'M9_RESTORED status=0' in out, out
    print('PASS seq9: quit while the Helm view is open exits cleanly, exact stty restore', flush=True)


def run_m1_m8b_regression():
    print('--- Full M1-M8B regression (unmodified accept-*.py, kind-sauron-test) ---', flush=True)
    for script, extra_args in [
        ('accept-m3.py', ['filters']),
        ('accept-m4.py', []),
        ('accept-m4-forward.py', []),
        ('accept-m5.py', []),
        ('accept-m5-combined.py', []),
        ('accept-m6.py', []),
        ('accept-m7.py', []),
        ('accept-m8.py', []),
        ('accept-m8b.py', []),
    ]:
        print(f'>>> {script} {" ".join(extra_args)}', flush=True)
        subprocess.run(
            [sys.executable, str(ROOT / 'scripts' / script), *extra_args],
            cwd=ROOT, check=True, timeout=1200,
        )
    print('PASS: full M1-M8B regression green, zero drift from M9', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster-m9.sh', 'check')
    m.run('bash', 'scripts/test-cluster-m9.sh', 'flux-reset', timeout=60)
    m.run('bash', 'scripts/test-cluster-m9.sh', 'argocd-reset', timeout=60)
    m.run('cargo', 'build', '--locked', timeout=180)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-m9]\nreadonly = true\n')
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    try:
        launch = ' '.join([
            str(m.BINARY), '--kubeconfig', str(M9_CONFIG), '--context', 'kind-sauron-m9',
            '--config', str(OPERATIONAL), '--mutation-test-cluster-verified',
            '-n', 'sauron-m9', 'kustomizations.kustomize.toolkit.fluxcd.io',
        ])
        shell = 'm9_before=$(stty -g); ' + launch + '; m9_code=$?; m9_after=$(stty -g); '
        shell += 'if [ "$m9_before" = "$m9_after" ]; then echo M9_RESTORED status=$m9_code; else echo M9_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('kustomizations.kustomize', 'list synchronized', 'READ ONLY')

        seq1_readonly_denies_flux_and_argocd_actions_zero_writes()
        forward_port = start_forward()
        print(f'PASS seq9a: M4 forward on 127.0.0.1:{forward_port} started before M9 navigation', flush=True)
        seq2_flux_suspend_resume_reconcile_round_trip()
        check_forward(forward_port)
        print(f'PASS seq9b: forward on 127.0.0.1:{forward_port} still alive', flush=True)
        seq3_argocd_sync_refresh_rollback_round_trip()
        check_forward(forward_port)
        print(f'PASS seq9b: forward on 127.0.0.1:{forward_port} still alive', flush=True)
        seq4_helm_readonly_inspection()
        seq6b_never_reconciled_kustomization_shows_partial_never_healthy_or_empty()
        seq6_cancel_before_commit_sends_zero_writes()
        seq7_narrow_terminal_m9_document()
        seq8_m5_m6_m7_regression_touchpoints()
        check_forward(forward_port)
        print(f'PASS seq9b: forward on 127.0.0.1:{forward_port} still alive', flush=True)
        seq9_quit_while_helm_view_open()
    finally:
        m.tmux('kill-session', '-t', SESSION)
    seq5_crd_absence_shows_unsupported_never_healthy_or_empty()
    run_m1_m8b_regression()
    print('--- M9.7 soak (bounded, scoped down -- see scripts/soak-m9.py docstring) ---', flush=True)
    subprocess.run(
        [sys.executable, str(ROOT / 'scripts' / 'soak-m9.py'), '240'],
        cwd=ROOT, check=True, timeout=400,
    )
    print('PASS: M9.7 soak completed 240s bounded run against sauron-m9', flush=True)


if __name__ == '__main__':
    main()
