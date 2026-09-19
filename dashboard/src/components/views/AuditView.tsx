import React from 'react';
import { History, Shield, Clock } from 'lucide-react';
import { AuditLog } from '../../types';
import { WobblyCard, SketchBadge } from '../HandDrawnElements';

interface AuditViewProps {
  logs: AuditLog[];
}

export const AuditView: React.FC<AuditViewProps> = ({ logs }) => {
  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
          <span>Admin & System Audit Trail</span>
          <SketchBadge variant="yellow" rotation="-1deg">
            FR-6.7 Compliance
          </SketchBadge>
        </h2>
        <p className="text-base font-body text-[#2d2d2d]/80">
          Append-only security log of all key issuances, limit adjustments, quota cooldown transitions, and configuration edits.
        </p>
      </div>

      <WobblyCard decoration="tape" className="p-0 overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-left font-body text-base">
            <thead className="bg-[#e5e0d8] border-b-2 border-[#2d2d2d] font-heading font-bold text-sm">
              <tr>
                <th className="p-3">Timestamp</th>
                <th className="p-3">Actor</th>
                <th className="p-3">Action Type</th>
                <th className="p-3">Target</th>
                <th className="p-3">Details & Parameters</th>
              </tr>
            </thead>
            <tbody className="divide-y-2 divide-[#2d2d2d]/15 bg-white font-mono text-xs">
              {logs.map((log) => {
                const isSystem = log.actor === 'system';

                return (
                  <tr key={log.id} className="hover:bg-[#fdfbf7] transition-colors">
                    <td className="p-3 whitespace-nowrap text-[#2d2d2d]/70">
                      {new Date(log.timestamp).toLocaleString()}
                    </td>
                    <td className="p-3 whitespace-nowrap">
                      {isSystem ? (
                        <span className="font-bold text-[#ff4d4d] bg-[#ffebee] px-1.5 py-0.5 rounded border border-[#ff4d4d]/40">
                          system daemon
                        </span>
                      ) : (
                        <span className="font-bold text-[#2d5da1] bg-[#e8f0fe] px-1.5 py-0.5 rounded border border-[#2d5da1]/40">
                          {log.actor}
                        </span>
                      )}
                    </td>
                    <td className="p-3 whitespace-nowrap font-bold text-[#2d2d2d]">
                      {log.action}
                    </td>
                    <td className="p-3 whitespace-nowrap">
                      <span className="bg-[#e5e0d8] px-1.5 py-0.5 rounded border border-[#2d2d2d]/30 font-bold">
                        {log.targetName}
                      </span>
                    </td>
                    <td className="p-3 text-sm font-body text-[#2d2d2d]">
                      {log.details}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </WobblyCard>
    </div>
  );
};
