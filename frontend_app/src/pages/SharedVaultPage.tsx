import { useCallback, useEffect, useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import encryptedVaultApi from '../api/encryptedVault';
import SubscriptionDialog from '../components/SubscriptionDialog';
import { UnlockModal } from '../components/UnlockModal';
import {
  decryptBytesForShare,
  decryptShareBundle,
  encryptAttachmentForOwner,
  unlockEncryptedShareBundle,
} from '../crypto/encryptedVault';
import { hybridEncap } from '../crypto/kem';
import { encryptTitle, encryptTree } from '../crypto/vault';
import { getStorage } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import type { PublicMapShareAttachmentMetadata, PublicMapShareResponse } from '../types';
import { toBase64 } from '../crypto/utils';
import { getPlanErrorPrompt, type PlanErrorPrompt } from '../utils/planErrors';

type UnlockedShareState = {
  payload: Awaited<ReturnType<typeof decryptShareBundle>>;
  shareKey: Uint8Array;
};

function countNodes(node: { children: Array<{ children: unknown[] }> }): number {
  return 1 + node.children.reduce((sum, child) => sum + countNodes(child as { children: Array<{ children: unknown[] }> }), 0);
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${Math.round(bytes / (1024 * 1024))} MB`;
}

function saveBytesToFile(bytes: Uint8Array, fileName: string, contentType: string) {
  const payload = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
  const blob = new Blob([payload], { type: contentType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = fileName;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
}

function OutlinePreview({ node, depth = 0 }: { node: { id: string; text: string; children: Array<{ id: string; text: string; children: unknown[] }> }; depth?: number }) {
  if (depth > 2) return null;
  return (
    <li className="mt-2">
      <div className="rounded-xl border border-slate-800 bg-slate-950/60 px-3 py-2 text-sm text-slate-200">
        {node.text || 'Untitled node'}
      </div>
      {node.children.length > 0 && (
        <ul className="ml-4 border-l border-slate-800 pl-3">
          {node.children.slice(0, 5).map((child) => (
            <OutlinePreview key={child.id} node={child as { id: string; text: string; children: Array<{ id: string; text: string; children: unknown[] }> }} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
  );
}

export function SharedVaultPage() {
  const { shareId } = useParams<{ shareId: string }>();
  const navigate = useNavigate();
  const { accessToken, sessionKeys, username } = useAuthStore();
  const mode = useModeStore((state) => state.mode);
  const isLocalMode = mode === 'local';
  const storage = useMemo(() => getStorage(isLocalMode ? 'local' : 'server'), [isLocalMode]);

  const [share, setShare] = useState<PublicMapShareResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [unlockBusy, setUnlockBusy] = useState(false);
  const [unlocked, setUnlocked] = useState<UnlockedShareState | null>(null);
  const [downloadingAttachmentId, setDownloadingAttachmentId] = useState<string | null>(null);
  const [importBusy, setImportBusy] = useState(false);
  const [importMsg, setImportMsg] = useState('');
  const [showUnlockModal, setShowUnlockModal] = useState(false);
  const [planPrompt, setPlanPrompt] = useState<PlanErrorPrompt | null>(null);
  const [showSubscription, setShowSubscription] = useState(false);

  useEffect(() => {
    if (!shareId) {
      setLoading(false);
      setError('Missing share id');
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError('');
    encryptedVaultApi.getPublicShare(shareId)
      .then((data) => {
        if (!cancelled) setShare(data);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : 'Failed to load shared vault');
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [shareId]);

  const handleUnlock = useCallback(async () => {
    if (!share || !passphrase.trim()) {
      setError('Enter the share passphrase to decrypt this vault.');
      return;
    }
    setUnlockBusy(true);
    setError('');
    try {
      const ciphertext = await encryptedVaultApi.downloadUrl(share.download_url);
      const unlockedBundle = await unlockEncryptedShareBundle(ciphertext, passphrase, share.encryption_meta);
      setUnlocked(unlockedBundle);
    } catch (err) {
      setUnlocked(null);
      setError(err instanceof Error ? err.message : 'Failed to decrypt shared vault');
    } finally {
      setUnlockBusy(false);
    }
  }, [passphrase, share]);

  const handleDownloadJson = useCallback(() => {
    if (!unlocked) return;
    const blob = new Blob([
      JSON.stringify(
        {
          title: unlocked.payload.title,
          tree: unlocked.payload.tree,
          exported_at: unlocked.payload.exported_at,
          source_vault_id: unlocked.payload.source_vault_id,
        },
        null,
        2,
      ),
    ], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${unlocked.payload.title || share?.name || 'shared-vault'}.json`;
    document.body.appendChild(anchor);
    anchor.click();
    document.body.removeChild(anchor);
    URL.revokeObjectURL(url);
  }, [share?.name, unlocked]);

  const handleDownloadAttachment = useCallback(async (attachment: PublicMapShareAttachmentMetadata) => {
    if (!shareId || !unlocked) return;
    setDownloadingAttachmentId(attachment.id);
    setError('');
    try {
      const download = await encryptedVaultApi.getPublicShareAttachmentDownload(shareId, attachment.id);
      const ciphertext = await encryptedVaultApi.downloadUrl(download.download_url);
      const plaintext = await decryptBytesForShare(ciphertext, unlocked.shareKey);
      saveBytesToFile(plaintext, download.name, download.content_type || 'application/octet-stream');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to download shared attachment');
    } finally {
      setDownloadingAttachmentId(null);
    }
  }, [shareId, unlocked]);

  const handleImport = useCallback(async () => {
    if (!shareId || !share || !unlocked || !sessionKeys) return;
    setImportBusy(true);
    setImportMsg('');
    setPlanPrompt(null);
    setError('');
    try {
      const titleEnc = await encryptTitle(unlocked.payload.title, sessionKeys.masterKey);
      const { ephClassicalPublic, ephPqCiphertext, wrappedDek, dek } = await hybridEncap(
        sessionKeys.classicalPubKey,
        sessionKeys.pqPubKey,
      );
      const encBlob = await encryptTree(unlocked.payload.tree, dek);
      const created = await storage.createVault({
        title_encrypted: titleEnc,
        eph_classical_public: toBase64(ephClassicalPublic),
        eph_pq_ciphertext: toBase64(ephPqCiphertext),
        wrapped_dek: toBase64(wrappedDek),
      });
      await storage.uploadBlob(created.id, encBlob);

      let importedAttachments = 0;
      if (!isLocalMode && share.attachments.length > 0) {
        for (const attachment of share.attachments) {
          const download = await encryptedVaultApi.getPublicShareAttachmentDownload(shareId, attachment.id);
          const ciphertext = await encryptedVaultApi.downloadUrl(download.download_url);
          const plaintext = await decryptBytesForShare(ciphertext, unlocked.shareKey);
          const encrypted = await encryptAttachmentForOwner(plaintext, sessionKeys.masterKey);
          const init = await encryptedVaultApi.initAttachment(created.id, {
            name: attachment.name,
            content_type: download.content_type || attachment.content_type || 'application/octet-stream',
            size: encrypted.ciphertext.byteLength,
            node_id: attachment.node_id,
            encrypted: true,
            encryption_meta: encrypted.encryptionMeta,
          });
          const versionId = await encryptedVaultApi.uploadPresigned(init.upload_url, encrypted.ciphertext, {
            ...init.upload_headers,
            'Content-Type': download.content_type || attachment.content_type || 'application/octet-stream',
          });
          await encryptedVaultApi.completeAttachment(created.id, init.attachment_id, versionId ?? '', encrypted.checksumSha256);
          importedAttachments += 1;
        }
      }

      setImportMsg(importedAttachments > 0 ? `Imported vault with ${importedAttachments} attachment${importedAttachments === 1 ? '' : 's'}.` : 'Imported vault.');
      navigate(`/vaults/${created.id}`);
    } catch (err) {
      setPlanPrompt(getPlanErrorPrompt(err));
      setError(err instanceof Error ? err.message : 'Failed to import shared vault');
    } finally {
      setImportBusy(false);
    }
  }, [isLocalMode, navigate, sessionKeys, share, shareId, storage, unlocked]);

  const postLoginPath = `/shared/${shareId ?? ''}`;
  const canImport = Boolean(sessionKeys && unlocked);
  const nodeCount = unlocked ? countNodes(unlocked.payload.tree.root as { children: Array<{ children: unknown[] }> }) : 0;

  return (
    <div className="min-h-screen bg-[radial-gradient(circle_at_top_left,_rgba(124,58,237,0.18),_transparent_32%),linear-gradient(180deg,_#020617,_#0f172a)] px-4 py-8 text-white">
      <div className="mx-auto max-w-6xl">
        <div className="mb-8 flex flex-wrap items-center justify-between gap-3">
          <div>
            <p className="text-xs uppercase tracking-[0.24em] text-slate-400">Shared vault</p>
            <h1 className="mt-2 text-3xl font-semibold text-white">Open encrypted vault export</h1>
            <p className="mt-3 max-w-3xl text-sm leading-6 text-slate-300">
              This link contains an encrypted export snapshot. Decryption happens in your browser with the share passphrase. Live encrypted multi-user editing and active editor presence are not part of this share flow.
            </p>
          </div>
          <div className="rounded-2xl border border-slate-800 bg-slate-950/60 px-4 py-3 text-sm text-slate-300">
            Created {share ? new Date(share.created_at).toLocaleString() : '...'}
          </div>
        </div>

        {loading && (
          <div className="rounded-3xl border border-slate-800 bg-slate-950/60 px-6 py-8 text-slate-300">
            Loading encrypted share metadata…
          </div>
        )}

        {!loading && error && !share && (
          <div className="rounded-3xl border border-red-900/50 bg-red-950/30 px-6 py-8 text-red-200">
            {error}
          </div>
        )}

        {share && (
          <div className="grid gap-6 lg:grid-cols-[0.95fr_1.35fr]">
            <div className="rounded-3xl border border-slate-800 bg-slate-950/60 p-6 shadow-2xl shadow-slate-950/20">
              <p className="text-xs uppercase tracking-[0.22em] text-slate-500">Share details</p>
              <h2 className="mt-3 text-2xl font-semibold text-white">{share.name}</h2>
              <div className="mt-5 grid gap-3 sm:grid-cols-2">
                <div className="rounded-2xl border border-slate-800 bg-slate-900/70 p-4">
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-500">Scope</p>
                  <p className="mt-2 text-lg font-semibold text-white">{share.scope}</p>
                </div>
                <div className="rounded-2xl border border-slate-800 bg-slate-900/70 p-4">
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-500">Attachments</p>
                  <p className="mt-2 text-lg font-semibold text-white">{share.attachments.length}</p>
                </div>
                <div className="rounded-2xl border border-slate-800 bg-slate-900/70 p-4">
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-500">Encrypted size</p>
                  <p className="mt-2 text-lg font-semibold text-white">{formatBytes(share.size_bytes)}</p>
                </div>
                <div className="rounded-2xl border border-slate-800 bg-slate-900/70 p-4">
                  <p className="text-xs uppercase tracking-[0.18em] text-slate-500">Expires</p>
                  <p className="mt-2 text-lg font-semibold text-white">{share.expires_at ? new Date(share.expires_at).toLocaleString() : 'No expiry'}</p>
                </div>
              </div>

              <div className="mt-6 rounded-2xl border border-slate-800 bg-slate-900/55 p-4">
                <label className="text-xs uppercase tracking-[0.18em] text-slate-500">Share passphrase</label>
                <input
                  type="password"
                  value={passphrase}
                  onChange={(event) => setPassphrase(event.target.value)}
                  placeholder={share.passphrase_hint ? `Hint: ${share.passphrase_hint}` : 'Enter the share passphrase'}
                  className="mt-3 w-full rounded-2xl border border-slate-700 bg-slate-950/80 px-4 py-3 text-sm text-white outline-none focus:border-[var(--accent)]"
                />
                <button
                  type="button"
                  onClick={() => void handleUnlock()}
                  disabled={unlockBusy || !passphrase.trim()}
                  className="mt-4 w-full rounded-2xl bg-accent px-4 py-3 text-sm font-semibold text-white transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {unlockBusy ? 'Decrypting share…' : 'Decrypt shared vault'}
                </button>
              </div>

              {error && share && (
                <div className="mt-4 rounded-2xl border border-red-900/50 bg-red-950/30 px-4 py-3 text-sm text-red-200">
                  {error}
                </div>
              )}

              {planPrompt && (
                <div className="mt-4 rounded-2xl border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-100">
                  <p className="font-semibold text-amber-50">{planPrompt.title}</p>
                  <p className="mt-1 text-amber-100/90">{planPrompt.message}</p>
                  {planPrompt.shouldOpenSubscription && (
                    <button
                      type="button"
                      onClick={() => setShowSubscription(true)}
                      className="mt-3 rounded-lg bg-accent px-3 py-2 text-sm font-medium text-white transition hover:bg-accent-hover"
                    >
                      {planPrompt.ctaLabel}
                    </button>
                  )}
                </div>
              )}

              {unlocked && (
                <div className="mt-6 space-y-3">
                  <button type="button" onClick={handleDownloadJson} className="w-full rounded-2xl border border-slate-700 px-4 py-3 text-sm text-slate-200 transition hover:border-slate-500">
                    Download decrypted JSON
                  </button>

                  {!accessToken && (
                    <div className="rounded-2xl border border-slate-800 bg-slate-900/55 px-4 py-4 text-sm text-slate-300">
                      <p>Sign in to import this shared vault into your encrypted cloud workspace.</p>
                      <div className="mt-3 flex gap-2">
                        <Link to={`/login?next=${encodeURIComponent(postLoginPath)}`} className="rounded-xl bg-accent px-3 py-2 text-sm font-medium text-white transition hover:bg-accent-hover">Log in</Link>
                        <Link to={`/register?next=${encodeURIComponent(postLoginPath)}`} className="rounded-xl border border-slate-700 px-3 py-2 text-sm text-slate-200 transition hover:border-slate-500">Create account</Link>
                      </div>
                    </div>
                  )}

                  {accessToken && !sessionKeys && username && (
                    <div className="rounded-2xl border border-slate-800 bg-slate-900/55 px-4 py-4 text-sm text-slate-300">
                      <p>Unlock your session keys before importing this shared vault.</p>
                      <button type="button" onClick={() => setShowUnlockModal(true)} className="mt-3 rounded-xl bg-accent px-3 py-2 text-sm font-medium text-white transition hover:bg-accent-hover">
                        Unlock session keys
                      </button>
                    </div>
                  )}

                  <button
                    type="button"
                    onClick={() => void handleImport()}
                    disabled={!canImport || importBusy}
                    className="w-full rounded-2xl bg-white px-4 py-3 text-sm font-semibold text-slate-950 transition hover:bg-slate-200 disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    {importBusy ? 'Importing into your vaults…' : isLocalMode ? 'Import as local vault' : 'Import into my encrypted vaults'}
                  </button>
                  {importMsg && <p className="text-center text-sm text-emerald-300">{importMsg}</p>}
                </div>
              )}
            </div>

            <div className="space-y-6">
              <div className="rounded-3xl border border-slate-800 bg-slate-950/60 p-6 shadow-2xl shadow-slate-950/20">
                <p className="text-xs uppercase tracking-[0.22em] text-slate-500">Decrypted preview</p>
                {!unlocked && <p className="mt-4 text-sm leading-6 text-slate-300">Decrypt the share to preview the vault title, outline, and any included attachments.</p>}
                {unlocked && (
                  <>
                    <h2 className="mt-3 text-2xl font-semibold text-white">{unlocked.payload.title}</h2>
                    <p className="mt-2 text-sm text-slate-400">
                      Exported {new Date(unlocked.payload.exported_at).toLocaleString()} · {nodeCount} node{nodeCount === 1 ? '' : 's'}
                    </p>
                    <div className="mt-5 rounded-2xl border border-slate-800 bg-slate-900/55 p-4">
                      <p className="text-xs uppercase tracking-[0.18em] text-slate-500">Outline preview</p>
                      <ul className="mt-3">
                        <OutlinePreview node={unlocked.payload.tree.root as { id: string; text: string; children: Array<{ id: string; text: string; children: unknown[] }> }} />
                      </ul>
                    </div>
                  </>
                )}
              </div>

              <div className="rounded-3xl border border-slate-800 bg-slate-950/60 p-6 shadow-2xl shadow-slate-950/20">
                <p className="text-xs uppercase tracking-[0.22em] text-slate-500">Shared attachments</p>
                {share.attachments.length === 0 && <p className="mt-4 text-sm text-slate-300">This share does not include attachments.</p>}
                {share.attachments.length > 0 && !unlocked && <p className="mt-4 text-sm text-slate-300">Decrypt the share first to download any included attachments.</p>}
                {share.attachments.length > 0 && unlocked && (
                  <div className="mt-4 space-y-3">
                    {share.attachments.map((attachment) => (
                      <div key={attachment.id} className="rounded-2xl border border-slate-800 bg-slate-900/55 px-4 py-4">
                        <div className="flex flex-wrap items-center justify-between gap-3">
                          <div>
                            <p className="text-sm font-semibold text-white">{attachment.name}</p>
                            <p className="mt-1 text-xs text-slate-400">{formatBytes(attachment.size_bytes)} · {attachment.content_type}</p>
                          </div>
                          <button
                            type="button"
                            onClick={() => void handleDownloadAttachment(attachment)}
                            disabled={downloadingAttachmentId === attachment.id}
                            className="rounded-full border border-slate-700 px-3 py-1.5 text-xs text-slate-200 transition hover:border-slate-500 disabled:cursor-not-allowed disabled:opacity-60"
                          >
                            {downloadingAttachmentId === attachment.id ? 'Decrypting…' : 'Download decrypted'}
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {showUnlockModal && accessToken && !sessionKeys && (
        <UnlockModal onUnlocked={() => setShowUnlockModal(false)} />
      )}
      <SubscriptionDialog open={showSubscription} onClose={() => setShowSubscription(false)} />
    </div>
  );
}

export default SharedVaultPage;