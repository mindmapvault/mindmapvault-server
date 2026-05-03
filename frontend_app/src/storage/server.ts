// ── Server-mode storage adapter ──────────────────────────────────────────────
//
// Wraps the existing API client. Encrypted blobs are uploaded and downloaded
// through the backend, which talks to MinIO over the internal endpoint.

import { mindmapsApi } from '../api/mindmaps';
import type {
  MindMapCreatedResponse,
  MindMapDetail,
  MindMapListItem,
  StorageSummary,
  UpdateVaultMetaRequest,
  UpsertMindMapRequest,
} from '../types';
import type { StorageAdapter } from './types';

export class ServerStorageAdapter implements StorageAdapter {
  async listVaults(): Promise<MindMapListItem[]> {
    return mindmapsApi.list();
  }

  async getVault(id: string): Promise<MindMapDetail> {
    return mindmapsApi.get(id);
  }

  async createVault(body: UpsertMindMapRequest): Promise<MindMapCreatedResponse> {
    return mindmapsApi.create(body);
  }

  async updateVault(id: string, body: UpsertMindMapRequest): Promise<void> {
    const { url } = await mindmapsApi.update(id, body);
    // The presigned PUT URL is returned but the caller uploads via uploadBlob().
    void url;
  }

  async deleteVault(id: string): Promise<void> {
    await mindmapsApi.delete(id);
  }

  async uploadBlob(id: string, blob: Uint8Array): Promise<void> {
    await mindmapsApi.uploadBlob(id, blob);
  }

  async downloadBlob(id: string): Promise<Uint8Array> {
    return mindmapsApi.downloadBlob(id);
  }

  async updateMeta(id: string, body: UpdateVaultMetaRequest): Promise<void> {
    await mindmapsApi.updateMeta(id, body);
  }

  async getStorage(): Promise<StorageSummary> {
    return mindmapsApi.getStorage();
  }
}
