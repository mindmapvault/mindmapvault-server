import { useEffect, useMemo, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { authApi } from '../api/auth';
import { LogoBlock } from '../components/Logo';
import { aesEncrypt } from '../crypto/aes';
import { DEFAULT_ARGON2_PARAMS, deriveMasterAesKey, deriveMasterKey, deriveAuthToken } from '../crypto/kdf';
import { generateUserKeyPairs } from '../crypto/kem';
import { randomBytes, toBase64 } from '../crypto/utils';
import { isTauri } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import type { SessionKeys } from '../types';

const HOSTED_APP_BASE = 'https://app.mindmapvault.com';
const HOSTED_LOGIN_URL = `${HOSTED_APP_BASE}/login`;
const HOSTED_REGISTER_URL = `${HOSTED_APP_BASE}/register`;

function validateUsername(value: string) {
  if (!value) {
    return 'Username is required';
  }
  if (value.length < 3) {
    return 'Username must be at least 3 characters';
  }
  if (!/^[a-zA-Z0-9._-]+$/.test(value)) {
    return 'Username may use letters, numbers, dots, dashes, and underscores only';
  }
  return '';
}

function toFriendlyAuthError(err: unknown) {
  return err instanceof Error ? err.message : 'Registration failed';
}

function getSafeRedirectPath(searchParams: URLSearchParams, fallback = '/vaults') {
  const next = searchParams.get('next')?.trim();
  if (!next || !next.startsWith('/') || next.startsWith('//')) {
    return fallback;
  }

  return next;
}

export function RegisterPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { setTokens, setSessionKeys } = useAuthStore();
  const setMode = useModeStore((s) => s.setMode);
  const isDesktop = isTauri();

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  // `null` while unknown. The form stays available until the server actually
  // says sign-ups are closed, so an unreachable or older backend behaves the
  // way it always did rather than locking people out of registering.
  const [registrationEnabled, setRegistrationEnabled] = useState<boolean | null>(null);
  // An invite link carries its code in the URL, so the person following one
  // lands on a filled-in form rather than retyping what they were sent.
  const [inviteCode, setInviteCode] = useState(() => searchParams.get('invite')?.trim() ?? '');
  // Opens the form on a closed server for someone who was sent a code.
  const [redeemingInvite, setRedeemingInvite] = useState(() => Boolean(searchParams.get('invite')));
  const postAuthRedirect = useMemo(() => getSafeRedirectPath(searchParams), [searchParams]);

  useEffect(() => {
    if (isDesktop) {
      return;
    }

    let cancelled = false;
    authApi
      .getInstanceInfo()
      .then((info) => {
        if (!cancelled) {
          setRegistrationEnabled(info.registration_enabled);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setRegistrationEnabled(null);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [isDesktop]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');

    const normalizedUsername = username.trim();
    const usernameError = validateUsername(normalizedUsername);
    if (usernameError) {
      setError(usernameError);
      return;
    }

    if (!password || !confirm) {
      setError('Password and confirmation are required');
      return;
    }
    if (password !== confirm) {
      setError('Passwords do not match');
      return;
    }
    if (password.length < 12) {
      setError('Password must be at least 12 characters');
      return;
    }
    if (registrationEnabled === false && !inviteCode.trim()) {
      setError('An invite code is required on this server');
      return;
    }

    setLoading(true);
    try {
      // 1. Generate random Argon2 salt
      const salt = randomBytes(32);
      const saltB64 = toBase64(salt);

      // 2. Derive master key (Argon2id — takes a few seconds intentionally)
      const masterKey = await deriveMasterKey(password, saltB64, DEFAULT_ARGON2_PARAMS);

      // 3. Derive auth_token — this is what the server stores (hashed)
      const authToken = deriveAuthToken(masterKey);

      // 4. Generate X25519 + ML-KEM-768 keypairs
      const { classical, pq } = generateUserKeyPairs();

      // 5. Encrypt private keys with masterKey before sending to server
      const masterAesKey = await deriveMasterAesKey(masterKey);
      const classPrivEnc = await aesEncrypt(masterAesKey, classical.privateKey);
      const pqPrivEnc = await aesEncrypt(masterAesKey, pq.secretKey);

      // 6. Register
      await authApi.register({
        username: normalizedUsername,
        auth_token: authToken,
        argon2_salt: saltB64,
        argon2_params: DEFAULT_ARGON2_PARAMS,
        classical_public_key: toBase64(classical.publicKey),
        pq_public_key: toBase64(pq.publicKey),
        classical_priv_encrypted: toBase64(classPrivEnc),
        pq_priv_encrypted: toBase64(pqPrivEnc),
        // Sent only when the server needs one, so an open server never spends
        // an invite on a sign-up that did not require it.
        ...(registrationEnabled === false ? { invite_code: inviteCode.trim() } : {}),
      });

      const loginResp = await authApi.login(normalizedUsername, authToken);

      const keys: SessionKeys = {
        masterKey,
        classicalPrivKey: classical.privateKey,
        classicalPubKey: classical.publicKey,
        pqPrivKey: pq.secretKey,
        pqPubKey: pq.publicKey,
      };

      setTokens(loginResp.access_token, loginResp.refresh_token, normalizedUsername);
      setSessionKeys(keys);
      navigate(postAuthRedirect, { replace: true });
    } catch (err) {
      setError(toFriendlyAuthError(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <LogoBlock className="mb-8" />

        {/* Card */}
        <div className="rounded-2xl border border-slate-700 bg-surface-1 p-8 shadow-xl">
          <h2 className="mb-1 text-lg font-semibold text-white">
            {registrationEnabled === false ? 'Sign-ups are closed' : 'Create account'}
          </h2>
          <p className="mb-6 text-xs text-slate-500">
            {registrationEnabled === false
              ? 'The person running this server has turned off new accounts.'
              : 'Your password derives the encryption key — it never leaves your device.'}
          </p>

          {registrationEnabled === false && !redeemingInvite ? (
            <div className="space-y-3">
              <p className="text-sm text-slate-300">
                You need an invite code from whoever runs it. If you already have an account, sign
                in below.
              </p>
              <button
                type="button"
                onClick={() => setRedeemingInvite(true)}
                className="w-full rounded-lg border border-slate-600 py-2.5 text-sm font-medium text-slate-200 transition hover:border-accent hover:text-white"
              >
                I have an invite code
              </button>
            </div>
          ) : (
          <form onSubmit={handleSubmit} className="space-y-4">
            {registrationEnabled === false && (
              <div>
                <label className="mb-1.5 block text-sm font-medium text-slate-300">
                  Invite code
                </label>
                <input
                  type="text"
                  value={inviteCode}
                  onChange={(e) => setInviteCode(e.target.value)}
                  placeholder="MMV-XXXX-XXXX-XXXX"
                  autoComplete="off"
                  autoCapitalize="characters"
                  spellCheck={false}
                  required
                  className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 font-mono text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
                />
              </div>
            )}
            <div>
              <label className="mb-1.5 block text-sm font-medium text-slate-300">Username</label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="your_username"
                autoComplete="username"
                required
                className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-sm font-medium text-slate-300">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="min. 12 characters"
                autoComplete="new-password"
                required
                minLength={12}
                className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-sm font-medium text-slate-300">
                Confirm password
              </label>
              <input
                type="password"
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
                placeholder="••••••••••••"
                autoComplete="new-password"
                required
                className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              />
            </div>

            {error && (
              <p className="rounded-lg border border-red-800 bg-red-900/30 px-3 py-2 text-sm text-red-400">
                {error}
              </p>
            )}

            <button
              type="submit"
              disabled={loading || !username || !password || !confirm}
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
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
                  </svg>
                  Generating keys…
                </span>
              ) : (
                'Create account'
              )}
            </button>
          </form>
          )}
        </div>

        <p className="mt-4 text-center text-sm text-slate-500">
          Already have an account?{' '}
          <Link to={`/login${searchParams.toString() ? `?${searchParams.toString()}` : ''}`} className="text-accent hover:underline">
            Sign in
          </Link>
        </p>

        <p className="mt-2 text-center text-xs text-slate-500">
          Prefer the hosted in Cloud?{' '}
          <a href={HOSTED_LOGIN_URL} className="text-accent hover:underline">
            Sign in there
          </a>
          {' '}or{' '}
          <a href={HOSTED_REGISTER_URL} className="text-accent hover:underline">
            create a hosted account
          </a>
          .
        </p>

        {isDesktop && (
          <p className="mt-2 text-center text-xs text-slate-500">
            <button
              type="button"
              className="text-accent hover:underline"
              onClick={() => {
                setMode('local');
                navigate('/local-unlock');
              }}
            >
              Set up offline mode instead
            </button>
          </p>
        )}
      </div>
    </div>
  );
}
