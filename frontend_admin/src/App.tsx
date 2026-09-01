import { FormEvent, useDeferredValue, useEffect, useMemo, useState } from 'react';

// This console is for someone running MindMapVault on their own hardware — a
// box in a cupboard, a VPS, a NAS — usually for themselves and a handful of
// people they know. It answers four questions, in the order they get asked:
// is it working, who is on it, what are the rules, and what needs tidying.
//
// It is deliberately not a business dashboard. There are no plans, no billing
// and no revenue here, because this build has none of those things.

type DependencyHealth = {
  reachable: boolean;
  latency_ms: number;
  detail?: string | null;
};

type PurgeStatus = {
  last_run_at?: string | null;
  last_cleared: number;
  last_error?: string | null;
};

type StatusWarning = {
  code: string;
  title: string;
  detail: string;
};

type DiskUsage = {
  path: string;
  total_bytes: number;
  available_bytes: number;
  used_bytes: number;
};

type AdminStatus = {
  generated_at: string;
  version: string;
  started_at: string;
  uptime_seconds: number;
  server: {
    memory_bytes?: number | null;
    disk?: DiskUsage | null;
    disk_used_percent?: number | null;
  };
  database: DependencyHealth;
  database_stats?: { version?: string | null; size_bytes?: number | null } | null;
  object_storage: DependencyHealth;
  storage_bucket: string;
  bucket_stats?: { object_count: number; size_bytes: number; truncated: boolean } | null;
  totals: {
    accounts: number;
    locked_accounts: number;
    vaults: number;
    stored_bytes: number;
    open_invites: number;
  };
  purge: PurgeStatus;
  warnings: StatusWarning[];
};

type AdminUser = {
  id: string;
  username: string;
  created_at: string;
  first_name?: string | null;
  last_name?: string | null;
  email?: string | null;
  is_locked: boolean;
  locked_reason?: string | null;
  admin_note?: string | null;
  vault_count: number;
  used_bytes: number;
  storage_limit_bytes?: number | null;
};

type AdminAuditEvent = {
  public_id: string;
  entity_type: string;
  entity_id: string;
  action_type: string;
  summary: string;
  detail?: string | null;
  actor?: string | null;
  created_at: string;
};

type AdminOverview = {
  generated_at: string;
  metrics: {
    total_users: number;
    locked_users: number;
    total_vaults: number;
    total_used_bytes: number;
  };
  users: AdminUser[];
  audit_events: AdminAuditEvent[];
};

type InstanceSettings = {
  registration_enabled: boolean;
  user_storage_limit_bytes: number;
  max_attachment_size_bytes: number;
  auth_rate_limit_per_minute: number;
  failed_login_threshold: number;
  failed_login_lockout_minutes: number;
  trust_proxy_headers: boolean;
  updated_at: string;
};

type AdminSettingsResponse = {
  settings: InstanceSettings;
  observed_client_address: string;
  forwarded_header_present: boolean;
  max_upload_body_bytes: number;
};

type Invite = {
  id: string;
  code: string;
  label?: string | null;
  created_at: string;
  expires_at?: string | null;
  used_at?: string | null;
  used_by_username?: string | null;
  status: 'open' | 'used' | 'expired';
};

type InvitesResponse = {
  invites: Invite[];
  invites_required: boolean;
  register_url: string;
};

type ThemeChoice = 'system' | 'light' | 'dark';
type AdminView = 'status' | 'people' | 'settings' | 'maintenance';
type AccountFilter = 'all' | 'active' | 'locked';
type UserSort = 'created_desc' | 'storage_desc' | 'vaults_desc' | 'username_asc';

const ADMIN_TOKEN_KEY = 'mindmapvault-admin-token';
const THEME_KEY = 'mindmapvault-admin-theme';
const USERS_PAGE_SIZE = 12;
const MEGABYTE = 1024 * 1024;

const VIEW_META: Record<AdminView, { title: string; description: string; eyebrow: string }> = {
  status: {
    title: 'System status',
    description:
      'What it is running, whether the database and file storage are answering, how much is stored, and anything about the current setup worth knowing before it bites you.',
    eyebrow: 'Status',
  },
  people: {
    title: 'Users and access',
    description:
      'Every account on this server, what it is using, and the invite codes you hand out. Locking an account keeps its data and blocks sign-in; deleting removes the account and every vault in it.',
    eyebrow: 'People',
  },
  settings: {
    title: 'Server settings',
    description:
      'Who may sign up, how much each account may store, and how hard someone may hammer the sign-in form. Changes apply immediately — nothing needs restarting.',
    eyebrow: 'Settings',
  },
  maintenance: {
    title: 'Maintenance',
    description:
      'The cleanup job, what to back up, and a log of every change made from this console.',
    eyebrow: 'Maintenance',
  },
};

const NAV_ITEMS: Array<{ id: AdminView; label: string; caption: string }> = [
  { id: 'status', label: 'Status', caption: 'Health, version, and what is stored' },
  { id: 'people', label: 'People', caption: 'Accounts and invite codes' },
  { id: 'settings', label: 'Settings', caption: 'Sign-ups, limits, and throttling' },
  { id: 'maintenance', label: 'Maintenance', caption: 'Cleanup, backups, and the log' },
];

class AdminRequestError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'AdminRequestError';
    this.status = status;
  }
}

// Theme. "system" follows the operating system; the other two override it.
// Stored per browser, so a shared machine does not impose one operator's
// choice on the next. Storage can throw outright in a locked-down browser, so
// every access is guarded and simply falls back to following the system.
function readThemeChoice(): ThemeChoice {
  try {
    const saved = localStorage.getItem(THEME_KEY);
    if (saved === 'light' || saved === 'dark' || saved === 'system') {
      return saved;
    }
  } catch {
    /* storage unavailable */
  }
  return 'system';
}

function applyThemeChoice(choice: ThemeChoice) {
  const root = document.documentElement;
  if (choice === 'system') {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = choice;
  }

  try {
    localStorage.setItem(THEME_KEY, choice);
  } catch {
    /* the theme still applies for this page view */
  }
}

function getApiBase() {
  const configured = import.meta.env.VITE_ADMIN_API_BASE;
  if (typeof configured === 'string' && configured.trim()) {
    return configured.trim().replace(/\/$/, '');
  }

  // Same origin by default. The server serves this console and the API from
  // one process, and the dev server proxies /api to whatever VITE_BACKEND_URL
  // points at, so a relative base is right in both.
  //
  // It used to fall back to a hardcoded localhost:8090 and then to the hosted
  // SaaS API, which meant the console was inert on every self-hosted install
  // reached by its real hostname — it was asking a domain the operator does
  // not run for their users.
  return '/api';
}

function formatDate(value?: string | null) {
  if (!value) {
    return '—';
  }

  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return '—';
  }

  return parsed.toLocaleString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

// How long ago, in the words someone would actually use.
function formatAgo(value?: string | null) {
  if (!value) {
    return 'never';
  }

  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return 'never';
  }

  const seconds = Math.max(0, Math.round((Date.now() - parsed.getTime()) / 1000));
  if (seconds < 60) {
    return 'just now';
  }
  if (seconds < 3600) {
    const minutes = Math.round(seconds / 60);
    return `${minutes} minute${minutes === 1 ? '' : 's'} ago`;
  }
  if (seconds < 86400) {
    const hours = Math.round(seconds / 3600);
    return `${hours} hour${hours === 1 ? '' : 's'} ago`;
  }
  const days = Math.round(seconds / 86400);
  return `${days} day${days === 1 ? '' : 's'} ago`;
}

function formatUptime(seconds: number) {
  if (seconds < 60) {
    return `${Math.max(0, Math.round(seconds))}s`;
  }

  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);

  if (days > 0) {
    return `${days}d ${hours}h`;
  }
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return '0 B';
  }

  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }

  return `${size < 10 && unit > 0 ? size.toFixed(1) : Math.round(size)} ${units[unit]}`;
}

// Byte caps are entered in MB, which is how an operator thinks about them, and
// stored as bytes. Zero means unlimited on both sides of the conversion.
function bytesToMegabytesInput(value: number) {
  if (!value) {
    return '0';
  }
  const megabytes = value / MEGABYTE;
  return Number.isInteger(megabytes) ? String(megabytes) : megabytes.toFixed(2);
}

function megabytesInputToBytes(value: string) {
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    return 0;
  }
  return Math.round(parsed * MEGABYTE);
}

function normalizeQuery(value: string) {
  return value.trim().toLowerCase();
}

function matchesQuery(fields: Array<string | null | undefined>, query: string) {
  if (!query) {
    return true;
  }

  return fields.some((field) => (field ?? '').toLowerCase().includes(query));
}

function pageCount(total: number, pageSize: number) {
  return Math.max(1, Math.ceil(total / pageSize));
}

function paginate<T>(items: T[], page: number, pageSize: number) {
  const start = (page - 1) * pageSize;
  return items.slice(start, start + pageSize);
}

function compareUsers(left: AdminUser, right: AdminUser, sort: UserSort) {
  switch (sort) {
    case 'storage_desc':
      return right.used_bytes - left.used_bytes;
    case 'vaults_desc':
      return right.vault_count - left.vault_count;
    case 'username_asc':
      return left.username.localeCompare(right.username);
    case 'created_desc':
    default:
      return new Date(right.created_at).getTime() - new Date(left.created_at).getTime();
  }
}

// PostgreSQL reports its version with the packaging suffix attached
// ("16.15 (Debian 16.15-1.pgdg12+2)"). The number is the useful part.
function shortVersion(value: string) {
  return value.split(' ')[0];
}

// Thresholds match the ones the server warns at, so the colour and the warning
// on the same page never disagree.
function diskTone(percent: number) {
  if (percent >= 92) {
    return 'tone-danger';
  }
  if (percent >= 80) {
    return 'tone-warning';
  }
  return 'tone-positive';
}

function diskFill(percent: number) {
  if (percent >= 92) {
    return 'is-critical';
  }
  if (percent >= 80) {
    return 'is-warning';
  }
  return 'is-ok';
}

// How full an account is, as a percentage, when there is a limit to be full of.
function usagePercent(user: AdminUser) {
  if (!user.storage_limit_bytes || user.storage_limit_bytes <= 0) {
    return null;
  }
  return Math.min(100, Math.round((user.used_bytes / user.storage_limit_bytes) * 100));
}

export default function App() {
  const apiBase = useMemo(() => getApiBase(), []);
  const [activeView, setActiveView] = useState<AdminView>('status');
  const [tokenInput, setTokenInput] = useState('');
  const [token, setToken] = useState('');
  const [restoredToken, setRestoredToken] = useState('');
  const [status, setStatus] = useState<AdminStatus | null>(null);
  const [overview, setOverview] = useState<AdminOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const [userQuery, setUserQuery] = useState('');
  const [accountFilter, setAccountFilter] = useState<AccountFilter>('all');
  const [userSort, setUserSort] = useState<UserSort>('created_desc');
  const [userPage, setUserPage] = useState(1);
  const [selectedUserId, setSelectedUserId] = useState('');
  const [adminNoteDraft, setAdminNoteDraft] = useState('');
  const [lockedReasonDraft, setLockedReasonDraft] = useState('');
  const [activeUserActionId, setActiveUserActionId] = useState('');

  const [instance, setInstance] = useState<AdminSettingsResponse | null>(null);
  const [settingsDraft, setSettingsDraft] = useState<InstanceSettings | null>(null);
  const [settingsSaving, setSettingsSaving] = useState(false);
  const [settingsError, setSettingsError] = useState('');
  const [settingsNotice, setSettingsNotice] = useState('');

  const [invites, setInvites] = useState<InvitesResponse | null>(null);
  const [inviteLabel, setInviteLabel] = useState('');
  const [inviteExpiryDays, setInviteExpiryDays] = useState('14');
  const [inviteNeverExpires, setInviteNeverExpires] = useState(false);
  const [inviteBusy, setInviteBusy] = useState(false);
  const [inviteError, setInviteError] = useState('');
  const [copiedInviteId, setCopiedInviteId] = useState('');

  const [purgeRunning, setPurgeRunning] = useState(false);
  const [purgeNotice, setPurgeNotice] = useState('');
  const [theme, setTheme] = useState<ThemeChoice>(() => readThemeChoice());

  const deferredUserQuery = useDeferredValue(userQuery);

  useEffect(() => {
    applyThemeChoice(theme);
  }, [theme]);

  useEffect(() => {
    const saved = sessionStorage.getItem(ADMIN_TOKEN_KEY) ?? '';
    setTokenInput(saved);
    setRestoredToken(saved.trim());
  }, []);

  useEffect(() => {
    if (!restoredToken || token || loading) {
      return;
    }

    void authenticate(restoredToken, { persist: false, restored: true });
  }, [loading, restoredToken, token]);

  // Fetched the first time each screen is opened, then left alone so an
  // in-progress edit is never overwritten underneath the operator.
  useEffect(() => {
    if (!token) {
      return;
    }
    if (activeView === 'settings' && !instance) {
      void loadInstanceSettings(token);
    }
    if (activeView === 'people' && !invites) {
      void loadInvites(token);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeView, token, instance, invites]);

  function clearStoredSession() {
    sessionStorage.removeItem(ADMIN_TOKEN_KEY);
    setToken('');
    setRestoredToken('');
    setStatus(null);
    setOverview(null);
    setInstance(null);
    setSettingsDraft(null);
    setInvites(null);
  }

  async function requestAdmin<T>(path: string, activeToken: string, init?: RequestInit) {
    const headers = new Headers(init?.headers);
    headers.set('Authorization', `Bearer ${activeToken}`);
    if (init?.body && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json');
    }

    const response = await fetch(`${apiBase}${path}`, {
      ...init,
      headers: { ...Object.fromEntries(headers.entries()) },
    });

    const data = (await response.json().catch(() => ({}))) as T & { error?: string };
    if (!response.ok) {
      throw new AdminRequestError(data.error ?? 'The server refused that request', response.status);
    }

    return data;
  }

  async function loadSnapshot(activeToken: string) {
    const [nextStatus, nextOverview] = await Promise.all([
      requestAdmin<AdminStatus>('/admin/status', activeToken),
      requestAdmin<AdminOverview>('/admin/overview', activeToken),
    ]);

    return { nextStatus, nextOverview };
  }

  async function authenticate(activeToken: string, options: { persist: boolean; restored?: boolean }) {
    setLoading(true);
    setError('');

    try {
      const { nextStatus, nextOverview } = await loadSnapshot(activeToken);
      if (options.persist) {
        sessionStorage.setItem(ADMIN_TOKEN_KEY, activeToken);
      }
      setToken(activeToken);
      setRestoredToken('');
      setStatus(nextStatus);
      setOverview(nextOverview);
    } catch (err) {
      if (err instanceof AdminRequestError && err.status === 401) {
        clearStoredSession();
      }
      if (options.restored) {
        setError('That saved session is no longer valid. Enter the admin token again.');
      } else {
        setError(err instanceof Error ? err.message : 'Could not reach the server');
      }
    } finally {
      setLoading(false);
    }
  }

  async function refresh(activeToken: string) {
    setLoading(true);
    setError('');

    try {
      const { nextStatus, nextOverview } = await loadSnapshot(activeToken);
      setStatus(nextStatus);
      setOverview(nextOverview);
      // Invites go stale the moment someone uses one, and this is the only
      // button an operator will think to press. The settings form is left
      // alone on purpose — refetching it would discard an edit in progress.
      if (invites) {
        await loadInvites(activeToken);
      }
    } catch (err) {
      if (err instanceof AdminRequestError && err.status === 401) {
        clearStoredSession();
        setError('That session expired. Enter the admin token again.');
        return;
      }
      setError(err instanceof Error ? err.message : 'Could not reach the server');
    } finally {
      setLoading(false);
    }
  }

  function handleAuthSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = tokenInput.trim();
    if (!trimmed) {
      return;
    }
    void authenticate(trimmed, { persist: true });
  }

  function handleLogout() {
    clearStoredSession();
    setTokenInput('');
    setError('');
    setActiveView('status');
  }

  // ── Settings ───────────────────────────────────────────────────────────────

  async function loadInstanceSettings(activeToken: string) {
    setSettingsError('');
    try {
      const data = await requestAdmin<AdminSettingsResponse>('/admin/settings', activeToken);
      setInstance(data);
      setSettingsDraft(data.settings);
    } catch (err) {
      setSettingsError(err instanceof Error ? err.message : 'Could not read the settings');
    }
  }

  async function handleSaveSettings() {
    if (!settingsDraft || !token) {
      return;
    }

    setSettingsSaving(true);
    setSettingsError('');
    setSettingsNotice('');

    try {
      const data = await requestAdmin<AdminSettingsResponse>('/admin/settings', token, {
        method: 'POST',
        body: JSON.stringify({
          registration_enabled: settingsDraft.registration_enabled,
          user_storage_limit_bytes: settingsDraft.user_storage_limit_bytes,
          max_attachment_size_bytes: settingsDraft.max_attachment_size_bytes,
          auth_rate_limit_per_minute: settingsDraft.auth_rate_limit_per_minute,
          failed_login_threshold: settingsDraft.failed_login_threshold,
          failed_login_lockout_minutes: settingsDraft.failed_login_lockout_minutes,
          trust_proxy_headers: settingsDraft.trust_proxy_headers,
        }),
      });
      setInstance(data);
      setSettingsDraft(data.settings);
      setSettingsNotice('Saved. These rules are in effect now.');
      // The status warnings and the invite requirement both follow from these,
      // so pull them fresh rather than leaving stale advice on screen.
      setInvites(null);
      await refresh(token);
    } catch (err) {
      setSettingsError(err instanceof Error ? err.message : 'Could not save the settings');
    } finally {
      setSettingsSaving(false);
    }
  }

  function updateSettingsDraft(patch: Partial<InstanceSettings>) {
    setSettingsNotice('');
    setSettingsDraft((current) => (current ? { ...current, ...patch } : current));
  }

  // ── Invites ────────────────────────────────────────────────────────────────

  async function loadInvites(activeToken: string) {
    setInviteError('');
    try {
      setInvites(await requestAdmin<InvitesResponse>('/admin/invites', activeToken));
    } catch (err) {
      setInviteError(err instanceof Error ? err.message : 'Could not read the invite codes');
    }
  }

  async function handleCreateInvite(event: FormEvent) {
    event.preventDefault();
    if (!token) {
      return;
    }

    setInviteBusy(true);
    setInviteError('');

    try {
      const days = Number.parseInt(inviteExpiryDays, 10);
      const data = await requestAdmin<InvitesResponse>('/admin/invites', token, {
        method: 'POST',
        body: JSON.stringify({
          label: inviteLabel.trim() || null,
          expires_in_days: inviteNeverExpires || !Number.isFinite(days) ? null : days,
        }),
      });
      setInvites(data);
      setInviteLabel('');
      // The sidebar shows how many invites are open, on every screen.
      setStatus(await requestAdmin<AdminStatus>('/admin/status', token));
    } catch (err) {
      setInviteError(err instanceof Error ? err.message : 'Could not create an invite code');
    } finally {
      setInviteBusy(false);
    }
  }

  async function handleRevokeInvite(invite: Invite) {
    if (!token) {
      return;
    }
    const name = invite.label ? `the invite for ${invite.label}` : 'this invite';
    if (!window.confirm(`Revoke ${name}? Anyone holding the code will no longer be able to use it.`)) {
      return;
    }

    setInviteBusy(true);
    setInviteError('');
    try {
      setInvites(await requestAdmin<InvitesResponse>(`/admin/invites/${invite.id}`, token, { method: 'DELETE' }));
      setStatus(await requestAdmin<AdminStatus>('/admin/status', token));
    } catch (err) {
      setInviteError(err instanceof Error ? err.message : 'Could not revoke that invite');
    } finally {
      setInviteBusy(false);
    }
  }

  function inviteLink(invite: Invite) {
    if (!invites) {
      return invite.code;
    }
    return `${invites.register_url}?invite=${encodeURIComponent(invite.code)}`;
  }

  async function copyToClipboard(text: string, inviteId: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedInviteId(inviteId);
      window.setTimeout(() => setCopiedInviteId(''), 2000);
    } catch {
      // Clipboard access is blocked outside a secure context, which is exactly
      // where a home server on plain http lives. Say so instead of failing mute.
      setInviteError('This browser would not let the page copy. Select the link and copy it by hand.');
    }
  }

  // ── People ─────────────────────────────────────────────────────────────────

  const users = overview?.users ?? [];

  const filteredUsers = useMemo(() => {
    const query = normalizeQuery(deferredUserQuery);
    return users
      .filter((user) => {
        if (accountFilter === 'locked' && !user.is_locked) {
          return false;
        }
        if (accountFilter === 'active' && user.is_locked) {
          return false;
        }
        return matchesQuery(
          [user.username, user.email, user.first_name, user.last_name, user.admin_note],
          query,
        );
      })
      .sort((left, right) => compareUsers(left, right, userSort));
  }, [users, accountFilter, deferredUserQuery, userSort]);

  const totalUserPages = pageCount(filteredUsers.length, USERS_PAGE_SIZE);
  const safeUserPage = Math.min(userPage, totalUserPages);
  const pagedUsers = paginate(filteredUsers, safeUserPage, USERS_PAGE_SIZE);
  const selectedUser = users.find((user) => user.id === selectedUserId) ?? null;
  const selectedUserKey = selectedUser?.id ?? '';

  useEffect(() => {
    setUserPage(1);
  }, [accountFilter, deferredUserQuery, userSort]);

  useEffect(() => {
    if (!selectedUser) {
      setAdminNoteDraft('');
      setLockedReasonDraft('');
      return;
    }
    setAdminNoteDraft(selectedUser.admin_note ?? '');
    setLockedReasonDraft(selectedUser.locked_reason ?? '');
    // Keyed on the id alone: refetching the overview replaces the object, and
    // depending on it would wipe whatever the operator has typed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedUserKey]);

  async function runUserAction(user: AdminUser, path: string, body: unknown) {
    if (!token) {
      return;
    }

    setActiveUserActionId(user.id);
    setError('');
    try {
      const next = await requestAdmin<AdminOverview>(path, token, {
        method: 'POST',
        body: JSON.stringify(body),
      });
      setOverview(next);
      // Account changes move the numbers on the status page too.
      setStatus(await requestAdmin<AdminStatus>('/admin/status', token));
    } catch (err) {
      setError(err instanceof Error ? err.message : 'That action did not go through');
    } finally {
      setActiveUserActionId('');
    }
  }

  function handleToggleLock(user: AdminUser) {
    const locking = !user.is_locked;
    if (locking && !window.confirm(`Lock ${user.username}? They will not be able to sign in. Their vaults are untouched.`)) {
      return;
    }
    void runUserAction(user, `/admin/users/${user.id}/account-lock`, {
      locked: locking,
      reason: locking ? lockedReasonDraft.trim() || null : null,
    });
  }

  function handleSaveNote(user: AdminUser) {
    void runUserAction(user, `/admin/users/${user.id}/admin-details`, {
      admin_note: adminNoteDraft.trim() || null,
      locked_reason: lockedReasonDraft.trim() || null,
    });
  }

  function handleDeleteUser(user: AdminUser) {
    const typed = window.prompt(
      `Deleting ${user.username} removes the account and all ${user.vault_count} vault(s) in it, permanently. There is no undo and no backup taken.\n\nType the username to confirm:`,
    );
    if (typed !== user.username) {
      return;
    }
    void runUserAction(user, `/admin/users/${user.id}/delete-account`, { delete_all_data: true });
    setSelectedUserId('');
  }

  // ── Maintenance ────────────────────────────────────────────────────────────

  async function handleRunPurge() {
    if (!token) {
      return;
    }

    setPurgeRunning(true);
    setPurgeNotice('');
    setError('');
    try {
      const result = await requestAdmin<{ cleared: number; purge: PurgeStatus }>(
        '/admin/maintenance/purge-shares',
        token,
        { method: 'POST' },
      );
      setPurgeNotice(
        result.cleared === 0
          ? 'Nothing to clear — no expired or revoked shares were waiting.'
          : `Cleared ${result.cleared} expired share${result.cleared === 1 ? '' : 's'}.`,
      );
      await refresh(token);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'The cleanup could not run');
    } finally {
      setPurgeRunning(false);
    }
  }

  const hasSession = Boolean(token);
  const viewMeta = VIEW_META[activeView];

  return (
    <main className={hasSession ? 'admin-shell admin-shell-session' : 'admin-shell'}>
      <img src="/vault-mindmap-hero.svg" alt="" aria-hidden="true" draggable={false} className="hero-art" />

      {!hasSession ? (
        <section className="landing-shell">
          <section className="hero">
            <p className="eyebrow">MindMapVault server</p>
            <h1>MindMapVault server administration</h1>
            <p className="lede">
              Server health, user accounts, access rules and maintenance. Sign in with the value of{' '}
              <code>ADMIN_API_TOKEN</code> from this deployment's environment.
            </p>
          </section>

          <section className="auth-panel">
            <div>
              <p className="panel-label">Authentication</p>
              <strong>Admin token</strong>
              <p className="panel-help">
                It is kept in this browser tab only, and forgotten when you close it.
              </p>
            </div>

            <form className="auth-form" onSubmit={handleAuthSubmit}>
              <input
                type="password"
                value={tokenInput}
                onChange={(event) => setTokenInput(event.target.value)}
                placeholder="Admin bearer token"
                className="token-input"
              />
              <button type="submit" className="primary-button" disabled={!tokenInput.trim() || loading}>
                {loading ? 'Checking…' : 'Open console'}
              </button>
            </form>
          </section>

          {error && <p className="error-banner">{error}</p>}

          <div className="landing-footer">
              <div className="theme-toggle" role="group" aria-label="Colour theme">
                {(['system', 'light', 'dark'] as ThemeChoice[]).map((choice) => (
                  <button
                    key={choice}
                    type="button"
                    className={choice === theme ? 'theme-option is-active' : 'theme-option'}
                    onClick={() => setTheme(choice)}
                    aria-pressed={choice === theme}
                  >
                    {choice === 'system' && 'Auto'}
                    {choice === 'light' && 'Light'}
                    {choice === 'dark' && 'Dark'}
                  </button>
                ))}
              </div>
          </div>
        </section>
      ) : (
        <section className="control-plane">
          <aside className="sidepanel">
            <div className="sidepanel-brand">
              <p className="eyebrow">MindMapVault server</p>
              <h2>Admin console</h2>
              <p className="panel-help">
                {status ? `Version ${status.version} · up ${formatUptime(status.uptime_seconds)}` : 'Loading…'}
              </p>
            </div>

            <nav className="sidepanel-nav" aria-label="Console sections">
              {NAV_ITEMS.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  className={item.id === activeView ? 'nav-item is-active' : 'nav-item'}
                  onClick={() => setActiveView(item.id)}
                  aria-current={item.id === activeView ? 'page' : undefined}
                >
                  <span className="nav-badge" aria-hidden="true" />
                  <span className="nav-copy">
                    <strong>{item.label}</strong>
                    <span>{item.caption}</span>
                  </span>
                </button>
              ))}
            </nav>

            <div className="sidepanel-summary">
              <p className="panel-label">Right now</p>
              <div className="summary-stack">
                <article className="summary-card">
                  <span>Accounts</span>
                  <strong>{status?.totals.accounts ?? '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Vaults</span>
                  <strong>{status?.totals.vaults ?? '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Stored</span>
                  <strong>{status ? formatBytes(status.totals.stored_bytes) : '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Open invites</span>
                  <strong>{status?.totals.open_invites ?? '—'}</strong>
                </article>
              </div>
            </div>
          </aside>

          <section className="workspace-shell">
            <header className="workspace-topbar">
              <div>
                <p className="eyebrow">{viewMeta.eyebrow}</p>
                <h1>{viewMeta.title}</h1>
                <p className="lede workspace-lede">{viewMeta.description}</p>
              </div>
              <div className="topbar-controls">
                <div className="topbar-meta">
                  <div className="session-chip">
                    <span className="session-dot" aria-hidden="true" />
                    <span>{loading ? 'Checking…' : `Checked ${formatAgo(status?.generated_at)}`}</span>
                  </div>
                  <div className="theme-toggle" role="group" aria-label="Colour theme">
                {(['system', 'light', 'dark'] as ThemeChoice[]).map((choice) => (
                      <button
                    key={choice}
                    type="button"
                    className={choice === theme ? 'theme-option is-active' : 'theme-option'}
                    onClick={() => setTheme(choice)}
                    aria-pressed={choice === theme}
                  >
                    {choice === 'system' && 'Auto'}
                    {choice === 'light' && 'Light'}
                    {choice === 'dark' && 'Dark'}
                      </button>
                ))}
                  </div>
                </div>
                <div className="topbar-actions">
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => void refresh(token)}
                    disabled={loading}
                  >
                    {loading ? 'Checking…' : 'Check again'}
                  </button>
                  <button type="button" className="secondary-button" onClick={handleLogout}>
                    Sign out
                  </button>
                </div>
              </div>
            </header>

            {error && <p className="error-banner">{error}</p>}

            {/* ── Status ──────────────────────────────────────────────────── */}
            {activeView === 'status' && status && (
              <section className="view-stack">
                {status.warnings.length > 0 && (
                  <section className="panel">
                    <div className="panel-header panel-header-tight">
                      <div>
                        <p className="panel-label">Attention</p>
                        <h2>
                          {status.warnings.length === 1
                            ? '1 item needs attention'
                            : `${status.warnings.length} items need attention`}
                        </h2>
                      </div>
                    </div>
                    <div className="notice-stack">
                      {status.warnings.map((warning) => (
                        <article key={warning.code} className="notice-card">
                          <strong>{warning.title}</strong>
                          <p className="panel-help">{warning.detail}</p>
                          {(warning.code === 'open_and_unlimited'
                            || warning.code === 'registration_open'
                            || warning.code === 'proxy_header_ignored') && (
                            <button
                              type="button"
                              className="secondary-button action-button"
                              onClick={() => setActiveView('settings')}
                            >
                              Open Settings
                            </button>
                          )}
                        </article>
                      ))}
                    </div>
                  </section>
                )}

                {status.server.disk && (
                  <section className="panel">
                    <div className="panel-header panel-header-tight">
                      <div>
                        <p className="panel-label">Disk</p>
                        <h2>
                          {formatBytes(status.server.disk.available_bytes)} free of{' '}
                          {formatBytes(status.server.disk.total_bytes)}
                        </h2>
                      </div>
                      <span className={diskTone(status.server.disk_used_percent ?? 0)}>
                        {status.server.disk_used_percent}% used
                      </span>
                    </div>
                    <div className="usage-bar" role="img"
                      aria-label={`Disk ${status.server.disk_used_percent}% used`}>
                      <span
                        className={`usage-fill ${diskFill(status.server.disk_used_percent ?? 0)}`}
                        style={{ width: `${Math.max(2, status.server.disk_used_percent ?? 0)}%` }}
                      />
                    </div>
                    <p className="panel-help">
                      Measured on <code>{status.server.disk.path}</code> inside the server
                      container. With a normal Docker setup that is the same disk your database and
                      file-storage volumes sit on, so it is the number that runs out. If you put
                      those volumes on a different disk, check that one instead — set{' '}
                      <code>STATUS_DISK_PATH</code> to a path on it and this will follow.
                    </p>
                  </section>
                )}

                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Services</p>
                      <h2>Dependencies</h2>
                    </div>
                  </div>
                  <div className="health-grid">
                    <article className={status.database.reachable ? 'health-card is-ok' : 'health-card is-down'}>
                      <span className="health-dot" aria-hidden="true" />
                      <div>
                        <strong>Database</strong>
                        <p className="panel-help">
                          {status.database.reachable
                            ? `Answering in ${status.database.latency_ms} ms.`
                            : status.database.detail ?? 'Not answering.'}
                        </p>
                        {status.database.reachable && status.database_stats && (
                          <dl className="stat-list">
                            {status.database_stats.version && (
                              <div>
                                <dt>Version</dt>
                                <dd>PostgreSQL {shortVersion(status.database_stats.version)}</dd>
                              </div>
                            )}
                            {typeof status.database_stats.size_bytes === 'number' && (
                              <div>
                                <dt>On disk</dt>
                                <dd>{formatBytes(status.database_stats.size_bytes)}</dd>
                              </div>
                            )}
                          </dl>
                        )}
                        {!status.database.reachable && (
                          <p className="panel-help">
                            Nothing will work until this is back. Check that the PostgreSQL
                            container is running and that the server can reach it.
                          </p>
                        )}
                      </div>
                    </article>

                    <article className={status.object_storage.reachable ? 'health-card is-ok' : 'health-card is-down'}>
                      <span className="health-dot" aria-hidden="true" />
                      <div>
                        <strong>File storage</strong>
                        <p className="panel-help">
                          {status.object_storage.reachable
                            ? `Answering in ${status.object_storage.latency_ms} ms.`
                            : status.object_storage.detail ?? 'Not answering.'}
                        </p>
                        {status.object_storage.reachable && (
                          <dl className="stat-list">
                            <div>
                              <dt>Bucket</dt>
                              <dd>{status.storage_bucket}</dd>
                            </div>
                            {status.bucket_stats && (
                              <>
                                <div>
                                  <dt>Files</dt>
                                  <dd>
                                    {status.bucket_stats.object_count.toLocaleString()}
                                    {status.bucket_stats.truncated ? '+' : ''}
                                  </dd>
                                </div>
                                <div>
                                  <dt>Holding</dt>
                                  <dd>
                                    {status.bucket_stats.truncated ? 'at least ' : ''}
                                    {formatBytes(status.bucket_stats.size_bytes)}
                                  </dd>
                                </div>
                              </>
                            )}
                          </dl>
                        )}
                        {!status.object_storage.reachable && (
                          <p className="panel-help">
                            Maps and attachments live here. People can sign in but not open or save
                            anything. Check the Garage or MinIO container.
                          </p>
                        )}
                      </div>
                    </article>
                  </div>

                  {status.bucket_stats && !status.bucket_stats.truncated
                    && status.bucket_stats.size_bytes > status.totals.stored_bytes * 1.2
                    && status.totals.stored_bytes > 0 && (
                    <p className="panel-help">
                      The bucket holds {formatBytes(status.bucket_stats.size_bytes)} while the
                      accounts add up to {formatBytes(status.totals.stored_bytes)}. The difference is
                      older versions of maps, which are kept so people can roll back — each vault
                      keeps a set number and prunes the rest by itself. Revoked shares waiting for
                      the cleanup are in there too.
                    </p>
                  )}
                </section>

                <section className="metric-grid" aria-label="What is on this server">
                  <article className="metric-card metric-card-primary">
                    <p className="metric-label">Accounts</p>
                    <strong className="metric-value">{status.totals.accounts}</strong>
                    <p className="metric-detail">
                      {status.totals.locked_accounts > 0
                        ? `${status.totals.locked_accounts} locked out of signing in.`
                        : 'None locked.'}
                    </p>
                  </article>
                  <article className="metric-card metric-card-violet">
                    <p className="metric-label">Vaults</p>
                    <strong className="metric-value">{status.totals.vaults}</strong>
                    <p className="metric-detail">Encrypted mind maps and boards across all accounts.</p>
                  </article>
                  <article className="metric-card metric-card-sky">
                    <p className="metric-label">Stored</p>
                    <strong className="metric-value">{formatBytes(status.totals.stored_bytes)}</strong>
                    <p className="metric-detail">
                      Maps and their files. This is what is in the server, not what is free on the
                      host — check the disk itself for that.
                    </p>
                  </article>
                  <article className="metric-card metric-card-mint">
                    <p className="metric-label">Running</p>
                    <strong className="metric-value">{formatUptime(status.uptime_seconds)}</strong>
                    <p className="metric-detail">
                      Version {status.version}, started {formatDate(status.started_at)}.
                      {typeof status.server.memory_bytes === 'number'
                        && ` Using ${formatBytes(status.server.memory_bytes)} of memory.`}
                    </p>
                  </article>
                </section>

                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Cleanup</p>
                      <h2>Expired shares</h2>
                    </div>
                    <button
                      type="button"
                      className="secondary-button action-button"
                      onClick={() => setActiveView('maintenance')}
                    >
                      Maintenance
                    </button>
                  </div>
                  <p className="panel-help">
                    Share links that expired or were revoked have their encrypted copy deleted. This
                    runs once a day on its own.{' '}
                    {status.purge.last_run_at
                      ? `Last run ${formatAgo(status.purge.last_run_at)}, clearing ${status.purge.last_cleared}.`
                      : 'It has not run yet since this server started.'}
                  </p>
                </section>
              </section>
            )}

            {/* ── People ──────────────────────────────────────────────────── */}
            {activeView === 'people' && overview && (
              <section className="view-stack">
                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Invites</p>
                      <h2>Invite codes</h2>
                    </div>
                  </div>

                  <p className="panel-help">
                    {invites?.invites_required
                      ? 'Sign-ups are closed, so an invite code is the only way for someone to create an account here. Send them the link and the code is filled in for them.'
                      : 'Sign-ups are open, so anyone who can reach this server can create an account without a code. Codes still work, and become the only way in once you close sign-ups in Settings.'}
                  </p>

                  {inviteError && <p className="error-banner">{inviteError}</p>}

                  <form className="form-grid invite-form" onSubmit={handleCreateInvite}>
                    <label>
                      <span className="detail-label">Who is it for?</span>
                      <input
                        type="text"
                        className="detail-input"
                        value={inviteLabel}
                        onChange={(event) => setInviteLabel(event.target.value)}
                        placeholder="Anna, or the spare laptop"
                      />
                      <span className="panel-help field-help">
                        Just a reminder for you. It is never shown to whoever uses the code.
                      </span>
                    </label>
                    <label>
                      <span className="detail-label">Good for (days)</span>
                      <input
                        type="number"
                        min={1}
                        max={365}
                        className="detail-input number-input"
                        value={inviteExpiryDays}
                        onChange={(event) => setInviteExpiryDays(event.target.value)}
                        disabled={inviteNeverExpires}
                      />
                      <span className="detail-label switch-label field-help">
                        <input
                          type="checkbox"
                          checked={inviteNeverExpires}
                          onChange={(event) => setInviteNeverExpires(event.target.checked)}
                        />{' '}
                        Never expires
                      </span>
                    </label>
                    <div className="form-grid-span detail-actions">
                      <button type="submit" className="primary-button" disabled={inviteBusy}>
                        {inviteBusy ? 'Working…' : 'Create an invite'}
                      </button>
                    </div>
                  </form>

                  {invites && invites.invites.length > 0 ? (
                    <div className="table-wrap">
                      <table className="data-table">
                        <thead>
                          <tr>
                            <th>Code</th>
                            <th>For</th>
                            <th>State</th>
                            <th>Expires</th>
                            <th aria-label="Actions" />
                          </tr>
                        </thead>
                        <tbody>
                          {invites.invites.map((invite) => (
                            <tr key={invite.id}>
                              <td className="table-primary">
                                <code className="invite-code">{invite.code}</code>
                              </td>
                              <td>{invite.label || '—'}</td>
                              <td>
                                <span
                                  className={
                                    invite.status === 'open'
                                      ? 'tone-positive'
                                      : invite.status === 'used'
                                        ? 'tone-muted'
                                        : 'tone-danger'
                                  }
                                >
                                  {invite.status === 'open' && 'Ready to use'}
                                  {invite.status === 'used' && `Used by ${invite.used_by_username ?? 'someone'}`}
                                  {invite.status === 'expired' && 'Expired'}
                                </span>
                              </td>
                              <td>{invite.expires_at ? formatDate(invite.expires_at) : 'Never'}</td>
                              <td>
                                <div className="table-actions">
                                  {invite.status === 'open' && (
                                    <button
                                      type="button"
                                      className="secondary-button action-button"
                                      onClick={() => void copyToClipboard(inviteLink(invite), invite.id)}
                                    >
                                      {copiedInviteId === invite.id ? 'Copied' : 'Copy link'}
                                    </button>
                                  )}
                                  <button
                                    type="button"
                                    className="secondary-button danger-button action-button"
                                    onClick={() => void handleRevokeInvite(invite)}
                                    disabled={inviteBusy}
                                  >
                                    {invite.status === 'open' ? 'Revoke' : 'Remove'}
                                  </button>
                                </div>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  ) : (
                    <div className="empty-inline">
                      <p className="panel-label">No invite codes yet</p>
                      <p className="panel-help">Create one above when you want to let someone join.</p>
                    </div>
                  )}
                </section>

                <section className="panel panel-toolbar">
                  <div className="toolbar-copy">
                    <p className="panel-label">Accounts</p>
                    <h2>
                      {overview.metrics.total_users} account{overview.metrics.total_users === 1 ? '' : 's'}
                    </h2>
                  </div>
                  <div className="toolbar-actions toolbar-actions-wide">
                    <input
                      type="search"
                      className="search-input"
                      value={userQuery}
                      onChange={(event) => setUserQuery(event.target.value)}
                      placeholder="Search by name, email, or note"
                    />
                    <div className="filter-group">
                      {(['all', 'active', 'locked'] as AccountFilter[]).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={value === accountFilter ? 'filter-chip is-active' : 'filter-chip'}
                          onClick={() => setAccountFilter(value)}
                        >
                          {value === 'all' && 'Everyone'}
                          {value === 'active' && 'Can sign in'}
                          {value === 'locked' && 'Locked'}
                        </button>
                      ))}
                    </div>
                    <div className="select-wrap">
                      <select
                        className="select-input"
                        value={userSort}
                        onChange={(event) => setUserSort(event.target.value as UserSort)}
                      >
                        <option value="created_desc">Newest first</option>
                        <option value="storage_desc">Using the most</option>
                        <option value="vaults_desc">Most vaults</option>
                        <option value="username_asc">By name</option>
                      </select>
                    </div>
                  </div>
                </section>

                <section className="panel">
                  <div className="table-wrap">
                    <table className="data-table">
                      <thead>
                        <tr>
                          <th>Account</th>
                          <th>Joined</th>
                          <th>Vaults</th>
                          <th>Using</th>
                          <th>Can sign in</th>
                          <th aria-label="Actions" />
                        </tr>
                      </thead>
                      <tbody>
                        {pagedUsers.map((user) => {
                          const percent = usagePercent(user);
                          return (
                            <tr
                              key={user.id}
                              className={user.id === selectedUserId ? 'is-selected-row' : undefined}
                              onClick={() => setSelectedUserId(user.id)}
                            >
                              <td className="table-primary">
                                <strong>{user.username}</strong>
                                {user.email && <span>{user.email}</span>}
                              </td>
                              <td>{formatDate(user.created_at)}</td>
                              <td>{user.vault_count}</td>
                              <td>
                                {formatBytes(user.used_bytes)}
                                {percent !== null && <span> · {percent}% of the limit</span>}
                              </td>
                              <td>
                                <span className={user.is_locked ? 'tone-danger' : 'tone-positive'}>
                                  {user.is_locked ? 'Locked' : 'Yes'}
                                </span>
                              </td>
                              <td>
                                <div className="table-actions">
                                  <button
                                    type="button"
                                    className="secondary-button action-button"
                                    onClick={(event) => {
                                      event.stopPropagation();
                                      setSelectedUserId(user.id);
                                    }}
                                  >
                                    Manage
                                  </button>
                                </div>
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>

                  {filteredUsers.length === 0 && (
                    <div className="empty-inline">
                      <p className="panel-label">Nothing matches</p>
                      <p className="panel-help">Clear the search or the filter to see every account.</p>
                    </div>
                  )}

                  {totalUserPages > 1 && (
                    <div className="pagination-row">
                      <button
                        type="button"
                        className="secondary-button action-button"
                        onClick={() => setUserPage((page) => Math.max(1, page - 1))}
                        disabled={safeUserPage <= 1}
                      >
                        Previous
                      </button>
                      <span className="page-indicator">
                        Page {safeUserPage} of {totalUserPages}
                      </span>
                      <button
                        type="button"
                        className="secondary-button action-button"
                        onClick={() => setUserPage((page) => Math.min(totalUserPages, page + 1))}
                        disabled={safeUserPage >= totalUserPages}
                      >
                        Next
                      </button>
                    </div>
                  )}
                </section>

                {selectedUser && (
                  <section className="panel detail-panel">
                    <div className="panel-header panel-header-tight">
                      <div>
                        <p className="panel-label">Account</p>
                        <h2>{selectedUser.username}</h2>
                      </div>
                      <button
                        type="button"
                        className="secondary-button action-button"
                        onClick={() => setSelectedUserId('')}
                      >
                        Close
                      </button>
                    </div>

                    <div className="detail-list">
                      <span>Joined {formatDate(selectedUser.created_at)}</span>
                      <span>{selectedUser.vault_count} vault(s)</span>
                      <span>Using {formatBytes(selectedUser.used_bytes)}</span>
                      {selectedUser.storage_limit_bytes ? (
                        <span>Limit {formatBytes(selectedUser.storage_limit_bytes)}</span>
                      ) : (
                        <span>No storage limit set</span>
                      )}
                    </div>

                    <div className="detail-block">
                      <div className="form-grid">
                        <label className="form-grid-span">
                          <span className="detail-label">Your note</span>
                          <textarea
                            className="detail-input detail-textarea"
                            value={adminNoteDraft}
                            onChange={(event) => setAdminNoteDraft(event.target.value)}
                            placeholder="Anything you want to remember about this account"
                          />
                          <span className="panel-help field-help">
                            Only you see this. It is not shown to the account holder.
                          </span>
                        </label>
                        <label className="form-grid-span">
                          <span className="detail-label">Reason, if you lock them out</span>
                          <input
                            type="text"
                            className="detail-input"
                            value={lockedReasonDraft}
                            onChange={(event) => setLockedReasonDraft(event.target.value)}
                            placeholder="Left the household, suspected compromise…"
                          />
                        </label>
                      </div>
                      <div className="detail-actions">
                        <button
                          type="button"
                          className="primary-button"
                          onClick={() => handleSaveNote(selectedUser)}
                          disabled={activeUserActionId === selectedUser.id}
                        >
                          {activeUserActionId === selectedUser.id ? 'Saving…' : 'Save'}
                        </button>
                      </div>
                    </div>

                    <div className="detail-block">
                      <div className="panel-header panel-header-tight">
                        <div>
                          <p className="panel-label">Access</p>
                          <h2>
                            {selectedUser.is_locked
                              ? 'This account is locked'
                              : 'This account can sign in'}
                          </h2>
                        </div>
                      </div>
                      <p className="panel-help">
                        Locking blocks sign-in and leaves every vault exactly where it is, so it can
                        always be undone. Deleting cannot be.
                      </p>
                      <div className="detail-actions">
                        <button
                          type="button"
                          className="secondary-button"
                          onClick={() => handleToggleLock(selectedUser)}
                          disabled={activeUserActionId === selectedUser.id}
                        >
                          {selectedUser.is_locked ? 'Let them sign in again' : 'Lock this account'}
                        </button>
                        <button
                          type="button"
                          className="secondary-button danger-button"
                          onClick={() => handleDeleteUser(selectedUser)}
                          disabled={activeUserActionId === selectedUser.id}
                        >
                          Delete account and all its vaults
                        </button>
                      </div>
                      <p className="panel-help">
                        Note that you cannot reset anyone's password. Their password is what
                        decrypts their vaults and it never reaches this server — if they lose it,
                        the data is gone, and there is nothing this console can do about it.
                      </p>
                    </div>
                  </section>
                )}
              </section>
            )}

            {/* ── Settings ────────────────────────────────────────────────── */}
            {activeView === 'settings' && (
              <section className="view-stack">
                {settingsError && (
                  <section className="panel">
                    <p className="panel-label">Problem</p>
                    <p className="panel-help">{settingsError}</p>
                  </section>
                )}

                {!settingsDraft && !settingsError && (
                  <section className="panel">
                    <p className="panel-label">Loading</p>
                    <p className="panel-help">Reading the settings.</p>
                  </section>
                )}

                {settingsDraft && instance && (
                  <>
                    <section className="panel">
                      <div className="panel-header panel-header-tight">
                        <div>
                          <p className="panel-label">Access</p>
                          <h2>Registration</h2>
                        </div>
                      </div>
                      <div className="form-grid">
                        <label className="form-grid-span">
                          <span className="detail-label switch-label">
                            <input
                              type="checkbox"
                              checked={settingsDraft.registration_enabled}
                              onChange={(event) =>
                                updateSettingsDraft({ registration_enabled: event.target.checked })
                              }
                            />{' '}
                            Anyone can sign up
                          </span>
                          <span className="panel-help field-help">
                            {settingsDraft.registration_enabled
                              ? 'Anyone who can reach this server can create an account. Fine on a home network. If it is reachable from the internet, turn this off and hand out invite codes instead.'
                              : 'The sign-up form is closed. People join with an invite code from the People page. Existing accounts sign in as normal.'}
                          </span>
                        </label>
                      </div>
                    </section>

                    <section className="panel">
                      <div className="panel-header panel-header-tight">
                        <div>
                          <p className="panel-label">Storage</p>
                          <h2>Storage limits</h2>
                        </div>
                      </div>
                      <div className="form-grid">
                        <label>
                          <span className="detail-label">Per account (MB)</span>
                          <input
                            type="number"
                            min={0}
                            step={1}
                            className="detail-input number-input"
                            value={bytesToMegabytesInput(settingsDraft.user_storage_limit_bytes)}
                            onChange={(event) =>
                              updateSettingsDraft({
                                user_storage_limit_bytes: megabytesInputToBytes(event.target.value),
                              })
                            }
                          />
                          <span className="panel-help field-help">
                            0 means no limit, which is how the server has always behaved. An upload
                            that would cross the limit is refused before it is sent. Counts
                            attachments and shared copies — the mind maps themselves are small and
                            are not measured.
                          </span>
                        </label>
                        <label>
                          <span className="detail-label">Biggest single file (MB)</span>
                          <input
                            type="number"
                            min={0}
                            step={1}
                            className="detail-input number-input"
                            value={bytesToMegabytesInput(settingsDraft.max_attachment_size_bytes)}
                            onChange={(event) =>
                              updateSettingsDraft({
                                max_attachment_size_bytes: megabytesInputToBytes(event.target.value),
                              })
                            }
                          />
                          <span className="panel-help field-help">
                            0 means no limit beyond what the server will accept in one go, which is{' '}
                            {formatBytes(instance.max_upload_body_bytes)}. Setting this higher than
                            that changes nothing.
                          </span>
                        </label>
                      </div>
                    </section>

                    <section className="panel">
                      <div className="panel-header panel-header-tight">
                        <div>
                          <p className="panel-label">Sign-in</p>
                          <h2>Sign-in throttling</h2>
                        </div>
                      </div>
                      <div className="form-grid">
                        <label>
                          <span className="detail-label">Tries per minute, per address</span>
                          <input
                            type="number"
                            min={0}
                            step={1}
                            className="detail-input number-input"
                            value={settingsDraft.auth_rate_limit_per_minute}
                            onChange={(event) =>
                              updateSettingsDraft({
                                auth_rate_limit_per_minute: Number(event.target.value) || 0,
                              })
                            }
                          />
                          <span className="panel-help field-help">
                            Covers signing in, signing up and the salt lookup. One normal sign-in
                            costs two. 0 turns it off.
                          </span>
                        </label>
                        <label>
                          <span className="detail-label">Wrong passwords before lockout</span>
                          <input
                            type="number"
                            min={0}
                            step={1}
                            className="detail-input number-input"
                            value={settingsDraft.failed_login_threshold}
                            onChange={(event) =>
                              updateSettingsDraft({
                                failed_login_threshold: Number(event.target.value) || 0,
                              })
                            }
                          />
                          <span className="panel-help field-help">
                            Counted per username. 0 turns it off. Be aware that anyone who knows a
                            username can lock it deliberately by failing on purpose, which is why
                            the lockout lifts by itself.
                          </span>
                        </label>
                        <label>
                          <span className="detail-label">Locked out for (minutes)</span>
                          <input
                            type="number"
                            min={1}
                            max={1440}
                            step={1}
                            className="detail-input number-input"
                            value={settingsDraft.failed_login_lockout_minutes}
                            onChange={(event) =>
                              updateSettingsDraft({
                                failed_login_lockout_minutes: Number(event.target.value) || 1,
                              })
                            }
                          />
                          <span className="panel-help field-help">
                            Between 1 and 1440. These counters live in memory, so restarting the
                            server clears them all.
                          </span>
                        </label>
                        <label className="form-grid-span">
                          <span className="detail-label switch-label">
                            <input
                              type="checkbox"
                              checked={settingsDraft.trust_proxy_headers}
                              onChange={(event) =>
                                updateSettingsDraft({ trust_proxy_headers: event.target.checked })
                              }
                            />{' '}
                            There is a reverse proxy in front of this server
                          </span>
                          <span className="panel-help field-help">
                            Turn this on if traffic reaches the server through nginx, Caddy, Traefik
                            or similar, so it counts each visitor separately instead of treating the
                            whole internet as one. Leave it off otherwise — a visitor could then
                            claim any address they liked and walk around the limits.{' '}
                            <strong>
                              Right now this server thinks you are at {instance.observed_client_address}
                            </strong>
                            {instance.forwarded_header_present
                              ? ', and your request did come through a proxy.'
                              : ', and your request did not come through a proxy.'}{' '}
                            If that is not the machine you are sitting at, this setting is wrong.
                          </span>
                        </label>
                      </div>
                    </section>

                    <section className="panel">
                      <div className="detail-actions">
                        <button
                          type="button"
                          className="primary-button"
                          onClick={handleSaveSettings}
                          disabled={settingsSaving}
                        >
                          {settingsSaving ? 'Saving…' : 'Save'}
                        </button>
                        <button
                          type="button"
                          className="secondary-button"
                          onClick={() => {
                            setSettingsNotice('');
                            setSettingsDraft(instance.settings);
                          }}
                          disabled={settingsSaving}
                        >
                          Undo my changes
                        </button>
                      </div>
                      <div className="detail-list">
                        <span>Last changed {formatAgo(instance.settings.updated_at)}</span>
                        {settingsNotice && <span className="tone-positive">{settingsNotice}</span>}
                      </div>
                    </section>
                  </>
                )}
              </section>
            )}

            {/* ── Maintenance ─────────────────────────────────────────────── */}
            {activeView === 'maintenance' && status && overview && (
              <section className="view-stack">
                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Cleanup</p>
                      <h2>Expired share cleanup</h2>
                    </div>
                  </div>
                  <p className="panel-help">
                    When a share link expires or is revoked, the encrypted copy it served is deleted
                    and the space comes back. This happens once a day by itself; run it now if you
                    have just revoked something and want the space back immediately.
                  </p>
                  <div className="detail-list">
                    <span>
                      {status.purge.last_run_at
                        ? `Last run ${formatAgo(status.purge.last_run_at)}`
                        : 'Not run yet since this server started'}
                    </span>
                    <span>Cleared {status.purge.last_cleared} last time</span>
                    {status.purge.last_error && <span className="tone-danger">{status.purge.last_error}</span>}
                    {purgeNotice && <span className="tone-positive">{purgeNotice}</span>}
                  </div>
                  <div className="detail-actions">
                    <button
                      type="button"
                      className="primary-button"
                      onClick={() => void handleRunPurge()}
                      disabled={purgeRunning}
                    >
                      {purgeRunning ? 'Running…' : 'Run it now'}
                    </button>
                  </div>
                </section>

                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Backups</p>
                      <h2>Backups</h2>
                    </div>
                  </div>
                  <p className="panel-help">
                    This console cannot take a backup for you — the data lives in volumes outside
                    it. Three things matter, and they have to be backed up together, because a
                    database from one moment and object storage from another will not line up:
                  </p>
                  <ul className="checklist">
                    <li>
                      <strong>The database volume</strong> — accounts, vault records, shares and
                      these settings. A <code>pg_dump</code> works too.
                    </li>
                    <li>
                      <strong>The object storage volumes</strong> — the encrypted maps and
                      attachments themselves. Both the data and the metadata volume.
                    </li>
                    <li>
                      <strong>Your env file</strong> — in particular <code>JWT_SECRET</code>. Restore
                      the data with a different one and everyone is signed out.
                    </li>
                  </ul>
                  <p className="panel-help">
                    Worth knowing: nothing here can decrypt anyone's vaults, and neither can a
                    backup on its own. The data is encrypted with keys derived from each person's
                    password. That also means a lost password is unrecoverable — there is no reset.
                  </p>
                </section>

                <section className="panel">
                  <div className="panel-header panel-header-tight">
                    <div>
                      <p className="panel-label">Log</p>
                      <h2>Activity log</h2>
                    </div>
                  </div>
                  {overview.audit_events.length > 0 ? (
                    <div className="audit-list">
                      {overview.audit_events.map((event) => (
                        <article key={event.public_id} className="audit-item">
                          <div className="audit-row">
                            <strong>{event.summary}</strong>
                            <span>{formatAgo(event.created_at)}</span>
                          </div>
                          {event.detail && <p className="panel-help">{event.detail}</p>}
                        </article>
                      ))}
                    </div>
                  ) : (
                    <div className="empty-inline">
                      <p className="panel-label">Nothing yet</p>
                      <p className="panel-help">
                        Locking an account, changing a setting or creating an invite will show up
                        here.
                      </p>
                    </div>
                  )}
                </section>
              </section>
            )}

            {hasSession && !status && !loading && !error && (
              <section className="empty-dashboard">
                <p className="panel-label">Waiting for data</p>
                <h2>Signed in, but nothing has come back from the server yet.</h2>
                <p className="panel-help">Use “Check again” to try once more.</p>
              </section>
            )}
          </section>
        </section>
      )}
    </main>
  );
}
