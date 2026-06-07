import type { SyncStatus } from '../storage';

interface OfflineBannerProps {
  isOnline: boolean;
  status: SyncStatus;
  onRetrySync: () => void;
  onKeepLocal: () => void;
  onKeepServer: () => void;
}

export function OfflineBanner({ isOnline, status, onRetrySync, onKeepLocal, onKeepServer }: OfflineBannerProps) {
  const { state, pendingCount, lastSyncedAt, conflictVaultId } = status;

  const offline = !isOnline;
  const syncing = isOnline && state === 'syncing';
  const conflict = isOnline && state === 'conflict' && conflictVaultId != null;
  const error = isOnline && state === 'error';
  const hasPending = isOnline && state === 'idle' && pendingCount > 0;

  if (!offline && !syncing && !conflict && !error && !hasPending) return null;

  let colorClass = 'mm-offline-banner--info';
  if (offline) colorClass = 'mm-offline-banner--offline';
  if (conflict) colorClass = 'mm-offline-banner--conflict';
  if (error) colorClass = 'mm-offline-banner--error';

  return (
    <div className={`mm-offline-banner ${colorClass}`} role="status" aria-live="polite">
      {offline && (
        <>
          <span className="mm-offline-dot mm-offline-dot--red" />
          <span className="mm-offline-text">
            Offline
            {pendingCount > 0 && ` — ${pendingCount} unsaved change${pendingCount === 1 ? '' : 's'}`}
          </span>
          {pendingCount > 0 && (
            <span className="mm-offline-sub">Changes are saved locally and will sync when you reconnect.</span>
          )}
        </>
      )}

      {syncing && (
        <>
          <span className="mm-offline-spinner" />
          <span className="mm-offline-text">
            Syncing{pendingCount > 0 ? ` ${pendingCount} change${pendingCount === 1 ? '' : 's'}` : ''}…
          </span>
        </>
      )}

      {hasPending && (
        <>
          <span className="mm-offline-dot mm-offline-dot--amber" />
          <span className="mm-offline-text">{pendingCount} change{pendingCount === 1 ? '' : 's'} uploading…</span>
        </>
      )}

      {error && (
        <>
          <span className="mm-offline-dot mm-offline-dot--red" />
          <span className="mm-offline-text">Sync failed — some changes could not be uploaded.</span>
          <button className="mm-offline-action" onClick={onRetrySync}>Retry</button>
        </>
      )}

      {conflict && (
        <>
          <svg className="mm-offline-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M12 9v4m0 4h.01M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/>
          </svg>
          <span className="mm-offline-text">Server has newer changes. Which version do you want to keep?</span>
          <button className="mm-offline-action mm-offline-action--primary" onClick={onKeepLocal}>My offline edits</button>
          <button className="mm-offline-action" onClick={onKeepServer}>Server version</button>
        </>
      )}

      {isOnline && lastSyncedAt != null && state === 'idle' && pendingCount === 0 && (
        <span className="mm-offline-synced">
          ✓ Synced {new Date(lastSyncedAt).toLocaleTimeString()}
        </span>
      )}
    </div>
  );
}
