import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { authApi } from '../api/auth';
import { aesEncrypt } from '../crypto/aes';
import { generateUserKeyPairs } from '../crypto/kem';
import {
  DEFAULT_ARGON2_PARAMS,
  deriveMasterAesKey,
  deriveMasterKey,
} from '../crypto/kdf';
import { randomBytes, toBase64 } from '../crypto/utils';
import { useAuthStore } from '../store/auth';
import { PasswordInput } from '../components/PasswordInput';
import type { SessionKeys } from '../types';

/**
 * Finishes an account created by signing in through an identity provider.
 *
 * The provider established *who* the user is. It cannot establish what
 * decrypts their vaults: the key is derived here, from a passphrase that never
 * leaves the browser, and the server only ever receives the public keys and
 * the private ones already encrypted under it.
 *
 * The username is asked for rather than taken from a provider claim. A name
 * lifted from an IdP can collide with an existing local account, and resolving
 * that collision by merging would be an account takeover.
 */
export function FederatedEnrolPage() {
  const navigate = useNavigate();
  const { setTokens, setSessionKeys, accessToken, refreshToken } = useAuthStore();

  const [username, setUsername] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    const name = username.trim();
    if (!name) {
      setError('Choose a username');
      return;
    }
    if (!/^[A-Za-z0-9._-]+$/.test(name)) {
      setError('Use letters, digits, dot, dash and underscore only');
      return;
    }
    if (passphrase.length < 12) {
      setError('Use a passphrase of at least 12 characters');
      return;
    }
    if (passphrase !== confirm) {
      setError('The two passphrases do not match');
      return;
    }

    setLoading(true);
    try {
      // Exactly what registration does, because it produces exactly the same
      // thing: an account whose vaults only this passphrase can open.
      const salt = randomBytes(32);
      const saltB64 = toBase64(salt);
      const masterKey = await deriveMasterKey(passphrase, saltB64, DEFAULT_ARGON2_PARAMS);
      const { classical, pq } = generateUserKeyPairs();
      const masterAesKey = await deriveMasterAesKey(masterKey);
      const classPrivEnc = await aesEncrypt(masterAesKey, classical.privateKey);
      const pqPrivEnc = await aesEncrypt(masterAesKey, pq.secretKey);

      // No auth_token is sent. A federated account has no password credential,
      // so there is nothing for the sign-in route to verify and no counter for
      // an attacker to grind against.
      const result = await authApi.enrolFederatedAccount({
        username: name,
        argon2_salt: saltB64,
        argon2_params: DEFAULT_ARGON2_PARAMS,
        classical_public_key: toBase64(classical.publicKey),
        pq_public_key: toBase64(pq.publicKey),
        classical_priv_encrypted: toBase64(classPrivEnc),
        pq_priv_encrypted: toBase64(pqPrivEnc),
      });

      // The tokens are already ours from the callback; only the username has
      // changed, and the store keeps it for the unlock screen to show.
      if (accessToken && refreshToken) {
        setTokens(accessToken, refreshToken, result.username);
      }

      const keys: SessionKeys = {
        masterKey,
        classicalPrivKey: classical.privateKey,
        classicalPubKey: classical.publicKey,
        pqPrivKey: pq.secretKey,
        pqPubKey: pq.publicKey,
      };
      setSessionKeys(keys);
      navigate('/vaults', { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Could not finish setting up your account');
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface px-4">
      <div className="w-full max-w-md rounded-2xl border border-slate-700 bg-surface-1 p-8 shadow-2xl">
        <h1 className="text-xl font-bold text-white">Finish setting up</h1>
        <p className="mt-2 text-sm text-slate-400">
          You are signed in. One more step: choose a name, and a passphrase that will unlock
          your vaults.
        </p>

        <div className="mt-4 rounded-lg border border-slate-700 bg-surface p-3 text-xs text-slate-400">
          Your vaults are encrypted in this browser, so your passphrase never reaches the
          server and cannot be reset by an administrator. If you lose it, the vaults it
          protects are gone. Store it somewhere safe.
        </div>

        <form onSubmit={handleSubmit} className="mt-6 space-y-4">
          <label className="block">
            <span className="text-sm text-slate-300">Username</span>
            <input
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoFocus
              autoComplete="username"
              placeholder="how you appear in this instance"
              className="mt-1 w-full rounded-lg border border-slate-600 bg-surface px-4 py-2 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
            />
          </label>

          <label className="block">
            <span className="text-sm text-slate-300">Vault passphrase</span>
            <PasswordInput
              value={passphrase}
              onChange={(e) => setPassphrase(e.target.value)}
              autoComplete="new-password"
              placeholder="at least 12 characters"
              required
            />
          </label>

          <label className="block">
            <span className="text-sm text-slate-300">Confirm passphrase</span>
            <PasswordInput
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              autoComplete="new-password"
              placeholder="type it again"
              required
            />
          </label>

          {error && (
            <div className="rounded-lg border border-red-800 bg-red-900/30 px-4 py-3 text-sm text-red-200">
              {error}
            </div>
          )}

          <button
            type="submit"
            disabled={loading}
            className="w-full rounded-lg bg-accent px-4 py-2 font-medium text-white transition hover:bg-accent-hover disabled:opacity-50"
          >
            {loading ? 'Setting up…' : 'Finish'}
          </button>
        </form>
      </div>
    </div>
  );
}
