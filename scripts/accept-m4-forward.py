#!/usr/bin/env python3
"""M4.3 live acceptance. All fixture writes use guarded test-cluster.sh."""
import importlib.util
import pathlib
import re
import shlex
import socket
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('m4', ROOT / 'scripts/accept-m4.py')
assert spec and spec.loader
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
PORTS = set()
LAST_STARTED = 0


def closed(port):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=.2):
                pass
        except OSError:
            return
        time.sleep(.05)
    raise AssertionError(f'Listener leaked on {port}')


def http(port):
    with socket.create_connection(('127.0.0.1', port), timeout=5) as sock:
        sock.settimeout(10)
        sock.sendall(b'GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n')
        data = bytearray()
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data.extend(chunk)
            assert len(data) < 65536
        assert b'M4_HTTP_OK' in data, bytes(data)


def target():
    m.command('v1/pods -n sauron-fixtures -l test=m4-sessions')
    m.expect('pods [1 / 1;', 'm4-sessions', 'list synchronized')
    m.keys('Home')


def start(mapping='8080', picker=False):
    global LAST_STARTED
    target()
    if picker:
        m.keys('f')
        m.expect('Pod TCP port', '8080')
        m.keys('Enter')
    else:
        m.command('forward ' + mapping)
    # Existing Listening rows do not prove the new asynchronous start completed.
    deadline = time.monotonic() + 25
    output = ''
    while time.monotonic() < deadline:
        output = m.tmux('capture-pane', '-t', m.SESSION, '-p')
        rows = re.findall(r'(\d+)  TCP  sauron-fixtures/m4-sessions\s+kind-sauron-test\s+127\.0\.0\.1:(\d+) -> 8080  Listening', output)
        fresh = [(int(i), int(p)) for i, p in rows if int(i) > LAST_STARTED]
        if fresh:
            session, port = max(fresh)
            LAST_STARTED = session
            break
        time.sleep(.08)
    else:
        raise AssertionError('New forward did not start:\n' + output)
    PORTS.add(port)
    http(port)
    return session, port


def stop(session, port):
    m.command(f'pf_stop {session}')
    closed(port)


def resources(pid):
    status = pathlib.Path(f'/proc/{pid}/status').read_text()
    return {'rss_kib': int(re.search(r'VmRSS:\s+(\d+)', status).group(1)),
            'fds': len(list(pathlib.Path(f'/proc/{pid}/fd').iterdir())),
            'threads': int(re.search(r'Threads:\s+(\d+)', status).group(1))}


def exercise():
    m.expect('pods [', 'list synchronized', 'OPERATIONAL')
    first, port = start(picker=True)
    print(f'PASS auto port + declared picker + real TCP: session={first} port={port}', flush=True)
    m.command('deployments -n sauron-fixtures')
    m.expect('deployments.apps', 'PF 1')
    http(port)
    m.command('ns kube-system')
    m.expect('ns:kube-system', 'list synchronized')
    http(port)
    m.command('ctx kind-sauron-test-b')
    m.expect('ctx:kind-sauron-test-b', 'READ ONLY', 'list synchronized')
    http(port)
    m.command('forward 8080')
    m.expect('Port forwarding is unavailable in read-only mode')
    m.keys('Escape')  # validation palette only
    m.command('pf')
    m.expect('kind-sauron-test', 'Listening', '1 active')
    m.command('ctx kind-sauron-test')
    m.expect('ctx:kind-sauron-test', 'OPERATIONAL', 'list synchronized')
    target()
    m.command('logs web')
    m.expect('M4_WEB_LIVE', 'Streaming')
    http(port)
    m.keys('Escape')
    print('PASS navigation/ns/readonly-context/logs preserve original tunnel', flush=True)
    with socket.socket() as conflict:
        conflict.bind(('127.0.0.1', 0)); conflict.listen()
        m.command(f'forward {conflict.getsockname()[1]}:8080')
        m.expect('PortInUse', '1 active')
        http(port)
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0)); explicit = reservation.getsockname()[1]
    second, p2 = start(f'{explicit}:8080')
    assert p2 == explicit, (explicit, p2, m.tmux('capture-pane', '-t', m.SESSION, '-p'))
    third, p3 = start()
    fourth, p4 = start()
    target(); m.command('forward 8080'); m.expect('Forward limit reached')
    m.keys('Escape')  # close the validation palette, not table root
    m.command('pf'); m.expect('4 active')
    for p in [port, p2, p3, p4]: http(p)
    stop(second, p2)
    for p in [port, p3, p4]: http(p)
    print('PASS conflict/explicit port/four-session cap/individual stop', flush=True)
    stop(third, p3); stop(fourth, p4)
    idle = [socket.create_connection(('127.0.0.1', port), timeout=5) for _ in range(8)]
    try:
        m.command('pf'); m.expect('clients=8')
        with socket.create_connection(('127.0.0.1', port), timeout=5) as ninth:
            ninth.settimeout(5)
            try: assert ninth.recv(1) == b''
            except ConnectionResetError: pass
        m.expect('rejected=1')
    finally:
        for sock in idle: sock.close()
    m.expect('clients=0')
    stop(first, port)
    m.expect('0 active')
    # The actual binary belongs to the unique tmux pane; no stale-process testing.
    pane = m.tmux('display-message', '-t', m.SESSION, '-p', '#{pane_pid}').strip()
    children = pathlib.Path(f'/proc/{pane}/task/{pane}/children').read_text().split()
    pid = next(p for p in children if pathlib.Path(f'/proc/{p}/exe').resolve() == m.BINARY)
    before = resources(pid)
    for _ in range(12):
        i, p = start(); stop(i, p); m.expect('0 active')
    after = resources(pid)
    assert after['fds'] <= before['fds'] + 4, (before, after)
    assert after['threads'] <= before['threads'] + 1, (before, after)
    print(f'PASS 12 TCP start/stop cycles; resources before={before} after={after}', flush=True)
    # No readiness pause between start and cancel; completion must not need a frame.
    global LAST_STARTED
    for _ in range(5):
        target()
        LAST_STARTED += 1
        m.command('forward 8080')
        m.command(f'pf_stop {LAST_STARTED}')
        m.command('pf'); m.expect('0 active', absent=('Listening',))
    print('PASS five immediate startup/cancel cycles', flush=True)
    old, old_port = start()
    old_uid = m.run('kubectl', '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
                   'get', 'pod', 'm4-sessions', '-n', 'sauron-fixtures', '-o', 'jsonpath={.metadata.uid}')
    live_client = socket.create_connection(('127.0.0.1', old_port), timeout=5)
    try:
        m.run('bash', 'scripts/test-cluster.sh', 'm4-recreate', timeout=100)
        live_client.settimeout(5)
        try: assert live_client.recv(1) == b''
        except ConnectionResetError: pass
    finally:
        live_client.close()
    closed(old_port)
    m.command('pf'); m.expect('0 active')
    m.keys('End')  # the newest ended record follows the retained cycle history
    output = m.expect(f'{old}  TCP')
    assert any(reason in output for reason in ['TargetTerminating', 'TargetGone', 'TargetReplaced', 'TargetEnded']), output
    new_uid = m.run('kubectl', '--kubeconfig', str(m.CONFIG), '--context', 'kind-sauron-test',
                   'get', 'pod', 'm4-sessions', '-n', 'sauron-fixtures', '-o', 'jsonpath={.metadata.uid}')
    assert old_uid != new_uid
    time.sleep(2); closed(old_port)
    new, new_port = start()
    m.tmux('resize-window', '-t', m.SESSION, '-x', '32', '-y', '9')
    m.expect('Port forwards')
    http(new_port)
    m.tmux('resize-window', '-t', m.SESSION, '-x', '180', '-y', '44')
    m.expect('Listening')
    m.keys('C-c'); m.expect('M4_FORWARD_RESTORED status=0')
    for p in PORTS: closed(p)
    print('PASS target deletion/recreation/new UID/narrow/quit with no listeners', flush=True)


def main():
    m.run('bash', 'scripts/test-cluster.sh', 'check')
    m.run('bash', 'scripts/test-cluster.sh', 'm4-fixtures', timeout=100)
    m.run('cargo', 'build', '--locked', timeout=180)
    m.tmux('new-session', '-d', '-s', m.SESSION, '-x', '180', '-y', '44')
    try:
        launch = ' '.join(shlex.quote(s) for s in [str(m.BINARY), '--kubeconfig', str(m.CONFIG),
            '--context', 'kind-sauron-test', '--config', str(ROOT / 'tests/fixtures/operational.toml'),
            'v1/pods', '-n', 'sauron-fixtures'])
        shell = 'm4_before=$(stty -g); ' + launch + '; m4_result=$?; m4_after=$(stty -g); '
        shell += 'if [ "$m4_before" = "$m4_after" ]; then echo M4_FORWARD_RESTORED status=$m4_result; else echo M4_TERMINAL_BROKEN; fi'
        if '--policy-only' not in sys.argv:
            m.tmux('send-keys', '-t', m.SESSION, '-l', shell); m.keys('Enter')
            exercise()
        policy_shell = shell.replace(launch, launch + ' --readonly').replace('M4_FORWARD_RESTORED', 'M4_POLICY_RESTORED')
        m.tmux('send-keys', '-t', m.SESSION, '-l', policy_shell); m.keys('Enter')
        m.expect('READ ONLY', 'list synchronized')
        m.command('reload'); m.expect('Configuration reloaded', 'READ ONLY')
        m.command('forward 8080'); m.expect('Port forwarding is unavailable in read-only mode')
        m.keys('Escape')
        for command in ['exec worker -- echo MUST_NOT_RUN', 'shell worker', 'attach worker']:
            m.command(command); m.expect('unavailable in read-only mode')
            m.keys('Escape')
        m.command('pf'); m.expect('0 active', 'No forwards started')
        m.keys('C-c'); m.expect('M4_POLICY_RESTORED status=0')
        print('PASS CLI readonly survives reload of operational config', flush=True)
    finally:
        m.tmux('kill-session', '-t', m.SESSION)


if __name__ == '__main__': main()
