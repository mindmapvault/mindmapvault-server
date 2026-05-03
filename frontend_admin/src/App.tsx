import { FormEvent, useDeferredValue, useEffect, useMemo, useState } from 'react';

type AdminMetrics = {
  total_users: number;
  free_users: number;
  paid_users: number;
  locked_users: number;
  active_subscriptions: number;
  total_vaults: number;
  total_used_bytes: number;
  feedback_count: number;
  archived_feedback_count: number;
};

type AdminUser = {
  id: string;
  username: string;
  created_at: string;
  subscription_tier: string;
  effective_subscription_tier: string;
  plan_source: string;
  stripe_customer_id?: string | null;
  stripe_subscription_id?: string | null;
  stripe_subscription_status?: string | null;
  subscription_current_period_end?: string | null;
  first_name?: string | null;
  last_name?: string | null;
  email?: string | null;
  is_locked: boolean;
  locked_reason?: string | null;
  admin_note?: string | null;
  manual_subscription_tier?: string | null;
  manual_subscription_expires_at?: string | null;
  manual_subscription_reason?: string | null;
  manual_subscription_granted_by?: string | null;
  access_grants: UserAccessGrant[];
  vault_count: number;
  used_bytes: number;
  storage_limit_bytes: number;
};

type UserAccessGrant = {
  subscription_mode: string;
  ui_surface: string;
  plan: string;
  source: string;
  granted_at: string;
  expires_at?: string | null;
  note?: string | null;
};

type AccessGrantDraft = {
  subscription_mode: string;
  ui_surface: string;
  plan: string;
  source: string;
  granted_at: string;
  expires_at: string;
  note: string;
};

type AdminFeedback = {
  public_id: string;
  name?: string | null;
  email?: string | null;
  subject: string;
  message: string;
  page_url?: string | null;
  created_at: string;
  is_archived: boolean;
  archived_at?: string | null;
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
  metrics: AdminMetrics;
  users: AdminUser[];
  feedback: AdminFeedback[];
  audit_events: AdminAuditEvent[];
};

type AdminView = 'overview' | 'users' | 'feedback';
type PlanFilter = 'all' | 'paid' | 'free';
type AccessFilter = 'all' | 'open' | 'locked';
type PlanSourceFilter = 'all' | 'admin_override' | 'stripe' | 'base';
type UserSort = 'created_desc' | 'storage_desc' | 'vaults_desc' | 'username_asc';
type FeedbackFilter = 'all' | 'active' | 'archived';

const ADMIN_TOKEN_KEY = 'mindmapvault-admin-token';
const USERS_PAGE_SIZE = 12;
const FEEDBACK_PAGE_SIZE = 12;
const SUBSCRIPTION_MODE_OPTIONS = ['private_encrypted', 'shared_plaintext', 'realtime_collaboration', 'kanban'] as const;
const UI_SURFACE_OPTIONS = ['encrypted_vault_app', 'shared_map_app', 'collaboration_app', 'kanban_app', 'admin_dashboard'] as const;
const ACCESS_PLAN_OPTIONS = ['free', 'paid'] as const;
const ACCESS_SOURCE_OPTIONS = ['legacy_base', 'stripe', 'admin_override', 'direct_grant'] as const;

const VIEW_META: Record<AdminView, { title: string; description: string; eyebrow: string }> = {
  overview: {
    title: 'Operations view for users, plans, support actions, and storage pressure.',
    description: 'Use the snapshot workspace to track growth, billing ownership, support load, and the latest admin interventions from one place.',
    eyebrow: 'Overview',
  },
  users: {
    title: 'User operations workspace for account, plan, and support management.',
    description: 'Search accounts, filter by billing source or access state, and apply manual paid overrides without colliding with Stripe-owned plan state.',
    eyebrow: 'Users',
  },
  feedback: {
    title: 'Feedback inbox for moderation and product signal cleanup.',
    description: 'Search the latest submissions, archive noise instead of deleting by default, and remove entries only when they should disappear completely.',
    eyebrow: 'Feedback',
  },
};

const NAV_ITEMS: Array<{ id: AdminView; label: string; caption: string }> = [
  { id: 'overview', label: 'Overview', caption: 'Metrics, storage, and timeline' },
  { id: 'users', label: 'Users', caption: 'Accounts, notes, and plan overrides' },
  { id: 'feedback', label: 'Feedback', caption: 'Archive, restore, and delete' },
];

class AdminRequestError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'AdminRequestError';
    this.status = status;
  }
}

function getApiBase() {
  const configured = import.meta.env.VITE_ADMIN_API_BASE;
  if (typeof configured === 'string' && configured.trim()) {
    return configured.trim().replace(/\/$/, '');
  }

  if (typeof window !== 'undefined') {
    const { hostname } = window.location;
    if (hostname === 'localhost' || hostname === '127.0.0.1') {
      return 'http://127.0.0.1:8090/api';
    }
  }

  return 'https://api.mindmapvault.com/api';
}

function formatDate(value?: string | null) {
  if (!value) return '—';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: 'short',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
}

function formatDateForInput(value?: string | null) {
  if (!value) {
    return '';
  }

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return '';
  }

  const pad = (input: number) => input.toString().padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function planLabel(user: AdminUser) {
  const effective = user.effective_subscription_tier === 'paid' ? 'Paid' : 'Free';
  const source = user.plan_source === 'admin_override' ? 'manual' : user.plan_source;
  const status = user.stripe_subscription_status?.trim();
  return status ? `${effective} · ${source} · ${status}` : `${effective} · ${source}`;
}

function formatPercent(value: number) {
  return `${Math.round(value)}%`;
}

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) {
    return '0 B';
  }

  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let amount = value;
  let unitIndex = 0;

  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }

  const digits = amount >= 10 || unitIndex === 0 ? 0 : 1;
  return `${amount.toFixed(digits)} ${units[unitIndex]}`;
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

function statusTone(user: AdminUser) {
  const status = user.stripe_subscription_status?.toLowerCase();
  if (status === 'active' || status === 'trialing') {
    return 'tone-positive';
  }
  if (user.plan_source === 'admin_override') {
    return 'tone-accent';
  }
  if (user.effective_subscription_tier === 'paid') {
    return 'tone-sky';
  }
  return 'tone-muted';
}

function accessTone(user: AdminUser) {
  return user.is_locked ? 'tone-danger' : 'tone-positive';
}

function feedbackTone(item: AdminFeedback) {
  return item.is_archived ? 'tone-muted' : 'tone-positive';
}

function capacityPercent(user: AdminUser) {
  if (!user.storage_limit_bytes || user.storage_limit_bytes <= 0) {
    return 0;
  }

  return Math.min(100, (user.used_bytes / user.storage_limit_bytes) * 100);
}

function pageCount(total: number, pageSize: number) {
  return Math.max(1, Math.ceil(total / pageSize));
}

function paginate<T>(items: T[], page: number, pageSize: number) {
  const start = (page - 1) * pageSize;
  return items.slice(start, start + pageSize);
}

function compareUsers(left: AdminUser, right: AdminUser, sort: UserSort) {
  if (sort === 'storage_desc') {
    return right.used_bytes - left.used_bytes || right.vault_count - left.vault_count;
  }
  if (sort === 'vaults_desc') {
    return right.vault_count - left.vault_count || right.used_bytes - left.used_bytes;
  }
  if (sort === 'username_asc') {
    return left.username.localeCompare(right.username);
  }
  return new Date(right.created_at).getTime() - new Date(left.created_at).getTime();
}

function labelFromSnakeCase(value: string) {
  return value
    .split('_')
    .map((chunk) => chunk.charAt(0).toUpperCase() + chunk.slice(1))
    .join(' ');
}

function grantToDraft(grant: UserAccessGrant): AccessGrantDraft {
  return {
    subscription_mode: grant.subscription_mode,
    ui_surface: grant.ui_surface,
    plan: grant.plan,
    source: grant.source,
    granted_at: formatDateForInput(grant.granted_at),
    expires_at: formatDateForInput(grant.expires_at),
    note: grant.note ?? '',
  };
}

function createEmptyGrantDraft(): AccessGrantDraft {
  return {
    subscription_mode: 'shared_plaintext',
    ui_surface: 'shared_map_app',
    plan: 'free',
    source: 'direct_grant',
    granted_at: formatDateForInput(new Date().toISOString()),
    expires_at: '',
    note: '',
  };
}

export default function App() {
  const apiBase = useMemo(() => getApiBase(), []);
  const [activeView, setActiveView] = useState<AdminView>('overview');
  const [tokenInput, setTokenInput] = useState('');
  const [token, setToken] = useState('');
  const [restoredToken, setRestoredToken] = useState('');
  const [overview, setOverview] = useState<AdminOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [userQuery, setUserQuery] = useState('');
  const [feedbackQuery, setFeedbackQuery] = useState('');
  const [planFilter, setPlanFilter] = useState<PlanFilter>('all');
  const [accessFilter, setAccessFilter] = useState<AccessFilter>('all');
  const [planSourceFilter, setPlanSourceFilter] = useState<PlanSourceFilter>('all');
  const [userSort, setUserSort] = useState<UserSort>('created_desc');
  const [feedbackFilter, setFeedbackFilter] = useState<FeedbackFilter>('all');
  const [userPage, setUserPage] = useState(1);
  const [feedbackPage, setFeedbackPage] = useState(1);
  const [selectedUserId, setSelectedUserId] = useState('');
  const [adminNoteDraft, setAdminNoteDraft] = useState('');
  const [lockedReasonDraft, setLockedReasonDraft] = useState('');
  const [planTierDraft, setPlanTierDraft] = useState('');
  const [planExpiryDraft, setPlanExpiryDraft] = useState('');
  const [planReasonDraft, setPlanReasonDraft] = useState('');
  const [accessGrantDrafts, setAccessGrantDrafts] = useState<AccessGrantDraft[]>([]);
  const [activeUserActionId, setActiveUserActionId] = useState('');
  const [activeFeedbackActionId, setActiveFeedbackActionId] = useState('');
  const deferredUserQuery = useDeferredValue(userQuery);
  const deferredFeedbackQuery = useDeferredValue(feedbackQuery);

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

  function clearStoredSession() {
    sessionStorage.removeItem(ADMIN_TOKEN_KEY);
    setToken('');
    setRestoredToken('');
    setOverview(null);
  }

  async function requestAdmin<T>(
    path: string,
    activeToken: string,
    init?: RequestInit,
  ) {
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
      const message = data.error ?? 'Failed to load admin overview';
      throw new AdminRequestError(message, response.status);
    }

    return data;
  }

  async function requestOverview(activeToken: string) {
    return requestAdmin<AdminOverview>('/admin/overview', activeToken);
  }

  async function authenticate(
    activeToken: string,
    options: { persist: boolean; restored?: boolean },
  ) {
    setLoading(true);
    setError('');

    try {
      const data = await requestOverview(activeToken);
      if (options.persist) {
        sessionStorage.setItem(ADMIN_TOKEN_KEY, activeToken);
      }
      setToken(activeToken);
      setRestoredToken('');
      setOverview(data);
    } catch (err) {
      const shouldClearSession = err instanceof AdminRequestError && err.status === 401;
      if (shouldClearSession) {
        clearStoredSession();
      }
      if (options.restored) {
        setError('Saved admin session is no longer valid. Enter the token again to continue.');
      } else {
        setError(err instanceof Error ? err.message : 'Failed to load admin overview');
      }
    } finally {
      setLoading(false);
    }
  }

  async function loadOverview(activeToken: string) {
    setLoading(true);
    setError('');

    try {
      const data = await requestOverview(activeToken);
      setOverview(data);
    } catch (err) {
      const shouldClearSession = err instanceof AdminRequestError && err.status === 401;
      if (shouldClearSession) {
        clearStoredSession();
      }
      setError(err instanceof Error ? err.message : 'Failed to load admin overview');
    } finally {
      setLoading(false);
    }
  }

  async function runUserAction(path: string, body: Record<string, unknown>) {
    setLoading(true);
    setError('');

    try {
      const data = await requestAdmin<AdminOverview>(path, token, {
        method: 'POST',
        body: JSON.stringify(body),
      });
      setOverview(data);
    } catch (err) {
      const shouldClearSession = err instanceof AdminRequestError && err.status === 401;
      if (shouldClearSession) {
        clearStoredSession();
      }
      setError(err instanceof Error ? err.message : 'Failed to run admin action');
    } finally {
      setLoading(false);
      setActiveUserActionId('');
    }
  }

  async function runFeedbackAction(path: string, body?: Record<string, unknown>) {
    setLoading(true);
    setError('');

    try {
      const data = await requestAdmin<AdminOverview>(path, token, {
        method: 'POST',
        body: body ? JSON.stringify(body) : undefined,
      });
      setOverview(data);
    } catch (err) {
      const shouldClearSession = err instanceof AdminRequestError && err.status === 401;
      if (shouldClearSession) {
        clearStoredSession();
      }
      setError(err instanceof Error ? err.message : 'Failed to run admin action');
    } finally {
      setLoading(false);
      setActiveFeedbackActionId('');
    }
  }

  const selectedUser = overview?.users.find((user) => user.id === selectedUserId) ?? null;

  useEffect(() => {
    if (!overview?.users.length) {
      setSelectedUserId('');
      return;
    }

    if (!selectedUserId || !overview.users.some((user) => user.id === selectedUserId)) {
      setSelectedUserId(overview.users[0].id);
    }
  }, [overview, selectedUserId]);

  useEffect(() => {
    if (!selectedUser) {
      setAdminNoteDraft('');
      setLockedReasonDraft('');
      setPlanTierDraft('');
      setPlanExpiryDraft('');
      setPlanReasonDraft('');
      setAccessGrantDrafts([]);
      return;
    }

    setAdminNoteDraft(selectedUser.admin_note ?? '');
    setLockedReasonDraft(selectedUser.locked_reason ?? '');
    setPlanTierDraft(selectedUser.manual_subscription_tier ?? '');
    setPlanExpiryDraft(formatDateForInput(selectedUser.manual_subscription_expires_at));
    setPlanReasonDraft(selectedUser.manual_subscription_reason ?? '');
    setAccessGrantDrafts(selectedUser.access_grants.map(grantToDraft));
  }, [selectedUser]);

  useEffect(() => {
    setUserPage(1);
  }, [deferredUserQuery, planFilter, accessFilter, planSourceFilter, userSort]);

  useEffect(() => {
    setFeedbackPage(1);
  }, [deferredFeedbackQuery, feedbackFilter]);

  function handleToggleUserLock(user: AdminUser) {
    setActiveUserActionId(user.id);
    void runUserAction(`/admin/users/${encodeURIComponent(user.id)}/account-lock`, {
      locked: !user.is_locked,
      reason: user.is_locked ? null : lockedReasonDraft || user.locked_reason || null,
    });
  }

  function handleDeleteUser(user: AdminUser) {
    const confirmed = window.confirm(
      `Delete ${user.username} and all stored vault data? This will remove the account and every encrypted blob version.`,
    );
    if (!confirmed) {
      return;
    }

    setActiveUserActionId(user.id);
    void runUserAction(`/admin/users/${encodeURIComponent(user.id)}/delete-account`, {
      delete_all_data: true,
    });
  }

  function handleSaveAdminDetails() {
    if (!selectedUser) {
      return;
    }

    setActiveUserActionId(selectedUser.id);
    void runUserAction(`/admin/users/${encodeURIComponent(selectedUser.id)}/admin-details`, {
      admin_note: adminNoteDraft || null,
      locked_reason: lockedReasonDraft || null,
    });
  }

  function handleSavePlanOverride() {
    if (!selectedUser) {
      return;
    }

    const expiresAt = planExpiryDraft ? new Date(planExpiryDraft).toISOString() : null;
    setActiveUserActionId(selectedUser.id);
    void runUserAction(`/admin/users/${encodeURIComponent(selectedUser.id)}/plan-override`, {
      manual_subscription_tier: planTierDraft || null,
      manual_subscription_expires_at: expiresAt,
      reason: planReasonDraft || null,
    });
  }

  function handleClearPlanOverride() {
    if (!selectedUser) {
      return;
    }

    setPlanTierDraft('');
    setPlanExpiryDraft('');
    setPlanReasonDraft('');
    setActiveUserActionId(selectedUser.id);
    void runUserAction(`/admin/users/${encodeURIComponent(selectedUser.id)}/plan-override`, {
      manual_subscription_tier: null,
      manual_subscription_expires_at: null,
      reason: null,
    });
  }

  function handleAddAccessGrant() {
    setAccessGrantDrafts((current) => [...current, createEmptyGrantDraft()]);
  }

  function handleAccessGrantDraftChange(index: number, field: keyof AccessGrantDraft, value: string) {
    setAccessGrantDrafts((current) =>
      current.map((draft, draftIndex) =>
        draftIndex === index
          ? {
              ...draft,
              [field]: value,
            }
          : draft,
      ),
    );
  }

  function handleRemoveAccessGrant(index: number) {
    setAccessGrantDrafts((current) => current.filter((_, draftIndex) => draftIndex !== index));
  }

  function handleSaveAccessGrants() {
    if (!selectedUser) {
      return;
    }

    const access_grants = accessGrantDrafts
      .filter((grant) => grant.subscription_mode && grant.ui_surface && grant.plan && grant.source && grant.granted_at)
      .map((grant) => ({
        subscription_mode: grant.subscription_mode,
        ui_surface: grant.ui_surface,
        plan: grant.plan,
        source: grant.source,
        granted_at: new Date(grant.granted_at).toISOString(),
        expires_at: grant.expires_at ? new Date(grant.expires_at).toISOString() : null,
        note: grant.note.trim() || null,
      }));

    setActiveUserActionId(selectedUser.id);
    void runUserAction(`/admin/users/${encodeURIComponent(selectedUser.id)}/access-grants`, {
      access_grants,
    });
  }

  function handleToggleFeedbackArchive(item: AdminFeedback) {
    setActiveFeedbackActionId(item.public_id);
    void runFeedbackAction(`/admin/feedback/${encodeURIComponent(item.public_id)}/archive`, {
      archived: !item.is_archived,
    });
  }

  function handleDeleteFeedback(item: AdminFeedback) {
    const confirmed = window.confirm(`Delete feedback "${item.subject}" from the admin inbox?`);
    if (!confirmed) {
      return;
    }

    setActiveFeedbackActionId(item.public_id);
    void runFeedbackAction(`/admin/feedback/${encodeURIComponent(item.public_id)}/delete`);
  }

  function handleAuthSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = tokenInput.trim();
    if (!trimmed) {
      return;
    }

    setActiveView('overview');
    void authenticate(trimmed, { persist: true });
  }

  function handleLogout() {
    clearStoredSession();
    setTokenInput('');
    setUserQuery('');
    setFeedbackQuery('');
    setPlanFilter('all');
    setAccessFilter('all');
    setPlanSourceFilter('all');
    setFeedbackFilter('all');
    setUserSort('created_desc');
    setSelectedUserId('');
    setActiveView('overview');
    setError('');
  }

  const metrics = overview?.metrics;
  const hasSession = Boolean(token);
  const totalUsers = metrics?.total_users ?? 0;
  const paidShare = totalUsers > 0 && metrics ? (metrics.paid_users / totalUsers) * 100 : 0;
  const feedbackPerUser = totalUsers > 0 && metrics ? metrics.feedback_count / totalUsers : 0;
  const usersWithEmail = overview?.users.filter((user) => Boolean(user.email?.trim())).length ?? 0;
  const reachableShare = totalUsers > 0 ? (usersWithEmail / totalUsers) * 100 : 0;
  const expiringSoonCount =
    overview?.users.filter((user) => {
      if (!user.subscription_current_period_end) {
        return false;
      }
      const timestamp = new Date(user.subscription_current_period_end).getTime();
      if (Number.isNaN(timestamp)) {
        return false;
      }
      const sevenDaysFromNow = Date.now() + 7 * 24 * 60 * 60 * 1000;
      return timestamp <= sevenDaysFromNow;
    }).length ?? 0;
  const activeFeedbackCount = overview?.feedback.filter((item) => !item.is_archived).length ?? 0;
  const manualOverrideCount = overview?.users.filter((user) => user.plan_source === 'admin_override').length ?? 0;
  const multiAccessUsersCount = overview?.users.filter((user) => user.access_grants.length > 1).length ?? 0;

  const filteredUsers = useMemo(() => {
    if (!overview) {
      return [];
    }

    const query = normalizeQuery(deferredUserQuery);
    return [...overview.users]
      .filter((user) => {
        const planMatches = planFilter === 'all' || user.effective_subscription_tier === planFilter;
        const accessMatches =
          accessFilter === 'all' ||
          (accessFilter === 'locked' && user.is_locked) ||
          (accessFilter === 'open' && !user.is_locked);
        const sourceMatches = planSourceFilter === 'all' || user.plan_source === planSourceFilter;
        if (!planMatches || !accessMatches || !sourceMatches) {
          return false;
        }

        return matchesQuery(
          [
            user.username,
            user.email,
            user.first_name,
            user.last_name,
            user.subscription_tier,
            user.effective_subscription_tier,
            user.stripe_subscription_status,
            user.admin_note,
            user.locked_reason,
            user.manual_subscription_reason,
            ...user.access_grants.map((grant) => `${grant.subscription_mode} ${grant.ui_surface} ${grant.plan} ${grant.source} ${grant.note ?? ''}`),
          ],
          query,
        );
      })
      .sort((left, right) => compareUsers(left, right, userSort));
  }, [accessFilter, deferredUserQuery, overview, planFilter, planSourceFilter, userSort]);

  const filteredUsedBytes = filteredUsers.reduce((total, user) => total + user.used_bytes, 0);
  const pagedUsers = paginate(filteredUsers, userPage, USERS_PAGE_SIZE);
  const userPages = pageCount(filteredUsers.length, USERS_PAGE_SIZE);

  const filteredFeedback = useMemo(() => {
    if (!overview) {
      return [];
    }

    const query = normalizeQuery(deferredFeedbackQuery);
    return overview.feedback.filter((item) => {
      const archiveMatches =
        feedbackFilter === 'all' ||
        (feedbackFilter === 'active' && !item.is_archived) ||
        (feedbackFilter === 'archived' && item.is_archived);
      if (!archiveMatches) {
        return false;
      }

      return matchesQuery([item.subject, item.message, item.name, item.email, item.page_url], query);
    });
  }, [deferredFeedbackQuery, feedbackFilter, overview]);

  const pagedFeedback = paginate(filteredFeedback, feedbackPage, FEEDBACK_PAGE_SIZE);
  const feedbackPages = pageCount(filteredFeedback.length, FEEDBACK_PAGE_SIZE);
  const topUsers = (overview?.users ?? []).slice().sort((left, right) => right.used_bytes - left.used_bytes).slice(0, 5);
  const viewMeta = VIEW_META[activeView];

  return (
    <main className={hasSession ? 'admin-shell admin-shell-session' : 'admin-shell'}>
      <img src="/vault-mindmap-hero.svg" alt="" aria-hidden="true" draggable={false} className="hero-art" />

      {!hasSession ? (
        <section className="landing-shell">
          <section className="hero">
            <p className="eyebrow">MindMapVault control plane</p>
            <h1>Secure admin workspace for support, billing, and usage oversight.</h1>
            <p className="lede">
              Unlock the control plane with the admin token, then work from a dedicated internal surface instead of stitching together production checks by hand.
            </p>
          </section>

          <section className="auth-panel">
            <div>
              <p className="panel-label">Access</p>
              <strong>Protected admin session</strong>
              <p className="panel-help">Enter the admin bearer token to unlock the control plane. It stays only in this browser session.</p>
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
                {loading ? 'Loading…' : 'Open dashboard'}
              </button>
            </form>
          </section>

          {error && <p className="error-banner">{error}</p>}
        </section>
      ) : (
        <section className="control-plane">
          <aside className="sidepanel">
            <div className="sidepanel-brand">
              <p className="eyebrow">MindMapVault control plane</p>
              <h2>Admin workspace</h2>
              <p className="panel-help">Switch between overview, users, and feedback while keeping support, billing, and moderation actions separate.</p>
            </div>

            <nav className="sidepanel-nav" aria-label="Admin sections">
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
              <p className="panel-label">Snapshot</p>
              <div className="summary-stack">
                <article className="summary-card">
                  <span>Total users</span>
                  <strong>{metrics?.total_users ?? '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Stored vaults</span>
                  <strong>{metrics?.total_vaults ?? '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Used capacity</span>
                  <strong>{metrics ? formatBytes(metrics.total_used_bytes) : '—'}</strong>
                </article>
                <article className="summary-card">
                  <span>Multi-surface</span>
                  <strong>{multiAccessUsersCount}</strong>
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
                <div className="session-chip">
                  <span className="session-dot" aria-hidden="true" />
                  <span>{loading ? 'Refreshing snapshot' : `Updated ${formatDate(overview?.generated_at)}`}</span>
                </div>
                <div className="topbar-actions">
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => void loadOverview(token)}
                    disabled={loading}
                  >
                    {loading ? 'Refreshing…' : 'Refresh data'}
                  </button>
                  <button type="button" className="secondary-button" onClick={handleLogout}>
                    End session
                  </button>
                </div>
              </div>
            </header>

            {error && <p className="error-banner">{error}</p>}

            {activeView === 'overview' && metrics && overview && (
              <>
                <section className="metric-grid" aria-label="admin metrics">
                  <article className="metric-card metric-card-primary">
                    <p className="metric-label">Registered users</p>
                    <strong className="metric-value">{metrics.total_users}</strong>
                    <p className="metric-detail">All cloud accounts currently stored in the selected backend.</p>
                  </article>
                  <article className="metric-card metric-card-violet">
                    <p className="metric-label">Stored vaults</p>
                    <strong className="metric-value">{metrics.total_vaults}</strong>
                    <p className="metric-detail">Encrypted vault records currently associated with registered users.</p>
                  </article>
                  <article className="metric-card metric-card-rose">
                    <p className="metric-label">Used capacity</p>
                    <strong className="metric-value">{formatBytes(metrics.total_used_bytes)}</strong>
                    <p className="metric-detail">Total encrypted blob storage currently consumed across all vault versions.</p>
                  </article>
                  <article className="metric-card metric-card-orange">
                    <p className="metric-label">Paid plans</p>
                    <strong className="metric-value">{metrics.paid_users}</strong>
                    <p className="metric-detail">Users whose effective plan currently resolves to paid.</p>
                  </article>
                  <article className="metric-card metric-card-sky">
                    <p className="metric-label">Active subscriptions</p>
                    <strong className="metric-value">{metrics.active_subscriptions}</strong>
                    <p className="metric-detail">Stripe subscriptions currently marked active or trialing.</p>
                  </article>
                  <article className="metric-card metric-card-mint">
                    <p className="metric-label">Feedback items</p>
                    <strong className="metric-value">{metrics.feedback_count}</strong>
                    <p className="metric-detail">Total stored feedback submissions from the public site.</p>
                  </article>
                  <article className="metric-card metric-card-slate">
                    <p className="metric-label">Archived feedback</p>
                    <strong className="metric-value">{metrics.archived_feedback_count}</strong>
                    <p className="metric-detail">Items hidden from the active inbox without being deleted.</p>
                  </article>
                  <article className="metric-card metric-card-deep">
                    <p className="metric-label">Manual overrides</p>
                    <strong className="metric-value">{manualOverrideCount}</strong>
                    <p className="metric-detail">Accounts currently resolving their plan from an admin override instead of Stripe or base state.</p>
                  </article>
                </section>

                <section className="insight-grid" aria-label="usage insights">
                  <article className="insight-card accent-indigo">
                    <p className="panel-label">Conversion</p>
                    <h2>{formatPercent(paidShare)}</h2>
                    <p className="panel-help">Paid share across all registered users.</p>
                  </article>
                  <article className="insight-card accent-sky">
                    <p className="panel-label">Reachability</p>
                    <h2>{formatPercent(reachableShare)}</h2>
                    <p className="panel-help">Users with an email address on file for direct follow-up.</p>
                  </article>
                  <article className="insight-card accent-mint">
                    <p className="panel-label">Feedback density</p>
                    <h2>{feedbackPerUser.toFixed(2)}</h2>
                    <p className="panel-help">Feedback submissions per registered user.</p>
                  </article>
                  <article className="insight-card accent-rose">
                    <p className="panel-label">Locked users</p>
                    <h2>{metrics.locked_users}</h2>
                    <p className="panel-help">Accounts currently prevented from starting new sessions.</p>
                  </article>
                </section>

                <section className="status-row status-row-wide">
                  <div className="notes-card">
                    <h2>Snapshot time</h2>
                    <p>{formatDate(overview.generated_at)}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Expiring in 7 days</h2>
                    <p>{expiringSoonCount}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Active feedback</h2>
                    <p>{activeFeedbackCount}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Locked accounts</h2>
                    <p>{metrics.locked_users}</p>
                  </div>
                </section>

                <section className="workspace-grid workspace-grid-extended">
                  <article className="workspace-card workspace-primary">
                    <div className="panel-header">
                      <div>
                        <p className="panel-label">Storage leaders</p>
                        <h2>Largest accounts right now</h2>
                      </div>
                    </div>
                    <div className="signal-list">
                      {topUsers.map((user) => (
                        <div key={user.id}>
                          <strong>{user.username}</strong>
                          <span>
                            {formatBytes(user.used_bytes)} across {user.vault_count} vault{user.vault_count === 1 ? '' : 's'}
                          </span>
                        </div>
                      ))}
                    </div>
                  </article>

                  <article className="workspace-card">
                    <div className="panel-header">
                      <div>
                        <p className="panel-label">Recent actions</p>
                        <h2>Admin timeline</h2>
                      </div>
                    </div>
                    <div className="audit-list">
                      {overview.audit_events.slice(0, 8).map((event) => (
                        <article key={event.public_id} className="audit-item">
                          <div className="audit-row">
                            <strong>{event.summary}</strong>
                            <span>{formatDate(event.created_at)}</span>
                          </div>
                          <p>{event.detail || `${event.entity_type} · ${event.action_type}`}</p>
                        </article>
                      ))}
                    </div>
                  </article>
                </section>
              </>
            )}

            {activeView === 'users' && overview && (
              <section className="page-stack">
                <section className="panel panel-toolbar">
                  <div className="toolbar-copy">
                    <p className="panel-label">User management</p>
                    <h2>Accounts, plans, billing source, and internal notes</h2>
                    <p className="panel-help">Filter by effective plan, lock state, or plan source, then use the detail panel for support notes and manual paid grants.</p>
                  </div>
                  <div className="toolbar-actions toolbar-actions-wide">
                    <input
                      type="search"
                      value={userQuery}
                      onChange={(event) => setUserQuery(event.target.value)}
                      placeholder="Search by username, name, email, status, or notes"
                      className="token-input search-input"
                    />
                    <div className="filter-group" role="group" aria-label="Plan filters">
                      {(['all', 'paid', 'free'] as PlanFilter[]).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={value === planFilter ? 'filter-chip is-active' : 'filter-chip'}
                          onClick={() => setPlanFilter(value)}
                        >
                          {value === 'all' ? 'All plans' : value === 'paid' ? 'Paid only' : 'Free only'}
                        </button>
                      ))}
                    </div>
                    <div className="filter-group" role="group" aria-label="Access filters">
                      {(['all', 'open', 'locked'] as AccessFilter[]).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={value === accessFilter ? 'filter-chip is-active' : 'filter-chip'}
                          onClick={() => setAccessFilter(value)}
                        >
                          {value === 'all' ? 'All access' : value === 'open' ? 'Open' : 'Locked'}
                        </button>
                      ))}
                    </div>
                    <div className="filter-group" role="group" aria-label="Plan source filters">
                      {(['all', 'admin_override', 'stripe', 'base'] as PlanSourceFilter[]).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={value === planSourceFilter ? 'filter-chip is-active' : 'filter-chip'}
                          onClick={() => setPlanSourceFilter(value)}
                        >
                          {value === 'all' ? 'All sources' : value === 'admin_override' ? 'Manual' : value}
                        </button>
                      ))}
                    </div>
                    <label className="select-wrap">
                      <span className="panel-label">Sort</span>
                      <select value={userSort} onChange={(event) => setUserSort(event.target.value as UserSort)} className="select-input">
                        <option value="created_desc">Newest first</option>
                        <option value="storage_desc">Highest storage</option>
                        <option value="vaults_desc">Most vaults</option>
                        <option value="username_asc">Username A-Z</option>
                      </select>
                    </label>
                  </div>
                </section>

                <section className="status-row compact-row status-row-wide">
                  <div className="notes-card">
                    <h2>Filtered users</h2>
                    <p>{filteredUsers.length}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Filtered storage</h2>
                    <p>{formatBytes(filteredUsedBytes)}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Multi-surface users</h2>
                    <p>{multiAccessUsersCount}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Locked users</h2>
                    <p>{metrics?.locked_users ?? 0}</p>
                  </div>
                </section>

                <section className="user-management-grid">
                  <section className="panel panel-wide">
                    <div className="panel-header">
                      <div>
                        <p className="panel-label">Accounts</p>
                        <h2>Registered accounts and plan status</h2>
                      </div>
                      <div className="pagination-row">
                        <button type="button" className="secondary-button action-button" onClick={() => setUserPage((page) => Math.max(1, page - 1))} disabled={userPage === 1}>
                          Previous
                        </button>
                        <span className="page-indicator">Page {userPage} of {userPages}</span>
                        <button type="button" className="secondary-button action-button" onClick={() => setUserPage((page) => Math.min(userPages, page + 1))} disabled={userPage >= userPages}>
                          Next
                        </button>
                      </div>
                    </div>

                    <div className="table-wrap">
                      <table className="data-table">
                        <thead>
                          <tr>
                            <th>User</th>
                            <th>Plan</th>
                            <th>Access</th>
                            <th>Vaults</th>
                            <th>Used capacity</th>
                            <th>Created</th>
                            <th>Email</th>
                            <th>Actions</th>
                          </tr>
                        </thead>
                        <tbody>
                          {pagedUsers.map((user) => (
                            <tr key={user.id || user.username} className={selectedUserId === user.id ? 'is-selected-row' : undefined}>
                              <td>
                                <div className="table-primary">
                                  <strong>{user.username}</strong>
                                  <span>{[user.first_name, user.last_name].filter(Boolean).join(' ') || 'No profile name'}</span>
                                </div>
                              </td>
                              <td>
                                <div className="plan-stack">
                                  <span className={statusTone(user)}>{planLabel(user)}</span>
                                  <span>{user.manual_subscription_reason || user.stripe_customer_id || 'No override reason'}</span>
                                </div>
                              </td>
                              <td>
                                <div className="plan-stack">
                                  <span className={accessTone(user)}>{user.is_locked ? 'Locked' : 'Open'}</span>
                                  <span>{user.locked_reason || 'No lock reason'}</span>
                                </div>
                              </td>
                              <td>{user.vault_count}</td>
                              <td>
                                <div className="capacity-stack">
                                  <strong>{formatBytes(user.used_bytes)}</strong>
                                  <span>{formatPercent(capacityPercent(user))} of {formatBytes(user.storage_limit_bytes)}</span>
                                </div>
                              </td>
                              <td>{formatDate(user.created_at)}</td>
                              <td>{user.email || '—'}</td>
                              <td>
                                <div className="table-actions">
                                  <button type="button" className="secondary-button action-button" onClick={() => setSelectedUserId(user.id)}>
                                    Manage
                                  </button>
                                  <button
                                    type="button"
                                    className="secondary-button action-button"
                                    onClick={() => handleToggleUserLock(user)}
                                    disabled={loading || activeUserActionId === user.id}
                                  >
                                    {activeUserActionId === user.id ? 'Working…' : user.is_locked ? 'Unlock' : 'Lock'}
                                  </button>
                                  <button
                                    type="button"
                                    className="secondary-button danger-button action-button"
                                    onClick={() => handleDeleteUser(user)}
                                    disabled={loading || activeUserActionId === user.id}
                                  >
                                    Delete data
                                  </button>
                                </div>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>

                    {filteredUsers.length === 0 && (
                      <div className="empty-inline">
                        <p className="panel-label">No matches</p>
                        <p className="panel-help">Adjust the search or filters to show accounts from the current snapshot.</p>
                      </div>
                    )}
                  </section>

                  <aside className="panel detail-panel">
                    <div className="panel-header">
                      <div>
                        <p className="panel-label">User detail</p>
                        <h2>{selectedUser?.username || 'Select an account'}</h2>
                      </div>
                    </div>

                    {selectedUser ? (
                      <div className="detail-stack">
                        <div className="detail-block">
                          <div className="detail-grid">
                            <div>
                              <span className="detail-label">Effective plan</span>
                              <strong>{selectedUser.effective_subscription_tier}</strong>
                            </div>
                            <div>
                              <span className="detail-label">Plan source</span>
                              <strong>{selectedUser.plan_source}</strong>
                            </div>
                            <div>
                              <span className="detail-label">Stripe status</span>
                              <strong>{selectedUser.stripe_subscription_status || '—'}</strong>
                            </div>
                            <div>
                              <span className="detail-label">Period end</span>
                              <strong>{formatDate(selectedUser.subscription_current_period_end)}</strong>
                            </div>
                          </div>
                        </div>

                        <div className="detail-block">
                          <div className="panel-header panel-header-tight">
                            <div>
                              <p className="panel-label">Support notes</p>
                              <h2>Admin details</h2>
                            </div>
                          </div>
                          <div className="form-grid">
                            <label>
                              <span className="detail-label">Admin note</span>
                              <textarea value={adminNoteDraft} onChange={(event) => setAdminNoteDraft(event.target.value)} className="detail-input detail-textarea" placeholder="Internal support context" />
                            </label>
                            <label>
                              <span className="detail-label">Lock reason</span>
                              <textarea value={lockedReasonDraft} onChange={(event) => setLockedReasonDraft(event.target.value)} className="detail-input detail-textarea" placeholder="Why the account is locked or should be locked" />
                            </label>
                          </div>
                          <div className="detail-actions">
                            <button type="button" className="primary-button" onClick={handleSaveAdminDetails} disabled={loading || activeUserActionId === selectedUser.id}>
                              {activeUserActionId === selectedUser.id ? 'Saving…' : 'Save details'}
                            </button>
                          </div>
                        </div>

                        <div className="detail-block">
                          <div className="panel-header panel-header-tight">
                            <div>
                              <p className="panel-label">Billing control</p>
                              <h2>Manual plan override</h2>
                            </div>
                          </div>
                          <div className="form-grid">
                            <label>
                              <span className="detail-label">Override tier</span>
                              <select value={planTierDraft} onChange={(event) => setPlanTierDraft(event.target.value)} className="select-input detail-input">
                                <option value="">No manual override</option>
                                <option value="free">Free</option>
                                <option value="paid">Paid</option>
                              </select>
                            </label>
                            <label>
                              <span className="detail-label">Expires at</span>
                              <input type="datetime-local" value={planExpiryDraft} onChange={(event) => setPlanExpiryDraft(event.target.value)} className="detail-input" />
                            </label>
                            <label className="form-grid-span">
                              <span className="detail-label">Reason</span>
                              <textarea value={planReasonDraft} onChange={(event) => setPlanReasonDraft(event.target.value)} className="detail-input detail-textarea" placeholder="Why this override exists" />
                            </label>
                          </div>
                          <div className="detail-actions">
                            <button type="button" className="primary-button" onClick={handleSavePlanOverride} disabled={loading || activeUserActionId === selectedUser.id}>
                              {activeUserActionId === selectedUser.id ? 'Saving…' : 'Save override'}
                            </button>
                            <button type="button" className="secondary-button" onClick={handleClearPlanOverride} disabled={loading || activeUserActionId === selectedUser.id}>
                              Clear override
                            </button>
                          </div>
                          <div className="detail-list">
                            <span>Manual tier: {selectedUser.manual_subscription_tier || '—'}</span>
                            <span>Expires: {formatDate(selectedUser.manual_subscription_expires_at)}</span>
                            <span>Granted by: {selectedUser.manual_subscription_granted_by || '—'}</span>
                          </div>
                        </div>

                        <div className="detail-block">
                          <div className="panel-header panel-header-tight">
                            <div>
                              <p className="panel-label">Product access</p>
                              <h2>Interface grants</h2>
                            </div>
                            <button type="button" className="secondary-button action-button" onClick={handleAddAccessGrant}>
                              Add grant
                            </button>
                          </div>
                          <div className="grant-stack">
                            {accessGrantDrafts.map((grant, index) => (
                              <div key={`${grant.subscription_mode}-${grant.ui_surface}-${index}`} className="grant-card">
                                <div className="grant-grid">
                                  <label>
                                    <span className="detail-label">Mode</span>
                                    <select value={grant.subscription_mode} onChange={(event) => handleAccessGrantDraftChange(index, 'subscription_mode', event.target.value)} className="select-input detail-input">
                                      {SUBSCRIPTION_MODE_OPTIONS.map((value) => (
                                        <option key={value} value={value}>{labelFromSnakeCase(value)}</option>
                                      ))}
                                    </select>
                                  </label>
                                  <label>
                                    <span className="detail-label">UI</span>
                                    <select value={grant.ui_surface} onChange={(event) => handleAccessGrantDraftChange(index, 'ui_surface', event.target.value)} className="select-input detail-input">
                                      {UI_SURFACE_OPTIONS.map((value) => (
                                        <option key={value} value={value}>{labelFromSnakeCase(value)}</option>
                                      ))}
                                    </select>
                                  </label>
                                  <label>
                                    <span className="detail-label">Plan</span>
                                    <select value={grant.plan} onChange={(event) => handleAccessGrantDraftChange(index, 'plan', event.target.value)} className="select-input detail-input">
                                      {ACCESS_PLAN_OPTIONS.map((value) => (
                                        <option key={value} value={value}>{labelFromSnakeCase(value)}</option>
                                      ))}
                                    </select>
                                  </label>
                                  <label>
                                    <span className="detail-label">Source</span>
                                    <select value={grant.source} onChange={(event) => handleAccessGrantDraftChange(index, 'source', event.target.value)} className="select-input detail-input">
                                      {ACCESS_SOURCE_OPTIONS.map((value) => (
                                        <option key={value} value={value}>{labelFromSnakeCase(value)}</option>
                                      ))}
                                    </select>
                                  </label>
                                  <label>
                                    <span className="detail-label">Granted at</span>
                                    <input type="datetime-local" value={grant.granted_at} onChange={(event) => handleAccessGrantDraftChange(index, 'granted_at', event.target.value)} className="detail-input" />
                                  </label>
                                  <label>
                                    <span className="detail-label">Expires at</span>
                                    <input type="datetime-local" value={grant.expires_at} onChange={(event) => handleAccessGrantDraftChange(index, 'expires_at', event.target.value)} className="detail-input" />
                                  </label>
                                  <label className="form-grid-span">
                                    <span className="detail-label">Note</span>
                                    <input type="text" value={grant.note} onChange={(event) => handleAccessGrantDraftChange(index, 'note', event.target.value)} className="detail-input" placeholder="Why this access exists" />
                                  </label>
                                </div>
                                <div className="detail-actions detail-actions-end">
                                  <button type="button" className="secondary-button danger-button action-button" onClick={() => handleRemoveAccessGrant(index)}>
                                    Remove grant
                                  </button>
                                </div>
                              </div>
                            ))}
                            {accessGrantDrafts.length === 0 && (
                              <div className="empty-inline empty-inline-tight">
                                <p className="panel-help">No explicit grants stored. The encrypted app still resolves through the legacy-compatible access model.</p>
                              </div>
                            )}
                          </div>
                          <div className="detail-actions">
                            <button type="button" className="primary-button" onClick={handleSaveAccessGrants} disabled={loading || activeUserActionId === selectedUser.id}>
                              {activeUserActionId === selectedUser.id ? 'Saving…' : 'Save grants'}
                            </button>
                          </div>
                        </div>
                      </div>
                    ) : (
                      <div className="empty-inline">
                        <p className="panel-label">No selection</p>
                        <p className="panel-help">Choose a user from the table to edit notes, lock context, or a manual plan override.</p>
                      </div>
                    )}
                  </aside>
                </section>
              </section>
            )}

            {activeView === 'feedback' && overview && (
              <section className="page-stack">
                <section className="panel panel-toolbar">
                  <div className="toolbar-copy">
                    <p className="panel-label">Feedback inbox</p>
                    <h2>Public site feedback and support signals</h2>
                    <p className="panel-help">Search by subject, sender, content, or page URL, then archive or restore submissions without deleting them immediately.</p>
                  </div>
                  <div className="toolbar-actions toolbar-actions-wide">
                    <input
                      type="search"
                      value={feedbackQuery}
                      onChange={(event) => setFeedbackQuery(event.target.value)}
                      placeholder="Search subject, email, message, or URL"
                      className="token-input search-input"
                    />
                    <div className="filter-group" role="group" aria-label="Feedback filters">
                      {(['all', 'active', 'archived'] as FeedbackFilter[]).map((value) => (
                        <button
                          key={value}
                          type="button"
                          className={value === feedbackFilter ? 'filter-chip is-active' : 'filter-chip'}
                          onClick={() => setFeedbackFilter(value)}
                        >
                          {value === 'all' ? 'All feedback' : value === 'active' ? 'Active only' : 'Archived only'}
                        </button>
                      ))}
                    </div>
                  </div>
                </section>

                <section className="status-row compact-row status-row-wide">
                  <div className="notes-card">
                    <h2>Filtered items</h2>
                    <p>{filteredFeedback.length}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Active inbox</h2>
                    <p>{activeFeedbackCount}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Archived</h2>
                    <p>{metrics?.archived_feedback_count ?? 0}</p>
                  </div>
                  <div className="notes-card">
                    <h2>Total recorded</h2>
                    <p>{metrics?.feedback_count ?? 0}</p>
                  </div>
                </section>

                <section className="panel">
                  <div className="panel-header">
                    <div>
                      <p className="panel-label">Feedback</p>
                      <h2>Latest submissions</h2>
                    </div>
                    <div className="pagination-row">
                      <button type="button" className="secondary-button action-button" onClick={() => setFeedbackPage((page) => Math.max(1, page - 1))} disabled={feedbackPage === 1}>
                        Previous
                      </button>
                      <span className="page-indicator">Page {feedbackPage} of {feedbackPages}</span>
                      <button type="button" className="secondary-button action-button" onClick={() => setFeedbackPage((page) => Math.min(feedbackPages, page + 1))} disabled={feedbackPage >= feedbackPages}>
                        Next
                      </button>
                    </div>
                  </div>

                  <div className="feedback-list feedback-list-page">
                    {pagedFeedback.map((item) => (
                      <article key={item.public_id} className={item.is_archived ? 'feedback-card is-archived' : 'feedback-card'}>
                        <div className="feedback-meta">
                          <div className="feedback-heading">
                            <strong>{item.subject}</strong>
                            <div className="feedback-status-row">
                              <span>{formatDate(item.created_at)}</span>
                              <span className={feedbackTone(item)}>{item.is_archived ? 'Archived' : 'Active'}</span>
                            </div>
                          </div>
                          <div className="table-actions">
                            <button
                              type="button"
                              className="secondary-button action-button"
                              onClick={() => handleToggleFeedbackArchive(item)}
                              disabled={loading || activeFeedbackActionId === item.public_id}
                            >
                              {activeFeedbackActionId === item.public_id ? 'Working…' : item.is_archived ? 'Restore' : 'Archive'}
                            </button>
                            <button
                              type="button"
                              className="secondary-button danger-button action-button"
                              onClick={() => handleDeleteFeedback(item)}
                              disabled={loading || activeFeedbackActionId === item.public_id}
                            >
                              Delete
                            </button>
                          </div>
                        </div>
                        <p className="feedback-author">
                          {item.name || 'Anonymous'}
                          {item.email ? ` · ${item.email}` : ''}
                        </p>
                        <p className="feedback-message">{item.message}</p>
                        <p className="feedback-link">{item.page_url || 'No source URL recorded'}</p>
                        {item.is_archived && <p className="feedback-link">Archived at {formatDate(item.archived_at)}</p>}
                      </article>
                    ))}
                  </div>

                  {filteredFeedback.length === 0 && (
                    <div className="empty-inline">
                      <p className="panel-label">No matches</p>
                      <p className="panel-help">Adjust the search or archive filter to show feedback entries from the current snapshot.</p>
                    </div>
                  )}
                </section>
              </section>
            )}

            {hasSession && !overview && !loading && !error && (
              <section className="empty-dashboard">
                <p className="panel-label">Waiting for data</p>
                <h2>The session is active, but the dashboard has not received an overview yet.</h2>
                <p className="panel-help">Use Refresh data to load the current admin snapshot.</p>
              </section>
            )}
          </section>
        </section>
      )}
    </main>
  );
}