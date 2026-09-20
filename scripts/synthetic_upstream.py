#!/usr/bin/env python3
"""Deterministic synthetic LLM upstream for Kinetix benchmarking (NFR-1.8).

Speaks two wire formats on one port, selected by path:
  * POST /openai/v1/chat/completions  -- OpenAI SSE (`data:` frames, [DONE])
  * POST /gemini/v1beta/models/<m>:streamGenerateContent?alt=sse -- Gemini SSE

Token cadence is fixed (TOKENS, DELAY_MS) so any measured variance is Kinetix
overhead, not inference. No external dependencies; stdlib only.
"""
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKENS = int(os.environ.get("SYN_TOKENS", "40"))
DELAY_MS = float(os.environ.get("SYN_DELAY_MS", "2"))
TTFT_MS = float(os.environ.get("SYN_TTFT_MS", "5"))

WORD = "lorem"

# Wall-clock time of the most recent successful upstream write, used by the
# cancellation benchmark to measure how fast Kinetix drops a dead client.
LAST_WRITE = time.time()
_LW_LOCK = threading.Lock()


def mark_write():
    global LAST_WRITE
    with _LW_LOCK:
        LAST_WRITE = time.time()



def _split_json_fragments(n: int) -> list:
    """Split a JSON tool-argument payload into n fragments (NFR-1.9).

    The concatenation of the fragments is always valid JSON, so the reassembly
    path can be exercised with large, many-piece arguments.
    """
    payload = '{"city": "' + ("Paris-" * 40) + '", "note": "' + ("x" * 400) + '"}'
    n = max(1, min(n, len(payload)))
    size = (len(payload) + n - 1) // n
    return [payload[i:i + size] for i in range(0, len(payload), size)]

class _Disconnected(Exception):
    """Client went away mid-stream (expected under cancellation benchmarks)."""


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):  # silence
        pass

    def do_GET(self):
        # Model discovery endpoint.
        if self.path.endswith("/_last_write"):
            body = json.dumps({"last_write": LAST_WRITE}).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.endswith("/models"):
            body = json.dumps(
                {
                    "data": [
                        {"id": "syn-openai", "context_window": 200000, "max_output_tokens": 8192},
                        {"id": "syn-gemini", "context_window": 200000, "max_output_tokens": 8192},
                    ]
                }
            ).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        self.send_response(404)
        self.send_header("content-length", "0")
        self.end_headers()

    def do_POST(self):
        length = int(self.headers.get("content-length", "0") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            req = json.loads(raw or b"{}")
        except Exception:
            req = {}
        model = req.get("model", "syn")
        stream = bool(req.get("stream"))

        want_tools = bool(req.get("tools"))
        # NFR-1.9: a large tool argument delivered as many small fragments, to
        # exercise incremental argument reassembly under load. The benchmark
        # client sets this on the request body.
        tool_fragments = int(req.get("tool_fragments", 0) or 0)

        if "/gemini/" in self.path:
            self._gemini(model, want_tools)
        else:
            self._openai(model, stream, want_tools, tool_fragments)

    # -- OpenAI-compatible SSE -------------------------------------------
    def _openai(self, model, stream, want_tools=False, tool_fragments=0):
        if not stream:
            body = json.dumps(
                {
                    "id": "syn-1",
                    "object": "chat.completion",
                    "model": model,
                    "choices": [
                        {"index": 0, "message": {"role": "assistant", "content": WORD * 4}, "finish_reason": "stop"}
                    ],
                    "usage": {"prompt_tokens": 100, "completion_tokens": TOKENS, "total_tokens": 100 + TOKENS},
                }
            ).encode()
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return

        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        # No content-length on a stream: signal end-of-body by closing.
        self.close_connection = True

        def frame(obj):
            try:
                self.wfile.write(b"data: " + json.dumps(obj).encode() + b"\n\n")
                self.wfile.flush()
                mark_write()
            except (BrokenPipeError, ConnectionResetError):
                raise _Disconnected()

        try:
            time.sleep(TTFT_MS / 1000.0)
            frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                   "choices": [{"index": 0, "delta": {"role": "assistant"}, "finish_reason": None}]})
            if want_tools:
                # Emit a function call with the arguments split across frames so
                # the tool-argument reassembly path is exercised.
                frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                       "choices": [{"index": 0, "delta": {"tool_calls": [
                           {"index": 0, "id": "call_syn_1", "type": "function",
                            "function": {"name": "get_weather", "arguments": ""}}]},
                           "finish_reason": None}]})
                frags = (
                    ['{"city"', ': "Par', 'is"}']
                    if tool_fragments <= 0
                    else _split_json_fragments(tool_fragments)
                )
                for frag in frags:
                    frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                           "choices": [{"index": 0, "delta": {"tool_calls": [
                               {"index": 0, "function": {"arguments": frag}}]},
                               "finish_reason": None}]})
                frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                       "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})
                frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                       "choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": 12,
                                                "total_tokens": 112}})
                self.wfile.write(b"data: [DONE]\n\n")
                self.wfile.flush()
                return
            for _ in range(TOKENS):
                frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                       "choices": [{"index": 0, "delta": {"content": WORD}, "finish_reason": None}]})
                if DELAY_MS:
                    time.sleep(DELAY_MS / 1000.0)
            frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                   "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})
            frame({"id": "syn-1", "object": "chat.completion.chunk", "model": model,
                   "choices": [], "usage": {"prompt_tokens": 100, "completion_tokens": TOKENS,
                                            "total_tokens": 100 + TOKENS}})
            self.wfile.write(b"data: [DONE]\n\n")
            self.wfile.flush()
        except (_Disconnected, BrokenPipeError, ConnectionResetError):
            pass

    # -- Gemini SSE -------------------------------------------------------
    def _gemini(self, model, want_tools=False):
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("cache-control", "no-cache")
        self.send_header("connection", "close")
        self.end_headers()
        self.close_connection = True

        def frame(obj):
            try:
                self.wfile.write(b"data: " + json.dumps(obj).encode() + b"\r\n\r\n")
                self.wfile.flush()
                mark_write()
            except (BrokenPipeError, ConnectionResetError):
                raise _Disconnected()

        try:
            time.sleep(TTFT_MS / 1000.0)
            if want_tools:
                frame({"candidates": [{"content": {"role": "model", "parts": [{"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}}]}}]})
                frame({"candidates": [{"content": {"role": "model", "parts": []}, "finishReason": "STOP"}],
                       "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 10,
                                         "totalTokenCount": 110}})
                return
            for _ in range(TOKENS):
                frame({"candidates": [{"content": {"role": "model", "parts": [{"text": WORD}]}}]})
                if DELAY_MS:
                    time.sleep(DELAY_MS / 1000.0)
            frame({"candidates": [{"content": {"role": "model", "parts": []}, "finishReason": "STOP"}],
                   "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": TOKENS,
                                     "totalTokenCount": 100 + TOKENS}})
        except (_Disconnected, BrokenPipeError, ConnectionResetError):
            pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9099
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    srv.daemon_threads = True
    srv.serve_forever()


if __name__ == "__main__":
    main()
