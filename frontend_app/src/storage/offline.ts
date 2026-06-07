// ── Offline-capable storage adapter ──────────────────────────────────────────
//
// Wraps ServerStorageAdapter with a read-through / write-through IndexedDB cache.
//
// When online  → reads and writes go to the server; results are cached in IDB.
// When offline → reads are served from IDB; writes are cached locally and
//               pushed to a sync queue that is drained on reconnect.
//
// Conflict detection: if the server's minio_version_id has changed while we
// were offline, a 'conflict' status is emitted so the UI can ask the user
// whether to keep their offline edits or discard them.

import type {
  MindMapCreatedResponse,
  MindMapDetail,
  MindMapListItem,
  StorageSummary,
  UpdateVaultMetaRequest,
  UpsertMindMapRequest,
} from '../types';
import { MmvIdb, type SyncQueueEntry } from './idb';
import { ServerStorageAdapter } from './server';
import type { StorageAdapter } from './types';

export type SyncState = 'idle' | 'syncing' | 'conflict' | 'error';

export interface SyncStatus {
  state: SyncState;
  pendingCount: number;
  lastSyncedAt: number | null;
  conflictVaultId?: string;
}

function toArrayBuffer(blob: Uint8Array): ArrayBuffer {
  return blob.buffer.slice(blob.byteOffset, blob.byteOffset + blob.byteLength) as ArrayBuffer;
}

export class OfflineStorageAdapter implements StorageAdapter {
  private inner: ServerStorageAdapter;
  private idb: MmvIdb;
  private _status: SyncStatus = { state: 'idle', pendingCount: 0, lastSyncedAt: null };
  private statusListeners = new Set<(s: SyncStatus) => void>();
  private draining = false;

  constructor() {
    this.inner = new ServerStorageAdapter();
    this.idb = new MmvIdb();
    void this.idb.getPendingCount().then((count) => {
      this.emitStatus({ ...this._status, pendingCount: count });
    }).catch(() => { /* IDB unavailable — graceful degradation */ });
  }

  // ── Observable status ───────────────────────────────────────────────────────

  get status(): SyncStatus { return this._status; }

  onStatusChange(cb: (s: SyncStatus) => void): () => void {
    this.statusListeners.add(cb);
    cb(this._status);
    return () => this.statusListeners.delete(cb);
  }

  private emitStatus(status: SyncStatus): void {
    this._status = status;
    for (const cb of this.statusListeners) cb(status);
  }

  private async refreshPendingCount(): Promise<void> {
    const count = await this.idb.getPendingCount();
    this.emitStatus({ ...this._status, pendingCount: count });
  }

  isOnline(): boolean { return navigator.onLine; }

  // ── StorageAdapter implementation ───────────────────────────────────────────

  async listVaults(): Promise<MindMapListItem[]> {
    if (this.isOnline()) {
      const items = await this.inner.listVaults();
      await this.idb.setVaultList(items).catch(() => {});
      return items;
    }
    const cached = await this.idb.getVaultList();
    if (cached) return cached;
    throw new Error('Vault list is not available offline. Connect to the internet to load your vaults.');
  }

  async getVault(id: string): Promise<MindMapDetail> {
    if (this.isOnline()) {
      const detail = await this.inner.getVault(id);
      await this.idb.setVaultMeta(id, detail).catch(() => {});
      return detail;
    }
    const cached = await this.idb.getVaultMeta(id);
    if (cached) return cached;
    throw new Error('This vault is not available offline. Connect to the internet to open it.');
  }

  async createVault(body: UpsertMindMapRequest): Promise<MindMapCreatedResponse> {
    if (!this.isOnline()) throw new Error('Creating a new vault requires an internet connection.');
    return this.inner.createVault(body);
  }

  async updateVault(id: string, body: UpsertMindMapRequest): Promise<void> {
    if (this.isOnline()) {
      await this.inner.updateVault(id, body);
    } else {
      await this.idb.enqueue({ op: 'updateVault', vault_id: id, payload: body, created_at: Date.now(), attempts: 0 });
      await this.refreshPendingCount();
    }
  }

  async deleteVault(id: string): Promise<void> {
    if (!this.isOnline()) throw new Error('Deleting a vault requires an internet connection.');
    await this.inner.deleteVault(id);
    await Promise.all([
      this.idb.removeVaultMeta(id).catch(() => {}),
      this.idb.removeBlob(id).catch(() => {}),
    ]);
  }

  async uploadBlob(id: string, blob: Uint8Array): Promise<void> {
    const buf = toArrayBuffer(blob);
    const existing = await this.idb.getBlob(id).catch(() => null);
    const base_version_id = existing?.version_id ?? null;

    // Always persist locally first so the user's work is never lost
    await this.idb.setBlob(id, buf, base_version_id).catch(() => {});

    if (this.isOnline()) {
      await this.inner.uploadBlob(id, blob);
      // Refresh to capture the new minio_version_id from the server
      const detail = await this.inner.getVault(id);
      await this.idb.setVaultMeta(id, detail).catch(() => {});
      await this.idb.setBlob(id, buf, detail.minio_version_id).catch(() => {});
      this.emitStatus({ ...this._status, lastSyncedAt: Date.now() });
    } else {
      await this.idb.enqueue({
        op: 'uploadBlob',
        vault_id: id,
        base_version_id,
        created_at: Date.now(),
        attempts: 0,
      });
      await this.refreshPendingCount();
    }
  }

  async downloadBlob(id: string): Promise<Uint8Array> {
    if (this.isOnline()) {
      const bytes = await this.inner.downloadBlob(id);
      const meta = await this.idb.getVaultMeta(id).catch(() => null);
      const buf = toArrayBuffer(bytes);
      await this.idb.setBlob(id, buf, meta?.minio_version_id ?? null).catch(() => {});
      return bytes;
    }
    const cached = await this.idb.getBlob(id);
    if (cached) return new Uint8Array(cached.buf);
    throw new Error('Vault data is not cached for offline use. Open this vault once while online to enable offline access.');
  }

  async updateMeta(id: string, body: UpdateVaultMetaRequest): Promise<void> {
    if (this.isOnline()) {
      await this.inner.updateMeta(id, body);
    } else {
      await this.idb.enqueue({ op: 'updateMeta', vault_id: id, payload: body, created_at: Date.now(), attempts: 0 });
      await this.refreshPendingCount();
    }
  }

  async getStorage(): Promise<StorageSummary> {
    if (this.isOnline()) return this.inner.getStorage();
    return { vaults: [], total_bytes: 0, free_tier_bytes: 0 };
  }

  // ── Sync queue drain ────────────────────────────────────────────────────────

  async drainSyncQueue(): Promise<void> {
    if (!this.isOnline() || this.draining) return;
    const queue = await this.idb.getSyncQueue();
    if (queue.length === 0) return;

    this.draining = true;
    this.emitStatus({ ...this._status, state: 'syncing' });

    const conflictEntries: SyncQueueEntry[] = [];
    let anyError = false;

    for (const entry of queue) {
      try {
        if (entry.op === 'uploadBlob') {
          const serverDetail = await this.inner.getVault(entry.vault_id);
          const serverVersion = serverDetail.minio_version_id;
          if (
            entry.base_version_id !== null &&
            serverVersion !== null &&
            entry.base_version_id !== serverVersion
          ) {
            conflictEntries.push(entry);
            continue;
          }
        }
        await this.executeQueueEntry(entry);
        await this.idb.dequeue(entry.seq!);
      } catch {
        anyError = true;
        await this.idb.incrementAttempts(entry.seq!).catch(() => {});
        const refreshed = await this.idb.getSyncQueue();
        const failed = refreshed.find((e) => e.seq === entry.seq);
        if (failed && failed.attempts >= 3) {
          await this.idb.dequeue(entry.seq!).catch(() => {});
        }
      }
    }

    this.draining = false;
    const remaining = await this.idb.getPendingCount();

    if (conflictEntries.length > 0) {
      this.emitStatus({
        state: 'conflict',
        pendingCount: remaining,
        lastSyncedAt: this._status.lastSyncedAt,
        conflictVaultId: conflictEntries[0].vault_id,
      });
    } else if (anyError && remaining > 0) {
      this.emitStatus({ state: 'error', pendingCount: remaining, lastSyncedAt: this._status.lastSyncedAt });
    } else {
      this.emitStatus({ state: 'idle', pendingCount: 0, lastSyncedAt: Date.now() });
    }
  }

  private async executeQueueEntry(entry: SyncQueueEntry): Promise<void> {
    if (entry.op === 'updateVault') {
      await this.inner.updateVault(entry.vault_id, entry.payload as UpsertMindMapRequest);
    } else if (entry.op === 'uploadBlob') {
      const cached = await this.idb.getBlob(entry.vault_id);
      if (!cached) throw new Error('Offline blob cache missing');
      const bytes = new Uint8Array(cached.buf);
      await this.inner.uploadBlob(entry.vault_id, bytes);
      const detail = await this.inner.getVault(entry.vault_id);
      await this.idb.setVaultMeta(entry.vault_id, detail).catch(() => {});
      await this.idb.setBlob(entry.vault_id, cached.buf, detail.minio_version_id).catch(() => {});
    } else if (entry.op === 'updateMeta') {
      await this.inner.updateMeta(entry.vault_id, entry.payload as UpdateVaultMetaRequest);
    }
  }

  // ── Conflict resolution ─────────────────────────────────────────────────────

  async resolveConflict(vaultId: string, choice: 'local' | 'server'): Promise<void> {
    const queue = await this.idb.getSyncQueue();
    const vaultOps = queue.filter((e) => e.vault_id === vaultId);

    if (choice === 'local') {
      // Upload local changes, overwriting the server version
      for (const entry of vaultOps) {
        try {
          await this.executeQueueEntry(entry);
          await this.idb.dequeue(entry.seq!);
        } catch { /* best-effort */ }
      }
    } else {
      // Discard local changes: remove all pending ops for this vault
      await this.idb.clearSyncQueueForVault(vaultId);
    }

    const remaining = await this.idb.getPendingCount();
    this.emitStatus({ state: 'idle', pendingCount: remaining, lastSyncedAt: Date.now() });
  }
}
