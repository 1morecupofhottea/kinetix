import React, { useState } from 'react';
import { Compass, Plus, ArrowRight, Trash2 } from 'lucide-react';
import { ModelAlias, Route, ModelConfig } from '../../types';
import { WobblyCard, SketchButton, SketchBadge } from '../HandDrawnElements';
import { DESIGN_TOKENS } from '../../lib/designSystem';

interface AliasesViewProps {
  aliases: ModelAlias[];
  routes: Route[];
  models: ModelConfig[];
  onAddAlias: (alias: ModelAlias) => void;
  onDeleteAlias: (id: string) => void;
}

export const AliasesView: React.FC<AliasesViewProps> = ({
  aliases,
  routes,
  models,
  onAddAlias,
  onDeleteAlias,
}) => {
  const [showAddModal, setShowAddModal] = useState(false);
  const [aliasName, setAliasName] = useState('');
  const [targetType, setTargetType] = useState<'route' | 'model'>('route');
  const [targetId, setTargetId] = useState(routes[0]?.id || '');
  const [description, setDescription] = useState('');

  const handleCreateSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!aliasName.trim()) return;

    let targetDisplayName = '';
    if (targetType === 'route') {
      const c = routes.find((x) => x.id === targetId) || routes[0];
      targetDisplayName = `Route: ${c?.name || ''}`;
    } else {
      const m = models.find((x) => x.id === targetId) || models[0];
      targetDisplayName = `Model: ${m?.displayName || ''}`;
    }

    const newAlias: ModelAlias = {
      id: `alias-${Date.now()}`,
      aliasName: aliasName.trim().toLowerCase(),
      targetType,
      targetId,
      targetDisplayName,
      description: description.trim() || 'Custom mapped alias',
    };

    onAddAlias(newAlias);
    setShowAddModal(false);
    setAliasName('');
    setDescription('');
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4">
        <div>
          <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
            <span>Model Aliasing & Routing Table</span>
            <SketchBadge variant="yellow" rotation="-1deg">
              FR-5 Spec
            </SketchBadge>
          </h2>
          <p className="text-base font-body text-[#2d2d2d]/80">
            Expose clean, stable model names (like <code>coder</code> or <code>fast</code>) to tools like Pi, routing them to routes or specific upstream models.
          </p>
        </div>

        <SketchButton
          variant="primary"
          size="md"
          onClick={() => setShowAddModal(true)}
          className="gap-2 font-heading font-bold"
        >
          <Plus className="w-5 h-5" />
          Add Model Alias
        </SketchButton>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {aliases.map((alias, idx) => {
          const rotation = idx % 2 === 0 ? '-0.5deg' : '0.5deg';

          return (
            <WobblyCard
              key={alias.id}
              decoration={idx % 2 === 0 ? 'tape' : 'tack-blue'}
              rotation={rotation}
              className="p-5 flex flex-col justify-between"
            >
              <div>
                <div className="flex items-start justify-between gap-2 mb-3">
                  <div>
                    <span className="text-xs font-mono text-[#2d2d2d]/60 block mb-1">
                      Client-Facing Model Name:
                    </span>
                    <h3 className="text-2xl font-heading font-bold text-[#2d2d2d]">
                      "{alias.aliasName}"
                    </h3>
                  </div>

                  <SketchBadge variant={alias.targetType === 'route' ? 'yellow' : 'blue'}>
                    {alias.targetType === 'route' ? '⚡ Route' : 'Direct Model'}
                  </SketchBadge>
                </div>

                <div className="p-3 bg-[#fdfbf7] border-2 border-[#2d2d2d] sketch-shadow-sm rounded mb-3 flex items-center gap-2 text-sm font-mono">
                  <span className="font-bold text-[#2d5da1]">{alias.aliasName}</span>
                  <ArrowRight className="w-4 h-4 text-[#2d2d2d]" />
                  <span className="font-bold text-[#2d2d2d]">{alias.targetDisplayName}</span>
                </div>

                <p className="text-sm font-body text-[#2d2d2d]/80">
                  {alias.description}
                </p>
              </div>

              <div className="pt-3 border-t border-[#2d2d2d]/20 flex justify-end mt-4">
                <button
                  onClick={() => onDeleteAlias(alias.id)}
                  className="text-xs font-mono text-[#ff4d4d] hover:underline flex items-center gap-1 cursor-pointer"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                  Remove Alias
                </button>
              </div>
            </WobblyCard>
          );
        })}
      </div>

      {/* Add Alias Modal */}
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
                <Compass className="w-6 h-6 text-[#2d5da1]" />
                Add Model Alias
              </h3>

              <form onSubmit={handleCreateSubmit} className="space-y-4 font-body">
                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Client Alias Name (e.g. coder, fast, sonnet)
                  </label>
                  <input
                    type="text"
                    required
                    placeholder="e.g. coder"
                    value={aliasName}
                    onChange={(e) => setAliasName(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-mono sketch-shadow-sm focus:outline-none"
                    style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                  />
                </div>

                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Target Type
                    </label>
                    <select
                      value={targetType}
                      onChange={(e) => setTargetType(e.target.value as any)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
                    >
                      <option value="route">Route (With Fallback)</option>
                      <option value="model">Direct Model</option>
                    </select>
                  </div>

                  <div>
                    <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                      Target Destination
                    </label>
                    <select
                      value={targetId}
                      onChange={(e) => setTargetId(e.target.value)}
                      className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none"
                      style={{ borderRadius: DESIGN_TOKENS.radii.wobbly }}
                    >
                      {targetType === 'route'
                        ? routes.map((c) => (
                            <option key={c.id} value={c.id}>
                              ⚡ {c.name}
                            </option>
                          ))
                        : models.map((m) => (
                            <option key={m.id} value={m.id}>
                              {m.displayName}
                            </option>
                          ))}
                    </select>
                  </div>
                </div>

                <div>
                  <label className="block text-sm font-heading font-bold text-[#2d2d2d] mb-1">
                    Description / Purpose
                  </label>
                  <input
                    type="text"
                    placeholder="e.g. Primary Pi coding target"
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    className="w-full bg-white border-2 border-[#2d2d2d] px-3 py-2 text-base font-body sketch-shadow-sm focus:outline-none"
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
                    Save Alias
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
