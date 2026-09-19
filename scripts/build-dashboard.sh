#!/usr/bin/env bash
# Build the dashboard and then the Kinetix binary that embeds it.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DASH="$ROOT/dashboard"

if [ ! -d "$DASH/node_modules" ]; then
  echo "==> installing dashboard dependencies"
  (cd "$DASH" && npm install)
fi

echo "==> building dashboard bundle"
(cd "$DASH" && npm run build)

echo "==> building kinetix (embedded assets)"
(cd "$ROOT" && cargo build --release "$@")

echo "==> done: $ROOT/target/release/kinetix"
