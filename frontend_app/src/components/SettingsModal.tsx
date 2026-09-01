import { useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { authApi } from '../api/auth';
import type { UserProfile } from '../types';
import { PwaInstallButton } from './PwaInstallButton';
import { VaultIcon } from './Logo';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import { AutosaveMode, useThemeStore } from '../store/theme';

export type SettingsTab = 'account' | 'appearance' | 'support';

const PRESETS = [
  '#6366f1', '#8b5cf6', '#ec4899', '#ef4444', '#f97316',
  '#eab308', '#22c55e', '#14b8a6', '#06b6d4', '#3b82f6',
];

const autosaveOptions: Array<{ value: AutosaveMode; label: string }> = [
  { value: 'change', label: 'After each change' },
  { value: '30s', label: 'Every 30 seconds' },
  { value: '5m', label: 'Every 5 minutes' },
  { value: 'never', label: 'Never' },
];

const inputStyle = {
  background: 'var(--surface-2)',
  color: 'var(--text-primary)',
  border: '1px solid var(--border-light)',
} as const;

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <span
      className="mb-2 block text-xs font-semibold uppercase tracking-wider"
      style={{ color: 'var(--text-muted)' }}
    >
      {children}
    </span>
  );
}

const icons: Record<SettingsTab, ReactNode> = {
  account: (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
      <circle cx="12" cy="8" r="4" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M4 20c0-3.3 3.6-6 8-6s8 2.7 8 6" />
    </svg>
  ),
  appearance: (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
      <circle cx="13.5" cy="6.5" r="1.5" /><circle cx="17.5" cy="10.5" r="1.5" />
      <circle cx="8.5" cy="7.5" r="1.5" /><circle cx="6.5" cy="12.5" r="1.5" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M12 2a10 10 0 1 0 0 20c.83 0 1.5-.67 1.5-1.5 0-.39-.15-.74-.39-1.01-.23-.26-.39-.61-.39-1 0-.83.67-1.5 1.5-1.5H16a6 6 0 0 0 6-6c0-5.52-4.48-9-10-9z" />
    </svg>
  ),
  support: (
    <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
      <circle cx="12" cy="12" r="9" />
      <path strokeLinecap="round" strokeLinejoin="round" d="M9.5 9.5a2.5 2.5 0 1 1 3.2 2.4c-.5.2-.7.6-.7 1.1v.5" />
      <circle cx="12" cy="17" r=".6" fill="currentColor" stroke="none" />
    </svg>
  ),
};

const tabTitles: Record<SettingsTab, string> = {
  account: 'Account',
  appearance: 'Appearance',
  support: 'Help',
};

interface SettingsModalProps {
  open: boolean;
  onClose: () => void;
  initialTab?: SettingsTab;
}

export function SettingsModal({ open, onClose, initialTab = 'account' }: SettingsModalProps) {
  const {
    mode, primaryColor, autoLogoutMinutes, autosaveMode,
    toggleMode, setPrimaryColor, setAutoLogoutMinutes, setAutosaveMode,
  } = useThemeStore();
  const { username, logout } = useAuthStore();
  const isLocalMode = useModeStore((s) => s.mode) === 'local';
  // A local desktop profile has no account on any server, so the parts that
  // talk to one are hidden rather than shown broken.
  const hasServerAccount = !isLocalMode && !!username;

  const [tab, setTab] = useState<SettingsTab>(initialTab);

  const [deleteConfirmText, setDeleteConfirmText] = useState('');
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState('');
  const deletePhrase = username?.trim() || 'DELETE';

  const order: SettingsTab[] = ['account', 'appearance', 'support'];

  useEffect(() => {
    if (open) setTab(initialTab);
  }, [open, initialTab]);

  useEffect(() => {
    if (!open) {
      setDeleteConfirmText('');
      setDeleteError('');
      setDeleteBusy(false);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [open, onClose]);

  const handleDeleteAccount = async () => {
    if (deleteBusy || deleteConfirmText.trim() !== deletePhrase) return;
    setDeleteBusy(true);
    setDeleteError('');
    try {
      await authApi.deleteProfile();
      onClose();
      logout();
    } catch (err) {
      setDeleteError(err instanceof Error ? err.message : 'Failed to delete account data');
      setDeleteBusy(false);
    }
  };

  if (!open) return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[100] flex items-center justify-center bg-slate-950/70 p-0 backdrop-blur-sm sm:p-4"
      onClick={onClose}
      data-testid="settings-modal"
    >
      <div
        className="flex h-full w-full flex-col overflow-hidden rounded-none shadow-2xl sm:h-[min(82vh,46rem)] sm:max-w-4xl sm:flex-row sm:rounded-2xl"
        style={{ background: 'var(--surface-1)', border: '1px solid var(--border)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <aside
          className="shrink-0 border-b sm:w-56 sm:border-b-0 sm:border-r"
          style={{ borderColor: 'var(--border)', background: 'var(--surface)' }}
        >
          <div className="flex items-center gap-2 px-4 py-4 sm:px-5">
            <VaultIcon size={24} />
            <span className="text-sm font-bold" style={{ color: 'var(--text-primary)' }}>MindMapVault</span>
          </div>
          <nav className="flex gap-1 overflow-x-auto px-2 pb-2 sm:flex-col sm:px-3 sm:pb-4">
            {order.map((id) => {
              const active = id === tab;
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => setTab(id)}
                  data-testid={`settings-tab-${id}`}
                  className="flex shrink-0 items-center gap-2.5 whitespace-nowrap rounded-lg px-3 py-2 text-sm font-medium transition"
                  style={{
                    background: active ? 'var(--surface-2)' : 'transparent',
                    color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                  }}
                >
                  {icons[id]}
                  {tabTitles[id]}
                </button>
              );
            })}
          </nav>
        </aside>

        <div className="flex min-w-0 flex-1 flex-col">
          <header
            className="flex items-center justify-between gap-3 border-b px-5 py-4"
            style={{ borderColor: 'var(--border)' }}
          >
            <h2 className="text-xl font-semibold" style={{ color: 'var(--text-primary)' }}>
              {tabTitles[tab]}
            </h2>
            <button
              type="button"
              onClick={onClose}
              title="Close"
              aria-label="Close settings"
              className="rounded-lg p-1.5 transition hover:bg-[var(--surface-2)]"
              style={{ color: 'var(--text-secondary)' }}
            >
              <svg className="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                <path strokeLinecap="round" strokeLinejoin="round" d="M6 6l12 12M18 6L6 18" />
              </svg>
            </button>
          </header>

          <div className="flex-1 overflow-y-auto px-5 py-5">
            {tab === 'account' && (
              <AccountTab
                hasServerAccount={hasServerAccount}
                autoLogoutMinutes={autoLogoutMinutes}
                setAutoLogoutMinutes={setAutoLogoutMinutes}
                deletePhrase={deletePhrase}
                deleteConfirmText={deleteConfirmText}
                setDeleteConfirmText={setDeleteConfirmText}
                deleteBusy={deleteBusy}
                deleteError={deleteError}
                onDeleteAccount={() => { void handleDeleteAccount(); }}
              />
            )}

            {tab === 'appearance' && (
              <AppearanceTab
                mode={mode}
                primaryColor={primaryColor}
                toggleMode={toggleMode}
                setPrimaryColor={setPrimaryColor}
                autosaveMode={autosaveMode}
                setAutosaveMode={setAutosaveMode}
                showInstall={!isLocalMode}
              />
            )}

            {tab === 'support' && <SupportTab />}
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

// ─── Account ─────────────────────────────────────────────────────────────────

function AccountTab({
  hasServerAccount,
  autoLogoutMinutes,
  setAutoLogoutMinutes,
  deletePhrase,
  deleteConfirmText,
  setDeleteConfirmText,
  deleteBusy,
  deleteError,
  onDeleteAccount,
}: {
  hasServerAccount: boolean;
  autoLogoutMinutes: number | null;
  setAutoLogoutMinutes: (minutes: number | null) => void;
  deletePhrase: string;
  deleteConfirmText: string;
  setDeleteConfirmText: (value: string) => void;
  deleteBusy: boolean;
  deleteError: string;
  onDeleteAccount: () => void;
}) {
  return (
    <div className="space-y-6">
      {hasServerAccount && <ProfileSection />}

      <section
        className={hasServerAccount ? 'border-t pt-6' : undefined}
        style={hasServerAccount ? { borderColor: 'var(--border)' } : undefined}
      >
        <SectionLabel>Security</SectionLabel>
        <label className="mb-2 block text-sm" style={{ color: 'var(--text-primary)' }}>
          Auto-logout after inactivity
        </label>
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={1}
            max={1440}
            step={1}
            value={autoLogoutMinutes ?? ''}
            onChange={(e) => {
              const value = e.target.value.trim();
              if (!value) { setAutoLogoutMinutes(null); return; }
              const minutes = Math.max(1, Math.min(1440, Math.trunc(Number(value))));
              if (Number.isFinite(minutes)) setAutoLogoutMinutes(minutes);
            }}
            placeholder="Never"
            aria-label="Auto-logout after inactivity, in minutes"
            className="w-full rounded-lg px-3 py-2 text-sm"
            style={inputStyle}
          />
          <button
            type="button"
            onClick={() => setAutoLogoutMinutes(null)}
            title="Disable automatic logout"
            className="rounded-lg px-3 py-2 text-sm font-medium transition"
            style={{ background: 'var(--surface-2)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)' }}
          >
            Never
          </button>
        </div>
        <p className="mt-2 text-xs" style={{ color: 'var(--text-muted)' }}>
          Minutes of inactivity before automatic logout. Leave empty or use Never to disable.
        </p>
      </section>

      <section className="border-t pt-6" style={{ borderColor: 'var(--border)' }}>
        <SectionLabel>Password</SectionLabel>
        <p className="text-sm leading-6" style={{ color: 'var(--text-secondary)' }}>
          Your password never reaches the server — it is what decrypts your vaults in this
          browser. That also means nobody, including whoever runs this server, can reset it
          for you.
        </p>
      </section>

      {hasServerAccount && (
        <section className="border-t pt-6" style={{ borderColor: 'var(--border)' }}>
          <SectionLabel>Danger zone</SectionLabel>
          <p className="text-sm" style={{ color: 'var(--text-primary)' }}>
            Permanently delete your account, its vault metadata, and every encrypted file you
            have uploaded to this server.
          </p>
          <label className="mt-3 mb-2 block text-sm" style={{ color: 'var(--text-primary)' }}>
            Type {deletePhrase} to confirm
          </label>
          <input
            type="text"
            value={deleteConfirmText}
            onChange={(e) => setDeleteConfirmText(e.target.value)}
            placeholder={deletePhrase}
            aria-label="Confirm account deletion"
            className="w-full rounded-lg px-3 py-2 text-sm"
            style={inputStyle}
          />
          <button
            type="button"
            onClick={onDeleteAccount}
            disabled={deleteBusy || deleteConfirmText.trim() !== deletePhrase}
            className="mt-3 w-full rounded-lg px-3 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
            style={{ background: 'rgba(239, 68, 68, 0.12)', color: '#ef4444', border: '1px solid rgba(239, 68, 68, 0.35)' }}
          >
            {deleteBusy ? 'Deleting account data…' : 'Delete account'}
          </button>
          <p className="mt-2 text-xs" style={{ color: 'var(--text-muted)' }}>
            This cannot be undone and will sign you out immediately.
          </p>
          {deleteError && <p className="mt-2 text-xs" style={{ color: '#ef4444' }}>{deleteError}</p>}
        </section>
      )}
    </div>
  );
}

function ProfileSection() {
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [username, setUsername] = useState('');
  const [firstName, setFirstName] = useState('');
  const [lastName, setLastName] = useState('');
  const [email, setEmail] = useState('');
  const [original, setOriginal] = useState({ firstName: '', lastName: '', email: '' });

  useEffect(() => {
    let mounted = true;
    setLoading(true);
    setError(null);
    authApi
      .getProfile()
      .then((profile: UserProfile) => {
        if (!mounted) return;
        setUsername(profile.username);
        const next = {
          firstName: profile.first_name ?? '',
          lastName: profile.last_name ?? '',
          email: profile.email ?? '',
        };
        setFirstName(next.firstName);
        setLastName(next.lastName);
        setEmail(next.email);
        setOriginal(next);
      })
      .catch((e) => { if (mounted) setError(e instanceof Error ? e.message : String(e)); })
      .finally(() => { if (mounted) setLoading(false); });
    return () => { mounted = false; };
  }, []);

  const dirty =
    firstName.trim() !== original.firstName ||
    lastName.trim() !== original.lastName ||
    email.trim() !== original.email;
  const emailValid = email.trim() === '' || email.includes('@');

  const handleSave = async () => {
    if (saving || !dirty || !emailValid) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      const updated = await authApi.updateProfile({
        first_name: firstName.trim(),
        last_name: lastName.trim(),
        email: email.trim(),
      });
      const next = {
        firstName: updated.first_name ?? '',
        lastName: updated.last_name ?? '',
        email: updated.email ?? '',
      };
      setFirstName(next.firstName);
      setLastName(next.lastName);
      setEmail(next.email);
      setOriginal(next);
      setSaved(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-2">
        <SectionLabel>Profile</SectionLabel>
        <button
          type="button"
          onClick={() => { void handleSave(); }}
          disabled={!dirty || saving || loading || !emailValid}
          data-testid="profile-save"
          className="rounded-lg px-3 py-1.5 text-xs font-semibold transition disabled:opacity-50"
          style={{ background: 'var(--accent)', color: 'white' }}
        >
          {saving ? 'Saving…' : 'Save'}
        </button>
      </div>

      {username && (
        <p className="mb-3 text-xs" style={{ color: 'var(--text-muted)' }}>
          Signed in as <span style={{ color: 'var(--text-secondary)' }}>{username}</span>
        </p>
      )}
      {error && (
        <p
          className="mb-3 rounded-lg px-3 py-2 text-xs"
          style={{ background: 'rgba(239, 68, 68, 0.12)', color: '#fca5a5' }}
        >
          {error}
        </p>
      )}

      <div className="grid gap-3 sm:grid-cols-2">
        <label className="text-xs" style={{ color: 'var(--text-secondary)' }}>
          First name
          <input
            type="text" value={firstName}
            onChange={(e) => { setFirstName(e.target.value); setSaved(false); }}
            placeholder="First name" autoComplete="given-name"
            className="mt-1 w-full rounded-lg px-3 py-2 text-sm" style={inputStyle}
          />
        </label>
        <label className="text-xs" style={{ color: 'var(--text-secondary)' }}>
          Last name
          <input
            type="text" value={lastName}
            onChange={(e) => { setLastName(e.target.value); setSaved(false); }}
            placeholder="Last name" autoComplete="family-name"
            className="mt-1 w-full rounded-lg px-3 py-2 text-sm" style={inputStyle}
          />
        </label>
        <label className="col-span-full text-xs" style={{ color: 'var(--text-secondary)' }}>
          Email
          <input
            type="email" value={email}
            onChange={(e) => { setEmail(e.target.value); setSaved(false); }}
            placeholder="you@example.com" autoComplete="email"
            className="mt-1 w-full rounded-lg px-3 py-2 text-sm" style={inputStyle}
          />
        </label>
      </div>

      {!emailValid && (
        <p className="mt-2 text-xs" style={{ color: '#fca5a5' }}>
          Enter a valid email address or leave it empty.
        </p>
      )}
      {saved && <p className="mt-2 text-xs" style={{ color: '#34d399' }}>Profile saved.</p>}
      <p className="mt-2 text-xs leading-5" style={{ color: 'var(--text-muted)' }}>
        Optional, and how whoever administers this server can recognise your account. Unlike
        your vaults, these fields are stored unencrypted and are visible in the admin console.
        Your username cannot be changed here.
      </p>
    </section>
  );
}

// ─── Appearance ──────────────────────────────────────────────────────────────

function AppearanceTab({
  mode, primaryColor, toggleMode, setPrimaryColor, autosaveMode, setAutosaveMode, showInstall,
}: {
  mode: 'dark' | 'light';
  primaryColor: string;
  toggleMode: () => void;
  setPrimaryColor: (color: string) => void;
  autosaveMode: AutosaveMode;
  setAutosaveMode: (mode: AutosaveMode) => void;
  showInstall: boolean;
}) {
  return (
    <div className="space-y-6">
      <section>
        <SectionLabel>Theme</SectionLabel>
        <div
          className="flex items-center justify-between rounded-xl px-4 py-3"
          style={{ background: 'var(--surface-2)' }}
        >
          <span className="text-sm" style={{ color: 'var(--text-primary)' }}>
            {mode === 'dark' ? 'Dark mode' : 'Light mode'}
          </span>
          <button
            onClick={toggleMode}
            title={`Switch to ${mode === 'dark' ? 'light' : 'dark'} mode`}
            data-testid="settings-theme-toggle"
            className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium transition"
            style={{ background: 'var(--surface-1)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)' }}
          >
            {mode === 'dark' ? (
              <>
                <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <circle cx="12" cy="12" r="4" />
                  <path strokeLinecap="round" d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M4.93 19.07l1.41-1.41m11.32-11.32 1.41-1.41" />
                </svg>
                Switch to light
              </>
            ) : (
              <>
                <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 24 24">
                  <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
                </svg>
                Switch to dark
              </>
            )}
          </button>
        </div>
      </section>

      <section>
        <SectionLabel>Accent colour</SectionLabel>
        <div className="flex flex-wrap gap-2">
          {PRESETS.map((c) => (
            <button
              key={c}
              onClick={() => setPrimaryColor(c)}
              className="h-9 w-9 rounded-lg transition-transform hover:scale-110"
              style={{
                backgroundColor: c,
                outline: primaryColor.toLowerCase() === c ? `2.5px solid ${c}` : 'none',
                outlineOffset: '2px',
                boxShadow: primaryColor.toLowerCase() === c ? '0 0 0 1px var(--surface-1)' : 'none',
              }}
              title={c}
              aria-label={`Use accent colour ${c}`}
            />
          ))}
        </div>
        <div
          className="mt-3 flex items-center gap-3 rounded-lg px-3 py-2"
          style={{ background: 'var(--surface-2)' }}
        >
          <label className="text-sm" style={{ color: 'var(--text-secondary)' }}>Custom</label>
          <input
            type="color" value={primaryColor}
            onChange={(e) => setPrimaryColor(e.target.value)}
            aria-label="Custom accent colour"
            className="h-7 w-10 cursor-pointer rounded border-0 bg-transparent p-0"
          />
          <span className="font-mono text-xs" style={{ color: 'var(--text-muted)' }}>{primaryColor}</span>
        </div>
      </section>

      <section className="border-t pt-6" style={{ borderColor: 'var(--border)' }}>
        <SectionLabel>Editor</SectionLabel>
        <label className="mb-2 block text-sm" style={{ color: 'var(--text-primary)' }}>Autosave</label>
        <select
          value={autosaveMode}
          onChange={(e) => setAutosaveMode(e.target.value as AutosaveMode)}
          aria-label="Autosave"
          className="w-full rounded-lg px-3 py-2 text-sm"
          style={inputStyle}
        >
          {autosaveOptions.map((option) => (
            <option key={option.value} value={option.value}>{option.label}</option>
          ))}
        </select>
        <p className="mt-2 text-xs" style={{ color: 'var(--text-muted)' }}>
          Choose whether vault edits save after each change, on an interval, or only when saved manually.
        </p>
      </section>

      {showInstall && (
        <section className="border-t pt-6" style={{ borderColor: 'var(--border)' }}>
          <SectionLabel>Install app</SectionLabel>
          <PwaInstallButton className="w-full" />
        </section>
      )}
    </div>
  );
}

// ─── Help ────────────────────────────────────────────────────────────────────

function SupportTab() {
  return (
    <div className="space-y-6">
      <section>
        <SectionLabel>Getting help</SectionLabel>
        <p className="text-sm leading-6" style={{ color: 'var(--text-primary)' }}>
          This is a self-hosted MindMapVault. Whoever runs this server is your first stop for
          anything about your account — sign-ups, invites, storage limits, or a machine that is
          full.
        </p>
        <p className="mt-3 text-sm leading-6" style={{ color: 'var(--text-secondary)' }}>
          For documentation, bug reports, and pull requests, use the public repository. Product
          questions and feedback about MindMapVault itself can be emailed to the maintainer.
        </p>
      </section>

      <section className="border-t pt-6" style={{ borderColor: 'var(--border)' }}>
        <SectionLabel>Links</SectionLabel>
        <a
          href="https://github.com/mindmapvault/mindmapvault-server"
          target="_blank"
          rel="noreferrer"
          className="inline-flex w-full items-center justify-center rounded-lg px-3 py-2 text-sm font-medium transition"
          style={{ background: 'var(--surface-2)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)' }}
        >
          Open project repository
        </a>
        <a
          href="https://www.mindmapvault.com/homelab/"
          target="_blank"
          rel="noreferrer"
          className="mt-3 inline-flex w-full items-center justify-center rounded-lg px-3 py-2 text-sm font-medium transition"
          style={{ background: 'transparent', color: 'var(--text-primary)', border: '1px solid var(--border-light)' }}
        >
          Self-hosting guide
        </a>
        <a
          href="mailto:admin@mindmapvault.com"
          className="mt-3 inline-flex w-full items-center justify-center rounded-lg px-3 py-2 text-sm font-medium transition"
          style={{ background: 'transparent', color: 'var(--text-primary)', border: '1px solid var(--border-light)' }}
        >
          Email admin@mindmapvault.com
        </a>
      </section>
    </div>
  );
}

export default SettingsModal;
