export const DESIGN_TOKENS = {
  colors: {
    background: '#fdfbf7', // Warm Paper
    foreground: '#2d2d2d', // Soft Pencil Black
    muted: '#e5e0d8', // Old Paper / Erased Pencil
    accent: '#ff4d4d', // Red Correction Marker
    border: '#2d2d2d', // Pencil Lead
    secondaryAccent: '#2d5da1', // Blue Ballpoint Pen
    postit: '#fff9c4', // Post-it Yellow
    postitBorder: '#ecd76e',
    penGreen: '#2e7d32',
    markerOrange: '#d97706',
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
    standard: '4px 4px 0px 0px #2d2d2d',
    emphasized: '8px 8px 0px 0px #2d2d2d',
    subtle: '2px 2px 0px 0px #2d2d2d',
    softPaper: '3px 3px 0px 0px rgba(45, 45, 45, 0.1)',
    blue: '4px 4px 0px 0px #2d5da1',
    red: '4px 4px 0px 0px #ff4d4d',
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
