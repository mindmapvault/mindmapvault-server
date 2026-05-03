import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type AppMode = 'server' | 'local' | null;

interface ModeState {
  /** null = not yet chosen (show mode selection page) */
  mode: AppMode;
  setMode: (mode: AppMode) => void;
  clearMode: () => void;
}

export const useModeStore = create<ModeState>()(
  persist(
    (set) => ({
      mode: null,
      setMode: (mode) => set({ mode }),
      clearMode: () => set({ mode: null }),
    }),
    { name: 'mindmapvault-mode' },
  ),
);
