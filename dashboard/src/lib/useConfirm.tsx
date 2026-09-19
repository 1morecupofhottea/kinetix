import React, { useCallback, useState } from 'react';
import { AlertTriangle } from 'lucide-react';
import { SketchButton } from '../components/HandDrawnElements';
import { DESIGN_TOKENS } from './designSystem';

interface ConfirmRequest {
  title: string;
  message: string;
  detail?: string;
  confirmLabel: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
}

/**
 * Promise-based confirmation for destructive actions. Every irreversible
 * operation (delete, revoke, clear) should gate on this so a stray click can
 * never remove configuration or a credential.
 *
 * Usage:
 *   const { confirm, confirmNode } = useConfirm();
 *   ...
 *   if (!(await confirm({ title, message, confirmLabel: 'Delete' }))) return;
 *   ...
 *   return <>{confirmNode}...</>;
 */
export function useConfirm() {
  const [req, setReq] = useState<ConfirmRequest | null>(null);

  const confirm = useCallback(
    (opts: Omit<ConfirmRequest, 'resolve' | 'danger' | 'confirmLabel'> & {
      confirmLabel?: string;
      danger?: boolean;
    }) =>
      new Promise<boolean>((resolve) => {
        setReq({
          title: opts.title,
          message: opts.message,
          detail: opts.detail,
          confirmLabel: opts.confirmLabel || 'Confirm',
          danger: opts.danger ?? true,
          resolve,
        });
      }),
    [],
  );

  const close = (ok: boolean) => {
    req?.resolve(ok);
    setReq(null);
  };

  const confirmNode = req ? (
    <div className="fixed inset-0 z-[60] flex items-center justify-center p-4 bg-black/40 backdrop-blur-xs">
      <div
        className="w-full max-w-md bg-[var(--paper)] border-2 border-[var(--ink)] p-6 sketch-shadow"
        style={{ borderRadius: DESIGN_TOKENS.radii.wobblyLg }}
      >
        <h3 className="text-xl font-heading font-bold text-[var(--ink)] mb-2 flex items-center gap-2">
          <AlertTriangle className="w-5 h-5 text-[var(--marker-red)]" />
          {req.title}
        </h3>
        <p className="text-base font-body text-[var(--ink)]/90">{req.message}</p>
        {req.detail && (
          <p
            className="mt-2 p-2 text-sm font-mono bg-[var(--postit)] border border-[var(--marker-orange)] text-[var(--warn-text)]"
            style={{ borderRadius: DESIGN_TOKENS.radii.wobblyMd }}
          >
            {req.detail}
          </p>
        )}
        <div className="mt-5 flex justify-end gap-3">
          <SketchButton variant="ghost" onClick={() => close(false)}>
            Cancel
          </SketchButton>
          <SketchButton variant={req.danger ? 'danger' : 'primary'} onClick={() => close(true)}>
            {req.confirmLabel}
          </SketchButton>
        </div>
      </div>
    </div>
  ) : null;

  return { confirm, confirmNode };
}
