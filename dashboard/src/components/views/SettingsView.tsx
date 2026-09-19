import React, { useState } from 'react';
import { Settings, ShieldCheck, KeyRound, LogOut, Info } from 'lucide-react';
import { WobblyCard, SketchBadge, SketchButton } from '../HandDrawnElements';
import { Kinetix } from '../../lib/resources';

interface SettingsViewProps {
  onLogout?: () => void;
}

/**
 * Settings & Security. The dashboard password is stored server-side as a hash
 * (never in the browser); changing it invalidates every existing session,
 * including the one making the change, so the operator is signed out and must
 * log in again.
 */
export const SettingsView: React.FC<SettingsViewProps> = ({ onLogout }) => {
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirmPw, setConfirmPw] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setDone(false);
    if (next.length < 8) {
      setError('New password must be at least 8 characters.');
      return;
    }
    if (next !== confirmPw) {
      setError('New password and confirmation do not match.');
      return;
    }
    setBusy(true);
    try {
      await Kinetix.changePassword(current, next);
      setDone(true);
      setCurrent('');
      setNext('');
      setConfirmPw('');
      // All sessions (including this one) are now invalid; return to login.
      setTimeout(() => onLogout?.(), 1500);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-3xl font-heading font-bold text-[#2d2d2d] flex items-center gap-2">
          <Settings className="w-7 h-7 text-[#2d5da1]" />
          <span>Settings &amp; Security</span>
          <SketchBadge variant="blue" rotation="1deg">
            Control Plane
          </SketchBadge>
        </h2>
        <p className="text-base font-body text-[#2d2d2d]/80">
          Session handling and the administrator password. Everything else is configured under the Gateway and
          Configuration sections, or from the <span className="font-mono">kinetix</span> CLI.
        </p>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        <WobblyCard decoration="tack" className="p-6">
          <h3 className="text-2xl font-heading font-bold text-[#2d2d2d] mb-4 flex items-center gap-2">
            <KeyRound className="w-6 h-6 text-[#ff4d4d]" />
            Change Administrator Password
          </h3>

          <form onSubmit={submit} className="space-y-4">
            <div>
              <label className="block text-sm font-heading font-bold mb-1">Current Password</label>
              <input
                type="password"
                value={current}
                onChange={(e) => setCurrent(e.target.value)}
                className="w-full px-3 py-2 bg-white border-2 border-[#2d2d2d] sketch-shadow-sm font-mono text-sm focus:outline-none focus:border-[#2d5da1]"
                style={{ borderRadius: '12px 16px 12px 16px / 16px 12px 16px 12px' }}
                autoComplete="current-password"
              />
            </div>
            <div>
              <label className="block text-sm font-heading font-bold mb-1">New Password (min 8 chars)</label>
              <input
                type="password"
                value={next}
                onChange={(e) => setNext(e.target.value)}
                className="w-full px-3 py-2 bg-white border-2 border-[#2d2d2d] sketch-shadow-sm font-mono text-sm focus:outline-none focus:border-[#2d5da1]"
                style={{ borderRadius: '12px 16px 12px 16px / 16px 12px 16px 12px' }}
                autoComplete="new-password"
              />
            </div>
            <div>
              <label className="block text-sm font-heading font-bold mb-1">Confirm New Password</label>
              <input
                type="password"
                value={confirmPw}
                onChange={(e) => setConfirmPw(e.target.value)}
                className="w-full px-3 py-2 bg-white border-2 border-[#2d2d2d] sketch-shadow-sm font-mono text-sm focus:outline-none focus:border-[#2d5da1]"
                style={{ borderRadius: '12px 16px 12px 16px / 16px 12px 16px 12px' }}
                autoComplete="new-password"
              />
            </div>

            {error && (
              <div className="p-2 bg-[#ffebee] border-2 border-[#ff4d4d] rounded text-sm font-mono text-[#b71c1c]">
                {error}
              </div>
            )}
            {done && (
              <div className="p-2 bg-[#e8f5e9] border-2 border-[#2e7d32] rounded text-sm font-mono text-[#1b5e20]">
                Password changed. All sessions invalidated — signing you out…
              </div>
            )}

            <SketchButton type="submit" variant="primary" disabled={busy || !current || !next} className="gap-2">
              <ShieldCheck className="w-4 h-4" />
              {busy ? 'Updating…' : 'Update Password'}
            </SketchButton>
          </form>
        </WobblyCard>

        <div className="space-y-6">
          <WobblyCard variant="muted" className="p-5">
            <h4 className="text-lg font-heading font-bold mb-2 flex items-center gap-2">
              <Info className="w-5 h-5 text-[#2d5da1]" /> Session Policy
            </h4>
            <ul className="list-disc list-inside space-y-1.5 text-sm font-body text-[#2d2d2d]/85">
              <li>Sessions are held in server memory with a TTL (default 12 h).</li>
              <li>
                <strong>Restarting the server invalidates every session</strong>, so a browser must log in again — a
                stale cookie can never grant access.
              </li>
              <li>Changing the password invalidates all sessions immediately.</li>
              <li>The password is stored only as a hash, never in the browser.</li>
            </ul>
          </WobblyCard>

          <WobblyCard decoration="tape" className="p-5">
            <h4 className="text-lg font-heading font-bold mb-2">CLI Equivalent</h4>
            <p className="text-sm font-body text-[#2d2d2d]/85 mb-2">
              The same change can be made without the dashboard (works while the server is stopped):
            </p>
            <pre className="bg-[#2d2d2d] text-[#fdfbf7] text-xs font-mono p-3 rounded overflow-x-auto">
{`kinetix password set 'a-new-strong-password'
kinetix password show`}
            </pre>
          </WobblyCard>

          {onLogout && (
            <SketchButton variant="danger" onClick={onLogout} className="gap-2">
              <LogOut className="w-4 h-4" /> Sign Out Now
            </SketchButton>
          )}
        </div>
      </div>
    </div>
  );
};
