#!/usr/bin/env bash
# Secret service storage against a throw-away gnome-keyring in a private D-Bus session
# with a temporary HOME; the user's real keyring is never used.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_HOME="$PWD/.build-tools/cargo"
export PATH="$HOME/.cargo/bin:$PATH"
# The temporary HOME below must not make rustup look for (and download) a toolchain.
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" RUSTUP_AUTO_INSTALL=0
cargo build --locked -p harbor-core --tests >/dev/null 2>&1 || true
home=$(mktemp -d)
trap 'rm -rf "$home"' EXIT
HOME="$home" XDG_DATA_HOME="$home/.local/share" XDG_RUNTIME_DIR="$home/run" \
HARBOR_KEYRING_TEST=isolated HARBOR_TEST_HOST_DB_PORT="${HARBOR_TEST_HOST_DB_PORT:-}" \
  dbus-run-session -- bash -c '
    mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
    printf "test-keyring" | gnome-keyring-daemon --unlock --components=secrets >/dev/null
    cargo test --locked -p harbor-core --test keyring -- --ignored --nocapture
  '
