import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { mindmapsApi } from '../api/mindmaps';
import { ApiError } from '../api/client';
import encryptedVaultApi from '../api/encryptedVault';
import { BoardEditor, type BoardEditorHandle } from '../components/BoardEditor';
import { decryptBoard, decryptTitle, encryptBoard, encryptTitle } from '../crypto/vault';
import { isVaultPreviewAttachmentMeta } from '../utils/vaultPreview';
import { hybridDecap, hybridEncap } from '../crypto/kem';
import { fromBase64, toBase64 } from '../crypto/utils';
import { encryptAttachmentForOwner, decryptAttachmentForOwner } from '../crypto/encryptedVault';
import { getStorage } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import type { BoardData, BoardImageCard, BoardPdfCard } from '../board/BoardTypes';
import { emptyBoardData } from '../board/BoardTypes';
import { renderPdfThumbnail } from '../utils/pdfThumbnail';

// Prefix used to mark that a card's src is an encrypted attachment, not a plain URL
const ATTACHMENT_PREFIX = 'attachment:';

export function BoardPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { sessionKeys } = useAuthStore();
  const mode = useModeStore((s) => s.mode);
  const isLocalMode = mode === 'local';
  const storage = useMemo(() => getStorage(), []);

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [saveMsg, setSaveMsg] = useState('');

  const [title, setTitle] = useState('');
  const [versionLabel, setVersionLabel] = useState('');
  const [boardData, setBoardData] = useState<BoardData | null>(null);

  // Map: "attachment:ID" → object URL (for rendering encrypted images)
  const [resolvedImageSrcs, setResolvedImageSrcs] = useState<Record<string, string>>({});
  // Track all created object URLs so we can revoke them on unmount
  const objectUrlsRef = useRef<string[]>([]);

  const boardEditorRef = useRef<BoardEditorHandle>(null);
  const pendingSaveRef = useRef<BoardData | null>(null);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isDirtyRef = useRef(false);
  const [isDirty, setIsDirty] = useState(false);

  // Revoke object URLs on unmount
  useEffect(() => {
    return () => {
      for (const url of objectUrlsRef.current) URL.revokeObjectURL(url);
    };
  }, []);

  // ── Resolve attachment images in loaded board data ────────────────────────
  const resolveAttachmentImages = useCallback(async (data: BoardData) => {
    if (!sessionKeys || isLocalMode) return;
    const imageCards = (data.cards ?? []).filter(
      (c): c is BoardImageCard => c.type === 'image' && c.src.startsWith(ATTACHMENT_PREFIX),
    );
    const pdfCards = (data.cards ?? []).filter(
      (c): c is BoardPdfCard => c.type === 'pdf' && c.src.startsWith(ATTACHMENT_PREFIX),
    );
    if (imageCards.length === 0 && pdfCards.length === 0) return;

    const resolved: Record<string, string> = {};
    await Promise.allSettled([
      ...imageCards.map(async (card) => {
        const attachmentId = card.src.slice(ATTACHMENT_PREFIX.length);
        try {
          const { download_url, encryption_meta, content_type } = await encryptedVaultApi.getAttachmentDownload(id!, attachmentId);
          const cipher = await encryptedVaultApi.downloadUrl(download_url);
          const plain = await decryptAttachmentForOwner(cipher, encryption_meta ?? undefined, sessionKeys.masterKey);
          const ab = plain.buffer.slice(plain.byteOffset, plain.byteOffset + plain.byteLength) as ArrayBuffer;
          const blob = new Blob([ab], { type: content_type || 'image/jpeg' });
          const objUrl = URL.createObjectURL(blob);
          objectUrlsRef.current.push(objUrl);
          resolved[card.src] = objUrl;
        } catch {
          // Individual image failure is non-fatal
        }
      }),
      ...pdfCards.map(async (card) => {
        const attachmentId = card.src.slice(ATTACHMENT_PREFIX.length);
        try {
          const { download_url, encryption_meta } = await encryptedVaultApi.getAttachmentDownload(id!, attachmentId);
          const cipher = await encryptedVaultApi.downloadUrl(download_url);
          const plain = await decryptAttachmentForOwner(cipher, encryption_meta ?? undefined, sessionKeys.masterKey);
          const { thumbnail } = await renderPdfThumbnail(plain);
          resolved[card.src] = thumbnail;
        } catch {
          // PDF thumbnail failure is non-fatal
        }
      }),
    ]);
    if (Object.keys(resolved).length > 0) {
      setResolvedImageSrcs((prev) => ({ ...prev, ...resolved }));
    }
  }, [id, isLocalMode, sessionKeys]);

  // ── Load ────────────────────────────────────────────────────────────────────
  const load = useCallback(async () => {
    if (!id || !sessionKeys) return;
    setLoading(true);
    setError('');
    try {
      const detail = await storage.getVault(id);
      const plainTitle = await decryptTitle(detail.title_encrypted, sessionKeys.masterKey);
      setTitle(plainTitle);

      const dek = await hybridDecap(
        sessionKeys.classicalPrivKey,
        sessionKeys.pqPrivKey,
        fromBase64(detail.eph_classical_public),
        fromBase64(detail.eph_pq_ciphertext),
        fromBase64(detail.wrapped_dek),
      );

      let data: BoardData;
      try {
        const blob = await storage.downloadBlob(id);
        data = await decryptBoard(blob, dek);
      } catch (blobErr) {
        if (
          blobErr instanceof ApiError &&
          (blobErr.status === 404 ||
            (blobErr.status === 500 && blobErr.message === 'storage error'))
        ) {
          // Blob missing from storage (deleted or never uploaded) — open empty board.
          data = emptyBoardData(plainTitle);
          setSaveMsg('Board content not found in storage — starting with empty board.');
        } else {
          throw blobErr;
        }
      }
      setBoardData(data);
      void resolveAttachmentImages(data);

      const vdt = new Date(detail.updated_at);
      if (!isLocalMode) {
        void mindmapsApi.listVersions(id).then((versions) => {
          const total = Math.max(versions.length, detail.total_version_count ?? 0);
          setVersionLabel(`v${total}`);
        }).catch(() => {
          setVersionLabel(`v ${vdt.toLocaleDateString()}`);
        });
      } else {
        setVersionLabel(`v ${vdt.toLocaleDateString()}`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load board');
    } finally {
      setLoading(false);
    }
  }, [id, sessionKeys, isLocalMode, storage, resolveAttachmentImages]);

  useEffect(() => {
    if (sessionKeys) void load();
    else setLoading(false);
  }, [sessionKeys, load]);

  // Keyboard shortcut
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        if (pendingSaveRef.current) void flushSave(pendingSaveRef.current);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── Board preview (server mode) ──────────────────────────────────────────────
  const syncBoardPreview = useCallback(async (data: BoardData) => {
    if (!id || !sessionKeys || isLocalMode || !boardEditorRef.current) return;
    try {
      const blob = await boardEditorRef.current.captureCanvas();
      if (!blob) return;
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const contentType = blob.type || 'image/webp';
      const encrypted = await encryptAttachmentForOwner(bytes, sessionKeys.masterKey);
      const nodeCount = data.cards.length;
      const attachmentCount = data.cards.filter((c) => c.type === 'image').length;
      const init = await encryptedVaultApi.initAttachment(id, {
        name: '__vault_preview_dark.webp',
        content_type: contentType,
        size: encrypted.ciphertext.byteLength,
        encrypted: true,
        encryption_meta: {
          ...encrypted.encryptionMeta,
          cryptmind_role: 'vault_preview',
          preview_theme: 'dark',
          node_count: nodeCount,
          note_count: 0,
          attachment_count: attachmentCount,
        },
      });
      const versionId = await encryptedVaultApi.uploadPresigned(init.upload_url, encrypted.ciphertext, {
        ...init.upload_headers,
        'Content-Type': contentType,
      });
      await encryptedVaultApi.completeAttachment(id, init.attachment_id, versionId ?? '', encrypted.checksumSha256);
      // Delete old preview attachments
      const attachmentList = await encryptedVaultApi.listAttachments(id);
      for (const a of attachmentList) {
        if (a.id === init.attachment_id) continue;
        const meta = (a.encryption_meta ?? null) as Record<string, unknown> | null;
        if (isVaultPreviewAttachmentMeta(meta)) {
          await encryptedVaultApi.deleteAttachment(id, a.id);
        }
      }
    } catch {
      // Preview generation is non-fatal
    }
  }, [id, isLocalMode, sessionKeys]);

  // ── Save ────────────────────────────────────────────────────────────────────
  const flushSave = useCallback(async (data: BoardData) => {
    if (!id || !sessionKeys) return;
    if (saveTimerRef.current) {
      clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    setSaving(true);
    setError('');
    setSaveMsg('');
    try {
      const titleEnc = await encryptTitle(title, sessionKeys.masterKey);
      const { ephClassicalPublic, ephPqCiphertext, wrappedDek, dek } = await hybridEncap(
        sessionKeys.classicalPubKey,
        sessionKeys.pqPubKey,
      );
      const encBlob = await encryptBoard(data, dek);
      await storage.updateVault(id, {
        title_encrypted: titleEnc,
        eph_classical_public: toBase64(ephClassicalPublic),
        eph_pq_ciphertext: toBase64(ephPqCiphertext),
        wrapped_dek: toBase64(wrappedDek),
      });
      await storage.uploadBlob(id, encBlob);
      void syncBoardPreview(data);
      pendingSaveRef.current = null;
      isDirtyRef.current = false;
      setIsDirty(false);
      setSaveMsg('Saved');
      setTimeout(() => setSaveMsg(''), 3000);

      const refreshed = await storage.getVault(id);
      const sdt = new Date(refreshed.updated_at);
      if (!isLocalMode) {
        void mindmapsApi.listVersions(id).then((versions) => {
          const total = Math.max(versions.length, refreshed.total_version_count ?? 0);
          setVersionLabel(`v${total}`);
        }).catch(() => {
          setVersionLabel(`v ${sdt.toLocaleDateString()}`);
        });
      } else {
        setVersionLabel(`v ${sdt.toLocaleDateString()}`);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed');
    } finally {
      setSaving(false);
    }
  }, [id, sessionKeys, storage, title, isLocalMode, syncBoardPreview]);

  const handleBoardChange = useCallback((data: BoardData) => {
    pendingSaveRef.current = data;
    setBoardData(data);
    if (!isDirtyRef.current) {
      isDirtyRef.current = true;
      setIsDirty(true);
    }
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      if (pendingSaveRef.current) void flushSave(pendingSaveRef.current);
    }, 2000);
  }, [flushSave]);

  // ── Image upload ────────────────────────────────────────────────────────────
  const handlePickImage = useCallback(async (file: File): Promise<{ storedSrc: string; displaySrc: string }> => {
    const plaintext = new Uint8Array(await file.arrayBuffer());

    // Local mode: store as data URL inline in the board JSON
    if (isLocalMode || !sessionKeys) {
      return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
          const dataUrl = reader.result as string;
          resolve({ storedSrc: dataUrl, displaySrc: dataUrl });
        };
        reader.onerror = () => reject(new Error('Failed to read file'));
        reader.readAsDataURL(file);
      });
    }

    // Server mode: encrypt and upload as an attachment
    const encrypted = await encryptAttachmentForOwner(plaintext, sessionKeys.masterKey);
    const init = await encryptedVaultApi.initAttachment(id!, {
      name: file.name,
      content_type: file.type || 'image/jpeg',
      size: encrypted.ciphertext.byteLength,
      encrypted: true,
      encryption_meta: {
        ...encrypted.encryptionMeta,
        content_type: file.type || 'image/jpeg',
        cryptmind_role: 'board_image',
      },
    });
    const versionId = await encryptedVaultApi.uploadPresigned(init.upload_url, encrypted.ciphertext, {
      ...init.upload_headers,
      'Content-Type': file.type || 'image/jpeg',
    });
    await encryptedVaultApi.completeAttachment(id!, init.attachment_id, versionId ?? '', encrypted.checksumSha256);

    // Create an object URL from the decrypted bytes for immediate display
    const blob = new Blob([plaintext], { type: file.type || 'image/jpeg' });
    const objUrl = URL.createObjectURL(blob);
    objectUrlsRef.current.push(objUrl);

    const storedSrc = `${ATTACHMENT_PREFIX}${init.attachment_id}`;
    setResolvedImageSrcs((prev) => ({ ...prev, [storedSrc]: objUrl }));

    return { storedSrc, displaySrc: objUrl };
  }, [id, isLocalMode, sessionKeys]);

  // ── PDF upload ───────────────────────────────────────────────────────────────
  const handlePickPdf = useCallback(async (file: File): Promise<{ storedSrc: string; thumbnailSrc: string; pageCount: number }> => {
    const plaintext = new Uint8Array(await file.arrayBuffer());
    const { thumbnail, pageCount } = await renderPdfThumbnail(plaintext);

    // Local mode: store raw PDF as data URL inline
    if (isLocalMode || !sessionKeys) {
      return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve({ storedSrc: reader.result as string, thumbnailSrc: thumbnail, pageCount });
        reader.onerror = () => reject(new Error('Failed to read PDF'));
        reader.readAsDataURL(file);
      });
    }

    const encrypted = await encryptAttachmentForOwner(plaintext, sessionKeys.masterKey);
    const init = await encryptedVaultApi.initAttachment(id!, {
      name: file.name,
      content_type: 'application/pdf',
      size: encrypted.ciphertext.byteLength,
      encrypted: true,
      encryption_meta: {
        ...encrypted.encryptionMeta,
        content_type: 'application/pdf',
        cryptmind_role: 'board_pdf',
      },
    });
    const versionId = await encryptedVaultApi.uploadPresigned(init.upload_url, encrypted.ciphertext, {
      ...init.upload_headers,
      'Content-Type': 'application/pdf',
    });
    await encryptedVaultApi.completeAttachment(id!, init.attachment_id, versionId ?? '', encrypted.checksumSha256);

    const storedSrc = `${ATTACHMENT_PREFIX}${init.attachment_id}`;
    setResolvedImageSrcs((prev) => ({ ...prev, [storedSrc]: thumbnail }));

    return { storedSrc, thumbnailSrc: thumbnail, pageCount };
  }, [id, isLocalMode, sessionKeys]);

  // ── Render ──────────────────────────────────────────────────────────────────
  if (!sessionKeys && !isLocalMode) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100vh', color: '#888' }}>
        Not authenticated
      </div>
    );
  }

  if (loading) {
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100vh', color: '#888', gap: 12 }}>
        <svg style={{ animation: 'spin 0.9s linear infinite' }} width="22" height="22" viewBox="0 0 24 24" fill="none">
          <circle opacity=".2" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"/>
          <path opacity=".8" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z"/>
        </svg>
        Loading board…
      </div>
    );
  }

  if (error && !boardData) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100vh', color: '#e74c3c', gap: 12 }}>
        <span>{error}</span>
        <button onClick={() => void load()} style={{ padding: '8px 18px', borderRadius: 7, background: '#7c6af7', color: '#fff', border: 'none', cursor: 'pointer' }}>Retry</button>
      </div>
    );
  }

  const data = boardData ?? emptyBoardData(title || 'Evidence Board');

  return (
    <BoardEditor
      ref={boardEditorRef}
      title={title}
      versionLabel={versionLabel}
      data={data}
      isDirty={isDirty}
      saving={saving}
      saveMsg={saveMsg}
      error={error}
      resolvedImageSrcs={resolvedImageSrcs}
      onPickImage={handlePickImage}
      onPickPdf={handlePickPdf}
      onBack={() => navigate('/vaults')}
      onSave={handleBoardChange}
    />
  );
}
