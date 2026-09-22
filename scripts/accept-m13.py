#!/usr/bin/env python3
"""M13 combined acceptance; explicit verified isolated kind config only,
no production fallback. Reuses accept-m4.py's tmux/expect/run helpers,
matching every prior milestone's own established pattern. Every scenario
already proven exhaustively at the unit layer (unknown-theme rejection,
config-resolve field-flow-through, restart persistence) is not re-derived
here -- see src/{ui,app,config,brand}.rs and docs/M13_ACCEPTANCE.md for
that mapping. This script proves what only a real terminal + real
filesystem + a real process restart can prove: live theme apply/reject/
list; that an unsaved theme change never reaches disk while an explicit
:theme_save does, with real bytes read back; that a persisted theme
survives a genuine process restart; that the ASCII banner appears on a
wide terminal, stays absent on the existing 32x9 narrow-terminal contract,
and stays hidden when banner=false even on a wide terminal; that the
standard 180x40 terminal every prior milestone's own accept-*.py script
already uses keeps rendering the plain "ctx:X" breadcrumb + banner exactly
as M13.3 shipped (never silently swapped for the info panel); and that the
full M13.5 info panel (Context/Cluster/Namespace/Resource/Objects) only
appears on a genuinely large terminal, with the real live API server URL
in the Cluster field."""
import importlib.util
import pathlib
import shutil
import tempfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]

spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def new_session(width=180, height=40):
    session = 'm13-' + uuid.uuid4().hex[:8]
    m.tmux('new-session', '-d', '-s', session, '-x', str(width), '-y', str(height))
    return session


def keys(session, *args):
    m.tmux('send-keys', '-t', session, *args)


def command(session, text):
    keys(session, ':')
    m.tmux('send-keys', '-t', session, '-l', text)
    keys(session, 'Enter')


def expect(session, *terms, absent=(), timeout=20):
    deadline = time.monotonic() + timeout
    output = ''
    while time.monotonic() < deadline:
        output = m.tmux('capture-pane', '-t', session, '-p')
        if all(t in output for t in terms) and all(t not in output for t in absent):
            return output
        time.sleep(.1)
    raise AssertionError(f'Expected {terms}, absent {absent}\n{output}')


def launch(session, xdg_config_home):
    cmd = (
        f'XDG_CONFIG_HOME={xdg_config_home} {m.BINARY} --kubeconfig {m.CONFIG} '
        f'--context kind-sauron-test -n sauron-fixtures v1/pods'
    )
    m.tmux('send-keys', '-t', session, '-l', cmd)
    keys(session, 'Enter')


def seq1_theme_apply_reject_list(xdg):
    session = new_session()
    try:
        launch(session, xdg)
        expect(session, 'pods [', 'list synchronized')
        command(session, 'theme light')
        expect(session, 'Theme set to "light"', '(not saved)')
        command(session, 'theme bogus')
        expect(session, 'Unknown theme')
        keys(session, 'Escape')
        time.sleep(.3)
        command(session, 'theme')
        expect(session, 'Themes:', '[light]')
        keys(session, 'q')
        print('PASS seq1: :theme applies live, rejects unknown explicitly, lists with active marked', flush=True)
    finally:
        m.tmux('kill-session', '-t', session)


def seq2_theme_save_persists_and_survives_restart(xdg):
    session = new_session()
    try:
        launch(session, xdg)
        expect(session, 'pods [', 'list synchronized')
        cfg_file = xdg / 'sauron' / 'config.toml'
        command(session, 'theme mono')
        expect(session, 'Theme set to "mono"')
        assert not cfg_file.exists(), 'a live-only change must never reach disk'
        command(session, 'theme_save')
        expect(session, '"mono" saved')
        assert cfg_file.exists(), ':theme_save must actually write the config file'
        text = cfg_file.read_text()
        assert 'theme = "mono"' in text, text
        keys(session, 'q')
        time.sleep(1)
        print('PASS seq2: :theme_save writes real bytes to disk, unsaved changes never do', flush=True)
    finally:
        m.tmux('kill-session', '-t', session)
    # Real process restart, second binary invocation entirely.
    session2 = new_session()
    try:
        launch(session2, xdg)
        expect(session2, 'pods [', 'list synchronized')
        command(session2, 'theme')
        expect(session2, 'Themes:', '[mono]')
        keys(session2, 'q')
        print('PASS seq2b: a persisted theme survives a real process restart', flush=True)
    finally:
        m.tmux('kill-session', '-t', session2)


def seq3_banner_wide_narrow_disabled(xdg_wide, xdg_narrow, xdg_disabled):
    session = new_session(180, 40)
    try:
        launch(session, xdg_wide)
        out = expect(session, 'pods [', 'list synchronized', 'SAUR-ON', '◉', '╭───────╮')
        assert '╭───────╮' in out
        keys(session, 'q')
        print('PASS seq3a: wide terminal (180x40) shows the ASCII banner', flush=True)
    finally:
        m.tmux('kill-session', '-t', session)

    session2 = new_session(32, 9)
    try:
        launch(session2, xdg_narrow)
        expect(session2, 'pods [', absent=('SAUR-ON', '╭───────╮'))
        keys(session2, 'q')
        print('PASS seq3b: 32x9 narrow terminal stays uncorrupted, no banner', flush=True)
    finally:
        m.tmux('kill-session', '-t', session2)

    cfgdir = xdg_disabled / 'sauron'
    cfgdir.mkdir(parents=True, exist_ok=True)
    (cfgdir / 'config.toml').write_text('banner = false\n')
    session3 = new_session(180, 40)
    try:
        launch(session3, xdg_disabled)
        expect(session3, 'pods [', 'list synchronized', absent=('SAUR-ON', '╭───────╮'))
        keys(session3, 'q')
        print('PASS seq3c: banner = false hides it even on a wide terminal', flush=True)
    finally:
        m.tmux('kill-session', '-t', session3)


def seq4_expanded_panel_only_on_a_genuinely_large_terminal(xdg_standard, xdg_large):
    # The standard 180x40 every prior milestone's own accept-*.py script
    # already uses must keep the plain M13.3 breadcrumb, never the panel --
    # this is the exact regression class found live while building M13.5.
    session = new_session(180, 40)
    try:
        launch(session, xdg_standard)
        out = expect(session, 'pods [', 'list synchronized', 'ctx:kind-sauron-test',
                     absent=('Cluster:', 'Namespace:', 'Objects:'))
        assert 'ctx:kind-sauron-test' in out and '› ns:sauron-fixtures ›' in out
        keys(session, 'q')
        print('PASS seq4a: 180x40 keeps the plain breadcrumb, never the info panel', flush=True)
    finally:
        m.tmux('kill-session', '-t', session)

    session2 = new_session(220, 50)
    try:
        launch(session2, xdg_large)
        out = expect(session2, 'pods [', 'list synchronized',
                     'Context:', 'Cluster:', 'Namespace:', 'Resource:', 'Objects:')
        assert 'https://' in out, 'Cluster field must show the real live API server URL'
        assert 'kind-sauron-test' in out
        assert 'sauron-fixtures' in out
        keys(session2, 'q')
        print('PASS seq4b: 220x50 shows the full info panel with the real server URL', flush=True)
    finally:
        m.tmux('kill-session', '-t', session2)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('cargo', 'build', '--locked', timeout=180)
    scratch = pathlib.Path(tempfile.mkdtemp(prefix='m13-accept-'))
    try:
        seq1_theme_apply_reject_list(scratch / 'seq1')
        seq2_theme_save_persists_and_survives_restart(scratch / 'seq2')
        seq3_banner_wide_narrow_disabled(
            scratch / 'seq3-wide', scratch / 'seq3-narrow', scratch / 'seq3-disabled'
        )
        seq4_expanded_panel_only_on_a_genuinely_large_terminal(
            scratch / 'seq4-standard', scratch / 'seq4-large'
        )
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


if __name__ == '__main__':
    main()
