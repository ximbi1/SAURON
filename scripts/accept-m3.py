#!/usr/bin/env python3
"""Reproducible read-only live PTY acceptance; fixture writes use test-cluster.sh only."""
import json
import pathlib
import shlex
import subprocess
import sys
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
CONFIG = ROOT / '.test-cluster/config'
BINARY = ROOT / 'target/debug/sauron'
CONTEXT = 'kind-sauron-test'
SESSION = 'm3-' + uuid.uuid4().hex[:8]


def run(*args):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, capture_output=True, timeout=40).stdout


def tmux(*args):
    return run('tmux', '-L', 'sauron-m3', *args)


def screen():
    return tmux('capture-pane', '-t', SESSION, '-p')


def keys(*args):
    tmux('send-keys', '-t', SESSION, *args)


def command(text):
    keys(':')
    tmux('send-keys', '-t', SESSION, '-l', text)
    keys('Enter')


def expect(*terms, absent=(), timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        output = screen()
        if all(term in output for term in terms) and all(term not in output for term in absent):
            return output
        time.sleep(.08)
    raise AssertionError(f'Expected {terms}, absent {absent}\n{output}')


def snapshot(resource, expression, *options):
    result = json.loads(run(str(BINARY), '--kubeconfig', str(CONFIG), '--context', CONTEXT,
                            resource, '-n', 'sauron-fixtures', '--snapshot', '--output', 'json',
                            '--filter', expression, *options))
    return result


def filters():
    for expression in ['observ', '"observatory"', '/^observ/', 'NOT /nomatch/',
                       '(name=observatory OR name=other) AND NOT label.missing',
                       'label.example.test/Team=Ops', 'label.empty=""',
                       'field:/spec/count=8 AND field:/spec/enabled=true AND field:/spec/ratio>1.4',
                       'cpu:field:/spec/cpu>100m AND memory:field:/spec/memory=1024Mi AND percent:field:/spec/percent=75%']:
        result = snapshot('eyes', expression)
        assert [o['metadata']['name'] for o in result['items']] == ['observatory'], expression
    unknown = snapshot('v1/pods', 'NOT (cpu>100m)')
    assert not unknown['items'] and unknown['unknownExcluded'] > 0
    restarted = snapshot('v1/pods', 'restarts>0 AND age>1s')
    assert 'crashloop' in [o['metadata']['name'] for o in restarted['items']]
    selected = snapshot('v1/pods', 'age>1s', '-l', 'app=healthy', '--field-selector', 'status.phase=Running')
    assert len(selected['items']) == 1
    assert selected['labelSelector'] == 'app=healthy'
    expect('pods [', 'list synchronized')
    command('v1/pods -n sauron-fixtures -l app=healthy -f status.phase=Running / age>1s')
    expect('pods [1 / 1;', '-l app=healthy', '-f status.phase=Running')
    command('eyes / label.example.test/Team=Ops AND field:/spec/enabled=true')
    expect('eyes.testing.sauron.local [1 / 1;', 'observatory')
    keys('[')
    expect('pods [1 / 1;', '-l app=healthy', '-f status.phase=Running', '/ age>1s')
    keys('r')
    expect('pods [1 / 1;', 'list synchronized')
    keys(']')
    expect('eyes.testing.sauron.local [1 / 1;', 'observatory')
    keys('/')
    keys('C-u')
    tmux('send-keys', '-t', SESSION, '-l', '/[/')
    keys('Enter')
    expect('invalid or oversized regex', 'observatory')
    keys('C-u')
    tmux('send-keys', '-t', SESSION, '-l', '/^observ/')
    keys('Enter')
    expect('observatory', '//^observ/', absent=('invalid or oversized regex',))
    command('ctx kind-sauron-test-b')
    expect('ctx:kind-sauron-test-b', 'list synchronized', '//^observ/')
    command('ns sauron-fixtures')
    expect('observatory', 'list synchronized', '//^observ/')
    for _ in range(3):
        for text in ['ns kube-system', 'ns *', 'ns sauron-fixtures']:
            command(text)
        expect('ns:sauron-fixtures', 'observatory', '//^observ/')
    for _ in range(3):
        command('ctx kind-sauron-test')
        command('ctx kind-sauron-test-b')
        expect('ctx:kind-sauron-test-b', 'list synchronized', '//^observ/')
    command('v1/pods -n sauron-fixtures / cpu>100m')
    expect('No TRUE matches', 'unknown excluded:')
    command('v1/pods -n sauron-fixtures -l app=healthy / age>1s')
    expect('pods [1 / 1;', '-l app=healthy')
    keys('0')
    expect('ns:*', 'pods [1 / 1;', '-l app=healthy')
    command('ns sauron-fixtures')
    expect('pods [1 / 1;', '-l app=healthy')
    command('po')
    expect('ambiguous')
    keys('C-u')
    tmux('send-keys', '-t', SESSION, '-l', 'v1/pods -n sauron-fixtures')
    keys('Enter')
    expect('pods [', 'list synchronized')
    print('PASS filters: snapshots, types, unknowns, selectors, history, malformed recovery, rapid scope changes, ambiguity')


def sorting():
    run('bash', 'scripts/test-cluster.sh', 'm3-sort-fixtures')
    expect('pods [', 'list synchronized')
    command('configmaps -n sauron-fixtures -l test=m3-sort')
    expect('configmaps [3 / 3;', 'm3-sort-a')
    def ordered(names, selected=None):
        output = expect(*names, 'list synchronized')
        # Retry ordering too: a watch modification can land after a previous render.
        deadline = time.monotonic() + 15
        while True:
            rows = [line for line in output.splitlines() if any(name in line for name in names) and line.startswith('│')]
            positions = [next(i for i, row in enumerate(rows) if name in row) for name in names]
            # M10.1/M10.7 inserted a selection/severity marker between the
            # cursor '›' and the row's own text (e.g. '│›  ? name'), so
            # the cursor is no longer directly adjacent to the name --
            # check cursor-presence-on-the-matching-row instead of a
            # fixed-width literal substring.
            selection_ok = selected is None or any(
                row.startswith('│›') and selected in row for row in rows
            )
            if positions == sorted(positions) and selection_ok:
                return output
            if time.monotonic() > deadline:
                raise AssertionError(f'Ordering {names} or selection {selected} failed\n{output}')
            time.sleep(.08)
            output = screen()
    command('sort count:field:/data/rank')
    ordered(['m3-sort-a', 'm3-sort-b', 'm3-sort-c'], 'm3-sort-a')
    keys('I')
    ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'], 'm3-sort-a')
    for spec in ['memory:field:/data/memory:desc', 'bool:field:/data/enabled:desc']:
        command('sort ' + spec)
        ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'], 'm3-sort-a')
    command('sort count:field:/data/rank')
    run('bash', 'scripts/test-cluster.sh', 'm3-sort-update')
    ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'], 'm3-sort-a')
    command('eyes')
    expect('observatory')
    keys('[')
    ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'], 'm3-sort-a')
    for text in ['ns *', 'ns sauron-fixtures', 'ctx kind-sauron-test-b', 'ns sauron-fixtures']:
        command(text)
    ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'])
    expect('count:field:/data/rank')
    # Reselect A deliberately, then delete and recreate its name through guarded script.
    keys('Home')
    keys('j')
    ordered(['m3-sort-b', 'm3-sort-a', 'm3-sort-c'], 'm3-sort-a')
    run('bash', 'scripts/test-cluster.sh', 'm3-sort-delete')
    expect('configmaps [2 / 2;', absent=('m3-sort-a',))
    run('bash', 'scripts/test-cluster.sh', 'm3-sort-fixtures')
    output = ordered(['m3-sort-a', 'm3-sort-b', 'm3-sort-c'])
    assert not any(
        line.startswith('│›') and 'm3-sort-a' in line for line in output.splitlines()
    ), output
    command('v1/pods -n sauron-fixtures / restarts>=0')
    expect('crashloop')
    command('sort restarts:desc')
    expect('RESTARTS ↓')
    command('sort age')
    expect('AGE ↑')
    command('sort name:desc')
    expect('NAME ↓')
    print('PASS sorting: typed/unknown-last, live update, UID selection, history, rapid scopes, same-name replacement')


def main():
    run('bash', 'scripts/test-cluster.sh', 'check')
    launch = shlex.join([str(BINARY), '--kubeconfig', str(CONFIG), '--context', CONTEXT,
                        'v1/pods', '-n', 'sauron-fixtures', '--readonly'])
    tmux('new-session', '-d', '-s', SESSION, '-x', '180', '-y', '35')
    try:
        tmux('send-keys', '-t', SESSION, '-l', launch)
        keys('Enter')
        if sys.argv[1:] == ['filters']:
            filters()
        elif sys.argv[1:] == ['sorting']:
            sorting()
        else:
            raise ValueError('Usage: python3 scripts/accept-m3.py filters')
        keys('C-c')
        # A shell command after exit demonstrates normal terminal input/output restoration.
        tmux('send-keys', '-t', SESSION, '-l', 'echo M3_TERMINAL_RESTORED')
        keys('Enter')
        expect('M3_TERMINAL_RESTORED')
        print('PASS terminal restored')
    finally:
        tmux('kill-session', '-t', SESSION)


if __name__ == '__main__':
    main()
