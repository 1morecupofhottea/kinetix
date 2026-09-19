import React, { useState } from 'react';
import { Key, Plus, Copy, Check, ShieldAlert, Sparkles, Terminal, Trash2, Power } from 'lucide-react';
import { VirtualKey } from '../../types';
import { WobblyCard, SketchButton, SketchBadge } from '../HandDrawnElements';
import { formatCurrency, formatTokens } from '../../lib/designSystem';

interface KeysViewProps {
  keys: VirtualKey[];
  onAddKey: (newKey: VirtualKey) => Promise<{ key: VirtualKey; fullKey: string } | null>;
  onUpdateKeyStatus: (id: string, status: 'active' | 'disabled' | 'revoked') => void;
}

export const KeysView: React.FC<KeysViewProps> = ({ keys, onAddKey, onUpdateKeyStatus }) => {
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [copiedKeyId, setCopiedKeyId] = useState<string | null>(null);
  const [newlyCreatedKey, setNewlyCreatedKey] = useState<{ name: string; key: string } | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Form state
  const [name, setName] = useState('');
  const [owner, setOwner] = useState('');
  const [tag, setTag] = useState('');
  const [allowedModels, setAllowedModels] = useState('*');
  const [rpmLimit, setRpmLimit] = useState(60);
  const [tpmLimit, setTpmLimit] = useState(100000);
  const [dailyBudget, setDailyBudget] = useState(15.0);
  const [monthlyBudget, setMonthlyBudget] = useState(60.0);

  const handleCopy = (keyText: string, id: string) => {
    navigator.clipboard.writeText(keyText);
    setCopiedKeyId(id);
    setTimeout(() => setCopiedKeyId(null), 2000);
  };

  const handleCreateSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsSubmitting(true);
    setError(null);

    const newKeyObj: VirtualKey = {
      id: '',
      key: '',
      name: name.trim() || 'Untitled Key',
      owner: owner.trim() || 'Team Member',
      tag: tag.trim() || 'general',
      allowedModels: allowedModels
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean),
      rpmLimit: Number(rpmLimit) || 0,
      tpmLimit: Number(tpmLimit) || 0,
      dailyBudget: Number(dailyBudget) || 0,
      monthlyBudget: Number(monthlyBudget) || 0,
      currentDailySpend: 0,
      currentMonthlySpend: 0,
      createdAt: new Date().toISOString(),
      expiresAt: null,
      status: 'active',
      totalRequests: 0,
      totalTokens: 0,
    };

    const result = await onAddKey(newKeyObj);
    setIsSubmitting(false);
    if (!result) {
      setError('Failed to create key. See the banner for details.');
      return;
    }
    // The server returns the full key exactly once (FR-3.1).
    setNewlyCreatedKey({ name: result.key.name, key: result.fullKey });
    setShowCreateModal(false);
    setName('');
    setOwner('');
    setTag('');
  };

  return (
    <div className="space-y-6">
      {/* Top Banner & Action */}
      <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
            <span>Virtual Keys & Client Access</span>
            <SketchBadge variant="yellow" rotation="-1deg">
              {keys.filter((k) => k.status === 'active').length} Active
            </SketchBadge>
          </h2>
          <p className="text-base font-body text-[#2d2d2d]/80">
            Issue scoped credentials for developers and coding tools like Pi. Real upstream keys never leave Kinetix.
          </p>
        </div>

        <SketchButton
          variant="primary"
          size="md"
          onClick={() => setShowCreateModal(true)}
          className="gap-2 font-heading font-bold"
        >
          <Plus className="w-5 h-5" />
          Issue New Virtual Key
        </SketchButton>
      </div>

      {/* Newly Created Key Alert (Shown once!) */}
      {newlyCreatedKey && (
        <WobblyCard decoration="tape" variant="postit" className="p-4 bg-[#fff9c4]">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="flex items-center gap-2 mb-1">
                <Sparkles className="w-5 h-5 text-[#d97706]" />
                <h3 className="font-heading font-bold text-xl text-[#2d2d2d]">
                  New Virtual Key Generated for {newlyCreatedKey.name}!
                </h3>
              </div>
              <p className="text-sm font-body text-[#2d2d2d]/80 mb-2">
                Copy this key now! In accordance with security requirements, this raw key is never shown again in full.
              </p>
              <div className="flex items-center gap-2 bg-white px-3 py-2 border-2 border-[#2d2d2d] font-mono text-sm sketch-shadow-sm select-all">
                <code className="text-[#2d2d2d] font-bold">{newlyCreatedKey.key}</code>
                <button
                  onClick={() => handleCopy(newlyCreatedKey.key, 'newly-created')}
                  className="ml-auto p-1.5 hover:bg-[#e5e0d8] border border-[#2d2d2d] rounded cursor-pointer"
                >
                  {copiedKeyId === 'newly-created' ? (
                    <Check className="w-4 h-4 text-[#2e7d32]" />
                  ) : (
                    <Copy className="w-4 h-4 text-[#2d2d2d]" />
                  )}
                </button>
              </div>
            </div>
            <button
              onClick={() => setNewlyCreatedKey(null)}
              className="text-[#2d2d2d] font-bold hover:text-[#ff4d4d] cursor-pointer"
            >
              ✕
            </button>
          </div>
        </WobblyCard>
      )}

      {/* Keys List Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {keys.map((k, idx) => {
          const rotation = idx % 2 === 0 ? '-0.5deg' : '0.5deg';
          const dailyPct = Math.min(100, Math.round((k.currentDailySpend / k.dailyBudget) * 100));
          const monthlyPct = Math.min(100, Math.round((k.currentMonthlySpend / k.monthlyBudget) * 100));

          return (
            <WobblyCard
              key={k.id}
              decoration={idx % 3 === 0 ? 'tape' : idx % 3 === 1 ? 'tack' : 'none'}
              rotation={rotation}
              className="flex flex-col justify-between"
            >
              <div>
                {/* Header of Key Card */}
                <div className="flex items-start justify-between gap-2 mb-2">
                  <div>
                    <h3 className="text-xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
                      <Key className="w-5 h-5 text-[#2d5da1]" />
                      {k.name}
                    </h3>
                    <p className="text-sm font-body text-[#2d2d2d]/70">
                      Owner: <strong>{k.owner}</strong> • Tag:{' '}
                      <span className="font-mono text-xs bg-[#e5e0d8] px-1.5 py-0.5 rounded border border-[#2d2d2d]/40">
                        {k.tag}
                      </span>
                    </p>
                  </div>

                  {k.status === 'active' ? (
                    <SketchBadge variant="green" rotation="1deg">
                      Active
                    </SketchBadge>
                  ) : k.status === 'disabled' ? (
                    <SketchBadge variant="yellow" rotation="-1deg">
                      Disabled
                    </SketchBadge>
                  ) : (
                    <SketchBadge variant="red" rotation="1deg">
                      Revoked
                    </SketchBadge>
                  )}
                </div>

                {/* Key masked string (raw keys are shown once at creation) */}
                <div className="flex items-center gap-2 bg-[#fdfbf7] p-2 border-2 border-dashed border-[#2d2d2d] mb-4 text-xs font-mono">
                  <span className="truncate flex-1">
                    {k.key}
                  </span>
                  <span className="text-[10px] text-[#2d2d2d]/50 shrink-0">shown once</span>
                </div>

                {/* Limits & Budgets */}
                <div className="space-y-3 text-sm font-body mb-4">
                  <div>
                    <div className="flex justify-between text-xs font-heading font-bold mb-1">
                      <span>Daily Spend: {formatCurrency(k.currentDailySpend)}</span>
                      <span>Limit: {formatCurrency(k.dailyBudget)} ({dailyPct}%)</span>
                    </div>
                    <div className="w-full h-3 bg-[#e5e0d8] border-2 border-[#2d2d2d] rounded-full overflow-hidden">
                      <div
                        className={`h-full border-r-2 border-[#2d2d2d] ${
                          dailyPct > 80 ? 'bg-[#ff4d4d]' : 'bg-[#2d5da1]'
                        }`}
                        style={{ width: `${dailyPct}%` }}
                      />
                    </div>
                  </div>

                  <div>
                    <div className="flex justify-between text-xs font-heading font-bold mb-1">
                      <span>Monthly Spend: {formatCurrency(k.currentMonthlySpend)}</span>
                      <span>Cap: {formatCurrency(k.monthlyBudget)} ({monthlyPct}%)</span>
                    </div>
                    <div className="w-full h-3 bg-[#e5e0d8] border-2 border-[#2d2d2d] rounded-full overflow-hidden">
                      <div
                        className={`h-full border-r-2 border-[#2d2d2d] ${
                          monthlyPct > 80 ? 'bg-[#ff4d4d]' : 'bg-[#2e7d32]'
                        }`}
                        style={{ width: `${monthlyPct}%` }}
                      />
                    </div>
                  </div>

                  <div className="grid grid-cols-2 gap-2 text-xs font-mono bg-white p-2.5 border border-[#2d2d2d] rounded">
                    <div>
                      Rate Limit: <strong>{k.rpmLimit} RPM</strong>
                    </div>
                    <div>
                      Tokens: <strong>{formatTokens(k.tpmLimit)} TPM</strong>
                    </div>
                    <div className="col-span-2 pt-1 border-t border-[#2d2d2d]/15">
                      Allowed Models:{' '}
                      <strong className="text-[#2d5da1]">{k.allowedModels.join(', ')}</strong>
                    </div>
                  </div>
                </div>
              </div>

              {/* Actions Footer */}
              <div className="flex flex-wrap items-center justify-between gap-2 border-t-2 border-dashed border-[#2d2d2d]/30 pt-3 mt-2">
                <span className="text-xs font-mono text-[#2d2d2d]/60">
                  Total: {k.totalRequests.toLocaleString()} reqs • {formatTokens(k.totalTokens)} tok
                </span>

                <div className="flex items-center gap-2 shrink-0">
                  {k.status === 'active' ? (
                    <button
                      onClick={() => onUpdateKeyStatus(k.id, 'disabled')}
                      className="px-2 py-1 text-xs font-heading font-bold border border-[#2d2d2d] bg-white hover:bg-[#fff9c4] rounded flex items-center gap-1 cursor-pointer whitespace-nowrap"
                      title="Temporarily disable key"
                    >
                      <Power className="w-3.5 h-3.5 text-[#d97706]" />
                      Disable
                    </button>
                  ) : (
                    <button
                      onClick={() => onUpdateKeyStatus(k.id, 'active')}
                      className="px-2 py-1 text-xs font-heading font-bold border border-[#2d2d2d] bg-white hover:bg-[#e8f5e9] rounded flex items-center gap-1 cursor-pointer whitespace-nowrap"
                      title="Activate key"
                    >
                      <Power className="w-3.5 h-3.5 text-[#2e7d32]" />
                      Activate
                    </button>
                  )}

                  <button
                    onClick={() => onUpdateKeyStatus(k.id, 'revoked')}
                    className="px-2 py-1 text-xs font-heading font-bold border border-[#2d2d2d] bg-white hover:bg-[#ff4d4d] hover:text-white rounded flex items-center gap-1 cursor-pointer whitespace-nowrap"
                    title="Permanently revoke key"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    Revoke
                  </button>
                </div>
              </div>
            </WobblyCard>
          );
        })}
      </div>

      {/* Pi Agent Integration Helper Card */}
      <WobblyCard decoration="tack" variant="muted" className="p-5">
        <div className="flex items-center gap-2 mb-2">
          <Terminal className="w-6 h-6 text-[#2d5da1]" />
          <h3 className="text-2xl font-heading font-bold text-[#2d2d2d]">
            How to configure Pi Coding Agent with Kinetix
          </h3>
        </div>
        <p className="text-base font-body text-[#2d2d2d]/80 mb-3">
          Configure Pi to speak to Kinetix using standard OpenAI or Anthropic provider settings. Point the base URL at your Kinetix proxy:
        </p>

        <div className="bg-[#2d2d2d] text-[#fdfbf7] p-4 rounded-lg font-mono text-sm overflow-x-auto sketch-shadow-sm border-2 border-[#2d2d2d]">
          <pre>{`// ~/.pi/config.json
{
  "providers": {
    "kinetix": {
      "baseUrl": "${window.location.origin}/v1",
      "apiKey": "<paste the sk-kinetix-... key shown at creation>",
      "api": "openai-completions",
      "models": ["coder", "fast"]
    }
  }
}`}</pre>
        </div>
      </WobblyCard>

      {/* Create Modal */}
      {showCreateModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
          <div className="w-full max-w-lg">
            <WobblyCard decoration="tape" className="bg-[#fdfbf7] p-6 relative">
              <button
                onClick={() => setShowCreateModal(false)}
                className="absolute top-4 right-4 text-[#2d2d2d] font-bold text-xl hover:text-[#ff4d4d] cursor-pointer"
              >
                ✕
              </button>

              <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-4 flex items-center gap-2">
                <Key className="w-6 h-6 text-[#2d5da1]" />
                Issue New Virtual Key
              </h3>

              <form onSubmit={handleCreateSubmit} className="space-y-4 font-body">
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Key Description / Name
                  </label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. Alice (Pi Coding Agent)"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                  />
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Owner Name / Email
                    </label>
                    <input
                      type="text"
                      required
                      placeholder="e.g. Alice Vance"
                      value={owner}
                      onChange={(e) => setOwner(e.target.value)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                    />
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Tag (Attribution)
                    </label>
                    <input
                      type="text"
                      placeholder="e.g. dev-pi, ci, agent"
                      value={tag}
                      onChange={(e) => setTag(e.target.value)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                    />
                  </div>
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Allowed Models / Routes
                  </label>
                  <input
                    type="text"
                    placeholder="* or coder, fast, gemini-*"
                    value={allowedModels}
                    onChange={(e) => setAllowedModels(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: '255px 25px 225px 25px / 25px 225px 25px 255px' }}
                  />
                  <p className="text-xs text-[#2d2d2d]/60 mt-1">Use * to allow all configured models and routes.</p>
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Daily Budget (USD)
                    </label>
                    <input
                      type="number"
                      step="1"
                      min="1"
                      value={dailyBudget}
                      onChange={(e) => setDailyBudget(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                    />
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Monthly Budget (USD)
                    </label>
                    <input
                      type="number"
                      step="5"
                      min="5"
                      value={monthlyBudget}
                      onChange={(e) => setMonthlyBudget(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                    />
                  </div>
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      RPM Limit (Req/min)
                    </label>
                    <input
                      type="number"
                      value={rpmLimit}
                      onChange={(e) => setRpmLimit(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '15px 225px 255px 25px / 255px 25px 225px 15px' }}
                    />
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      TPM Limit (Tok/min)
                    </label>
                    <input
                      type="number"
                      value={tpmLimit}
                      onChange={(e) => setTpmLimit(Number(e.target.value))}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: '255px 15px 225px 15px / 15px 225px 15px 255px' }}
                    />
                  </div>
                </div>

                <div className="pt-2 flex justify-end gap-3">
                  {error && <span className="text-xs text-[#b71c1c] font-mono self-center">{error}</span>}
                  <SketchButton
                    type="button"
                    variant="ghost"
                    onClick={() => setShowCreateModal(false)}
                  >
                    Cancel
                  </SketchButton>
                  <SketchButton type="submit" variant="danger" className="font-bold" disabled={isSubmitting}>
                    {isSubmitting ? 'Generating…' : 'Generate Virtual Key'}
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
