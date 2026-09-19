import React, { useState } from 'react';
import { Server, Plus, RefreshCw, CheckCircle2, Globe, Cpu, Sliders, ExternalLink, HelpCircle, Trash2, X } from 'lucide-react';
import { Provider, ModelConfig } from '../../types';
import { WobblyCard, SketchButton, SketchBadge } from '../HandDrawnElements';
import { DESIGN_TOKENS } from '../../lib/designSystem';
import { Kinetix, DiscoveredModel } from '../../lib/resources';

interface ProvidersViewProps {
  providers: Provider[];
  models: ModelConfig[];
  onAddProvider: (provider: Provider) => void;
  onAddModel: (model: ModelConfig) => void;
  onDeleteModel: (modelId: string) => void;
  onDeleteProvider: (providerId: string) => void;
  onRefresh?: () => void;
}

export const ProvidersView: React.FC<ProvidersViewProps> = ({
  providers,
  models,
  onAddProvider,
  onAddModel,
  onDeleteModel,
  onDeleteProvider,
  onRefresh,
}) => {
  const [selectedProviderId, setSelectedProviderId] = useState<string>(providers[0]?.id || '');
  const [showAddProviderModal, setShowAddProviderModal] = useState(false);
  const [showAddModelModal, setShowAddModelModal] = useState(false);
  const [confirmDeleteModelId, setConfirmDeleteModelId] = useState<string | null>(null);
  const [confirmDeleteProviderId, setConfirmDeleteProviderId] = useState<string | null>(null);
  const [isDiscovering, setIsDiscovering] = useState(false);
  const [discoveryResults, setDiscoveryResults] = useState<DiscoveredModel[] | null>(null);
  const [pingStatus, setPingStatus] = useState<Record<string, { ok: boolean; pingMs: number; error?: string }>>({});

  // New Provider Form State
  const [name, setName] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [wireFormat, setWireFormat] = useState<'gemini' | 'openai' | 'anthropic'>('gemini');
  const [authScheme, setAuthScheme] = useState<'bearer' | 'custom_header' | 'query_param'>('query_param');
  const [customHeader, setCustomHeader] = useState('');

  // New Custom Model Form State
  const [modelUpstreamId, setModelUpstreamId] = useState('');
  const [modelDisplayName, setModelDisplayName] = useState('');
  const [modelContextWindow, setModelContextWindow] = useState(128000);
  const [modelMaxOutput, setModelMaxOutput] = useState(8192);
  const [modelInputPrice, setModelInputPrice] = useState(1.0);
  const [modelOutputPrice, setModelOutputPrice] = useState(4.0);
  const [capText, setCapText] = useState(true);
  const [capVision, setCapVision] = useState(true);
  const [capReasoning, setCapReasoning] = useState(false);
  const [capTools, setCapTools] = useState(true);

  const activeProvider = providers.find((p) => p.id === selectedProviderId) || providers[0];
  const providerModels = models.filter((m) => m.providerId === activeProvider?.id);

  const handleTestPing = async (providerId: string) => {
    const firstModel = models.find((m) => m.providerId === providerId)?.upstreamModelId;
    try {
      const r = await Kinetix.test(providerId, firstModel || 'test');
      setPingStatus((prev) => ({
        ...prev,
        [providerId]: { ok: r.ok, pingMs: r.latency_ms ?? 0, error: r.error },
      }));
    } catch (e) {
      setPingStatus((prev) => ({
        ...prev,
        [providerId]: { ok: false, pingMs: 0, error: e instanceof Error ? e.message : String(e) },
      }));
    }
  };

  const handleFetchModelsDiscovery = async () => {
    if (!activeProvider) return;
    setIsDiscovering(true);
    setDiscoveryResults(null);
    try {
      const found = await Kinetix.discover(activeProvider.id);
      setDiscoveryResults(found);
    } catch (e) {
      setDiscoveryResults([]);
      setPingStatus((prev) => ({
        ...prev,
        [activeProvider.id]: {
          ok: false,
          pingMs: 0,
          error: e instanceof Error ? e.message : String(e),
        },
      }));
    } finally {
      setIsDiscovering(false);
    }
  };

  const handleImportDiscoveredModel = (m: DiscoveredModel) => {
    const newModel: ModelConfig = {
      id: '',
      providerId: activeProvider.id,
      providerName: activeProvider.name,
      upstreamModelId: m.id,
      displayName: m.display_name || m.id,
      enabled: true,
      contextWindow: m.context_window ?? 128000,
      maxOutputTokens: m.max_output_tokens ?? 8192,
      capabilities: {
        text: true,
        vision: false,
        reasoning: m.id.includes('pro') || m.id.includes('thinking'),
        toolCalling: true,
        audio: false,
      },
      prices: {
        inputPer1M: 0,
        outputPer1M: 0,
        cachedPer1M: 0,
        thinkingPer1M: 0,
      },
      parameters: {},
      thinkingMap: { scale: 'off', mappedField: '' },
    };

    onAddModel(newModel);
    setDiscoveryResults((prev) => prev?.filter((x) => x.id !== m.id) || null);
  };

  const handleCreateProvider = (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !baseUrl.trim()) return;

    const newProv: Provider = {
      id: '',
      name: name.trim(),
      baseUrl: baseUrl.trim(),
      wireFormat,
      authScheme,
      customHeaderName: authScheme === 'custom_header' ? customHeader : undefined,
      status: 'healthy',
      modelsCount: 0,
      accountsCount: 1,
      timeoutMs: 60000,
      capabilityMode: 'permissive',
      lastPingMs: 0,
    };

    onAddProvider(newProv);
    setSelectedProviderId(newProv.id);
    setShowAddProviderModal(false);
    setName('');
    setBaseUrl('');
  };

  const handleCreateCustomModel = (e: React.FormEvent) => {
    e.preventDefault();
    if (!modelUpstreamId.trim()) return;

    const newModel: ModelConfig = {
      id: '',
      providerId: activeProvider.id,
      providerName: activeProvider.name,
      upstreamModelId: modelUpstreamId.trim(),
      displayName: modelDisplayName.trim() || modelUpstreamId.trim(),
      enabled: true,
      contextWindow: Number(modelContextWindow) || 128000,
      maxOutputTokens: Number(modelMaxOutput) || 8192,
      capabilities: {
        text: capText,
        vision: capVision,
        reasoning: capReasoning,
        toolCalling: capTools,
        audio: false,
      },
      prices: {
        inputPer1M: Number(modelInputPrice) || 0,
        outputPer1M: Number(modelOutputPrice) || 0,
        cachedPer1M: Number(modelInputPrice ? (modelInputPrice * 0.25).toFixed(2) : 0),
        thinkingPer1M: capReasoning ? Number(modelOutputPrice) || 0 : 0,
      },
      parameters: {},
      thinkingMap: {
        scale: capReasoning ? 'medium' : 'off',
        mappedField: capReasoning ? 'thinkingConfig' : '',
      },
    };

    onAddModel(newModel);
    setShowAddModelModal(false);
    setModelUpstreamId('');
    setModelDisplayName('');
  };

  return (
    <div className="space-y-6">
      {/* Top Banner */}
      <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
            <span>Upstream Providers & Models</span>
            <SketchBadge variant="yellow" rotation="-1deg">
              No Vendor Presets (FR-10)
            </SketchBadge>
          </h2>
          <p className="text-base font-body text-[#2d2d2d]/80">
            Configure upstream LLM APIs, fetch model lists, define parameter clamping, and map thinking controls.
          </p>
        </div>

        <SketchButton
          variant="primary"
          size="md"
          onClick={() => setShowAddProviderModal(true)}
          className="gap-2 font-heading font-bold"
        >
          <Plus className="w-5 h-5" />
          Add Upstream Provider
        </SketchButton>
      </div>

      {/* Main layout */}
      {providers.length === 0 ? (
        <WobblyCard decoration="tack" className="p-10 text-center bg-white">
          <Server className="w-12 h-12 text-[#2d5da1] mx-auto mb-3 opacity-60" />
          <h3 className="text-2xl font-heading font-bold text-[#2d2d2d]">No Upstream Providers Configured</h3>
          <p className="text-base font-body text-[#2d2d2d]/80 max-w-lg mx-auto mt-2 mb-6">
            Register your upstream LLM providers (e.g. Gemini, OpenAI, Anthropic, DeepSeek, or local Ollama). Kinetix proxies client calls and maps protocols automatically.
          </p>
          <SketchButton
            variant="primary"
            size="md"
            onClick={() => setShowAddProviderModal(true)}
            className="gap-2 font-heading font-bold"
          >
            <Plus className="w-5 h-5" />
            Add First Upstream Provider
          </SketchButton>
        </WobblyCard>
      ) : (
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Left: Providers Selector */}
          <div className="space-y-4">
            <h3 className="text-xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
              <Server className="w-5 h-5 text-[#2d5da1]" />
              Configured Upstreams ({providers.length})
            </h3>

            {providers.map((prov, idx) => {
              const isSelected = prov.id === activeProvider?.id;
              const tilt = idx % 2 === 0 ? '-rotate-0.5' : 'rotate-0.5';
              const ping = pingStatus[prov.id];

              return (
                <div
                  key={prov.id}
                  onClick={() => {
                    setSelectedProviderId(prov.id);
                    setDiscoveryResults(null);
                  }}
                  className={`p-4 border-2 border-[#2d2d2d] cursor-pointer transition-all ${tilt} ${
                    isSelected
                      ? 'bg-[#fff9c4] sketch-shadow -translate-y-1 font-bold'
                      : 'bg-white hover:bg-[#f4efe8] sketch-shadow-sm'
                  }`}
                  style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <span className="font-mono text-xs px-2 py-0.5 bg-white border border-[#2d2d2d] rounded uppercase">
                        {prov.wireFormat} wire
                      </span>
                      <h4 className="font-heading text-lg mt-1 text-[#2d2d2d]">{prov.name}</h4>
                      <p className="text-xs font-mono text-[#2d2d2d]/70 truncate max-w-[200px]">
                        {prov.baseUrl}
                      </p>
                    </div>

                    <div className="flex flex-col items-end gap-1">
                      <SketchBadge variant={prov.status === 'healthy' ? 'green' : prov.status === 'degraded' ? 'yellow' : 'red'}>
                        {prov.status}
                      </SketchBadge>
                      {ping && (
                        <span className={`text-xs font-mono ${ping.ok ? 'text-[#2e7d32]' : 'text-[#ff4d4d]'}`}>
                          {ping.ok ? `${ping.pingMs}ms` : 'error'}
                        </span>
                      )}
                    </div>
                  </div>

                  <div className="mt-3 pt-2 border-t border-[#2d2d2d]/20 flex items-center justify-between text-xs font-mono">
                    <span>Auth: <strong>{prov.authScheme}</strong></span>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleTestPing(prov.id);
                      }}
                      className="hover:underline text-[#2d5da1] cursor-pointer"
                    >
                      {pingStatus[prov.id] ? (pingStatus[prov.id].ok ? '⚡ OK' : '⚡ Failed') : '⚡ Test Ping'}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          {/* Right: Active Provider & Models Editor */}
          {activeProvider && (
            <div className="lg:col-span-2 space-y-6">
              <WobblyCard decoration="tape" className="p-6">
                {/* Provider Info Header */}
                <div className="flex flex-wrap items-start justify-between gap-4 pb-4 border-b-2 border-dashed border-[#2d2d2d]/30 mb-4">
                  <div>
                    <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
                      <Globe className="w-6 h-6 text-[#2d5da1]" />
                      {activeProvider.name}
                    </h3>
                    <code className="text-sm font-mono text-[#2d2d2d]/80 bg-[#e5e0d8] px-2 py-0.5 rounded border border-[#2d2d2d]/30 inline-block mt-1">
                      Base URL: {activeProvider.baseUrl}
                    </code>
                  </div>

                  <div className="flex flex-wrap items-center gap-2">
                    <SketchButton
                      variant="secondary"
                      size="sm"
                      disabled={isDiscovering}
                      onClick={handleFetchModelsDiscovery}
                      className="gap-1.5 font-heading"
                    >
                      <RefreshCw className={`w-4 h-4 ${isDiscovering ? 'animate-spin' : ''}`} />
                      {isDiscovering ? 'Querying Upstream...' : 'Fetch Models (Discovery)'}
                    </SketchButton>
                    <SketchButton
                      variant="primary"
                      size="sm"
                      onClick={() => setShowAddModelModal(true)}
                      className="gap-1 font-heading font-bold"
                    >
                      <Plus className="w-4 h-4" />
                      Add Model
                    </SketchButton>

                    {confirmDeleteProviderId === activeProvider.id ? (
                      <div className="flex items-center gap-1 bg-[#ffebee] px-2.5 py-1 border border-[#ff4d4d] rounded text-xs font-heading">
                        <span className="text-[#b71c1c] font-bold">Delete {activeProvider.name}?</span>
                        <button
                          onClick={() => {
                            onDeleteProvider(activeProvider.id);
                            setConfirmDeleteProviderId(null);
                          }}
                          className="px-2 py-0.5 bg-[#ff4d4d] text-white rounded font-bold hover:bg-[#d32f2f] cursor-pointer"
                        >
                          Confirm
                        </button>
                        <button
                          onClick={() => setConfirmDeleteProviderId(null)}
                          className="px-2 py-0.5 bg-white border border-[#2d2d2d] rounded hover:bg-[#e5e0d8] cursor-pointer"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setConfirmDeleteProviderId(activeProvider.id)}
                        className="px-2.5 py-1 text-xs font-heading font-bold text-[#ff4d4d] hover:bg-[#ffebee] border border-[#ff4d4d]/50 hover:border-[#ff4d4d] rounded flex items-center gap-1 cursor-pointer transition-colors"
                        title="Delete this upstream provider"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                        <span>Delete Provider</span>
                      </button>
                    )}
                  </div>
                </div>

              {/* Model Discovery Results (if any) */}
              {discoveryResults && (
                <div className="p-4 bg-[#fff9c4] border-2 border-[#2d2d2d] sketch-shadow-sm mb-6 rounded-lg">
                  <h4 className="font-heading font-bold text-lg text-[#2d2d2d] mb-1">
                    🔍 Discovered Upstream Models (Live Probe)
                  </h4>
                  <p className="text-sm font-body text-[#2d2d2d]/80 mb-3">
                    The endpoint returned the following model IDs. Select which models to import into Kinetix:
                  </p>
                  <div className="flex flex-wrap gap-2">
                    {discoveryResults.length === 0 && (
                      <span className="text-sm font-mono text-[#b71c1c]">
                        No models returned (check the credential or the error above).
                      </span>
                    )}
                    {discoveryResults.map((m) => (
                      <div
                        key={m.id}
                        className="bg-white border-2 border-[#2d2d2d] px-3 py-1.5 text-xs font-mono sketch-shadow-sm flex items-center gap-2 rounded"
                      >
                        <span className="font-bold">{m.id}</span>
                        {m.context_window ? (
                          <span className="text-[#2d2d2d]/50">{m.context_window.toLocaleString()} ctx</span>
                        ) : null}
                        {m.already_imported ? (
                          <span className="text-[#2e7d32] font-bold">✓ imported</span>
                        ) : (
                          <button
                            onClick={() => handleImportDiscoveredModel(m)}
                            className="bg-[#2e7d32] text-white px-2 py-0.5 rounded hover:bg-[#1b5e20] cursor-pointer"
                          >
                            + Import
                          </button>
                        )}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Models List for this Provider */}
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <h4 className="text-xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
                    <Cpu className="w-5 h-5 text-[#ff4d4d]" />
                    Configured Models ({providerModels.length})
                  </h4>
                  {providerModels.length > 0 && (
                    <button
                      onClick={() => setShowAddModelModal(true)}
                      className="text-xs font-heading font-bold text-[#2d5da1] hover:underline flex items-center gap-1 cursor-pointer"
                    >
                      <Plus className="w-3.5 h-3.5" />
                      Configure Another Model
                    </button>
                  )}
                </div>

                {providerModels.length === 0 ? (
                  <div className="p-8 text-center bg-white border-2 border-dashed border-[#2d2d2d]/30 rounded-lg">
                    <Cpu className="w-10 h-10 text-[#2d2d2d]/40 mx-auto mb-2" />
                    <p className="font-heading font-bold text-lg text-[#2d2d2d]">No Models Configured</p>
                    <p className="text-sm font-body text-[#2d2d2d]/70 max-w-md mx-auto mt-1 mb-4">
                      Probe upstream models via live discovery or manually register custom upstream model IDs for this provider.
                    </p>
                    <div className="flex items-center justify-center gap-3">
                      <SketchButton
                        variant="secondary"
                        size="sm"
                        onClick={handleFetchModelsDiscovery}
                        disabled={isDiscovering}
                      >
                        Fetch Models (Discovery)
                      </SketchButton>
                      <SketchButton
                        variant="primary"
                        size="sm"
                        onClick={() => setShowAddModelModal(true)}
                      >
                        + Add Custom Model
                      </SketchButton>
                    </div>
                  </div>
                ) : (
                  <div className="space-y-3">
                    {providerModels.map((m) => (
                      <div
                        key={m.id}
                        className="p-4 bg-white border-2 border-[#2d2d2d] sketch-shadow-sm rounded-lg"
                      >
                        <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-3 border-b border-[#2d2d2d]/20 pb-2 mb-3">
                          <div>
                            <div className="flex items-center gap-2 flex-wrap">
                              <span className="font-heading font-bold text-lg text-[#2d2d2d]">
                                {m.displayName}
                              </span>
                              <span className="text-xs font-mono bg-[#e5e0d8] px-1.5 py-0.5 rounded border border-[#2d2d2d]/40">
                                id: {m.upstreamModelId}
                              </span>
                            </div>
                            <span className="text-xs font-mono text-[#2d2d2d]/70">
                              Context: {m.contextWindow.toLocaleString()} tokens • Max Output: {m.maxOutputTokens}
                            </span>
                          </div>

                          {/* Capabilities badges & Delete button */}
                          <div className="flex flex-wrap items-center gap-2">
                            <div className="flex flex-wrap items-center gap-1">
                              {m.capabilities.text && <SketchBadge variant="default">Text</SketchBadge>}
                              {m.capabilities.vision && <SketchBadge variant="blue">Vision</SketchBadge>}
                              {m.capabilities.reasoning && <SketchBadge variant="yellow">Reasoning</SketchBadge>}
                              {m.capabilities.toolCalling && <SketchBadge variant="green">Tools</SketchBadge>}
                            </div>

                            {confirmDeleteModelId === m.id ? (
                              <div className="flex items-center gap-1 bg-[#ffebee] px-2 py-1 border border-[#ff4d4d] rounded text-xs font-heading">
                                <span className="text-[#b71c1c] font-bold">Remove model?</span>
                                <button
                                  onClick={() => {
                                    onDeleteModel(m.id);
                                    setConfirmDeleteModelId(null);
                                  }}
                                  className="px-2 py-0.5 bg-[#ff4d4d] text-white rounded font-bold hover:bg-[#d32f2f] cursor-pointer"
                                >
                                  Delete
                                </button>
                                <button
                                  onClick={() => setConfirmDeleteModelId(null)}
                                  className="px-2 py-0.5 bg-white border border-[#2d2d2d] rounded hover:bg-[#e5e0d8] cursor-pointer"
                                >
                                  Cancel
                                </button>
                              </div>
                            ) : (
                              <button
                                onClick={() => setConfirmDeleteModelId(m.id)}
                                className="px-2 py-1 text-xs font-heading font-bold text-[#ff4d4d] hover:bg-[#ffebee] border border-[#ff4d4d]/40 hover:border-[#ff4d4d] rounded flex items-center gap-1 cursor-pointer transition-colors"
                                title="Remove model from provider"
                              >
                                <Trash2 className="w-3.5 h-3.5" />
                                <span>Remove</span>
                              </button>
                            )}
                          </div>
                        </div>

                        {/* Prices & Parameter policies */}
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs font-mono">
                          <div className="bg-[#fdfbf7] p-2 border border-[#2d2d2d] rounded">
                            <strong className="font-heading text-sm text-[#2d2d2d] block mb-1">
                              💵 Token Pricing (Admin Defined)
                            </strong>
                            <div>Input: ${m.prices.inputPer1M} / 1M</div>
                            <div>Output: ${m.prices.outputPer1M} / 1M</div>
                            <div>Cached: ${m.prices.cachedPer1M} / 1M</div>
                            {m.capabilities.reasoning && (
                              <div>Thinking: ${m.prices.thinkingPer1M} / 1M</div>
                            )}
                          </div>

                          <div className="bg-[#fdfbf7] p-2 border border-[#2d2d2d] rounded">
                            <strong className="font-heading text-sm text-[#2d2d2d] block mb-1">
                              ⚙️ Parameter & Thinking Controls
                            </strong>
                            <div>Temperature Policy: <strong>Clamp (0.0 - 2.0)</strong></div>
                            <div>
                              Thinking Scale:{' '}
                              <strong className="text-[#2d5da1]">{m.thinkingMap.scale}</strong>
                            </div>
                            <div className="truncate">
                              Mapped Field: <code>{m.thinkingMap.mappedField}</code>
                            </div>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </WobblyCard>
          </div>
        )}
      </div>
    )}

      {/* Add Provider Modal */}
      {showAddProviderModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
          <div className="w-full max-w-lg">
            <WobblyCard decoration="tape" className="bg-[#fdfbf7] p-6 relative">
              <button
                onClick={() => setShowAddProviderModal(false)}
                className="absolute top-4 right-4 text-[#2d2d2d] font-bold text-xl hover:text-[#ff4d4d] cursor-pointer"
              >
                ✕
              </button>

              <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-4 flex items-center gap-2">
                <Server className="w-6 h-6 text-[#2d5da1]" />
                Add Upstream Provider (No Presets)
              </h3>

              <form onSubmit={handleCreateProvider} className="space-y-4 font-body">
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Provider Name
                  </label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. Google Gemini, Mistral, Local vLLM"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                  />
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Endpoint Base URL
                  </label>
                  <input
                    type="url"
                    required
                    placeholder="https://api.openai.com/v1 or custom host"
                    value={baseUrl}
                    onChange={(e) => setBaseUrl(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobbly }}
                  />
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Wire Format
                    </label>
                    <select
                      value={wireFormat}
                      onChange={(e) => setWireFormat(e.target.value as any)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none font-mono"
                      style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                    >
                      <option value="gemini">Gemini API</option>
                      <option value="openai">OpenAI Compatible</option>
                      <option value="anthropic">Anthropic Messages</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Auth Scheme
                    </label>
                    <select
                      value={authScheme}
                      onChange={(e) => setAuthScheme(e.target.value as any)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none font-mono"
                      style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                    >
                      <option value="query_param">Query Param (?key=...)</option>
                      <option value="bearer">Bearer Header</option>
                      <option value="custom_header">Custom Header (e.g. x-api-key)</option>
                    </select>
                  </div>
                </div>

                {authScheme === 'custom_header' && (
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Custom Header Name
                    </label>
                    <input
                      type="text"
                      placeholder="e.g. x-api-key"
                      value={customHeader}
                      onChange={(e) => setCustomHeader(e.target.value)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    />
                  </div>
                )}

                <div className="pt-2 flex justify-end gap-3">
                  <SketchButton
                    type="button"
                    variant="ghost"
                    onClick={() => setShowAddProviderModal(false)}
                  >
                    Cancel
                  </SketchButton>
                  <SketchButton type="submit" variant="danger" className="font-bold">
                    Save Provider
                  </SketchButton>
                </div>
              </form>
            </WobblyCard>
          </div>
        </div>
      )}

      {/* Add Model Modal */}
      {showAddModelModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
          <div className="w-full max-w-lg max-h-[90vh] overflow-y-auto">
            <WobblyCard decoration="tack" className="bg-[#fdfbf7] p-6 relative">
              <button
                onClick={() => setShowAddModelModal(false)}
                className="absolute top-4 right-4 text-[#2d2d2d] font-bold text-xl hover:text-[#ff4d4d] cursor-pointer"
              >
                ✕
              </button>

              <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-1 flex items-center gap-2">
                <Cpu className="w-6 h-6 text-[#ff4d4d]" />
                Configure Model for {activeProvider.name}
              </h3>
              <p className="text-sm font-body text-[#2d2d2d]/80 mb-4">
                Define the model identifier, token capabilities, and per-million token pricing.
              </p>

              <form onSubmit={handleCreateCustomModel} className="space-y-4">
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Upstream Model ID (Wire Name)
                  </label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. gemini-2.5-flash, claude-3-7-sonnet, gpt-4o"
                    value={modelUpstreamId}
                    onChange={(e) => setModelUpstreamId(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                  />
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Display Name
                  </label>
                  <input
                    type="text"
                    placeholder="e.g. Gemini 2.5 Flash (Production)"
                    value={modelDisplayName}
                    onChange={(e) => setModelDisplayName(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobbly }}
                  />
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Context Window
                    </label>
                    <input
                      type="number"
                      required
                      min={1000}
                      step={1000}
                      value={modelContextWindow}
                      onChange={(e) => setModelContextWindow(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    />
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Max Output
                    </label>
                    <input
                      type="number"
                      required
                      min={100}
                      value={modelMaxOutput}
                      onChange={(e) => setModelMaxOutput(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    />
                  </div>
                </div>

                {/* Token Pricing */}
                <div className="grid grid-cols-2 gap-3 bg-[#f4efe8] p-3 border border-[#2d2d2d] rounded">
                  <div>
                    <label className="block text-xs font-heading font-bold text-[#2d2d2d] mb-1">
                      Input Price ($ / 1M)
                    </label>
                    <input
                      type="number"
                      step={0.01}
                      min={0}
                      value={modelInputPrice}
                      onChange={(e) => setModelInputPrice(Number(e.target.value))}
                      className="w-full bg-white border border-[#2d2d2d] px-2 py-1 text-sm font-mono focus:outline-none rounded"
                    />
                  </div>
                  <div>
                    <label className="block text-xs font-heading font-bold text-[#2d2d2d] mb-1">
                      Output Price ($ / 1M)
                    </label>
                    <input
                      type="number"
                      step={0.01}
                      min={0}
                      value={modelOutputPrice}
                      onChange={(e) => setModelOutputPrice(Number(e.target.value))}
                      className="w-full bg-white border border-[#2d2d2d] px-2 py-1 text-sm font-mono focus:outline-none rounded"
                    />
                  </div>
                </div>

                {/* Capabilities */}
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-2">
                    Model Capabilities
                  </label>
                  <div className="grid grid-cols-2 gap-2 text-sm font-body">
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={capText}
                        onChange={(e) => setCapText(e.target.checked)}
                        className="w-4 h-4 accent-[#ff4d4d]"
                      />
                      <span>Text Generation</span>
                    </label>
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={capVision}
                        onChange={(e) => setCapVision(e.target.checked)}
                        className="w-4 h-4 accent-[#ff4d4d]"
                      />
                      <span>Vision / Multimodal</span>
                    </label>
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={capReasoning}
                        onChange={(e) => setCapReasoning(e.target.checked)}
                        className="w-4 h-4 accent-[#ff4d4d]"
                      />
                      <span>Reasoning / Thinking</span>
                    </label>
                    <label className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={capTools}
                        onChange={(e) => setCapTools(e.target.checked)}
                        className="w-4 h-4 accent-[#ff4d4d]"
                      />
                      <span>Tool Calling / JSON</span>
                    </label>
                  </div>
                </div>

                <div className="pt-2 flex justify-end gap-3">
                  <SketchButton
                    type="button"
                    variant="ghost"
                    onClick={() => setShowAddModelModal(false)}
                  >
                    Cancel
                  </SketchButton>
                  <SketchButton type="submit" variant="primary" className="font-bold">
                    Save Model Configuration
                  </SketchButton>
                </div>
              </form>
            </WobblyCard>
          </div>
        </div>
      )}
    </div>
  );
};
