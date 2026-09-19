#!/usr/bin/env python3
"""Cancellation-latency benchmark (NFR-1.10).

Starts a streaming request through Kinetix against a *slow* synthetic upstream
(a long, paced token cadence), disconnects the client mid-stream, and measures
how quickly Kinetix notices and cancels the upstream. The upstream records the
wall-clock time of its last successfully-written byte; Kinetix is considered to
have cancelled promptly once the upstream stops being written to.

The measurement is: after the client socket is closed, how many ms until the
upstream's write loop observes the broken pipe (i.e. Kinetix dropped the
upstream connection). We approximate this by having the synthetic upstream log
per-request last-write timestamps and comparing against the disconnect time.

Usage: scripts/cancel_bench.py [--url ...] [--key ...] [--rounds N]
"""
import argparse
import json
import socket
import threading
import time
import urllib.request


def one_round(url, key, upstream_port):
    body = json.dumps({
        "model": "syn-openai",
        "stream": True,
        "max_tokens": 100000,
        "messages": [{"role": "user", "content": "hi"}],
    }).encode()
    req = urllib.request.Request(
        url, data=body, method="POST",
        headers={"authorization": "Bearer " + key,
                 "content-type": "application/json",
                 "accept": "text/event-stream"})

    # Read a few chunks, then hard-close the socket to simulate a client crash.
    resp = urllib.request.urlopen(req, timeout=30)
    for _ in range(3):
        resp.read(256)
    sock = resp.fp.raw._sock  # underlying socket
    t_disconnect = time.time()
    sock.shutdown(socket.SHUT_RDWR)
    sock.close()

    # Ask the synthetic upstream how long it kept writing after that.
    time.sleep(0.5)
    try:
        with urllib.request.urlopen(
            f"http://127.0.0.1:{upstream_port}/_last_write", timeout=5) as r:
            info = json.load(r)
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)}
    return {"disconnect_at": t_disconnect, "upstream": info}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", required=True)
    ap.add_argument("--key", required=True)
    ap.add_argument("--upstream-port", type=int, default=9099)
    ap.add_argument("--rounds", type=int, default=10)
    a = ap.parse_args()

    lat = []
    for _ in range(a.rounds):
        r = one_round(a.url, a.key, a.upstream_port)
        if "error" in r:
            print("err", r["error"])
            continue
        # ms between client disconnect and the upstream's last write.
        delta_ms = (r["upstream"]["last_write"] - r["disconnect_at"]) * 1000.0
        lat.append(max(0.0, delta_ms))
    lat.sort()
    if lat:
        p = lambda q: round(lat[min(len(lat) - 1, int(round(q * (len(lat) - 1))))], 1)
        print(json.dumps({"rounds": len(lat), "p50_ms": p(0.5), "p95_ms": p(0.95),
                          "max_ms": round(lat[-1], 1)}))


if __name__ == "__main__":
    main()
