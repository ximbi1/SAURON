# Installing SAUR-ON

SAUR-ON ships as a single self-contained binary. No installer, no runtime
dependencies beyond a working kubeconfig.

## From a release archive

1. Download the archive matching your platform:

   | Platform | Archive |
   | --- | --- |
   | Linux x86_64 | `sauron-<version>-x86_64-unknown-linux-gnu.tar.gz` |
   | Linux aarch64 | `sauron-<version>-aarch64-unknown-linux-gnu.tar.gz` |
   | macOS x86_64 (Intel) | `sauron-<version>-x86_64-apple-darwin.tar.gz` |
   | macOS aarch64 (Apple Silicon) | `sauron-<version>-aarch64-apple-darwin.tar.gz` |

2. Extract it:

   ```sh
   tar xzf sauron-<version>-<target>.tar.gz
   cd sauron-<version>-<target>
   ```

3. (Optional but recommended) verify the checksum against
   `SHA256SUMS.txt` in the same release:

   ```sh
   sha256sum -c SHA256SUMS.txt --ignore-missing
   ```

4. Move the binary somewhere on your `PATH`:

   ```sh
   install -m 0755 sauron ~/.local/bin/sauron
   ```

5. Run it:

   ```sh
   sauron info
   ```

Each archive also contains `LICENSE-MIT` and `LICENSE-APACHE` (SAUR-ON is
dual-licensed, MIT OR Apache-2.0 — pick whichever suits you).

## From source

Requires a Rust toolchain (`rust-version` in `Cargo.toml` states the
minimum supported version).

```sh
git clone <repository-url>
cd SAURON
cargo build --release
./target/release/sauron info
```

## Verifying you're set up correctly

`sauron info --offline` never loads a kubeconfig or makes any network
request — a safe first check on any machine:

```sh
sauron info --offline
```

Then, against a real cluster:

```sh
sauron --check
```

reports discovered API resources without opening the interactive UI —
useful for confirming connectivity/permissions before the first real
session.

## Configuration

SAUR-ON reads `config.toml` from `$XDG_CONFIG_HOME/sauron/` (or
`~/.config/sauron/` if `XDG_CONFIG_HOME` is unset). It is never created
automatically — SAUR-ON runs with safe defaults (read-only) if the file
does not exist. See `HANDBOOK.md` and `docs/RUNBOOK.md` for the full
configuration surface (themes, keymaps, workspaces, bookmarks, plugins).
