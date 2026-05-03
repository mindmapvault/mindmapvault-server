import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { authApi } from '../api/auth';
import { buildPasswordRotationBundle } from '../crypto/keyRotation';
import type { LocalProfileForRotation, VaultEntryForRotation } from '../crypto/keyRotation';
import { getServerStorage } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import('@tauri-apps/api/core');
  return tauriInvoke<T>(cmd, args);
}

export function ChangePasswordPage() {
  const navigate = useNavigate();
  const { sessionKeys, setSessionKeys, setTokens, username } = useAuthStore();
  const mode = useModeStore((s) => s.mode);
  const isLocalMode = mode === 'local';

  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [working, setWorking] = useState(false);
  const [progress, setProgress] = useState('');
  const [error, setError] = useState('');
  const [done, setDone] = useState(false);

  // Redirect non-local sessions
  useEffect(() => {
    if (!sessionKeys) {
      navigate('/vaults', { replace: true });
    }
  }, [sessionKeys, navigate]);

  const handleRotate = async () => {
    setError('');

    if (!currentPassword) { setError('Current password is required'); return; }
    if (newPassword.length < 8) { setError('New password must be at least 8 characters'); return; }
    if (newPassword === currentPassword) { setError('New password must differ from the current password'); return; }
    if (newPassword !== confirmPassword) { setError('New passwords do not match'); return; }

    setWorking(true);
    try {
      if (isLocalMode) {
        // ── Local (Tauri) path ─────────────────────────────────────────────
        setProgress('Loading profile…');
        const profile = await invoke<LocalProfileForRotation | null>('get_local_profile');
        if (!profile) throw new Error('No active profile found — are you logged in?');

        setProgress('Loading vault index…');
        const vaults = await invoke<VaultEntryForRotation[]>('list_local_vaults');

        setProgress('Verifying current password and deriving new keys… (this takes a few seconds)');
        const bundle = await buildPasswordRotationBundle(
          currentPassword,
          newPassword,
          profile,
          vaults,
        );

        setProgress(`Re-encrypting ${vaults.length} vault title${vaults.length === 1 ? '' : 's'}…`);
        await invoke('apply_local_password_rotation', {
          newProfile: bundle.newProfile,
          updatedVaults: bundle.updatedVaults,
        });

        if (sessionKeys) {
          setSessionKeys({ ...sessionKeys, masterKey: bundle.newMasterKey });
        }
      } else {
        // ── Server path ───────────────────────────────────────────────────
        // Fetch the key bundle (includes argon2_salt/params for re-deriving
        // the master key) and the full vault list so we can build the
        // rotation bundle entirely client-side.  The server never sees
        // plaintext keys or passwords.
        setProgress('Fetching key bundle from server…');
        const keyBundle = await authApi.getKeyBundle();

        setProgress('Loading vault list…');
        const serverStorage = getServerStorage();
        const vaultItems = await serverStorage.listVaults();

        const profile: LocalProfileForRotation = {
          username: username ?? '',
          argon2_salt: keyBundle.argon2_salt,
          argon2_params: keyBundle.argon2_params,
          classical_public_key: keyBundle.classical_public_key,
          pq_public_key: keyBundle.pq_public_key,
          classical_priv_encrypted: keyBundle.classical_priv_encrypted,
          pq_priv_encrypted: keyBundle.pq_priv_encrypted,
          key_version: keyBundle.key_version,
          created_at: '',
        };

        const vaults: VaultEntryForRotation[] = vaultItems.map((v) => ({
          id: v.id,
          title_encrypted: v.title_encrypted,
          vault_note_encrypted: v.vault_note_encrypted ?? null,
        }));

        setProgress(
          `Verifying current password and re-encrypting ${vaults.length} vault title${vaults.length === 1 ? '' : 's'}… (this takes a few seconds)`,
        );
        const bundle = await buildPasswordRotationBundle(
          currentPassword,
          newPassword,
          profile,
          vaults,
        );

        // Submit the rotation bundle.  The server:
        //   1. Re-verifies current_auth_token against the stored hash.
        //   2. Checks that every vault in the account is covered by updated_vaults.
        //   3. Commits credential row + all vault title/note updates in a single
        //      DB transaction — all-or-nothing, no partial rotation possible.
        //   4. Returns fresh JWT tokens.
        // Vault blobs in object storage are NOT touched: they are KEM-encrypted
        // to the user's keypair which is unchanged, so all historical blob
        // versions remain decryptable after the rotation.
        setProgress('Committing rotation on server…');
        const rotateResp = await authApi.rotateCredentials({
          current_auth_token: bundle.currentAuthToken,
          new_auth_token: bundle.newAuthToken,
          new_argon2_salt: bundle.newProfile.argon2_salt,
          new_argon2_params: bundle.newProfile.argon2_params,
          new_classical_priv_encrypted: bundle.newProfile.classical_priv_encrypted,
          new_pq_priv_encrypted: bundle.newProfile.pq_priv_encrypted,
          new_key_version: bundle.newProfile.key_version,
          updated_vaults: bundle.updatedVaults.map((v) => ({
            id: v.id,
            title_encrypted: v.title_encrypted,
            vault_note_encrypted: v.vault_note_encrypted,
          })),
        });

        // Update session: new master key + fresh tokens.
        if (sessionKeys) {
          setSessionKeys({ ...sessionKeys, masterKey: bundle.newMasterKey });
        }
        if (username) {
          setTokens(rotateResp.access_token, rotateResp.refresh_token, username);
        }
      }

      setProgress('');
      setDone(true);
      setCurrentPassword('');
      setNewPassword('');
      setConfirmPassword('');
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      if (
        msg.includes('Current password is incorrect') ||
        msg.includes('decrypt') ||
        msg.includes('AES-GCM') ||
        msg.includes('OperationError')
      ) {
        setError('Current password is incorrect — please try again.');
      } else {
        setError(`Password change failed: ${msg}`);
      }
    } finally {
      setWorking(false);
      setProgress('');
    }
  };

  return (
    <div className="min-h-screen bg-[var(--bg)] flex flex-col items-center justify-center px-4 py-12">
      <div className="w-full max-w-md">
        <div className="mb-6 flex items-center gap-3">
          <button
            onClick={() => navigate('/vaults')}
            className="rounded-lg border border-[var(--border)] px-3 py-1.5 text-sm text-[var(--fg-muted)] transition hover:border-[var(--accent)] hover:text-[var(--fg)]"
          >
            ← Back
          </button>
          <h1 className="text-xl font-semibold text-[var(--fg)]">Change Password</h1>
        </div>

        <div className="rounded-xl border border-[var(--border)] bg-[var(--surface-1)] p-6 space-y-4">
          {done ? (
            <div className="space-y-4">
              <div className="flex items-center gap-3 rounded-lg border border-green-700/50 bg-green-900/20 px-4 py-3 text-sm text-green-300">
                <svg className="h-5 w-5 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                </svg>
                Password changed successfully. All vault titles and keys have been re-encrypted.
              </div>
              <button
                onClick={() => navigate('/vaults')}
                className="w-full rounded-lg bg-[var(--accent)] px-4 py-2.5 text-sm font-medium text-white transition hover:bg-[var(--accent-hover)]"
              >
                Return to vaults
              </button>
            </div>
          ) : (
            <>
              <p className="text-sm text-[var(--fg-muted)]">
                Your private keys and all vault titles will be re-encrypted with the new password.
                Vault data (the mind-map content) is not touched — it is protected by your key-pair,
                which does not change.
              </p>

              <div className="space-y-3">
                <label className="block text-sm text-[var(--fg-muted)]">
                  Current password
                  <input
                    type="password"
                    value={currentPassword}
                    onChange={(e) => setCurrentPassword(e.target.value)}
                    autoComplete="current-password"
                    disabled={working}
                    className="mt-1 w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-[var(--fg)] placeholder-[var(--fg-muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
                    placeholder="Current password"
                  />
                </label>

                <label className="block text-sm text-[var(--fg-muted)]">
                  New password
                  <input
                    type="password"
                    value={newPassword}
                    onChange={(e) => setNewPassword(e.target.value)}
                    autoComplete="new-password"
                    disabled={working}
                    className="mt-1 w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-[var(--fg)] placeholder-[var(--fg-muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
                    placeholder="New password (min 8 characters)"
                  />
                </label>

                <label className="block text-sm text-[var(--fg-muted)]">
                  Confirm new password
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={(e) => setConfirmPassword(e.target.value)}
                    autoComplete="new-password"
                    disabled={working}
                    onKeyDown={(e) => e.key === 'Enter' && !working && void handleRotate()}
                    className="mt-1 w-full rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-2 text-[var(--fg)] placeholder-[var(--fg-muted)] focus:border-[var(--accent)] focus:outline-none disabled:opacity-60"
                    placeholder="Repeat new password"
                  />
                </label>
              </div>

              {error && (
                <div className="rounded-lg border border-red-800 bg-red-900/30 px-4 py-3 text-sm text-red-400">
                  {error}
                </div>
              )}

              {working && progress && (
                <div className="flex items-center gap-2 text-sm text-[var(--fg-muted)]">
                  <svg className="h-4 w-4 animate-spin" fill="none" viewBox="0 0 24 24">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                  {progress}
                </div>
              )}

              <div className="flex gap-3 pt-1">
                <button
                  onClick={() => navigate('/vaults')}
                  disabled={working}
                  className="flex-1 rounded-lg border border-[var(--border)] px-4 py-2.5 text-sm text-[var(--fg-muted)] transition hover:border-[var(--accent)] hover:text-[var(--fg)] disabled:opacity-50"
                >
                  Cancel
                </button>
                <button
                  onClick={() => void handleRotate()}
                  disabled={working || !currentPassword || !newPassword || !confirmPassword}
                  className="flex-1 rounded-lg bg-[var(--accent)] px-4 py-2.5 text-sm font-medium text-white transition hover:bg-[var(--accent-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {working ? 'Changing…' : 'Change password'}
                </button>
              </div>

              <p className="text-xs text-[var(--fg-muted)]">
                This operation cannot be undone. Make sure you remember the new password —
                there is no recovery option without it.
              </p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
