import { useState } from 'react';
import { authApi } from '../api/auth';
import { aesDecrypt } from '../crypto/aes';
import { deriveMasterAesKey, deriveMasterKey, deriveAuthToken } from '../crypto/kdf';
import { fromBase64 } from '../crypto/utils';
import { useAuthStore } from '../store/auth';
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
  const { username, setSessionKeys } = useAuthStore();
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleUnlock = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    if (!password) {
      setError('Password is required');
      return;
    }
    setLoading(true);
    try {
      // 1. Fetch Argon2 parameters for this user
      const saltResp = await authApi.getSalt(username!);

      // 2. Re-derive master key
      const masterKey = await deriveMasterKey(
        password,
        saltResp.argon2_salt,
        saltResp.argon2_params,
      );

      // 3. Re-derive auth token and log in to get the encrypted key bundle
      const authToken = deriveAuthToken(masterKey);
      const loginResp = await authApi.login(username!, authToken);

      // 4. Decrypt the private key bundle with masterKey
      const masterAesKey = await deriveMasterAesKey(masterKey);
      const classicalPrivKey = await aesDecrypt(
        masterAesKey,
        fromBase64(loginResp.classical_priv_encrypted),
      );
      const pqPrivKey = await aesDecrypt(masterAesKey, fromBase64(loginResp.pq_priv_encrypted));

      const keys: SessionKeys = {
        masterKey,
        classicalPrivKey,
        classicalPubKey: fromBase64(loginResp.classical_public_key),
        pqPrivKey,
        pqPubKey: fromBase64(loginResp.pq_public_key),
      };

      setSessionKeys(keys);
      onUnlocked();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unlock failed');
    } finally {
      setLoading(false);
    }
  };

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
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Enter your password"
              className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              autoFocus
            />
          </div>

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
