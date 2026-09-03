import { useEffect, useState } from 'react';
import { authApi } from '../api/auth';
import { buildPasswordRotationBundle } from '../crypto/keyRotation';
import type { LocalProfileForRotation } from '../crypto/keyRotation';
import { useAuthStore } from '../store/auth';
import { PasswordInput } from './PasswordInput';

const inputStyle = {
  background: 'var(--surface-2)',
  color: 'var(--text-primary)',
  border: '1px solid var(--border-light)',
} as const;

/**
 * Changes a server account's password, entirely in the browser: one manifest
 * fetch, all re-encryption locally, one atomic POST. The failure handling is
 * the part worth reading — a lost response is disambiguated by comparing the
 * live salt against the old and new ones, so the user is told which password
 * is actually in force instead of being left to guess.
 * Design: docs/PASSWORD_ROTATION.md.
 */
export function PasswordRotationForm() {
  const { username, sessionKeys, setSessionKeys, setTokens, logout } = useAuthStore();

  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [working, setWorking] = useState(false);
  const [progress, setProgress] = useState('');
  const [error, setError] = useState('');
  const [done, setDone] = useState(false);
  // Set when the server committed the change but this session could not be
  // refreshed — the one state where the only good exit is a fresh sign-in.
  const [committedButDetached, setCommittedButDetached] = useState(false);

  // Leaving the page mid-rotation is safe for the data (the server either
  // committed or it didn't) but leaves the user not knowing which — warn.
  useEffect(() => {
    if (!working) return;
    const handler = (e: BeforeUnloadEvent) => {
      e.preventDefault();
    };
    window.addEventListener('beforeunload', handler);
    return () => window.removeEventListener('beforeunload', handler);
  }, [working]);

  const handleRotate = async () => {
    setError('');

    if (!currentPassword) { setError('Current password is required.'); return; }
    if (newPassword.length < 12) { setError('New password must be at least 12 characters.'); return; }
    if (newPassword === currentPassword) { setError('New password must differ from the current password.'); return; }
    if (newPassword !== confirmPassword) { setError('New passwords do not match.'); return; }

    setWorking(true);
    let oldSalt = '';
    let newSalt = '';
    try {
      setProgress('Fetching what needs re-encrypting…');
      const manifest = await authApi.getRotationManifest();
      oldSalt = manifest.argon2_salt;

      const profile: LocalProfileForRotation = {
        username: username ?? '',
        argon2_salt: manifest.argon2_salt,
        argon2_params: manifest.argon2_params,
        classical_public_key: '',
        pq_public_key: '',
        classical_priv_encrypted: manifest.classical_priv_encrypted,
        pq_priv_encrypted: manifest.pq_priv_encrypted,
        key_version: manifest.key_version,
        created_at: '',
      };

      setProgress(
        `Verifying the current password and re-encrypting ${manifest.vaults.length} ` +
        `title${manifest.vaults.length === 1 ? '' : 's'} and ${manifest.attachments.length} ` +
        `attachment key${manifest.attachments.length === 1 ? '' : 's'}…`,
      );
      const bundle = await buildPasswordRotationBundle(
        currentPassword,
        newPassword,
        profile,
        manifest.vaults.map((v) => ({
          id: v.id,
          title_encrypted: v.title_encrypted,
          vault_note_encrypted: v.vault_note_encrypted,
        })),
        manifest.attachments,
      );
      newSalt = bundle.newProfile.argon2_salt;

      setProgress('Applying the change on the server…');
      const resp = await authApi.rotateCredentials({
        current_auth_token: bundle.currentAuthToken,
        new_auth_token: bundle.newAuthToken,
        new_argon2_salt: bundle.newProfile.argon2_salt,
        new_argon2_params: bundle.newProfile.argon2_params,
        new_classical_priv_encrypted: bundle.newProfile.classical_priv_encrypted,
        new_pq_priv_encrypted: bundle.newProfile.pq_priv_encrypted,
        new_key_version: bundle.newProfile.key_version,
        updated_vaults: bundle.updatedVaults,
        updated_attachments: bundle.updatedAttachments,
      });

      if (sessionKeys) {
        setSessionKeys({ ...sessionKeys, masterKey: bundle.newMasterKey });
      }
      if (username) {
        setTokens(resp.access_token, resp.refresh_token, username);
      }
      setDone(true);
      setCurrentPassword('');
      setNewPassword('');
      setConfirmPassword('');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);

      // The commit is a single POST; anything that fails before it means
      // nothing changed. But a lost RESPONSE leaves the outcome unknown, and
      // a retry against a committed rotation fails looking like a wrong
      // password. The live salt settles it: rotation always writes a fresh
      // salt, and the salt endpoint needs no authentication.
      let verdict: 'unchanged' | 'committed' | 'unknown' = 'unknown';
      if (username && oldSalt) {
        try {
          const live = await authApi.getSalt(username);
          if (live.argon2_salt === oldSalt) verdict = 'unchanged';
          else if (newSalt && live.argon2_salt === newSalt) verdict = 'committed';
        } catch {
          // The probe failing (offline, throttled) leaves the honest answer: unknown.
        }
      }

      if (verdict === 'committed') {
        setCommittedButDetached(true);
        setError('');
      } else if (verdict === 'unchanged') {
        if (
          msg.includes('Current password is incorrect') ||
          msg.includes('AES-GCM') ||
          msg.includes('OperationError')
        ) {
          setError('Current password is incorrect — please try again.');
        } else {
          setError(`Password change failed and nothing was changed: ${msg}`);
        }
      } else {
        setError(
          'The connection was lost and it could not be confirmed whether the change was applied. ' +
          'Sign out and try the NEW password first; if it is refused, the old one still applies.',
        );
      }
    } finally {
      setWorking(false);
      setProgress('');
    }
  };

  if (committedButDetached) {
    return (
      <div className="rounded-xl p-4" style={{ background: 'var(--surface-2)' }}>
        <p className="text-sm font-medium" style={{ color: 'var(--text-primary)' }}>
          Your password was changed.
        </p>
        <p className="mt-2 text-sm" style={{ color: 'var(--text-secondary)' }}>
          The change reached the server, but this session could not pick it up. Sign in
          again with the new password.
        </p>
        <button
          type="button"
          onClick={logout}
          className="mt-3 w-full rounded-lg px-3 py-2 text-sm font-semibold text-white transition"
          style={{ background: 'var(--accent)' }}
        >
          Sign out now
        </button>
      </div>
    );
  }

  return (
    <div>
      <p className="text-sm leading-6" style={{ color: 'var(--text-secondary)' }}>
        Your password never reaches the server — it is what decrypts your vaults in this
        browser. That also means nobody, including whoever runs this server, can reset it
        for you, and changing it needs the current one.
      </p>
      <p className="mt-2 text-xs leading-5" style={{ color: 'var(--text-muted)' }}>
        Every other signed-in device will be signed out and will need the new password.
        Vaults, versions and attachments are untouched — only the keys around them change.
      </p>

      {done && (
        <p
          className="mt-3 rounded-lg px-3 py-2 text-sm"
          style={{ background: 'rgba(16, 185, 129, 0.12)', color: '#34d399' }}
          data-testid="rotation-done"
        >
          Password changed. This session carries on; other devices were signed out.
        </p>
      )}

      <div className="mt-3 space-y-3">
        <PasswordInput
          value={currentPassword}
          onChange={(e) => setCurrentPassword(e.target.value)}
          placeholder="Current password"
          autoComplete="current-password"
          disabled={working}
          className="w-full rounded-lg px-3 py-2 text-sm"
          style={inputStyle}
        />
        <PasswordInput
          value={newPassword}
          onChange={(e) => setNewPassword(e.target.value)}
          placeholder="New password (min. 12 characters)"
          autoComplete="new-password"
          disabled={working}
          className="w-full rounded-lg px-3 py-2 text-sm"
          style={inputStyle}
        />
        <PasswordInput
          value={confirmPassword}
          onChange={(e) => setConfirmPassword(e.target.value)}
          placeholder="Confirm new password"
          autoComplete="new-password"
          disabled={working}
          className="w-full rounded-lg px-3 py-2 text-sm"
          style={inputStyle}
        />
      </div>

      {progress && (
        <p className="mt-3 text-xs" style={{ color: 'var(--text-muted)' }} data-testid="rotation-progress">
          {progress}
        </p>
      )}
      {error && (
        <p
          className="mt-3 rounded-lg px-3 py-2 text-xs"
          style={{ background: 'rgba(239, 68, 68, 0.12)', color: '#fca5a5' }}
          data-testid="rotation-error"
        >
          {error}
        </p>
      )}

      <button
        type="button"
        onClick={() => { void handleRotate(); }}
        disabled={working || !currentPassword || !newPassword || !confirmPassword}
        data-testid="rotation-submit"
        className="mt-3 w-full rounded-lg px-3 py-2 text-sm font-semibold text-white transition disabled:cursor-not-allowed disabled:opacity-50"
        style={{ background: 'var(--accent)' }}
      >
        {working ? 'Changing password…' : 'Change password'}
      </button>
    </div>
  );
}

export default PasswordRotationForm;
