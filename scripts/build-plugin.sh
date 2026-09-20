#!/usr/bin/env bash
# Build a first-party plugin into a `.kxp` package.
#
# Usage: scripts/build-plugin.sh <plugin-dir>
#   e.g. scripts/build-plugin.sh plugins/antigravity-oauth
#
# Produces <plugin-dir>/<id>-<version>.kxp (a deterministic tar archive with
# plugin.toml, plugin.wasm, README.md, LICENSE). Requires the wasm32-unknown-unknown
# target and wasm-tools.
set -euo pipefail

DIR="${1:?usage: build-plugin.sh <plugin-dir>}"
DIR="${DIR%/}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/$DIR"

NAME="$(basename "$DIR")"
PKG_NAME="$(grep -m1 '^name' "$DIR/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"
VERSION="$(grep -m1 '^version' "$DIR/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"
PLUGIN_ID="$(grep -m1 '^id' "$DIR/plugin.toml" | sed -E 's/.*"(.*)".*/\1/')"

command -v wasm-tools >/dev/null || { echo "wasm-tools is required" >&2; exit 1; }
rustup target list --installed | grep -q wasm32-unknown-unknown \
  || { echo "run: rustup target add wasm32-unknown-unknown" >&2; exit 1; }

echo "==> building $PKG_NAME $VERSION ($PLUGIN_ID)"
( cd "$ROOT/plugins" && cargo build --release --target wasm32-unknown-unknown -p "$PKG_NAME" )

WASM="$ROOT/plugins/target/wasm32-unknown-unknown/release/${PKG_NAME//-/_}.wasm"
[ -f "$WASM" ] || { echo "expected $WASM" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> encoding component"
wasm-tools component new "$WASM" -o "$WORK/plugin.wasm"
wasm-tools validate --features component-model "$WORK/plugin.wasm"

cp "$DIR/plugin.toml" "$WORK/plugin.toml"
[ -f "$DIR/README.md" ] && cp "$DIR/README.md" "$WORK/README.md"
[ -f "$DIR/LICENSE" ] && cp "$DIR/LICENSE" "$WORK/LICENSE"

OUT="$DIR/${PLUGIN_ID}-${VERSION}.kxp"
# Deterministic archive: fixed mtime, sorted entries, no owner names.
tar --sort=name --mtime='UTC 2020-01-01' --owner=0 --group=0 --numeric-owner \
    -C "$WORK" -cf "$OUT" plugin.toml plugin.wasm README.md LICENSE 2>/dev/null \
  || tar -C "$WORK" -cf "$OUT" plugin.toml plugin.wasm README.md LICENSE

echo "==> wrote $OUT"
sha256sum "$OUT" | awk '{print "    sha256 " $1}'
