#!/usr/bin/env bash
# Deterministic synthetic upstream + benchmark matrix for Kinetix (NFR-1.8/1.9).
#
# Starts a tiny Python HTTP server that speaks BOTH the OpenAI and Gemini wire
# formats with a fixed, configurable token cadence, then drives Kinetix through
# a matrix of concurrency levels and paths (same-format passthrough,
# OpenAI->Gemini translation, tool-heavy streaming, large contexts), recording
# added latency, TTFT, CPU and RSS. Inference latency is zero (synthetic), so
# everything measured is Kinetix overhead.
#
# Usage: scripts/bench.sh [concurrency-list] [requests-per-level]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONCURRENCY="${1:-1 10 100 500 1000}"
NREQ="${2:-40}"
UPSTREAM_PORT="${UPSTREAM_PORT:-9099}"
OUT="${OUT:-/tmp/kinetix-bench.json}"

echo "==> starting synthetic upstream on 127.0.0.1:$UPSTREAM_PORT"
python3 "$ROOT/scripts/synthetic_upstream.py" "$UPSTREAM_PORT" &
UP_PID=$!
trap 'kill $UP_PID 2>/dev/null || true' EXIT
sleep 1

echo "==> building release binary"
(cd "$ROOT" && cargo build --release 2>&1 | tail -1)

echo "==> starting kinetix (fresh bench DB)"
BENCH_DIR="$(mktemp -d)"
rm -f "$BENCH_DIR/kinetix.db"*
KINETIX_BIND=127.0.0.1:8180 \
KINETIX_DATABASE_URL="sqlite://$BENCH_DIR/kinetix.db" \
KINETIX_MASTER_KEY="bench-master-key-0123456789abcdef" \
KINETIX_ADMIN_TOKEN="bench-admin-token-1234" \
KINETIX_BOOTSTRAP_FILE="$ROOT/scripts/bench-bootstrap.toml" \
KINETIX_ALLOW_PRIVATE_UPSTREAMS=true \
  "$ROOT/target/release/kinetix" > "$BENCH_DIR/kinetix.log" 2>&1 &
KX_PID=$!
trap 'kill $KX_PID $UP_PID 2>/dev/null || true' EXIT

for _ in $(seq 1 40); do
  if curl -sf http://127.0.0.1:8180/healthz >/dev/null 2>&1; then break; fi
  sleep 0.25
done
KEY="$(grep -o 'sk-kinetix-[a-f0-9]*' "$BENCH_DIR/kinetix.log" | head -1)"
echo "==> virtual key: ${KEY:0:20}..."

# Wait for the registry to see the seeded provider.
sleep 1

bench_one() { # $1=concurrency $2=path
  python3 "$ROOT/scripts/bench_client.py" \
    --url http://127.0.0.1:8180/v1/chat/completions \
    --key "$KEY" \
    --concurrency "$1" \
    --requests "$NREQ" \
    --path "$2"
}

echo "==> matrix"
RESULT="["
for c in $CONCURRENCY; do
  for path in passthrough translation tools large; do
    echo "--- concurrency=$c path=$path"
    ROW=$(bench_one "$c" "$path")
    echo "$ROW" | python3 -c 'import json,sys;d=json.load(sys.stdin);print("    p50={p50_overhead_ms}ms p95={p95_overhead_ms}ms p99={p99_overhead_ms}ms ttft_p50={ttft_p50_ms}ms ttft_p95={ttft_p95_ms}ms rps={rps} err={errors}".format(**d))'
    RESULT="$RESULT$ROW,"
  done
done
RESULT="${RESULT%,}]"

RSS="$(ps -o rss= -p $KX_PID 2>/dev/null | tr -d ' ' || echo 0)"
CPU="$(ps -o %cpu= -p $KX_PID 2>/dev/null | tr -d ' ' || echo 0)"
python3 - "$OUT" "$RSS" "$CPU" <<PY
import json,sys
out,rss,cpu=sys.argv[1],int(sys.argv[2]),float(sys.argv[3])
rows=json.loads('''$RESULT''')
json.dump({"results":rows,"kinetix_rss_kb":rss,"kinetix_cpu_pct":cpu}, open(out,"w"), indent=2)
print(f"==> wrote {out} (rss={rss}KB cpu={cpu}%)")
PY
