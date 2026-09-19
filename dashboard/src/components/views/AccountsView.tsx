import React, { useState } from 'react';
import { Users, Plus, ShieldCheck, Clock, AlertTriangle, RefreshCw, KeyRound, Sparkles, Trash2 } from 'lucide-react';
import { Account, Provider } from '../../types';
import { WobblyCard, SketchButton, SketchBadge } from '../HandDrawnElements';
import { formatCurrency, formatTokens, DESIGN_TOKENS } from '../../lib/designSystem';

interface AccountsViewProps {
  accounts: Account[];
  providers: Provider[];
  onAddAccount: (acc: Account & { apiKey?: string }) => void;
  onUpdateAccount: (acc: Account) => void;
  onDeleteAccount: (accountId: string) => void;
}

export const AccountsView: React.FC<AccountsViewProps> = ({
  accounts,
  providers,
  onAddAccount,
  onUpdateAccount,
  onDeleteAccount,
}) => {
  const [showAddModal, setShowAddModal] = useState(false);
  const [confirmDeleteAccountId, setConfirmDeleteAccountId] = useState<string | null>(null);
  const [label, setLabel] = useState('');
  const [providerId, setProviderId] = useState(providers[0]?.id || '');
  const [apiKey, setApiKey] = useState('');
  const [softQuota, setSoftQuota] = useState(100);

  const handleCreateSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!label.trim() || !apiKey.trim()) return;

    const prov = providers.find((p) => p.id === providerId) || providers[0];
    const masked = apiKey.slice(0, 6) + '...' + apiKey.slice(-4);

    const newAcc: Account & { apiKey?: string } = {
      id: '',
      providerId: prov.id,
      providerName: prov.name,
      label: label.trim(),
      keyMasked: masked,
      status: 'healthy',
      quotaType: 'monthly',
      softQuotaSpendLimit: softQuota,
      currentSpend: 0,
      requestsCount: 0,
      tokensCount: 0,
      priority: accounts.length + 1,
      apiKey: apiKey.trim(),
    };

    onAddAccount(newAcc);
    setShowAddModal(false);
    setLabel('');
    setApiKey('');
  };

  const handleResetCooldown = (acc: Account) => {
    onUpdateAccount({
      ...acc,
      status: 'healthy',
      cooldownUntil: null,
      lastError: undefined,
    });
  };

  return (
    <div className="space-y-6">
      {/* Top Banner */}
      <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
            <span>Key Pool & Accounts Health</span>
            <SketchBadge variant="yellow" rotation="-1deg">
              FR-4 & FR-12
            </SketchBadge>
          </h2>
          <p className="text-base font-body text-[#2d2d2d]/80">
            Accounts hold real upstream API keys securely. Individual keys cycle into cooldown on 429s or quota exhaustion without interrupting client requests.
          </p>
        </div>

        <SketchButton
          variant="primary"
          size="md"
          onClick={() => setShowAddModal(true)}
          className="gap-2 font-heading font-bold"
        >
          <Plus className="w-5 h-5" />
          Add Upstream Credential
        </SketchButton>
      </div>

      {/* Account Cards Grid */}
      {accounts.length === 0 ? (
        <WobblyCard decoration="tack" className="p-10 text-center bg-white">
          <KeyRound className="w-12 h-12 text-[#2d5da1] mx-auto mb-3 opacity-60" />
          <h3 className="text-2xl font-heading font-bold text-[#2d2d2d]">No Account Credentials or Pools Configured</h3>
          <p className="text-base font-body text-[#2d2d2d]/80 max-w-lg mx-auto mt-2 mb-6">
            Store multiple API keys and upstream accounts per provider. Kinetix groups them into active pools, tracks spend against soft quotas, and isolates keys from client applications.
          </p>
          <SketchButton
            variant="primary"
            size="md"
            onClick={() => setShowAddModal(true)}
            className="gap-2 font-heading font-bold"
          >
            <Plus className="w-5 h-5" />
            Add First Upstream Credential
          </SketchButton>
        </WobblyCard>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {accounts.map((acc, idx) => {
            const isCooldown = acc.status === 'cooldown';
            const isExhausted = acc.status === 'exhausted';
            const rotation = idx % 2 === 0 ? '-0.5deg' : '0.5deg';

            return (
              <WobblyCard
                key={acc.id}
                decoration={isCooldown ? 'tack' : idx % 2 === 0 ? 'tape' : 'none'}
                rotation={rotation}
                className={`p-5 flex flex-col justify-between ${
                  isCooldown ? 'bg-[#fff0f0]' : isExhausted ? 'bg-[#fffde7]' : 'bg-white'
                }`}
              >
                <div>
                  <div className="flex items-start justify-between gap-2 mb-2">
                    <div>
                      <span className="text-xs font-mono bg-[#e5e0d8] px-2 py-0.5 rounded border border-[#2d2d2d]/30 inline-block mb-1">
                        {acc.providerName}
                      </span>
                      <h3 className="text-xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
                        <KeyRound className="w-5 h-5 text-[#2d5da1]" />
                        {acc.label}
                      </h3>
                    </div>

                    {acc.status === 'healthy' ? (
                      <SketchBadge variant="green" rotation="1deg">
                        Healthy
                      </SketchBadge>
                    ) : acc.status === 'cooldown' ? (
                      <SketchBadge variant="red" rotation="-1deg">
                        In Cooldown (429)
                      </SketchBadge>
                    ) : (
                      <SketchBadge variant="yellow" rotation="1deg">
                        Quota Exhausted
                      </SketchBadge>
                    )}
                  </div>

                  {/* Key masked preview */}
                  <div className="flex items-center justify-between bg-[#fdfbf7] p-2 border-2 border-dashed border-[#2d2d2d] text-xs font-mono mb-4">
                    <span>Masked Secret: <strong>{acc.keyMasked}</strong></span>
                    <span className="text-[#2e7d32] font-bold">🔒 Encrypted at rest</span>
                  </div>

                  {/* Cooldown / Quota warning notice */}
                  {isCooldown && (
                    <div className="p-3 bg-[#ffebee] border-2 border-[#ff4d4d] sketch-shadow-sm mb-4 rounded text-xs font-mono text-[#b71c1c]">
                      <div className="flex items-center gap-1.5 font-bold mb-1">
                        <AlertTriangle className="w-4 h-4" />
                        <span>{acc.lastError || 'Rate Limit (HTTP 429)'}</span>
                      </div>
                      <div>Cooling down: {acc.cooldownUntil || 'until Retry-After passes'}</div>
                      <button
                        onClick={() => handleResetCooldown(acc)}
                        className="mt-2 px-2 py-1 bg-white border border-[#2d2d2d] text-[#2d2d2d] hover:bg-[#e5e0d8] rounded flex items-center gap-1 cursor-pointer font-bold"
                      >
                        <RefreshCw className="w-3 h-3" />
                        Clear Cooldown & Probe Upstream
                      </button>
                    </div>
                  )}

                  {/* Soft Quotas & Statistics */}
                  <div className="grid grid-cols-2 gap-2 text-xs font-mono bg-white p-3 border border-[#2d2d2d] rounded mb-4">
                    <div>
                      Requests Served: <strong>{acc.requestsCount.toLocaleString()}</strong>
                    </div>
                    <div>
                      Tokens Processed: <strong>{formatTokens(acc.tokensCount)}</strong>
                    </div>
                    <div>
                      Spend Accrued: <strong>{formatCurrency(acc.currentSpend)}</strong>
                    </div>
                    <div>
                      Soft Quota Cap:{' '}
                      <strong>
                        {acc.softQuotaSpendLimit ? formatCurrency(acc.softQuotaSpendLimit) : 'Unlimited'}
                      </strong>
                    </div>
                    {acc.quotaResetTime && (
                      <div className="col-span-2 text-[#2d5da1] pt-1 border-t border-[#2d2d2d]/20">
                        Quota Reset: <strong>{acc.quotaResetTime}</strong>
                      </div>
                    )}
                  </div>
                </div>

                {/* Footer */}
                <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[#2d2d2d]/20 pt-3 text-xs font-mono">
                  <div className="text-[#2d2d2d]/70">
                    <span>Priority: <strong>Tier #{acc.priority}</strong></span>
                  </div>

                  {confirmDeleteAccountId === acc.id ? (
                    <div className="flex items-center gap-1 bg-[#ffebee] px-2 py-1 border border-[#ff4d4d] rounded text-xs font-heading">
                      <span className="text-[#b71c1c] font-bold">Remove pool key?</span>
                      <button
                        onClick={() => {
                          onDeleteAccount(acc.id);
                          setConfirmDeleteAccountId(null);
                        }}
                        className="px-2 py-0.5 bg-[#ff4d4d] text-white rounded font-bold hover:bg-[#d32f2f] cursor-pointer"
                      >
                        Confirm
                      </button>
                      <button
                        onClick={() => setConfirmDeleteAccountId(null)}
                        className="px-2 py-0.5 bg-white border border-[#2d2d2d] rounded hover:bg-[#e5e0d8] cursor-pointer"
                      >
                        Cancel
                      </button>
                    </div>
                  ) : (
                    <button
                      onClick={() => setConfirmDeleteAccountId(acc.id)}
                      className="px-2 py-1 text-xs font-heading font-bold text-[#ff4d4d] hover:bg-[#ffebee] border border-[#ff4d4d]/40 hover:border-[#ff4d4d] rounded flex items-center gap-1 cursor-pointer transition-colors"
                      title="Remove this credential account from pool"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                      <span>Remove Pool Key</span>
                    </button>
                  )}
                </div>
              </WobblyCard>
            );
          })}
        </div>
      )}

      {/* Add Account Modal */}
      {showAddModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
          <div className="w-full max-w-lg">
            <WobblyCard decoration="tape" className="bg-[#fdfbf7] p-6 relative">
              <button
                onClick={() => setShowAddModal(false)}
                className="absolute top-4 right-4 text-[#2d2d2d] font-bold text-xl hover:text-[#ff4d4d] cursor-pointer"
              >
                ✕
              </button>

              <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-4 flex items-center gap-2">
                <KeyRound className="w-6 h-6 text-[#2d5da1]" />
                Add Upstream Provider Credential
              </h3>

              <form onSubmit={handleCreateSubmit} className="space-y-4 font-body">
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Provider
                  </label>
                  <select
                    value={providerId}
                    onChange={(e) => setProviderId(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                  >
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} ({p.wireFormat})
                      </option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Account Label / Identification
                  </label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. Gemini Team Pay-as-you-go #2"
                    value={label}
                    onChange={(e) => setLabel(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobbly }}
                  />
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Raw API Key (Encrypted immediately on storage)
                  </label>
                  <input
                    type="password"
                    required
                    placeholder="AIzaSy... or sk-..."
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                  />
                  <p className="text-xs text-[#2d2d2d]/60 mt-1">
                    Upstream keys never leave the server or appear in client responses.
                  </p>
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Soft Quota Spend Limit (USD/month)
                  </label>
                  <input
                    type="number"
                    value={softQuota}
                    onChange={(e) => setSoftQuota(Number(e.target.value))}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyBtn }}
                  />
                </div>

                <div className="pt-2 flex justify-end gap-3">
                  <SketchButton
                    type="button"
                    variant="ghost"
                    onClick={() => setShowAddModal(false)}
                  >
                    Cancel
                  </SketchButton>
                  <SketchButton type="submit" variant="danger" className="font-bold">
                    Save Key to Pool
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
