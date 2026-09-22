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
# comparison (M12.6) relies on. GNU tar's --sort/--mtime/--owner/--group
# flags don't exist on macOS's default bsdtar (libarchive) -- found live
# when the macOS release-workflow runners failed with "tar: Option
# --sort=name is not supported". Fixed portably: pin every staged file's
# mtime first (`touch -t`, supported by both GNU and BSD touch), then
# feed tar an explicitly pre-sorted member list via -T/--files-from
# (supported by both GNU tar and bsdtar) instead of relying on a
# sort/mtime/owner flag that only one of them has.
find "$stage_dir" -exec touch -t 197001010000 {} +
(
    cd "$repo_dir/target/package"
    find "$name" | LC_ALL=C sort > "$dist_dir/.members-$name.txt"
)
COPYFILE_DISABLE=1 tar -C "$repo_dir/target/package" -czf "$archive" \
    -T "$dist_dir/.members-$name.txt"
rm -f "$dist_dir/.members-$name.txt"

echo "packaged: $archive"
tar -tzf "$archive"
