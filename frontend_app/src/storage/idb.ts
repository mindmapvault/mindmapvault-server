// ── IndexedDB persistence for offline mode ───────────────────────────────────
//
// DB: mmv-offline-v1
// Stores:
//   vault_list  — cached vault list (single record keyed 'list')
//   vault_meta  — MindMapDetail per vault
//   vault_blobs — encrypted blob bytes per vault
//   sync_queue  — pending server operations (autoIncrement key)

import type { MindMapDetail, MindMapListItem } from '../types';

export interface CachedBlob {
  id: string;
  buf: ArrayBuffer;
  version_id: string | null;
  cached_at: number;
}

export interface SyncQueueEntry {
  seq?: number;
  op: 'updateVault' | 'uploadBlob' | 'updateMeta';
  vault_id: string;
  payload?: unknown;
  base_version_id?: string | null;
  created_at: number;
  attempts: number;
}

const DB_NAME = 'mmv-offline-v1';
const DB_VERSION = 1;

function promisifyRequest<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export class MmvIdb {
  private db: IDBDatabase | null = null;
  private opening: Promise<IDBDatabase> | null = null;

  private open(): Promise<IDBDatabase> {
    if (this.db) return Promise.resolve(this.db);
    if (this.opening) return this.opening;
    this.opening = new Promise<IDBDatabase>((resolve, reject) => {
      const req = indexedDB.open(DB_NAME, DB_VERSION);
      req.onupgradeneeded = (e) => {
        const db = (e.target as IDBOpenDBRequest).result;
        if (!db.objectStoreNames.contains('vault_list')) {
          db.createObjectStore('vault_list');
        }
        if (!db.objectStoreNames.contains('vault_meta')) {
          db.createObjectStore('vault_meta', { keyPath: 'id' });
        }
        if (!db.objectStoreNames.contains('vault_blobs')) {
          db.createObjectStore('vault_blobs', { keyPath: 'id' });
        }
        if (!db.objectStoreNames.contains('sync_queue')) {
          const qs = db.createObjectStore('sync_queue', { keyPath: 'seq', autoIncrement: true });
          qs.createIndex('by_created', 'created_at');
        }
      };
      req.onsuccess = () => { this.db = req.result; resolve(req.result); };
      req.onerror = () => reject(req.error);
      req.onblocked = () => reject(new Error('IndexedDB blocked'));
    });
    return this.opening;
  }

  // ── vault_list ──────────────────────────────────────────────────────────────

  async getVaultList(): Promise<MindMapListItem[] | null> {
    const db = await this.open();
    const tx = db.transaction('vault_list', 'readonly');
    const result = await promisifyRequest<{ items: MindMapListItem[]; cached_at: number } | undefined>(
      tx.objectStore('vault_list').get('list'),
    );
    return result?.items ?? null;
  }

  async setVaultList(items: MindMapListItem[]): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('vault_list', 'readwrite');
    await promisifyRequest(tx.objectStore('vault_list').put({ items, cached_at: Date.now() }, 'list'));
  }

  // ── vault_meta ──────────────────────────────────────────────────────────────

  async getVaultMeta(id: string): Promise<MindMapDetail | null> {
    const db = await this.open();
    const tx = db.transaction('vault_meta', 'readonly');
    const result = await promisifyRequest<(MindMapDetail & { cached_at: number }) | undefined>(
      tx.objectStore('vault_meta').get(id),
    );
    if (!result) return null;
    const { cached_at: _, ...detail } = result;
    void _;
    return detail as MindMapDetail;
  }

  async setVaultMeta(id: string, detail: MindMapDetail): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('vault_meta', 'readwrite');
    await promisifyRequest(tx.objectStore('vault_meta').put({ ...detail, id, cached_at: Date.now() }));
  }

  async removeVaultMeta(id: string): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('vault_meta', 'readwrite');
    await promisifyRequest(tx.objectStore('vault_meta').delete(id));
  }

  // ── vault_blobs ─────────────────────────────────────────────────────────────

  async getBlob(id: string): Promise<CachedBlob | null> {
    const db = await this.open();
    const tx = db.transaction('vault_blobs', 'readonly');
    const result = await promisifyRequest<CachedBlob | undefined>(
      tx.objectStore('vault_blobs').get(id),
    );
    return result ?? null;
  }

  async setBlob(id: string, buf: ArrayBuffer, version_id: string | null): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('vault_blobs', 'readwrite');
    await promisifyRequest(tx.objectStore('vault_blobs').put({ id, buf, version_id, cached_at: Date.now() } satisfies CachedBlob));
  }

  async removeBlob(id: string): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('vault_blobs', 'readwrite');
    await promisifyRequest(tx.objectStore('vault_blobs').delete(id));
  }

  // ── sync_queue ──────────────────────────────────────────────────────────────

  async enqueue(entry: Omit<SyncQueueEntry, 'seq'>): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('sync_queue', 'readwrite');
    await promisifyRequest(tx.objectStore('sync_queue').add(entry));
  }

  async getSyncQueue(): Promise<SyncQueueEntry[]> {
    const db = await this.open();
    const tx = db.transaction('sync_queue', 'readonly');
    return promisifyRequest<SyncQueueEntry[]>(tx.objectStore('sync_queue').getAll());
  }

  async dequeue(seq: number): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('sync_queue', 'readwrite');
    await promisifyRequest(tx.objectStore('sync_queue').delete(seq));
  }

  async incrementAttempts(seq: number): Promise<void> {
    const db = await this.open();
    const tx = db.transaction('sync_queue', 'readwrite');
    const store = tx.objectStore('sync_queue');
    const entry = await promisifyRequest<SyncQueueEntry | undefined>(store.get(seq));
    if (entry) await promisifyRequest(store.put({ ...entry, attempts: entry.attempts + 1 }));
  }

  async getPendingCount(): Promise<number> {
    const db = await this.open();
    const tx = db.transaction('sync_queue', 'readonly');
    return promisifyRequest<number>(tx.objectStore('sync_queue').count());
  }

  async clearSyncQueueForVault(vaultId: string): Promise<void> {
    const queue = await this.getSyncQueue();
    const toRemove = queue.filter((e) => e.vault_id === vaultId);
    const db = await this.open();
    for (const entry of toRemove) {
      const tx = db.transaction('sync_queue', 'readwrite');
      await promisifyRequest(tx.objectStore('sync_queue').delete(entry.seq!));
    }
  }
}
