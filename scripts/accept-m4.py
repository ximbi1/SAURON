#!/usr/bin/env python3
"""Live M4 checks, fresh binary and explicit verified isolated kubeconfig only."""
import pathlib
import shlex
import subprocess
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
SESSION = 'm4-' + uuid.uuid4().hex[:8]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'


def run(*args, timeout=40):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True,
                          timeout=timeout).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-m4', *args)


def keys(*args):
    tmux('send-keys', '-t', SESSION, *args)


def command(text):
    keys(':')
    tmux('send-keys', '-t', SESSION, '-l', text)
    keys('Enter')


def expect(*terms, absent=(), timeout=25):
    deadline = time.monotonic() + timeout
    matched = False
    while time.monotonic() < deadline:
        output = tmux('capture-pane', '-t', SESSION, '-p')
        if all(t in output for t in terms) and all(t not in output for t in absent):
            # An identical outgoing frame can still be on screen immediately after
            # send-keys; require a second observation after a render opportunity.
            if matched:
                return output
            matched = True
        else:
            matched = False
        time.sleep(.08)
    raise AssertionError(f'Expected {terms}, absent {absent}\n{output}')


def foundation():
    expect('pods [', 'list synchronized')
    # Exact live regression: Connected used to close this new palette, turning
    # the n in "info" into the namespace-picker shortcut.
    for _ in range(3):
        command('ctx kind-sauron-test-b')
        keys(':')
        time.sleep(.1)
        tmux('send-keys', '-t', SESSION, '-l', 'info')
        keys('Enter')
        expect('Runtime diagnostics', 'Context: kind-sauron-test-b')
        keys('Escape')
        command('ctx kind-sauron-test')
        expect('ctx:kind-sauron-test', 'list synchronized')
    command('pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;', 'healthy-')
    command('logs worker')
    expect('Logs:', 'healthy test workload', 'Streaming')
    keys('/')
    tmux('send-keys', '-t', SESSION, '-l', 'healthy')
    keys('Enter')
    expect('matches', 'Streaming')
    keys(':')
    keys('Escape')
    expect('Logs:', 'healthy test workload', 'Streaming')
    keys('Escape')
    expect('pods [1 / 1;')
    for _ in range(5):
        keys('l', 'Escape', 'l')
        expect('Logs:', 'healthy test workload', 'Streaming')
        keys('Escape')
        expect('pods [1 / 1;')
    command('pods -n sauron-fixtures / name=crashloop')
    expect('crashloop', 'pods [1 /')
    command('previous_logs worker')
    expect('Logs:', 'fixture: deliberate exit 1', 'Ended')
    keys('Escape')
    command('pods -n sauron-fixtures -l app=healthy')
    expect('pods [1 / 1;')
    for _ in range(3):
        command('logs worker')
        command('ns kube-system')
        expect('ns:kube-system', 'list synchronized', absent=('healthy test workload', 'Logs:'))
        command('ns sauron-fixtures')
        expect('pods [1 / 1;')
        command('logs worker')
        command('ctx kind-sauron-test-b')
        command('pods -n sauron-fixtures -l app=healthy')
        expect('ctx:kind-sauron-test-b', 'pods [1 / 1;', 'list synchronized')
        command('ctx kind-sauron-test')
        command('pods -n sauron-fixtures -l app=healthy')
        expect('ctx:kind-sauron-test', 'pods [1 / 1;', 'list synchronized')
    command('logs worker')
    expect('Streaming', 'healthy test workload')
    tmux('resize-window', '-t', SESSION, '-x', '32', '-y', '9')
    expect('Logs:')
    tmux('resize-window', '-t', SESSION, '-x', '150', '-y', '36')
    expect('Streaming', 'healthy test workload')
    keys('Escape')
    command('info')
    expect('Active sessions: 0')
    keys('Escape')
    command('logs worker')
    expect('Streaming', 'healthy test workload')
    keys('C-c')
    expect('M4_TERMINAL_RESTORED status=0')
    print('PASS M4.0: follow/previous/container/search/palette/rapid cancel/scope/32x9/owned cleanup/terminal')


def main():
    run('bash', 'scripts/test-cluster.sh', 'check')
    run('cargo', 'build', '--locked', timeout=180)
    tmux('new-session', '-d', '-s', SESSION, '-x', '150', '-y', '36')
    try:
        launch = ' '.join(shlex.quote(s) for s in [str(BINARY), '--kubeconfig', str(CONFIG),
                          '--context', 'kind-sauron-test', 'pods', '-n', 'sauron-fixtures'])
        # Compare exact stty state, not just whether echoed shell input appears.
        shell = 'm4_before=$(stty -g); ' + launch + '; m4_result=$?; m4_after=$(stty -g); '
        shell += 'if [ "$m4_before" = "$m4_after" ]; then echo M4_TERMINAL_RESTORED status=$m4_result; else echo M4_TERMINAL_BROKEN; fi'
        tmux('send-keys', '-t', SESSION, '-l', shell)
        keys('Enter')
        foundation()
    finally:
        tmux('kill-session', '-t', SESSION)


if __name__ == '__main__':
    main()
