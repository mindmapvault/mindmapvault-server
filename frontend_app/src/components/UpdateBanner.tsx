import { useEffect, useState } from 'react';
import { applyUpdate, isUpdateReady, onUpdateReady } from '../pwa/serviceWorker';

/**
 * Offers the reload when a newer build is installed and waiting.
 *
 * Deliberately a prompt rather than an automatic reload: a reload throws away
 * whatever is unsaved in the editor, so the moment is the user's to pick.
 */
export function UpdateBanner() {
  const [ready, setReady] = useState(isUpdateReady);
  const [applying, setApplying] = useState(false);

  useEffect(() => onUpdateReady(setReady), []);

  if (!ready) return null;

  return (
    <div className="mm-update-banner" role="status" aria-live="polite" data-testid="update-banner">
      <span className="mm-update-banner__text">A new version of MindMapVault is ready.</span>
      <button
        type="button"
        className="mm-update-banner__action"
        disabled={applying}
        onClick={() => { setApplying(true); applyUpdate(); }}
        data-testid="update-banner-reload"
      >
        {applying ? 'Reloading…' : 'Reload'}
      </button>
      <button
        type="button"
        className="mm-update-banner__dismiss"
        onClick={() => setReady(false)}
        aria-label="Dismiss"
        title="Dismiss — the update applies on your next reload"
      >
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round">
          <line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>
  );
}

export default UpdateBanner;
