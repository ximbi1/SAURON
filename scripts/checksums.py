#!/usr/bin/env python3
"""M12.6: SHA-256 checksums + a deterministic manifest for every archive in
a given directory (default target/dist, where scripts/package.sh writes
its own output). One algorithm, explicit: SHA-256, matching the
overwhelming convention every other tool's own release process already
uses (Rust's own release process, most GitHub release workflows).
Writes SHA256SUMS.txt (the exact format `sha256sum -c` reads back) and
manifest.json (name/bytes/sha256/mtime-independent -- the file itself
carries a generated-at timestamp, but the *inventory* entries never do,
so two runs over byte-identical archives produce byte-identical
manifest.json entries, letting an external verifier diff two manifests
structurally)."""
import hashlib
import json
import pathlib
import sys


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()


def main():
    directory = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else 'target/dist')
    archives = sorted(directory.glob('*.tar.gz'))
    if not archives:
        print(f'No .tar.gz archives found in {directory}', file=sys.stderr)
        sys.exit(1)
    sums_lines = []
    manifest_entries = []
    for archive in archives:
        digest = sha256_of(archive)
        size = archive.stat().st_size
        sums_lines.append(f'{digest}  {archive.name}')
        manifest_entries.append({'file': archive.name, 'bytes': size, 'sha256': digest})
    (directory / 'SHA256SUMS.txt').write_text('\n'.join(sums_lines) + '\n')
    manifest = {
        'schemaVersion': 1,
        'algorithm': 'sha256',
        'artifacts': manifest_entries,
    }
    (directory / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(f'{len(archives)} artifact(s) checksummed in {directory}')
    for line in sums_lines:
        print(' ', line)


if __name__ == '__main__':
    main()
