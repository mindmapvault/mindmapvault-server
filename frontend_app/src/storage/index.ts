// ── Storage factory ──────────────────────────────────────────────────────────
//
// Returns the correct StorageAdapter based on the current app mode.

import { LocalStorageAdapter } from './local';
import { ServerStorageAdapter } from './server';
import type { StorageAdapter } from './types';

export type { StorageAdapter } from './types';

let serverAdapter: ServerStorageAdapter | null = null;
let localAdapter: LocalStorageAdapter | null = null;

export function getServerStorage(): StorageAdapter {
  if (!serverAdapter) serverAdapter = new ServerStorageAdapter();
  return serverAdapter;
}

export function getLocalStorage(): StorageAdapter {
  if (!localAdapter) localAdapter = new LocalStorageAdapter();
  return localAdapter;
}

/** Returns the adapter for the given mode. */
export function getStorage(mode: 'server' | 'local'): StorageAdapter {
  return mode === 'local' ? getLocalStorage() : getServerStorage();
}

/** Detects whether we're running inside the Tauri desktop shell. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}
