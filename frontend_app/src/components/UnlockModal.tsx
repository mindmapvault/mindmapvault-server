import { useCallback, useEffect, useState } from 'react';
import { authApi } from '../api/auth';
import { aesDecrypt } from '../crypto/aes';
import { deriveMasterAesKey, deriveMasterKey } from '../crypto/kdf';
import {
  accountIdFromToken,
  forgetDevice,
  loadDeviceRecord,
  trustThisDevice,
  unwrapWithDevice,
} from '../crypto/trustedDevice';
import { fromBase64 } from '../crypto/utils';
import { useAuthStore } from '../store/auth';
import { PasswordInput } from './PasswordInput';
import type { SessionKeys } from '../types';

interface Props {
  onUnlocked: () => void;
}

/**
 * Shown when a user has a valid JWT (page reload) but their session keys
 * are not in memory. They must re-enter their password to re-derive the
 * master key and decrypt their private key bundle.
 */
export function UnlockModal({ onUnlocked }: Props) {
  const { username, accessToken, setSessionKeys } = useAuthStore();
  const userId = accountIdFromToken(accessToken);
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [remember, setRemember] = useState(false);
  /** Held back until the silent attempt has had its turn, so a trusted device
   *  does not flash a passphrase prompt before letting the user in. */
  const [checkingDevice, setCheckingDevice] = useState(true);

  /** Turns a master key into session keys and finishes. Shared by both paths,
   *  because from here on they are the same. */
  const openWithMasterKey = useCallback(
    async (masterKey: Uint8Array, bundle: { classical_priv_encrypted: string; pq_priv_encrypted: string; classical_public_key: string; pq_public_key: string }) => {
      const masterAesKey = await deriveMasterAesKey(masterKey);
      const classicalPrivKey = await aesDecrypt(
        masterAesKey,
        fromBase64(bundle.classical_priv_encrypted),
      );
      const pqPrivKey = await aesDecrypt(masterAesKey, fromBase64(bundle.pq_priv_encrypted));
      setSessionKeys({
        masterKey,
        classicalPrivKey,
        classicalPubKey: fromBase64(bundle.classical_public_key),
        pqPrivKey,
        pqPubKey: fromBase64(bundle.pq_public_key),
      } satisfies SessionKeys);
      onUnlocked();
    },
    [onUnlocked, setSessionKeys],
  );

  // ── Trusted device ────────────────────────────────────────────────────────
  // If this browser holds a key for this account, the master key can be
  // decrypted without asking. Every failure path here falls through to the
  // passphrase prompt rather than surfacing an error: not being remembered is
  // the normal case, not a fault.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        if (!userId) return;
        const record = await loadDeviceRecord(userId);
        if (!record || cancelled) return;

        const { wrapped_master_key: wrapped } = await authApi.getWrappedMasterKey(record.methodId);
        const masterKey = await unwrapWithDevice(record, wrapped);
        if (!masterKey) {
          // The stored copy was made under a master key that no longer exists
          // — almost always a passphrase rotation. The local key is useless
          // now, so drop it rather than retrying it on every unlock.
          await forgetDevice(userId);
          return;
        }
        if (cancelled) return;
        await openWithMasterKey(masterKey, await authApi.getKeyBundle());
      } catch {
        // A revoked device answers 404 here. Ask for the passphrase.
      } finally {
        // Unconditionally, even when this run was superseded. React's
        // development double-mount cancels the first pass, and gating this on
        // the cancelled flag left the spinner up for ever with nothing behind
        // it.
        setCheckingDevice(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [userId, openWithMasterKey]);

  const handleUnlock = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (!password) {
      setError('Password is required');
      return;
    }
    setLoading(true);
    try {
      // Everything needed to re-derive and unwrap the keys comes from one
      // authenticated call. This used to be /auth/salt followed by /auth/login
      // — two calls against the allowance meant for strangers, made by someone
      // who is already signed in, and it threw away the tokens the login
      // returned. On a page reload that was enough to have a user rate-limited
      // out of their own vault.
      const bundle = await authApi.getKeyBundle();

      const masterKey = await deriveMasterKey(
        password,
        bundle.argon2_salt,
        bundle.argon2_params,
      );

      // A wrong password fails in the AES-GCM tag rather than at the server.
      // Nothing is lost by that: whoever reaches this screen already holds a
      // session token, and could fetch this same bundle and attack it offline
      // without touching the sign-in route at all.
      const masterAesKey = await deriveMasterAesKey(masterKey);
      try {
        await aesDecrypt(masterAesKey, fromBase64(bundle.classical_priv_encrypted));
      } catch {
        setError('Incorrect password');
        return;
      }

      // Do this before opening: if trusting the device fails, the user should
      // still get in, but they should not be told it worked when it did not.
      if (remember && userId) {
        try {
          const trusted = await trustThisDevice(userId, masterKey);
          await authApi.registerUnlockMethod({
            id: trusted.methodId,
            kind: 'device',
            label: trusted.label,
            wrapped_master_key: trusted.wrappedMasterKey,
          });
        } catch {
          await forgetDevice(userId);
          setError('Unlocked, but this device could not be remembered.');
        }
      }

      await openWithMasterKey(masterKey, bundle);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unlock failed');
    } finally {
      setLoading(false);
    }
  };

  // A trusted device unlocks in well under a second, and showing the
  // passphrase form first would make it flash up and vanish on every reload.
  if (checkingDevice) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
        <p className="text-sm text-slate-400">Unlocking…</p>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-full max-w-sm rounded-2xl border border-slate-700 bg-surface-1 p-8 shadow-2xl">
        <div className="mb-6 flex items-center gap-3">
          <svg className="h-8 w-8 text-accent" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z"/>
          </svg>
          <div>
            <h2 className="text-lg font-bold text-white">Unlock Vault</h2>
            <p className="text-sm text-slate-400">Enter your password to load session keys</p>
          </div>
        </div>

        <form onSubmit={handleUnlock} className="space-y-4">
          <div>
            <label className="mb-1.5 block text-sm font-medium text-slate-300">
              Password for <span className="text-accent">{username}</span>
            </label>
            <PasswordInput
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter your password"
              autoComplete="current-password"
              autoFocus
            />
          </div>

          {userId && (
            <label className="flex items-start gap-2 text-sm text-slate-400">
              <input
                type="checkbox"
                checked={remember}
                onChange={(e) => setRemember(e.target.checked)}
                className="mt-0.5"
              />
              <span>
                Remember this device
                <span className="block text-xs text-slate-500">
                  Skips this step next time on this browser. Anyone who can use this browser
                  profile can then open your vaults, so leave it off on a shared machine. You
                  can undo it from account settings.
                </span>
              </span>
            </label>
          )}

          {error && (
            <p className="rounded-lg border border-red-800 bg-red-900/30 px-3 py-2 text-sm text-red-400">
              {error}
            </p>
          )}

          <button
            type="submit"
            disabled={loading || !password}
            className="w-full rounded-lg bg-accent py-2.5 font-medium text-white transition hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
          >
            {loading ? (
              <span className="flex items-center justify-center gap-2">
                <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
                  <circle
                    className="opacity-25"
                    cx="12"
                    cy="12"
                    r="10"
                    stroke="currentColor"
                    strokeWidth="4"
                  />
                  <path
                    className="opacity-75"
                    fill="currentColor"
                    d="M4 12a8 8 0 018-8v8H4z"
                  />
                </svg>
                Deriving keys…
              </span>
            ) : (
              'Unlock'
            )}
          </button>
        </form>
      </div>
    </div>
  );
}
