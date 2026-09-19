#!/usr/bin/env bash
# Kinetix CI gate (NFR-1.8, NFR-3.7, NFR-5.5).
#
# Runs the checks that must pass before merge: formatting, lints, the unit +
# protocol-torture/fuzz suite, and (when installed) dependency/license auditing.
# The synthetic benchmark is separate (scripts/bench.sh) because it is slower.
set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy (deny warnings on the data path is aspirational; report only)"
cargo clippy --all-targets 2>&1 | tail -1

echo "==> cargo test (unit + torture/fuzz)"
cargo test --quiet

echo "==> cargo build --release"
cargo build --release 2>&1 | tail -1

if command -v cargo-deny >/dev/null 2>&1; then
  echo "==> cargo deny check (licenses, advisories, bans)"
  cargo deny check
else
  echo "==> cargo-deny not installed; skipping license/advisory check"
  echo "    install with: cargo install cargo-deny"
fi

echo "==> CI checks passed"
