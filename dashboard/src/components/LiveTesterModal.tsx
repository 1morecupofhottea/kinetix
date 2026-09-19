import React, { useEffect, useRef, useState } from 'react';
import { X, Play, RefreshCw, Zap, CheckCircle2, ArrowRight, AlertTriangle } from 'lucide-react';
import { VirtualKey, Route, ModelConfig } from '../types';
import { WobblyCard, SketchButton, SketchBadge } from './HandDrawnElements';

interface LiveTesterModalProps {
  isOpen: boolean;
  onClose: () => void;
  keys: VirtualKey[];
  routes: Route[];
  models: ModelConfig[];
}

interface ExecMeta {
  servingAccount: string;
  servingProvider: string;
  routeId: string;
  fallbackHops: number;
  fallbackPath: string[];
  warnings: string[];
  ttftMs: number;
  latencyMs: number;
  statusCode: number;
}

/**
 * Sends a real request through the Kinetix pipeline via the admin-authenticated
 * `/admin/api/test-stream` endpoint. The chosen virtual key is identified by id;
 * the raw key never leaves the server (it is stored hashed). The SSE frames are
 * parsed back into text for display, and the Kinetix response headers are read off
 * the live response.
 */
export const LiveTesterModal: React.FC<LiveTesterModalProps> = ({
  isOpen,
  onClose,
  keys,
  routes,
  models,
}) => {
  const [selectedKeyId, setSelectedKeyId] = useState(keys[0]?.id || '');
  const [authMode, setAuthMode] = useState<'admin' | 'raw'>('admin');
  const [rawKey, setRawKey] = useState('');
  const [protocol, setProtocol] = useState<'openai' | 'anthropic'>('openai');
  const [target, setTarget] = useState<string>('');
  const [prompt, setPrompt] = useState(
    'Write a quick Rust function to calculate exponential backoff for an LLM pool key.',
  );
  const [stream, setStream] = useState(true);
  const [isLoading, setIsLoading] = useState(false);
  const [output, setOutput] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [meta, setMeta] = useState<ExecMeta | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Keys/routes/models arrive asynchronously; default the selection once they load.
  useEffect(() => {
    if (!selectedKeyId && keys.length > 0) setSelectedKeyId(keys[0].id);
  }, [keys, selectedKeyId]);

  if (!isOpen) return null;

  const defaultTarget =
    routes.find((c) => c.targets.length > 0)?.name || models[0]?.upstreamModelId || '';
  const effectiveTarget = target || defaultTarget;
  const useRaw = authMode === 'raw';

  const handleRunTest = async () => {
    setIsLoading(true);
    setOutput('');
    setError(null);
    setMeta(null);

    const controller = new AbortController();
    abortRef.current = controller;
    const started = performance.now();
    let ttft = 0;
    let accumulated = '';

    // Two modes (both hit the real pipeline):
    //  - admin: server-side /admin/api/test-stream, keyed by virtual-key id, so
    //    the raw secret never touches the browser (keys are stored hashed).
    //  - raw:   the browser calls the public /v1 surface directly with a pasted
    //    sk-kinetix-... key, exactly as a client like Pi would.
    const url = useRaw
      ? protocol === 'openai'
        ? '/v1/chat/completions'
        : '/v1/messages'
      : '/admin/api/test-stream';

    const headers: Record<string, string> = { 'Content-Type': 'application/json' };
    let body: Record<string, unknown>;
    if (useRaw) {
      if (protocol === 'openai') {
        headers['Authorization'] = `Bearer ${rawKey.trim()}`;
      } else {
        headers['x-api-key'] = rawKey.trim();
        headers['anthropic-version'] = '2023-06-01';
      }
      body = {
        model: effectiveTarget,
        max_tokens: 512,
        stream,
        ...(protocol === 'openai'
          ? { messages: [{ role: 'user', content: prompt }] }
          : { messages: [{ role: 'user', content: prompt }] }),
      };
    } else {
      body = {
        key_id: selectedKeyId,
        model: effectiveTarget,
        prompt,
        format: protocol,
        stream,
        max_tokens: 512,
      };
    }

    try {
      const res = await fetch(url, {
        method: 'POST',
        credentials: 'same-origin',
        headers,
        signal: controller.signal,
        body: JSON.stringify(body),
      });

      const servedBy = parseServedBy(res.headers.get('x-kinetix-served-by') || '');
      const routeId = res.headers.get('x-kinetix-route-id') || '';
      const warnings = parseWarningsHeader(res.headers.get('x-kinetix-warning') || '');
      const fallback = res.headers.get('x-kinetix-fallback') || '';
      const parsedFallback = parseFallbackHeader(
        fallback,
        res.headers.get('x-kinetix-fallback-path') || '',
      );

      if (!res.ok) {
        const text = await res.text();
        let message = `HTTP ${res.status}`;
        try {
          const j = JSON.parse(text);
          message = j?.error?.message || j?.error || message;
        } catch {
          /* keep default */
        }
        setError(message);
        setMeta({
          servingAccount: servedBy.account,
          servingProvider: servedBy.provider,
          routeId,
          warnings,
          fallbackHops: parsedFallback.hops,
          fallbackPath: parsedFallback.path,
          ttftMs: 0,
          latencyMs: Math.round(performance.now() - started),
          statusCode: res.status,
        });
        return;
      }

      const contentType = res.headers.get('content-type') || '';
      if (contentType.includes('text/event-stream') && res.body) {
        const reader = res.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });
          const frames = buffer.split('\n\n');
          buffer = frames.pop() || '';
          for (const frame of frames) {
            const text = extractText(frame, protocol);
            if (text) {
              if (!ttft) ttft = Math.round(performance.now() - started);
              accumulated += text;
              setOutput(accumulated);
            }
          }
        }
      } else {
        const j = await res.json();
        accumulated = extractNonStreamText(j, protocol);
        setOutput(accumulated);
        ttft = Math.round(performance.now() - started);
      }

      setMeta({
        servingAccount: servedBy.account,
        servingProvider: servedBy.provider,
        routeId,
        warnings,
        fallbackHops: parsedFallback.hops,
        fallbackPath: parsedFallback.path,
        ttftMs: ttft,
        latencyMs: Math.round(performance.now() - started),
        statusCode: res.status,
      });
    } catch (e) {
      if ((e as Error).name !== 'AbortError') {
        setError(e instanceof Error ? e.message : String(e));
      }
    } finally {
      setIsLoading(false);
      abortRef.current = null;
    }
  };

  const handleStop = () => {
    abortRef.current?.abort();
    setIsLoading(false);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
      <div className="w-full max-w-4xl max-h-[92vh] overflow-y-auto">
        <WobblyCard decoration="tape" className="bg-[#fdfbf7] p-6 relative">
          <button
            onClick={onClose}
            className="absolute top-4 right-4 p-1 rounded-full border-2 border-[#2d2d2d] bg-white hover:bg-[#ff4d4d] hover:text-white transition-colors cursor-pointer"
          >
            <X className="w-6 h-6" />
          </button>

          <div className="flex items-center gap-3 mb-4">
            <div className="p-2 bg-[#ff4d4d] text-white border-2 border-[#2d2d2d] wobbly-circle -rotate-3">
              <Zap className="w-6 h-6" />
            </div>
            <div>
              <h2 className="text-2xl md:text-3xl font-heading font-bold text-[#2d2d2d]">
                Live Proxy Interactive Tester
              </h2>
              <p className="text-base text-[#2d2d2d]/80 font-body">
                Runs a real request through the Kinetix pipeline and streams the upstream result back.
              </p>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-6">
            {/* Control Panel */}
            <div className="space-y-4">
              <div>
                <label className="block text-base font-heading font-bold text-[#2d2d2d] mb-1">
                  1. Virtual Key (Authorization)
                </label>
                <div className="grid grid-cols-2 gap-2 mb-2">
                  <button
                    type="button"
                    onClick={() => setAuthMode('admin')}
                    className={`py-1 px-2 border-2 border-[#2d2d2d] text-xs font-heading cursor-pointer ${
                      authMode === 'admin' ? 'bg-[#2d5da1] text-white font-bold' : 'bg-white'
                    }`}
                    style={{ borderRadius: '120px 10px 100px 10px / 10px 100px 10px 120px' }}
                  >
                    Server-side (by key)
                  </button>
                  <button
                    type="button"
                    onClick={() => setAuthMode('raw')}
                    className={`py-1 px-2 border-2 border-[#2d2d2d] text-xs font-heading cursor-pointer ${
                      authMode === 'raw' ? 'bg-[#ff4d4d] text-white font-bold' : 'bg-white'
                    }`}
                    style={{ borderRadius: '120px 10px 100px 10px / 10px 100px 10px 120px' }}
                  >
                    Paste raw key → /v1
                  </button>
                </div>
                {authMode === 'admin' ? (
                  <select
                    value={selectedKeyId}
                    onChange={(e) => setSelectedKeyId(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none focus:border-[#2d5da1]"
                    style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                  >
                    {keys.map((k) => (
                      <option key={k.id} value={k.id}>
                        {k.name} ({k.tag})
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    type="text"
                    value={rawKey}
                    onChange={(e) => setRawKey(e.target.value)}
                    placeholder="sk-kinetix-..."
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-sm font-mono sketch-shadow-sm focus:outline-none focus:border-[#ff4d4d]"
                    style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                  />
                )}
              </div>

              <div>
                <label className="block text-base font-heading font-bold text-[#2d2d2d] mb-1">
                  2. Inbound Format (Client Wire)
                </label>
                <div className="grid grid-cols-2 gap-2">
                  <button
                    type="button"
                    onClick={() => setProtocol('openai')}
                    className={`py-1.5 px-2 border-2 border-[#2d2d2d] text-sm font-heading cursor-pointer text-center ${
                      protocol === 'openai'
                        ? 'bg-[#2d5da1] text-white sketch-shadow-sm font-bold'
                        : 'bg-white text-[#2d2d2d]'
                    }`}
                    style={{ borderRadius: '120px 10px 100px 10px / 10px 100px 10px 120px' }}
                  >
                    OpenAI (/v1/chat)
                  </button>
                  <button
                    type="button"
                    onClick={() => setProtocol('anthropic')}
                    className={`py-1.5 px-2 border-2 border-[#2d2d2d] text-sm font-heading cursor-pointer text-center ${
                      protocol === 'anthropic'
                        ? 'bg-[#ff4d4d] text-white sketch-shadow-sm font-bold'
                        : 'bg-white text-[#2d2d2d]'
                    }`}
                    style={{ borderRadius: '120px 10px 100px 10px / 10px 100px 10px 120px' }}
                  >
                    Anthropic (/v1/messages)
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-base font-heading font-bold text-[#2d2d2d] mb-1">
                  3. Requested Model or Route
                </label>
                <select
                  value={effectiveTarget}
                  onChange={(e) => setTarget(e.target.value)}
                  className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none focus:border-[#2d5da1]"
                  style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                >
                  {routes.length > 0 && (
                    <optgroup label="Routes (With Automatic Fallback)">
                      {routes.map((c) => (
                        <option key={c.id} value={c.name}>
                          ⚡ Route: {c.name} ({c.targets.length} pool targets)
                        </option>
                      ))}
                    </optgroup>
                  )}
                  <optgroup label="Direct Models">
                    {models.map((m) => (
                      <option key={m.id} value={m.upstreamModelId}>
                        {m.displayName} ({m.providerName})
                      </option>
                    ))}
                  </optgroup>
                </select>
              </div>

              <div
                className="p-3 bg-[#fff9c4] border-2 border-[#2d2d2d] sketch-shadow-sm"
                style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
              >
                <label className="flex items-center gap-2 cursor-pointer select-none">
                  <input
                    type="checkbox"
                    checked={stream}
                    onChange={(e) => setStream(e.target.checked)}
                    className="w-4 h-4 accent-[#ff4d4d]"
                  />
                  <span className="font-heading font-bold text-sm text-[#2d2d2d]">
                    Stream (SSE) response
                  </span>
                </label>
              </div>

              {isLoading ? (
                <SketchButton variant="secondary" size="lg" onClick={handleStop} className="w-full gap-2 font-heading font-bold">
                  <RefreshCw className="w-5 h-5 animate-spin" />
                  Stop Stream
                </SketchButton>
              ) : (
                <SketchButton
                  variant="danger"
                  size="lg"
                  disabled={useRaw ? !rawKey.trim() || !effectiveTarget : !selectedKeyId || !effectiveTarget}
                  onClick={handleRunTest}
                  className="w-full gap-2 font-heading font-bold"
                >
                  <Play className="w-5 h-5 fill-white" />
                  Send Request
                </SketchButton>
              )}
            </div>

            {/* Prompt & Output Panel */}
            <div className="md:col-span-2 space-y-4">
              <div>
                <label className="block text-base font-heading font-bold text-[#2d2d2d] mb-1">
                  Prompt (Pi Coding Agent format)
                </label>
                <textarea
                  rows={3}
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                  className="w-full bg-white border-2 border-[#2d2d2d] p-3 font-body text-base sketch-shadow-sm focus:outline-none focus:border-[#2d5da1] resize-none"
                  style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                />
              </div>

              <div>
                <div className="flex items-center justify-between mb-1">
                  <label className="text-base font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
                    <span>Live SSE Stream Result</span>
                    {isLoading && (
                      <span className="inline-flex items-center gap-1 text-xs text-[#ff4d4d] animate-pulse">
                        <span className="w-2 h-2 rounded-full bg-[#ff4d4d]" /> Receiving chunks
                      </span>
                    )}
                  </label>
                  {meta && (
                    <span className="text-xs text-[#2d2d2d]/70 font-mono">
                      TTFT: {meta.ttftMs}ms | Total: {meta.latencyMs}ms
                    </span>
                  )}
                </div>

                <div
                  className="w-full min-h-[160px] max-h-[240px] overflow-y-auto bg-white border-2 border-[#2d2d2d] p-3 font-mono text-sm sketch-shadow-sm whitespace-pre-wrap select-text"
                  style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                >
                  {error && (
                    <span className="text-[#b71c1c] flex items-start gap-2">
                      <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
                      {error}
                    </span>
                  )}
                  {!error && output.length === 0 && !isLoading && (
                    <span className="text-[#2d2d2d]/40 font-body text-base">
                      Click "Send Request" to run a live request through the proxy...
                    </span>
                  )}
                  {output}
                </div>
              </div>

              {meta && (
                <div
                  className="p-3 bg-[#e5e0d8]/50 border-2 border-[#2d2d2d] sketch-shadow-sm space-y-2 text-sm"
                  style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                >
                  <div className="flex flex-wrap items-center justify-between gap-2 border-b border-[#2d2d2d]/20 pb-2">
                    <div className="flex items-center gap-2">
                      <CheckCircle2 className="w-4 h-4 text-[#2e7d32]" />
                      <span className="font-heading font-bold text-base">
                        {meta.servingAccount ? `Served By: ${meta.servingAccount}` : `Route ID: ${meta.routeId || '(unknown)'}`}
                      </span>
                    </div>
                    {meta.fallbackHops > 0 ? (
                      <SketchBadge variant="red" rotation="-1deg">
                        ⚡ Fallback Recovered ({meta.fallbackHops} hop)
                      </SketchBadge>
                    ) : (
                      <SketchBadge variant="green" rotation="1deg">
                        Direct Primary Key
                      </SketchBadge>
                    )}
                  </div>

                  {meta.fallbackPath.length > 0 && (
                    <div className="text-xs font-mono text-[#2d2d2d] bg-white p-2 border border-[#2d2d2d] rounded">
                      <strong className="font-heading">Fallback Sequence:</strong>
                      <div className="flex flex-wrap items-center gap-1.5 mt-1">
                        {meta.fallbackPath.map((step, idx) => (
                          <React.Fragment key={idx}>
                            <span className={step.includes('429') ? 'text-[#ff4d4d] font-bold' : 'text-[#2e7d32]'}>
                              {step}
                            </span>
                            {idx < meta.fallbackPath.length - 1 && (
                              <ArrowRight className="w-3.5 h-3.5 text-[#2d2d2d]" />
                            )}
                          </React.Fragment>
                        ))}
                      </div>
                    </div>
                  )}

                  {meta.warnings.length > 0 && (
                    <div className="text-xs font-mono text-[#d97706] bg-[#fff9c4] p-2 border border-[#d97706] rounded">
                      <strong className="font-heading">⚠ Portability warning:</strong>
                      <div className="mt-1">{meta.warnings.join('; ')}</div>
                    </div>
                  )}

                  <div className="text-[10px] font-mono text-[#2d2d2d]/60 break-all">
                    Route ID (opaque): {meta.routeId || '(none)'}
                    {meta.routeId && (
                      <button
                        type="button"
                        onClick={async () => {
                          try {
                            const r = await fetch(
                              `/admin/api/route-traces/${meta.routeId}`,
                              { credentials: 'same-origin' },
                            );
                            const j = await r.json();
                            if (!r.ok) {
                              alert(j.error || `HTTP ${r.status}`);
                              return;
                            }
                            const steps = (j.steps || [])
                              .map((s: any) => `${s.stage}${s.target ? ` [${s.target}]` : ''} ${s.detail} (${s.elapsed_ms}ms)`)
                              .join('\n');
                            alert(
                              `Route Trace for ${j.opaque_route_id}\n` +
                                `request: ${j.request_id}\n` +
                                `outcome: ${j.outcome} | commit: ${j.commit_state}\n` +
                                `final: ${j.final_target || '(none)'}\n\n${steps}`,
                            );
                          } catch (e) {
                            alert((e as Error).message);
                          }
                        }}
                        className="ml-2 underline text-[#2d5da1]"
                      >
                        resolve trace
                      </button>
                    )}
                  </div>

                  <div className="grid grid-cols-3 gap-2 text-center text-xs font-mono pt-1">
                    <div className="bg-white p-1 border border-[#2d2d2d] rounded">
                      Status: <strong>{meta.statusCode}</strong>
                    </div>
                    <div className="bg-white p-1 border border-[#2d2d2d] rounded">
                      TTFT: <strong>{meta.ttftMs}ms</strong>
                    </div>
                    <div className="bg-white p-1 border border-[#2d2d2d] rounded">
                      Total: <strong>{meta.latencyMs}ms</strong>
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
        </WobblyCard>
      </div>
    </div>
  );
};

/** Extract display text from one SSE frame, format-aware. */
function extractText(frame: string, protocol: 'openai' | 'anthropic'): string {
  const dataLines = frame
    .split('\n')
    .filter((l) => l.startsWith('data:'))
    .map((l) => l.slice(5).trim());
  if (dataLines.length === 0) return '';
  const data = dataLines.join('\n');
  if (data === '[DONE]') return '';
  try {
    const j = JSON.parse(data);
    if (protocol === 'anthropic') {
      if (j.type === 'content_block_delta') {
        return j.delta?.text || j.delta?.thinking || '';
      }
      return '';
    }
    const delta = j?.choices?.[0]?.delta;
    return delta?.content || delta?.reasoning_content || '';
  } catch {
    return '';
  }
}

function extractNonStreamText(j: any, protocol: 'openai' | 'anthropic'): string {
  try {
    if (protocol === 'anthropic') {
      return (j.content || [])
        .map((b: any) => b.text || b.thinking || '')
        .join('');
    }
    return j?.choices?.[0]?.message?.content || '';
  } catch {
    return '';
  }
}

function parseFallbackHeader(
  value: string,
  pathHeader: string,
): { hops: number; path: string[] } {
  if (!value) return { hops: 0, path: [] };
  const hops = Number(value);
  let path: string[] = [];
  if (pathHeader) {
    try {
      const parsed = JSON.parse(pathHeader);
      if (Array.isArray(parsed)) path = parsed.map(String);
    } catch {
      /* ignore malformed trace */
    }
  }
  return { hops: isFinite(hops) ? hops : 0, path };
}

/** Split the `X-Kinetix-Served-By` header ("Account (Provider)") into its parts. */
function parseServedBy(value: string): { account: string; provider: string } {
  const m = value.match(/^(.*?)\s*\((.*)\)\s*$/);
  if (m) return { account: m[1].trim(), provider: m[2].trim() };
  return { account: value.trim(), provider: '' };
}

/** Parse the `X-Kinetix-Warning` JSON array of portability warnings. */
function parseWarningsHeader(value: string): string[] {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value);
    if (Array.isArray(parsed)) return parsed.map(String);
  } catch {
    /* ignore malformed */
  }
  return [value];
}
