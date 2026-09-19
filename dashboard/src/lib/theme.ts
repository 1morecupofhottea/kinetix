import { useCallback, useEffect, useState } from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';

const STORAGE_KEY = 'kinetix_theme';

function systemPrefersDark(): boolean {
  return (
    typeof window !== 'undefined' &&
    window.matchMedia &&
    window.matchMedia('(prefers-color-scheme: dark)').matches
  );
}

function resolveDark(mode: ThemeMode): boolean {
  if (mode === 'system') return systemPrefersDark();
  return mode === 'dark';
}

function applyDark(dark: boolean) {
  const root = document.documentElement;
  root.classList.toggle('dark', dark);
  root.style.colorScheme = dark ? 'dark' : 'light';
}

function readStored(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    /* ignore */
  }
  return 'system';
}

/**
 * Light / dark / system theme with persistence. `mode` is the user's choice
 * (`system` follows the OS); `isDark` is the resolved state. When the OS
 * preference changes while in `system` mode the theme follows automatically.
 */
export function useTheme() {
  const [mode, setMode] = useState<ThemeMode>(readStored);
  const [isDark, setIsDark] = useState<boolean>(() => resolveDark(readStored()));

  // Apply whenever the mode (or, in system mode, the OS preference) changes.
  useEffect(() => {
    const dark = resolveDark(mode);
    setIsDark(dark);
    applyDark(dark);
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      /* ignore */
    }
  }, [mode]);

  // Follow the OS while in system mode.
  useEffect(() => {
    if (mode !== 'system' || !window.matchMedia) return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => {
      const dark = mq.matches;
      setIsDark(dark);
      applyDark(dark);
    };
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [mode]);

  const setTheme = useCallback((next: ThemeMode) => setMode(next), []);

  return { mode, isDark, setTheme };
}

/**
 * Apply the stored theme as early as possible (before React mounts) to avoid a
 * flash of the wrong palette.
 */
export function applyStoredThemeEarly() {
  const mode = readStored();
  applyDark(resolveDark(mode));
}
