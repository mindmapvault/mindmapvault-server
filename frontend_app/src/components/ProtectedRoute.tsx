import { Navigate, Outlet } from 'react-router-dom';
import { isTauri } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';

export function ProtectedRoute() {
  const isAuthenticated = useAuthStore((s) => s.isAuthenticated());
  const hasKeys = useAuthStore((s) => s.hasSessionKeys());
  const mode = useModeStore((s) => s.mode);
  const isDesktop = isTauri();

  // Local mode is desktop-only: only need session keys (no JWT tokens).
  if (isDesktop && mode === 'local') {
    if (!hasKeys) return <Navigate to="/local-unlock" replace />;
    return <Outlet />;
  }

  // Server mode: need a valid access token.
  if (!isAuthenticated) return <Navigate to={isDesktop ? '/login' : '/'} replace />;
  return <Outlet />;
}
