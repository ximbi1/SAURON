#!/usr/bin/env python3
"""M12.8 combined acceptance; explicit verified isolated kind config only,
no production fallback. Reuses accept-m4.py's tmux/expect/run helpers,
matching accept-m9/m10/m11.py's own established pattern. Every scenario
proven exhaustively at the unit layer (Trust default, ENV_ALLOWLIST
composition, GroupKillGuard abort-vs-cooperative-cancel, output bounds) is
not re-derived here -- see src/plugin.rs and docs/M12_ACCEPTANCE.md for
that mapping. This script proves what only a real terminal + real
subprocess + real OS process table can prove: a real approved plugin's
real stdout/exit code; that the child's real environment genuinely never
contains a live KUBECONFIG path or a live fixture secret value the parent
shell was holding; a real timeout that leaves zero process behind; a real
mid-run cancellation (Escape) that leaves zero process behind -- the exact
scenario that surfaced the GroupKillGuard bug earlier in M12; headless
--output json schemaVersion; and that the M12.5 packaged archive still
extracts and runs."""
import importlib.util
import json
import os
import pathlib
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm12-' + uuid.uuid4().hex[:8]
SCRATCH = pathlib.Path(
    '/tmp/claude-1000/-home-ximbi-SAURON/032f118e-790a-4f49-9ef1-eafc94ee8895'
    '/scratchpad'
)
CONFIG_HOME = SCRATCH / f'm12-config-{uuid.uuid4().hex[:8]}'
MARKER = os.getpid() % 1000
SECRET = f'SAURON_TEST_M12_SECRET_{uuid.uuid4().hex}'

spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

KCFG = ['--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test']


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


def write_config():
    (CONFIG_HOME / 'sauron').mkdir(parents=True, exist_ok=True)
    (CONFIG_HOME / 'sauron' / 'config.toml').write_text(f'''
[plugins.envdump]
executable = "/usr/bin/env"
trust = "approved"
timeout_secs = 5

[plugins.sleeper]
executable = "/bin/sleep"
args = ["2.{MARKER}00"]
trust = "approved"
timeout_secs = 1

[plugins.canceller]
executable = "/bin/sleep"
args = ["60.{MARKER}"]
trust = "approved"
timeout_secs = 30
''')


def pgrep_marker(marker):
    found = subprocess.run(['pgrep', '-f', f'sleep {marker}'], capture_output=True, text=True)
    return found.stdout.strip()


def seq1_plugin_runs_end_to_end_real_stdout_and_exit_code():
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;', timeout=15)
    command('plugin envdump')
    out = expect('Plugin: envdump on', 'PLUGIN: envdump', 'Exit code: 0', timeout=15)
    assert 'PATH=' in out, out
    keys('Escape')
    print('PASS seq1: :plugin envdump runs a real approved subprocess end-to-end, real stdout captured, exit code 0', flush=True)


def seq2_no_credential_leak_into_child_env():
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;', timeout=15)
    command('plugin envdump')
    out = expect('PLUGIN: envdump', 'Exit code: 0', timeout=15)
    assert str(m.CONFIG) not in out, f'kubeconfig path leaked into child env:\n{out}'
    assert SECRET not in out, f'parent-shell secret leaked into child env:\n{out}'
    assert 'KUBECONFIG' not in out, out
    assert 'SAURON_TEST_M12_SECRET' not in out, out
    keys('Escape')
    print('PASS seq2: real child env never contains KUBECONFIG path or the live parent-shell secret -- ENV_ALLOWLIST proven live', flush=True)


def seq3_timeout_kills_the_real_process_group():
    assert not pgrep_marker(f'2.{MARKER}00'), 'stale fixture process from a prior run'
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;', timeout=15)
    command('plugin sleeper')
    out = expect('PLUGIN: sleeper', 'TIMED OUT', timeout=20)
    assert 'process group killed' in out, out
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline and pgrep_marker(f'2.{MARKER}00'):
        time.sleep(.2)
    assert not pgrep_marker(f'2.{MARKER}00'), 'timed-out plugin process survived on the real OS process table'
    keys('Escape')
    print('PASS seq3: a real timeout kills the whole process group -- zero orphan on the real OS process table', flush=True)


def seq4_cancellation_mid_run_kills_the_real_process_group():
    assert not pgrep_marker(f'60.{MARKER}'), 'stale fixture process from a prior run'
    command('v1/pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;', timeout=15)
    command('plugin canceller')
    expect('Plugin: canceller on', 'Running plugin', timeout=10)
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline and not pgrep_marker(f'60.{MARKER}'):
        time.sleep(.1)
    assert pgrep_marker(f'60.{MARKER}'), 'plugin process never actually started'
    keys('Escape')
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline and pgrep_marker(f'60.{MARKER}'):
        time.sleep(.2)
    assert not pgrep_marker(f'60.{MARKER}'), (
        'cancelling mid-run left the real subprocess running -- the exact '
        'GroupKillGuard regression found earlier this milestone'
    )
    print('PASS seq4: cancelling a plugin mid-run (Escape) kills the real process group -- zero orphan; GroupKillGuard regression stays fixed', flush=True)


def seq5_quit_with_no_lingering_plugin_processes():
    keys('C-c')
    expect('M12_TERMINAL_RESTORED status=0', timeout=15)
    for marker in (f'2.{MARKER}00', f'60.{MARKER}'):
        assert not pgrep_marker(marker), f'orphan plugin process survived app quit: {marker}'
    print('PASS seq5: quit exits cleanly, exact stty restore, zero lingering plugin processes', flush=True)


def seq6_headless_output_schema_version():
    out = m.run(
        str(m.BINARY), *KCFG, 'v1/pods', '-n', 'sauron-fixtures', '-l', 'app=healthy',
        '--snapshot', '--output', 'json',
    )
    assert '\x1b' not in out, 'ANSI escape byte in headless JSON output'
    doc = json.loads(out)
    assert doc.get('schemaVersion') == 1, doc
    print('PASS seq6: headless --output json exits 0, no ANSI bytes, schemaVersion present with value 1 (serde_json::Value has no preserve_order, so presence/value is the guarantee, not byte position)', flush=True)


def seq7_packaged_archive_still_extracts_and_runs():
    dist = ROOT / 'target' / 'dist'
    archives = sorted(dist.glob('sauron-*-x86_64-unknown-linux-gnu.tar.gz'))
    assert archives, f'no packaged archive found in {dist} -- run scripts/package.sh first (M12.5)'
    archive = archives[-1]
    stage = SCRATCH / f'm12-package-verify-{uuid.uuid4().hex[:8]}'
    stage.mkdir(parents=True)
    subprocess.run(['tar', 'xzf', str(archive)], cwd=stage, check=True)
    dirs = [p for p in stage.iterdir() if p.is_dir()]
    assert len(dirs) == 1, dirs
    extracted = dirs[0]
    for name in ('sauron', 'LICENSE-MIT', 'LICENSE-APACHE', 'INSTALL.md'):
        assert (extracted / name).exists(), f'{name} missing from packaged archive'
    version = subprocess.run(
        [str(extracted / 'sauron'), '--version'], check=True, text=True, capture_output=True,
    ).stdout
    assert 'sauron' in version, version
    offline = subprocess.run(
        [str(extracted / 'sauron'), 'info', '--offline'], check=True, text=True, capture_output=True,
    ).stdout
    assert 'Offline' in offline, offline
    subprocess.run(['rm', '-rf', str(stage)], check=False)
    print(f'PASS seq7: packaged archive {archive.name} extracts cleanly, --version and info --offline both run for real', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('cargo', 'build', '--locked', timeout=180)
    write_config()
    m.tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '40')
    try:
        launch = ' '.join([
            f'XDG_CONFIG_HOME={CONFIG_HOME}',
            f'KUBECONFIG={m.CONFIG}',
            f'SAURON_TEST_M12_SECRET={SECRET}',
            str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
            '-n', 'sauron-fixtures', 'v1/pods',
        ])
        shell = 'm12_before=$(stty -g); ' + launch + '; m12_code=$?; m12_after=$(stty -g); '
        shell += 'if [ "$m12_before" = "$m12_after" ]; then echo M12_TERMINAL_RESTORED status=$m12_code; else echo M12_TERMINAL_BROKEN; fi'
        literal(shell)
        keys('Enter')
        expect('pods [', 'list synchronized', timeout=20)

        seq1_plugin_runs_end_to_end_real_stdout_and_exit_code()
        seq2_no_credential_leak_into_child_env()
        seq3_timeout_kills_the_real_process_group()
        seq4_cancellation_mid_run_kills_the_real_process_group()
        seq5_quit_with_no_lingering_plugin_processes()
        seq6_headless_output_schema_version()
        seq7_packaged_archive_still_extracts_and_runs()
    finally:
        m.tmux('kill-session', '-t', SESSION)
        subprocess.run(['rm', '-rf', str(CONFIG_HOME)], check=False)


if __name__ == '__main__':
    main()
