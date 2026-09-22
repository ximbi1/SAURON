#!/usr/bin/env bash
# M12.5: builds one platform archive from an already-built binary.
# Usage: scripts/package.sh TARGET BINARY_PATH
#   TARGET       Rust target triple, e.g. x86_64-unknown-linux-gnu
#   BINARY_PATH  path to the already-built `sauron` binary for that target
#
# Deliberately does not invoke `cargo build` itself: CI builds each target
# on its own native runner (see .github/workflows/release.yml); locally,
# this machine can only ever produce one target (x86_64-unknown-linux-gnu)
# since it has no rustup/cross-linker/macOS hardware -- see
# docs/M12_ACCEPTANCE.md's own M12.0 reconnaissance for why the other three
# platforms are CI-defined, not locally buildable here.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target="${1:?Usage: scripts/package.sh TARGET BINARY_PATH}"
binary_path="${2:?Usage: scripts/package.sh TARGET BINARY_PATH}"
[ -f "$binary_path" ] || { echo "binary not found: $binary_path" >&2; exit 1; }

version="$(grep '^version = ' "$repo_dir/Cargo.toml" | head -1 | sed -E 's/version = "(.*)"/\1/')"
name="sauron-${version}-${target}"
stage_dir="$repo_dir/target/package/$name"
dist_dir="$repo_dir/target/dist"

rm -rf "$stage_dir"
mkdir -p "$stage_dir" "$dist_dir"
install -m 0755 "$binary_path" "$stage_dir/sauron"
install -m 0644 "$repo_dir/LICENSE-MIT" "$stage_dir/LICENSE-MIT"
install -m 0644 "$repo_dir/LICENSE-APACHE" "$stage_dir/LICENSE-APACHE"
install -m 0644 "$repo_dir/docs/INSTALL.md" "$stage_dir/INSTALL.md"

archive="$dist_dir/$name.tar.gz"
# Deterministic member order and no embedded timestamps/owner IDs, so
# repeated packaging of byte-identical inputs produces byte-identical
# archives -- the exact property scripts/checksums.py's own manifest
# comparison (M12.6) relies on.
tar --sort=name --mtime='1970-01-01 00:00:00Z' --owner=0 --group=0 --numeric-owner \
    -C "$repo_dir/target/package" -czf "$archive" "$name"

echo "packaged: $archive"
tar -tzf "$archive"
