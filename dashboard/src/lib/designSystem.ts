export const DESIGN_TOKENS = {
  colors: {
    background: 'var(--paper)',
    foreground: 'var(--ink)',
    muted: 'var(--erased)',
    accent: 'var(--marker-red)',
    border: 'var(--ink)',
    secondaryAccent: 'var(--pen-blue)',
    postit: 'var(--postit)',
    postitBorder: 'var(--postit-border)',
    penGreen: 'var(--pen-green)',
    markerOrange: 'var(--marker-orange)',
  },
  radii: {
    wobbly: '255px 15px 225px 15px / 15px 225px 15px 255px',
    wobblyMd: '15px 255px 15px 225px / 225px 15px 255px 15px',
    wobblyLg: '20px 280px 20px 260px / 260px 20px 280px 20px',
    wobblyBtn: '255px 25px 225px 25px / 25px 225px 25px 255px',
    wobblyBadge: '120px 10px 100px 10px / 10px 100px 10px 120px',
    wobblyCardAlt: '255px 20px 240px 20px / 20px 240px 20px 255px',
    wobblyCircle: '50% 50% 50% 50% / 55% 45% 55% 45%',
  },
  shadows: {
    standard: '4px 4px 0px 0px var(--shadow-ink)',
    emphasized: '8px 8px 0px 0px var(--shadow-ink)',
    subtle: '2px 2px 0px 0px var(--shadow-ink)',
    softPaper: '3px 3px 0px 0px color-mix(in srgb, var(--shadow-ink) 10%, transparent)',
    blue: '4px 4px 0px 0px var(--pen-blue)',
    red: '4px 4px 0px 0px var(--marker-red)',
  },
};

export function formatCurrency(amount: number): string {
  if (amount < 0.001 && amount > 0) {
    return `< $0.001`;
  }
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: amount > 10 ? 2 : 3,
    maximumFractionDigits: 4,
  }).format(amount);
}

export function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) {
    return `${(tokens / 1_000_000).toFixed(2)}M`;
  }
  if (tokens >= 1_000) {
    return `${(tokens / 1_000).toFixed(1)}k`;
  }
  return tokens.toLocaleString();
}

export function formatLatency(ms: number): string {
  if (ms >= 1000) {
    return `${(ms / 1000).toFixed(2)}s`;
  }
  return `${Math.round(ms)}ms`;
}
