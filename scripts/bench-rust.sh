#!/usr/bin/env bash
# Dependency-free capacity/overhead benchmark (NFR-1.4/NFR-1.8).
#
# Unlike scripts/bench.sh (Python, GIL-bound), this rig is std-only Rust, so it
# can drive Kinetix at the NFR-1.4 reference load (200 concurrent streams) and
# beyond without the harness saturating first.
#
#   scripts/bench-rust.sh [concurrency-list] [requests-per-level]
# e.g. scripts/bench-rust.sh "1 10 100 200 500 1000" 3000
set -euo pipefail
cd "$(dirname "$0")/.."

CONC_LIST="${1:-1 10 100 200 500 1000}"
REQS="${2:-3000}"
TOKENS="${TOKENS:-60}"
# Per-token upstream delay (microseconds). A non-zero value makes each stream
# last longer, so the reference load (200 concurrent streams at ~50 rps) can be
# sustained rather than completed in a burst.
DELAY_US="${DELAY_US:-0}"
# Pin Kinetix to a single core to approximate the NFR-1.4 1-vCPU target.
CPUSET="${CPUSET:-}"
PORT="${PORT:-8180}"
UP="${UP:-9099}"
# NFR-1.8: when ALLOC_STATS=1, build with the `alloc-stats` feature and report
# allocations (and bytes) per request for each level by scraping the counters
# from /admin/api/metrics.
ALLOC_STATS="${ALLOC_STATS:-0}"

BIN=/tmp/bench_rust
rustc -O scripts/bench_rust.rs -o "$BIN"

if [ "$ALLOC_STATS" = "1" ]; then
  cargo build --release --quiet --features alloc-stats
else
  cargo build --release --quiet
fi

D=$(mktemp -d)
export KINETIX_BIND="127.0.0.1:$PORT"
export KINETIX_DATABASE_URL="sqlite://$D/bench.db"
export KINETIX_MASTER_KEY="$(head -c 32 /dev/urandom | xxd -p | tr -d '\n')"
export KINETIX_ADMIN_TOKEN="benchadmintoken123456"
export KINETIX_BOOTSTRAP_FILE="scripts/bench-bootstrap.toml"
export KINETIX_DATA_DIR="$D"
export KINETIX_ALLOW_PRIVATE_UPSTREAMS=true
export KINETIX_ALLOW_INSECURE_TLS=true

"$BIN" upstream "$UP" "$TOKENS" "$DELAY_US" >/tmp/bench-rust-up.log 2>&1 &
UPPID=$!
sleep 0.5
if [ -n "$CPUSET" ]; then
  taskset -c "$CPUSET" ./target/release/kinetix serve >/tmp/bench-rust-kinetix.log 2>&1 &
else
  ./target/release/kinetix serve >/tmp/bench-rust-kinetix.log 2>&1 &
fi
KPID=$!
for _ in $(seq 1 40); do
  curl -s "http://127.0.0.1:$PORT/healthz" >/dev/null 2>&1 && break
  sleep 0.3
done
KEY=$(grep -oE 'sk-kinetix-[0-9a-f]+' /tmp/bench-rust-kinetix.log | head -1)
echo "key: ${KEY:0:20}...  upstream tokens=$TOKENS"

# Admin session cookie for scraping metrics (used only when ALLOC_STATS=1).
CK=$(mktemp)
if [ "$ALLOC_STATS" = "1" ]; then
  curl -s -c "$CK" -X POST "http://127.0.0.1:$PORT/admin/api/login" \
    -H 'content-type: application/json' \
    -d "{\"password\":\"$KINETIX_ADMIN_TOKEN\"}" >/dev/null 2>&1 || true
fi
scrape_alloc() {
  curl -s -b "$CK" "http://127.0.0.1:$PORT/admin/api/metrics" 2>/dev/null \
    | awk '/^kinetix_allocations_total /{a=$2} /^kinetix_alloc_bytes_total /{b=$2} END{print a" "b}'
}

echo "concurrency | rps | ttft_p50 p95 p99 (ms) | total_p50 p95 p99 (ms) | errors | kinetix_rss_kB"
for c in $CONC_LIST; do
  A0=""; A1=""
  if [ "$ALLOC_STATS" = "1" ]; then A0=$(scrape_alloc); fi
  "$BIN" load "http://127.0.0.1:$PORT/v1/chat/completions" "$c" "$REQS" "$TOKENS" \
    | python3 -c '
import json,sys
d=json.load(sys.stdin)
print("{c:>5} | {rps:>7.1f} | {t50:>7.2f} {t95:>7.2f} {t99:>8.2f} | {T50:>7.2f} {T95:>7.2f} {T99:>8.2f} | {e:>4} |".format(
  c=d["concurrency"], rps=d["rps"], e=d["errors"],
  t50=d["ttft_p50_ms"], t95=d["ttft_p95_ms"], t99=d["ttft_p99_ms"],
  T50=d["total_p50_ms"], T95=d["total_p95_ms"], T99=d["total_p99_ms"]), end="")
'
  RSS=$(grep VmRSS "/proc/$KPID/status" 2>/dev/null | awk '{print $2}')
  echo -n " ${RSS:-?}"
  if [ "$ALLOC_STATS" = "1" ]; then
    A1=$(scrape_alloc)
    python3 -c "
import sys
a0,b0='$A0'.split(); a1,b1='$A1'.split()
reqs=$REQS
d=(int(a1)-int(a0))/reqs; db=(int(b1)-int(b0))/reqs
print(f' | allocs/req {d:.1f} bytes/req {db:.0f}', end='')
"
  fi
  echo
done
kill "$KPID" "$UPPID" 2>/dev/null || true
rm -rf "$D" "$CK"
