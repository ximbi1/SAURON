#!/usr/bin/env python3
"""M5 slice acceptance; explicit verified kind config only, no production fallback."""
import importlib.util
import pathlib
import shlex
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('m4_helpers', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def info_until(predicate, timeout=45):
    deadline = time.monotonic() + timeout
    output = ''
    while time.monotonic() < deadline:
        m.command('info')
        output = m.expect('Runtime diagnostics', 'Metrics requests started:')
        if predicate(output):
            return output
        time.sleep(.5)
    raise AssertionError(output)


def main():
    # 'metrics-absent' PTY mode was retired: scripts/test-cluster.sh m5-metrics-install
    # now leaves metrics-server permanently installed on the only isolated live
    # context (kind-sauron-test and its alias kind-sauron-test-b share the one
    # physical kind cluster), so there is no live context left without it. The
    # absent/forbidden/timeout/malformed paths are exercised by the fake-HTTP
    # transport tests instead (tests/watch_transport.rs); the one live-absent PTY
    # pass recorded before this fixture existed remains valid historical evidence,
    # not a repeatable mode. See docs/M5_ACCEPTANCE.md's execution journal.
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('cargo', 'build', '--locked', timeout=180)
    m.tmux('new-session', '-d', '-s', m.SESSION, '-x', '180', '-y', '40')
    try:
        launch = shlex.join([str(m.BINARY), '--kubeconfig', str(m.CONFIG), '--context',
            'kind-sauron-test', '--readonly', '-n', 'sauron-fixtures', '-l', 'app=healthy'])
        shell = 'm5_before=$(stty -g); ' + launch + '; m5_code=$?; m5_after=$(stty -g); '
        shell += 'if [ "$m5_before" = "$m5_after" ]; then echo M5_RESTORED status=$m5_code; else echo M5_TERMINAL_BROKEN; fi'
        m.tmux('send-keys', '-t', m.SESSION, '-l', shell); m.keys('Enter')
        m.expect('pods [1 / 1;', 'list synchronized')
        info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s and 'source timestamp: Some' in s)
        for command in ['ns kube-system', 'ctx kind-sauron-test-b', 'ctx kind-sauron-test', 'v1/pods -n sauron-fixtures -l app=healthy']:
            m.command(command)
        m.expect('ctx:kind-sauron-test', 'pods [1 / 1;', 'list synchronized')
        info_until(lambda s: 'CPU cores: UNKNOWN' not in s and 'CPU cores:' in s)
        m.command('v1/nodes')
        m.expect('nodes [1 / 1;', 'list synchronized')
        info_until(lambda s: 'Target Node/' in s and 'CPU cores: UNKNOWN' not in s)
        m.tmux('resize-window', '-t', m.SESSION, '-x', '32', '-y', '9')
        m.expect('Runtime diagnostics')
        m.tmux('resize-window', '-t', m.SESSION, '-x', '180', '-y', '40')
        print('PASS real Pod/container/Node samples + rapid scope changes + 32x9', flush=True)
        m.keys('C-c'); m.expect('M5_RESTORED status=0')
        print('PASS collector shutdown + terminal restoration', flush=True)
    finally:
        m.tmux('kill-session', '-t', m.SESSION)


if __name__ == '__main__':
    main()
