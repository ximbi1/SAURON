#!/usr/bin/env python3
"""M8.5 interactive TUI smoke pass -- one real terminal session (tmux),
not a headless/unit check. Explicit isolated `kind-sauron-test` only, via
`--mutation-test-cluster-verified` (this script IS the guarded harness that
independently proves cluster identity, through scripts/test-cluster.sh's
own Docker/API introspection, before ever passing that flag) plus an
operational per-context config with `readonly = false` for that context
only -- the same pattern `accept-m7.py` already uses for M4 forward/exec.

Covers: palette discoverability of :scale/:label, mutation-mode preview
rendering (TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/CONFIRMATION), cancel
before commit (zero mutation state left behind), dry-run vs commit as
distinct steps, double-press strong confirmation for delete, COMMIT
RESULT vs VERIFICATION shown as two separate lines, 32x9, and exact
terminal restoration on quit. This is NOT the full M8.6 combined
adversarial acceptance (scripts/accept-m8.py, not yet written) -- it is
the one required manual-equivalent interactive pass for M8.5 specifically.
Reuses accept-m4.py's tmux/expect helpers.
"""
import importlib.util
import pathlib
import shlex
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm8smoke-' + uuid.uuid4().hex[:8]
OPERATIONAL = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad/sauron-m8-operational.toml'
)
spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


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


def scale_cancel_then_commit_and_verify():
    command('apps/v1/deployments -n sauron-m8 / name=m8-deploy')
    expect('deployments.apps [', 'list synchronized')
    command('scale 2')
    expect('TARGET:', 'ACTION: scale', 'CHANGE:', 'replicas: 1 -> 2', 'POLICY:', 'PREFLIGHT:', 'CONFIRMATION:')
    keys('Escape')
    expect('deployments.apps [', absent=('TARGET:',))
    print('PASS: :scale preview renders TARGET/ACTION/CHANGE/POLICY/PREFLIGHT/CONFIRMATION; Escape cancels with zero residual mutation state')

    command('scale 2')
    expect('TARGET:', 'CHANGE:', 'replicas: 1 -> 2')
    keys('d')
    expect('PREFLIGHT (server dry-run): Committed')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    print('PASS: scale dry-run and commit are distinct steps; COMMIT RESULT and VERIFICATION render as two separate, correct facts')
    keys('Escape')

    command('scale 1')
    expect('CHANGE:', 'replicas: 2 -> 1')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS: scale reset back to replicas=1 through the same UX, fixture left deterministic')


def label_set_then_remove():
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [', 'list synchronized', '1 /')
    command('label smoke=set')
    expect('ACTION: label', 'CHANGE:', 'metadata.labels["smoke"] = "set"')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS: :label set renders the exact single-key change and verifies it')

    command('label smoke-')
    expect('ACTION: label', 'metadata.labels["smoke"] removed')
    keys('y')
    expect('COMMIT RESULT: Committed', 'VERIFICATION: Verified')
    keys('Escape')
    print('PASS: :label KEY- removal renders and verifies distinctly from set')


def delete_with_strong_confirmation():
    command('v1/pods -n sauron-m8 / name=m8-pod')
    expect('pods [', 'list synchronized')
    command('delete')
    expect('ACTION: delete', 'CONFIRMATION: strong -- press confirm twice to commit')
    keys('y')
    expect('CONFIRMATION: strong -- press confirm again to commit')
    keys('y')
    expect('COMMIT RESULT: Committed')
    expect('VERIFICATION:', absent=('not attempted',))
    print('PASS: delete requires a real double-press strong confirmation; a single press only arms, never commits; COMMIT RESULT/VERIFICATION both render')
    keys('Escape')


def narrow_terminal_mutation_preview():
    # 32x9 fits only a few lines of the preview at once; scrolling (not a
    # crash/corruption) is expected -- proven separately in the full-size
    # passes above. This check is specifically that a 32x9 terminal survives
    # opening a preview, scrolling it, confirming, and returning to the
    # table without panicking or corrupting the session.
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    command('v1/configmaps -n sauron-m8 / name=m8-meta')
    expect('configmaps [', '1 /')
    command('label smoke=narrow')
    expect('Label: m8-meta')
    keys(*(['j'] * 6))
    expect('CHANGE:', 'metadata.labels')
    keys('y')
    time.sleep(2)
    expect('Label: m8-meta', absent=('panicked',))
    keys('Escape')
    expect('configmaps [', '1 /')
    command('label smoke-')
    expect('Label: m8-meta')
    keys('y')
    time.sleep(2)
    expect('Label: m8-meta', absent=('panicked',))
    keys('Escape')
    expect('configmaps [', '1 /')
    m.tmux('resize-window', '-t', SESSION, '-x', '150', '-y', '36')
    print('PASS: 32x9 mutation preview open/scroll/confirm/commit/return-to-table does not panic or corrupt the session')


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm8-fixtures', timeout=60)
    m.run('cargo', 'build', '--locked', timeout=180)
    OPERATIONAL.parent.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = false\n')
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '150', '-y', '36')
    try:
        launch = shlex.join([
            str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
            '--config', str(OPERATIONAL), '--mutation-test-cluster-verified',
            '-n', 'sauron-m8', 'apps/v1/deployments',
        ])
        shell = 'm8_before=$(stty -g); ' + launch + '; m8_code=$?; m8_after=$(stty -g); '
        shell += 'if [ "$m8_before" = "$m8_after" ]; then echo M8_RESTORED status=$m8_code; else echo M8_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('deployments.apps [', 'list synchronized')

        scale_cancel_then_commit_and_verify()
        label_set_then_remove()
        delete_with_strong_confirmation()
        narrow_terminal_mutation_preview()

        # The active view still carries a "name=..." filter from the last
        # navigation; the first 'q' clears that filter (Back's documented
        # behavior), the second actually quits.
        keys('q')
        time.sleep(.3)
        keys('q')
        out = expect('M8_RESTORED status=0')
        assert 'M8_RESTORED status=0' in out, out
        print('PASS: quit from the table view exits cleanly with exact terminal (stty) restoration')
    finally:
        m.tmux('kill-session', '-t', SESSION)
    m.run('bash', 'scripts/test-cluster.sh', 'm8-reset', timeout=60)
    print('M8.5 interactive smoke pass: ALL PASS')


if __name__ == '__main__':
    main()
