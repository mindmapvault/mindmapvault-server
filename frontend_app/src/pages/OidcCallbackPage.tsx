import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { authApi } from '../api/auth';
import { useAuthStore } from '../store/auth';

/**
 * Where the server sends the browser after a federated sign-in.
 *
 * The tokens arrive in the URL fragment rather than the query string: a
 * fragment is never sent to a server, so it stays out of access logs, proxy
 * logs and `Referer` headers. It is read once and removed from history so a
 * back button cannot resurrect it.
 */
export function OidcCallbackPage() {
  const navigate = useNavigate();
  const { setTokens } = useAuthStore();
  const [error, setError] = useState('');
  // React 18 mounts effects twice in development. Consuming the fragment is
  // destructive, so it must happen once.
  const consumed = useRef(false);

  useEffect(() => {
    if (consumed.current) return;
    consumed.current = true;

    const params = new URLSearchParams(window.location.hash.replace(/^#/, ''));
    const access = params.get('access_token');
    const refresh = params.get('refresh_token');
    const needsEnrolment = params.get('enrol') === '1';

    // Clear it before anything else can read it, and replace the history entry
    // so the tokens are not reachable by going back.
    window.history.replaceState(null, '', window.location.pathname);

    if (!access || !refresh) {
      setError('That sign-in did not complete. Please try again.');
      return;
    }

    // The username is not known yet for a new account — enrolment sets it, and
    // for a returning one it is filled in from the profile below.
    setTokens(access, refresh, '');

    if (needsEnrolment) {
      navigate('/sso/finish', { replace: true });
      return;
    }

    // A returning federated user: pick the username up so the unlock screen can
    // say whose vaults it is asking about.
    authApi
      .getProfile()
      .then((profile) => setTokens(access, refresh, profile.username))
      .catch(() => {
        // Not fatal — the app works without it, and a failure here should not
        // strand someone who has just signed in successfully.
      })
      .finally(() => navigate('/vaults', { replace: true }));
  }, [navigate, setTokens]);

  if (error) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-surface px-4">
        <div className="w-full max-w-sm rounded-2xl border border-slate-700 bg-surface-1 p-8 text-center shadow-2xl">
          <p className="text-sm text-red-200">{error}</p>
          <button
            onClick={() => navigate('/login', { replace: true })}
            className="mt-4 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white transition hover:bg-accent-hover"
          >
            Back to sign in
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface">
      <p className="text-sm text-slate-400">Signing you in…</p>
    </div>
  );
}
