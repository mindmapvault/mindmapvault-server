import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MindMapEditorMode, MindMapEditorModePreference } from '../components/MindMapEditor.types';

const STORAGE_KEY = 'mindmapvault-editor-mode';

const isBrowser = () => typeof window !== 'undefined';

const readSavedPreference = (): MindMapEditorModePreference => {
  if (!isBrowser()) return 'auto';
  const raw = window.localStorage.getItem(STORAGE_KEY);
  return raw === 'desktop' || raw === 'mobile' ? raw : 'auto';
};

const detectAutoMode = (): MindMapEditorMode => {
  if (!isBrowser()) return 'desktop';
  const coarsePointer = window.matchMedia('(pointer: coarse)').matches;
  const noHover = window.matchMedia('(hover: none)').matches;
  const compactViewport = window.innerWidth < 960;
  return coarsePointer || noHover || compactViewport ? 'mobile' : 'desktop';
};

export function useAdaptiveEditorMode() {
  const [preference, setPreferenceState] = useState<MindMapEditorModePreference>(() => readSavedPreference());
  const [autoMode, setAutoMode] = useState<MindMapEditorMode>(() => detectAutoMode());

  useEffect(() => {
    if (!isBrowser()) return undefined;

    const syncAutoMode = () => setAutoMode(detectAutoMode());
    const coarsePointer = window.matchMedia('(pointer: coarse)');
    const noHover = window.matchMedia('(hover: none)');

    syncAutoMode();
    window.addEventListener('resize', syncAutoMode);
    coarsePointer.addEventListener?.('change', syncAutoMode);
    noHover.addEventListener?.('change', syncAutoMode);

    return () => {
      window.removeEventListener('resize', syncAutoMode);
      coarsePointer.removeEventListener?.('change', syncAutoMode);
      noHover.removeEventListener?.('change', syncAutoMode);
    };
  }, []);

  const setPreference = useCallback((next: MindMapEditorModePreference) => {
    setPreferenceState(next);
    if (!isBrowser()) return;
    if (next === 'auto') window.localStorage.removeItem(STORAGE_KEY);
    else window.localStorage.setItem(STORAGE_KEY, next);
  }, []);

  const mode = useMemo<MindMapEditorMode>(() => {
    return preference === 'auto' ? autoMode : preference;
  }, [autoMode, preference]);

  return {
    mode,
    autoMode,
    preference,
    setPreference,
  };
}