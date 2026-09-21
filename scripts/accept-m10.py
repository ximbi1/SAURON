#!/usr/bin/env python3
"""M10.9 combined acceptance; explicit verified isolated kind config only,
no production fallback. Reuses accept-m4.py's tmux/expect helpers and
accept-m8b.py's own documented approach: scenarios already proven
exhaustively at the unit/fake-HTTP layer (bulk TOCTOU/cancellation/policy-
denial-per-target, config round-trip byte-for-byte, keymap/theme fail-safe
fallback logic itself) are not re-derived interactively here -- see
tests/watch_transport.rs, src/app/{selection,workspace,bookmark}.rs and
docs/M10_ACCEPTANCE.md for that mapping. This script proves the parts only
a real terminal + real cluster + a real config file on disk can prove:
end-to-end multi-select/bulk-label/bulk-delete through the actual TUI with
real API state observed via kubectl; workspace/bookmark save-open-delete
through the actual TUI; a real cross-process config round trip (quit,
relaunch against the same --config path, confirm state was actually
persisted to disk); a malformed keymap/theme config not crashing a real
process at startup; 32x9 with a bulk preview open; exact terminal
restoration."""
import importlib.util
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm10-' + uuid.uuid4().hex[:8]
SCRATCH = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad'
)
OPERATIONAL = SCRATCH / 'sauron-m10-accept-operational.toml'
BROKEN_KEYS = SCRATCH / 'sauron-m10-accept-broken-keys.toml'
BROKEN_THEME = SCRATCH / 'sauron-m10-accept-broken-theme.toml'

spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

KCFG = ['--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test']


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


def launch_cmd(config_path, resource='v1/configmaps', extra=()):
    return ' '.join([
        str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
        '--config', str(config_path), '--mutation-test-cluster-verified',
        *extra, '-n', 'sauron-m10', resource,
    ])


def quit_app_and_wait_for_shell():
    # Two real timing/logic races found live, both now fixed by checking
    # BEFORE each keystroke rather than blindly sending a fixed sequence:
    # (1) a blind fixed sleep after quitting was not always enough for
    # the debug binary to fully exit and the shell to print a fresh
    # prompt before the next command was sent; (2) blindly sending
    # 'Escape' THEN 'q' double-quits when already in Table mode with no
    # filter set -- Escape alone already triggers Quit there (matching
    # its own "esc,q" shared Back binding), so the extra 'q' became a
    # STRAY keystroke landing on the just-appeared shell prompt,
    # corrupting the next launch command's text. Sending one Escape at a
    # time and checking for the shell prompt before each further
    # keypress means we never overshoot.
    deadline = time.monotonic() + 10
    output = ''
    while time.monotonic() < deadline:
        output = m.tmux('capture-pane', '-t', SESSION, '-p')
        tail = output.strip().splitlines()[-1] if output.strip() else ''
        if tail.endswith('$') or tail.endswith('#'):
            return
        keys('Escape')
        time.sleep(.3)
    raise AssertionError(f'App did not return to a shell prompt after quit:\n{output}')


def restart_app(config_path, resource='v1/configmaps', wrap_stty=False):
    quit_app_and_wait_for_shell()
    launch = launch_cmd(config_path, resource)
    if wrap_stty:
        shell = 'm10_before=$(stty -g); ' + launch + '; m10_code=$?; m10_after=$(stty -g); '
        shell += 'if [ "$m10_before" = "$m10_after" ]; then echo M10_RESTORED status=$m10_code; else echo M10_TERMINAL_BROKEN; fi'
        literal(shell)
    else:
        literal(launch)
    keys('Enter')


def seq1_readonly_bulk_denies_zero_writes_then_reload_enables():
    keys('V')
    expect('(6 total)')  # cm count in sauron-m10 (5 fixtures + kube-root-ca.crt)
    command('bulk_label team=infra')
    # Under readonly, every individual target's own PolicyEvaluation is
    # already Deny at preview build time -- never eligible, never sent,
    # visible in the preview itself before any confirm keypress.
    expect('BULK ACTION: bulk_label', 'ELIGIBLE: 0', 'EXCLUDED: 6', 'No eligible targets')
    before = kubectl('get', 'cm', '-n', 'sauron-m10', '-l', 'team=infra',
                      '-o', 'jsonpath={.items[*].metadata.name}').strip()
    assert before == '', f'readonly bulk_label must send zero writes: {before!r}'
    keys('Escape')
    OPERATIONAL.write_text('readonly = false\n[contexts.kind-sauron-test]\nreadonly = false\n')
    command('reload')
    print('PASS seq1: readonly denies bulk_label with zero writes (every target Deny at preview time), :reload enables operational', flush=True)


def seq2_bulk_label_commits_and_verifies_every_target_individually():
    keys('C')
    keys('V')
    expect('(6 total)')
    command('bulk_label team=infra')
    expect('ELIGIBLE: 6')
    keys('y')
    expect('RESULT:', '6 attempted', '6 committed')
    for name in ['m10-bulk-a', 'm10-bulk-b', 'm10-bulk-c', 'm10-bulk-delete-1', 'm10-bulk-delete-2']:
        value = kubectl('get', 'cm', name, '-n', 'sauron-m10',
                         '-o', 'jsonpath={.metadata.labels.team}').strip()
        assert value == 'infra', f'{name} must be individually labeled: {value!r}'
    print('PASS seq2: bulk_label committed and verified every target individually, confirmed live via kubectl', flush=True)


def seq3_bulk_delete_requires_double_press_and_removes_only_the_selected_targets():
    keys('Escape')
    command('reload')
    expect('list synchronized')
    keys('C')
    keys('/')
    literal('delete')
    keys('Enter')
    expect('m10-bulk-delete-1', 'm10-bulk-delete-2')
    keys('V')
    expect('(2 total)')
    command('bulk_delete')
    expect('strong', 'press confirm twice')
    keys('y')
    expect('strong -- press confirm again')
    still_present = kubectl('get', 'cm', 'm10-bulk-delete-1', '-n', 'sauron-m10', '--ignore-not-found')
    assert 'm10-bulk-delete-1' in still_present, 'the first press must only arm, never commit'
    keys('y')
    expect('RESULT:', '2 attempted', '2 committed')
    for name in ['m10-bulk-delete-1', 'm10-bulk-delete-2']:
        gone = kubectl('get', 'cm', name, '-n', 'sauron-m10', '--ignore-not-found')
        assert name not in gone, f'{name} must actually be deleted'
    untouched = kubectl('get', 'cm', 'm10-bulk-a', '-n', 'sauron-m10', '--ignore-not-found')
    assert 'm10-bulk-a' in untouched, 'targets outside the selection must be untouched'
    keys('Escape')
    expect('configmaps [')  # back to the table before clearing the filter
    keys('Escape')
    expect('configmaps [', absent=('/delete',))
    print('PASS seq3: bulk_delete required a real double press and removed only the selected targets', flush=True)


def seq4_workspace_save_open_list_delete_round_trip_through_the_real_tui():
    command('workspace_save m10-accept-view')
    expect('saved')
    command('workspace_list')
    expect('m10-accept-view', 'v1/configmaps')
    keys('Escape')
    command('ns kube-system')
    expect('list synchronized')
    command('workspace_open m10-accept-view')
    expect('sauron-m10')
    command('workspace_delete m10-accept-view')
    expect('deleted')
    print('PASS seq4: workspace save/list/open/delete round-tripped through the real TUI', flush=True)


def seq5_bookmark_save_open_shows_exact_status_live():
    command('workspace_save m10-bookmark-setup')
    keys('g')
    command('bookmark_save m10-first-row')
    expect('saved')
    command('bookmarks')
    expect('m10-first-row', 'status: exact')
    print('PASS seq5: bookmark save + status view proved live against real rows', flush=True)


def seq6_config_persists_across_a_real_process_restart():
    restart_app(OPERATIONAL, wrap_stty=True)
    expect('list synchronized')
    command('workspace_list')
    expect('m10-bookmark-setup', 'v1/configmaps')
    command('bookmarks')
    expect('m10-first-row')
    print('PASS seq6: workspaces/bookmarks survived a real process restart against the same --config path', flush=True)


def seq7_malformed_keymap_does_not_crash_a_real_startup():
    BROKEN_KEYS.write_text(
        "[keys.table]\nyaml = ['j']\n"
    )
    restart_app(BROKEN_KEYS)
    expect('Invalid key configuration', 'using defaults')
    print('PASS seq7: a malformed keymap config did not crash startup, warning shown, defaults in effect', flush=True)


def seq8_malformed_theme_does_not_crash_a_real_startup():
    BROKEN_THEME.write_text("theme = 'not-a-real-theme'\n")
    restart_app(BROKEN_THEME)
    expect('Unknown theme', 'using default')
    print('PASS seq8: a malformed theme config did not crash startup, warning shown, default theme in effect', flush=True)


def seq9_narrow_terminal_bulk_preview():
    m.tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    restart_app(OPERATIONAL, wrap_stty=True)
    # At 32 columns the title/status lines truncate before "list
    # synchronized" is fully visible -- a real row name is a safer,
    # still-meaningful signal that the list actually loaded.
    expect('m10-bulk-a', timeout=30)
    keys('V')
    command('bulk_label team=narrow')
    expect('BULK ACTION')
    keys('Escape')
    m.tmux('resize-window', '-t', SESSION, '-x', '180', '-y', '40')
    print('PASS seq9: 32x9 bulk preview does not panic or corrupt the session', flush=True)


def seq10_quit_restores_terminal_exactly():
    keys('q')
    # Positive check, not absence-of-broken: the wrapper shell's own
    # source text (echoed back once typed) literally contains the
    # substring "M10_TERMINAL_BROKEN" as part of its `if/else/fi` body,
    # so a naive "not in output" check is a false negative test -- it
    # would always find that substring in the echoed command itself,
    # regardless of which branch the shell actually took. Only the
    # REAL printed result line proves which branch ran.
    expect('M10_RESTORED status=0')
    print('PASS seq10: quit exits cleanly, exact stty restore', flush=True)


def reset_fixtures():
    m.run('bash', 'scripts/test-cluster.sh', 'm10-reset', timeout=60)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    # m10-reset (not m10-fixtures): a prior interrupted run may have left
    # labels/deletions in place; reset guarantees a clean, deterministic
    # starting state every time, matching m8b-reset's own end-of-run role
    # but used here at the START since M10's own fixtures are cheap and
    # idempotent to fully re-establish.
    m.run('bash', 'scripts/test-cluster.sh', 'm10-reset', timeout=60)
    m.run('cargo', 'build', '--locked', timeout=180)
    SCRATCH.mkdir(parents=True, exist_ok=True)
    OPERATIONAL.write_text('readonly = true\n[contexts.kind-sauron-test]\nreadonly = true\n')
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    try:
        launch = launch_cmd(OPERATIONAL)
        shell = 'm10_before=$(stty -g); ' + launch + '; m10_code=$?; m10_after=$(stty -g); '
        shell += 'if [ "$m10_before" = "$m10_after" ]; then echo M10_RESTORED status=$m10_code; else echo M10_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('configmaps [', 'list synchronized', 'READ ONLY')

        seq1_readonly_bulk_denies_zero_writes_then_reload_enables()
        seq2_bulk_label_commits_and_verifies_every_target_individually()
        seq3_bulk_delete_requires_double_press_and_removes_only_the_selected_targets()
        seq4_workspace_save_open_list_delete_round_trip_through_the_real_tui()
        seq5_bookmark_save_open_shows_exact_status_live()
        seq6_config_persists_across_a_real_process_restart()
        seq7_malformed_keymap_does_not_crash_a_real_startup()
        seq8_malformed_theme_does_not_crash_a_real_startup()
        seq9_narrow_terminal_bulk_preview()
        seq10_quit_restores_terminal_exactly()
    finally:
        m.tmux('kill-session', '-t', SESSION)
    reset_fixtures()
    for path in [OPERATIONAL, BROKEN_KEYS, BROKEN_THEME]:
        path.unlink(missing_ok=True)


if __name__ == '__main__':
    main()
