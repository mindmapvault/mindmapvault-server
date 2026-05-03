import type {
  MindMapCreatedResponse,
  MindMapDetail,
  MindMapListItem,
  PresignedUrlResponse,
  StorageSummary,
  UpdateVaultMetaRequest,
  UpsertMindMapRequest,
  VersionDetail,
} from '../types';
import { api } from './client';

export const mindmapsApi = {
  list: () => api.get<MindMapListItem[]>('/mindmaps'),
  get: (id: string) => api.get<MindMapDetail>(`/mindmaps/${id}`),
  create: (body: UpsertMindMapRequest) =>
    api.post<MindMapCreatedResponse>('/mindmaps', body),
  update: (id: string, body: UpsertMindMapRequest) =>
    api.put<PresignedUrlResponse>(`/mindmaps/${id}`, body),
  delete: (id: string) => api.delete<{ message: string }>(`/mindmaps/${id}`),
  getUploadUrl: (id: string) =>
    api.post<PresignedUrlResponse>(`/mindmaps/${id}/upload-url`, {}),
  uploadBlob: (id: string, blob: Uint8Array) =>
    api.postBytes<{ version_id: string }>(
      `/mindmaps/${id}/upload`,
      new Blob([new Uint8Array(blob).buffer], { type: 'application/octet-stream' }),
      'application/octet-stream',
    ),
  downloadBlob: (id: string, version_id?: string) =>
    api.getBytes(`/mindmaps/${id}/blob${version_id ? `?version_id=${encodeURIComponent(version_id)}` : ''}`),
  confirmUpload: (id: string, version_id: string) =>
    api.post<{ version_id: string }>(`/mindmaps/${id}/confirm-upload`, { version_id }),
  getDownloadUrl: (id: string, version_id?: string) =>
    api.get<PresignedUrlResponse>(
      `/mindmaps/${id}/download-url${version_id ? `?version_id=${encodeURIComponent(version_id)}` : ''}`,
    ),
  getStorage: () => api.get<StorageSummary>('/mindmaps/my/storage'),
  updateMeta: (id: string, body: UpdateVaultMetaRequest) =>
    api.put<{ ok: boolean }>(`/mindmaps/${id}/meta`, body),
  listVersions: (id: string) => api.get<VersionDetail[]>(`/mindmaps/${id}/versions`),
  deleteVersion: (id: string, versionId: string) =>
    api.delete<{ ok: boolean }>(`/mindmaps/${id}/versions/${encodeURIComponent(versionId)}`),
};

/** Upload an encrypted blob directly to a MinIO presigned PUT URL. */
export async function uploadBlob(presignedUrl: string, blob: Uint8Array): Promise<string | null> {
  const res = await fetch(presignedUrl, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/octet-stream' },
    body: blob as BodyInit,
  });
  if (!res.ok) throw new Error(`MinIO upload failed: ${res.status} ${res.statusText}`);
  return res.headers.get('x-amz-version-id');
}

/** Download an encrypted blob from a MinIO presigned GET URL. */
export async function downloadBlob(presignedUrl: string): Promise<Uint8Array> {
  const res = await fetch(presignedUrl);
  if (!res.ok) throw new Error(`MinIO download failed: ${res.status} ${res.statusText}`);
  return new Uint8Array(await res.arrayBuffer());
}
