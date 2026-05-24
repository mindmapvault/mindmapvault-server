import { useEffect, useMemo, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import { authApi } from '../api/auth';
import { DEFAULT_CLOUD_SERVER_URL, setServerUrl } from '../api/client';
import { DesktopTauriBadge } from '../components/DesktopTauriBadge';
import { LogoBlock } from '../components/Logo';
import { PwaInstallButton } from '../components/PwaInstallButton';
import { aesDecrypt } from '../crypto/aes';
import { deriveMasterAesKey, deriveMasterKey, deriveAuthToken } from '../crypto/kdf';
import { fromBase64 } from '../crypto/utils';
import { isTauri } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import { useThemeStore } from '../store/theme';
import type { SessionKeys } from '../types';
import packageJson from '../../package.json';

const HOSTED_APP_BASE = 'https://app.mindmapvault.com';
const HOSTED_LOGIN_URL = `${HOSTED_APP_BASE}/login`;
const HOSTED_REGISTER_URL = `${HOSTED_APP_BASE}/register`;
const PWA_INSTALL_DISMISSED_KEY = 'mindmapvault-pwa-install-dismissed-v1';

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
  return err instanceof Error ? err.message : 'Login failed';
}

function getSafeRedirectPath(searchParams: URLSearchParams, fallback = '/vaults') {
  const next = searchParams.get('next')?.trim();
  if (!next || !next.startsWith('/') || next.startsWith('//')) {
    return fallback;
  }

  return next;
}

export function LoginPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { setTokens, setSessionKeys } = useAuthStore();
  const appMode = useModeStore((s) => s.mode);
  const setMode = useModeStore((s) => s.setMode);
  const { mode: themeMode, toggleMode } = useThemeStore();
  const isDesktop = isTauri();

  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [showInstallPanel, setShowInstallPanel] = useState(!isDesktop);
  const postAuthRedirect = useMemo(() => getSafeRedirectPath(searchParams), [searchParams]);
  const registrationSucceeded = searchParams.get('registered') === '1';
  const appVersion = packageJson.version;

  useEffect(() => {
    const prefilledUsername = searchParams.get('username')?.trim();
    if (prefilledUsername) {
      setUsername(prefilledUsername);
    }
  }, [searchParams]);

  useEffect(() => {
    if (!isDesktop) return;
    if (appMode === 'server') return;
    navigate('/local-unlock', { replace: true });
  }, [appMode, isDesktop, navigate]);

  useEffect(() => {
    if (isDesktop) {
      setShowInstallPanel(false);
      return;
    }

    const isStandalone = window.matchMedia('(display-mode: standalone)').matches
      || ((window.navigator as Navigator & { standalone?: boolean }).standalone === true);
    const dismissed = localStorage.getItem(PWA_INSTALL_DISMISSED_KEY) === '1';
    setShowInstallPanel(!isStandalone && !dismissed);
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
    if (!password) {
      setError('Password is required');
      return;
    }
    setLoading(true);
    try {
      if (isDesktop) setServerUrl(DEFAULT_CLOUD_SERVER_URL);

      // 1. Fetch the user's Argon2 salt + params
      const saltResp = await authApi.getSalt(normalizedUsername);

      // 2. Derive master key — this is the expensive Argon2id step
      const masterKey = await deriveMasterKey(
        password,
        saltResp.argon2_salt,
        saltResp.argon2_params,
      );

      // 3. Derive auth_token (never sends the password or master key)
      const authToken = deriveAuthToken(masterKey);

      // 4. Login — server verifies Argon2id(auth_token) against stored hash
      const loginResp = await authApi.login(normalizedUsername, authToken);

      // 5. Decrypt private key bundle with the master key
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

      // 6. Store tokens in localStorage, session keys in memory only
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
    <div className={`relative flex min-h-screen items-center justify-center px-4 ${!isDesktop ? 'pb-44 sm:pb-36' : ''}`}>
      {/* Hero background illustration */}
      <img
        src="/vault-mindmap-hero.svg"
        alt=""
        aria-hidden="true"
        draggable={false}
        className="pointer-events-none absolute inset-0 h-full w-full object-contain opacity-10"
      />

      {/* Theme toggle */}
      <button
        type="button"
        onClick={toggleMode}
        className="absolute right-4 top-4 z-10 inline-flex items-center gap-1.5 rounded-lg border border-slate-600 px-3 py-1.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white"
        title={themeMode === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
      >
        {themeMode === 'dark' ? (
          <>
            <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
              <circle cx="12" cy="12" r="4" />
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M4.93 19.07l1.41-1.41m11.32-11.32 1.41-1.41" />
            </svg>
            <span>Light</span>
          </>
        ) : (
          <>
            <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 24 24">
              <path d="M21 12.79A9 9 0 1111.21 3a7 7 0 009.79 9.79z" />
            </svg>
            <span>Dark</span>
          </>
        )}
      </button>

      <div
        className={`absolute bottom-4 left-4 z-10 rounded-full px-3 py-1.5 text-xs font-semibold tracking-[0.18em] ${
          themeMode === 'light'
            ? 'border border-slate-300 bg-white/85 text-slate-700 shadow-sm backdrop-blur'
            : 'border border-slate-700 bg-surface-1/80 text-slate-300 shadow-lg backdrop-blur'
        }`}
      >
        APP v{appVersion}
      </div>

      {!isDesktop && showInstallPanel && (
        <div className="absolute bottom-4 left-1/2 z-10 w-[calc(100%-2rem)] max-w-md -translate-x-1/2">
          <div className={`rounded-2xl border px-4 py-3 backdrop-blur ${
            themeMode === 'light'
              ? 'border-slate-300 bg-white/80 shadow-sm'
              : 'border-slate-700 bg-surface-1/75 shadow-lg'
          }`}>
            <p className="text-sm font-semibold text-slate-100">Install MindMapVault app</p>
            <p className="mt-1 text-xs leading-relaxed text-slate-400">
              This adds an app-like shortcut (PWA) on this device for a faster, cleaner experience.
              Your vault data stays on your server account; this does not create an offline local vault.
            </p>
            <PwaInstallButton large showDismiss className="mt-3 w-full" onDismissed={() => setShowInstallPanel(false)} />
          </div>
        </div>
      )}

      <div className="relative z-[1] w-full max-w-sm">
        {/* Logo */}
        <LogoBlock className="mb-8" />

        {/* Card */}
        <div className="rounded-2xl border border-slate-700 bg-surface-1 p-8 shadow-xl">
          {isDesktop && (
            <div
              className={`mb-5 rounded-lg px-3 py-2 text-xs ${
                themeMode === 'light'
                  ? 'border border-emerald-300 bg-emerald-50 text-emerald-800'
                  : 'border border-emerald-900/60 bg-emerald-900/15 text-emerald-200'
              }`}
            >
              Desktop default is local-first. Cloud sync is optional and can be enabled when you choose it.
            </div>
          )}
          <h2 className="mb-6 text-lg font-semibold text-white">Sign in</h2>

          <form onSubmit={handleSubmit} className="space-y-4">
            {registrationSucceeded && (
              <p className="rounded-lg border border-emerald-800 bg-emerald-900/20 px-3 py-2 text-sm text-emerald-300">
                Account created. Sign in to open your vaults.
              </p>
            )}

            <div>
              <label className="mb-1.5 block text-sm font-medium text-slate-300">Username</label>
              <input
                type="text"
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="your_username"
                autoComplete="username"
                className="w-full rounded-lg border border-slate-600 bg-surface px-4 py-2.5 text-white placeholder-slate-500 focus:border-accent focus:outline-none focus:ring-1 focus:ring-accent"
              />
            </div>

            <div>
              <label className="mb-1.5 block text-sm font-medium text-slate-300">Password</label>
              <input
                type="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••••••••"
                autoComplete="current-password"
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
              disabled={loading || !username || !password}
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
                  Deriving keys…
                </span>
              ) : (
                'Sign in'
              )}
            </button>

          </form>
        </div>

        <p className="mt-4 text-center text-sm text-slate-500">
          No account?{' '}
          <Link to={`/register${searchParams.toString() ? `?${searchParams.toString()}` : ''}`} className="text-accent hover:underline">
            Create one
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
              className="text-slate-400 hover:text-accent hover:underline"
              onClick={() => {
                setMode('local');
                navigate('/local-unlock');
              }}
            >
              Back to offline local vault
            </button>
          </p>
        )}
      </div>

      {isDesktop && <DesktopTauriBadge />}
    </div>
  );
}
