// ── Storage factory ──────────────────────────────────────────────────────────
//
// Returns the correct StorageAdapter based on the current app mode.

import { LocalStorageAdapter } from './local';
import { OfflineStorageAdapter } from './offline';
import { ServerStorageAdapter } from './server';
import type { StorageAdapter } from './types';

export type { StorageAdapter } from './types';
export type { SyncStatus, SyncState } from './offline';
export { OfflineStorageAdapter } from './offline';

let serverAdapter: ServerStorageAdapter | null = null;
let localAdapter: LocalStorageAdapter | null = null;
let offlineAdapter: OfflineStorageAdapter | null = null;

export function getServerStorage(): StorageAdapter {
  if (!serverAdapter) serverAdapter = new ServerStorageAdapter();
  return serverAdapter;
}

export function getLocalStorage(): StorageAdapter {
  if (!localAdapter) localAdapter = new LocalStorageAdapter();
  return localAdapter;
}

/** Returns the offline-capable adapter (singleton). Used when running as installed PWA. */
export function getOfflineStorage(): OfflineStorageAdapter {
  if (!offlineAdapter) offlineAdapter = new OfflineStorageAdapter();
  return offlineAdapter;
}

/** Returns the adapter for the given mode. Defaults to server mode when omitted. */
export function getStorage(mode: 'server' | 'local' = 'server'): StorageAdapter {
  return mode === 'local' ? getLocalStorage() : getServerStorage();
}

/** Detects whether we're running inside the Tauri desktop shell. */
export function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/** Detects whether the app is running as an installed PWA (standalone display mode). */
export function isPwa(): boolean {
  if (typeof window === 'undefined') return false;
  return (
    window.matchMedia('(display-mode: standalone)').matches ||
    (window.navigator as unknown as { standalone?: boolean }).standalone === true
  );
}
