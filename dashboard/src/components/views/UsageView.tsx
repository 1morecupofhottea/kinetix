import React from 'react';
import { BarChart3, TrendingUp, DollarSign, Database, BrainCircuit, ShieldAlert } from 'lucide-react';
import { VirtualKey, ModelConfig, RequestLog } from '../../types';
import { WobblyCard, SketchBadge } from '../HandDrawnElements';
import { formatCurrency, formatTokens } from '../../lib/designSystem';

interface UsageViewProps {
  keys: VirtualKey[];
  models: ModelConfig[];
  requests: RequestLog[];
}

export const UsageView: React.FC<UsageViewProps> = ({ keys, models, requests }) => {
  const totalSpend = keys.reduce((acc, k) => acc + k.currentMonthlySpend, 0);
  const totalTokens = keys.reduce((acc, k) => acc + k.totalTokens, 0);
  const totalCachedTokens = requests.reduce((acc, r) => acc + r.cachedTokens, 0);
  const totalThinkingTokens = requests.reduce((acc, r) => acc + r.thinkingTokens, 0);

  return (
    <div className="space-y-6">
      {/* Top Banner */}
      <div>
        <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
          <span>Usage, Attribution & Spend</span>
          <SketchBadge variant="green" rotation="1deg">
            Accurate per FR-6
          </SketchBadge>
        </h2>
        <p className="text-base font-body text-[#2d2d2d]/80">
          Every request is attributed to a team member or project, tracking input, output, cached, and reasoning tokens.
        </p>
      </div>

      {/* Top metric overview cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
        <WobblyCard decoration="tack" className="p-4 bg-[#fff9c4]">
          <div className="flex items-center justify-between">
            <span className="text-sm font-heading font-bold text-[#2d2d2d]">Total Spend (MTD)</span>
            <DollarSign className="w-5 h-5 text-[#2d5da1]" />
          </div>
          <div className="text-3xl font-heading font-bold text-[#2d2d2d] mt-2">
            {formatCurrency(totalSpend)}
          </div>
          <span className="text-xs font-mono text-[#2d2d2d]/70 block mt-1">
            Across {keys.length} active virtual keys
          </span>
        </WobblyCard>

        <WobblyCard decoration="tape" className="p-4 bg-white">
          <div className="flex items-center justify-between">
            <span className="text-sm font-heading font-bold text-[#2d2d2d]">Tokens Processed</span>
            <TrendingUp className="w-5 h-5 text-[#ff4d4d]" />
          </div>
          <div className="text-3xl font-heading font-bold text-[#2d2d2d] mt-2">
            {formatTokens(totalTokens)}
          </div>
          <span className="text-xs font-mono text-[#2d2d2d]/70 block mt-1">
            Combined input & output
          </span>
        </WobblyCard>

        <WobblyCard className="p-4 bg-white">
          <div className="flex items-center justify-between">
            <span className="text-sm font-heading font-bold text-[#2d2d2d]">Context Cache Savings</span>
            <Database className="w-5 h-5 text-[#2e7d32]" />
          </div>
          <div className="text-3xl font-heading font-bold text-[#2e7d32] mt-2">
            {formatTokens(totalCachedTokens)}
          </div>
          <span className="text-xs font-mono text-[#2d2d2d]/70 block mt-1">
            Gemini discounted cache hits
          </span>
        </WobblyCard>

        <WobblyCard decoration="tack-blue" className="p-4 bg-white">
          <div className="flex items-center justify-between">
            <span className="text-sm font-heading font-bold text-[#2d2d2d]">Reasoning (Thinking)</span>
            <BrainCircuit className="w-5 h-5 text-[#2d5da1]" />
          </div>
          <div className="text-3xl font-heading font-bold text-[#2d5da1] mt-2">
            {formatTokens(totalThinkingTokens)}
          </div>
          <span className="text-xs font-mono text-[#2d2d2d]/70 block mt-1">
            Tracked on Pro / Claude targets
          </span>
        </WobblyCard>
      </div>

      {/* Spend by Virtual Key & Budget Limits */}
      <WobblyCard decoration="tape" className="p-6">
        <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-4 flex items-center gap-2">
          <BarChart3 className="w-6 h-6 text-[#ff4d4d]" />
          Spend by Virtual Key vs. Configured Budgets
        </h3>

        <div className="space-y-4">
          {keys.map((k) => {
            const pct = Math.min(100, Math.round((k.currentMonthlySpend / k.monthlyBudget) * 100));
            const isNearCap = pct >= 80;

            return (
              <div
                key={k.id}
                className="p-4 bg-[#fdfbf7] border-2 border-[#2d2d2d] sketch-shadow-sm rounded-lg"
              >
                <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-2 mb-2">
                  <div>
                    <span className="font-heading font-bold text-lg text-[#2d2d2d]">{k.name}</span>
                    <span className="text-xs font-mono text-[#2d2d2d]/60 ml-2">
                      ({k.owner} • {k.tag})
                    </span>
                  </div>

                  <div className="flex items-center gap-2">
                    <span className="font-heading font-bold text-base">
                      {formatCurrency(k.currentMonthlySpend)} / {formatCurrency(k.monthlyBudget)}
                    </span>
                    {isNearCap && (
                      <SketchBadge variant="red" rotation="-1deg">
                        Cap Alert ({pct}%)
                      </SketchBadge>
                    )}
                  </div>
                </div>

                {/* Hand-drawn progress bar */}
                <div className="w-full h-4 bg-[#e5e0d8] border-2 border-[#2d2d2d] rounded-full overflow-hidden relative">
                  <div
                    className={`h-full border-r-2 border-[#2d2d2d] transition-all duration-300 ${
                      pct > 80 ? 'bg-[#ff4d4d]' : 'bg-[#2d5da1]'
                    }`}
                    style={{ width: `${pct}%` }}
                  />
                </div>

                <div className="flex items-center justify-between text-xs font-mono text-[#2d2d2d]/70 mt-2">
                  <span>Daily spend: {formatCurrency(k.currentDailySpend)}</span>
                  <span>Requests: {k.totalRequests.toLocaleString()}</span>
                  <span>Total tokens: {formatTokens(k.totalTokens)}</span>
                </div>
              </div>
            );
          })}
        </div>
      </WobblyCard>

      {/* Model Unit Cost Reference Sheet */}
      <WobblyCard variant="muted" className="p-5">
        <h4 className="text-xl font-heading font-bold text-[#2d2d2d] mb-2">
          📌 Active Pricing Schema Reference (Versioned)
        </h4>
        <p className="text-sm font-body text-[#2d2d2d]/80 mb-3">
          Prism / Kinetix bundles no vendor pricing assumptions. All costs are computed from your explicit rates configured in Provider settings:
        </p>

        <div className="overflow-x-auto">
          <table className="w-full text-left font-mono text-xs bg-white border-2 border-[#2d2d2d] rounded">
            <thead className="bg-[#e5e0d8] border-b-2 border-[#2d2d2d] font-heading text-sm">
              <tr>
                <th className="p-2">Model Display Name</th>
                <th className="p-2">Input / 1M</th>
                <th className="p-2">Output / 1M</th>
                <th className="p-2">Cached / 1M</th>
                <th className="p-2">Thinking / 1M</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-[#2d2d2d]/20">
              {models.map((m) => (
                <tr key={m.id}>
                  <td className="p-2 font-bold">{m.displayName}</td>
                  <td className="p-2">${m.prices.inputPer1M.toFixed(2)}</td>
                  <td className="p-2">${m.prices.outputPer1M.toFixed(2)}</td>
                  <td className="p-2">${m.prices.cachedPer1M.toFixed(4)}</td>
                  <td className="p-2">
                    {m.capabilities.reasoning ? `$${m.prices.thinkingPer1M.toFixed(2)}` : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </WobblyCard>
    </div>
  );
};
