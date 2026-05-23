#!/usr/bin/env node

import { randomBytes, randomUUID } from 'node:crypto';

const DEFAULT_BASE_URL = 'http://127.0.0.1:8090';
const DEFAULT_ADMIN_TOKEN = 'change-me-admin-token';
const DEFAULT_TIMEOUT_MS = 30_000;

function parseArgs(argv) {
  const options = {
    baseUrl: DEFAULT_BASE_URL,
    adminToken: DEFAULT_ADMIN_TOKEN,
    timeoutMs: DEFAULT_TIMEOUT_MS,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = argv[i + 1];

    if (arg === '--base-url') {
      if (!next) throw new Error('--base-url requires a value');
      options.baseUrl = next;
      i += 1;
      continue;
    }

    if (arg === '--admin-token') {
      if (!next) throw new Error('--admin-token requires a value');
      options.adminToken = next;
      i += 1;
      continue;
    }

    if (arg === '--timeout-ms') {
      if (!next) throw new Error('--timeout-ms requires a value');
      options.timeoutMs = Number(next);
      i += 1;
      continue;
    }

    throw new Error(`Unknown argument: ${arg}`);
  }

  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs < 1) {
    throw new Error('--timeout-ms must be a positive integer');
  }

  options.timeoutMs = Math.floor(options.timeoutMs);
  return options;
}

function normalizeBaseUrl(url) {
  return url.trim().replace(/\/$/, '').replace(/\/api$/, '');
}

function joinUrl(baseUrl, path) {
  return `${baseUrl}${path.startsWith('/') ? path : `/${path}`}`;
}

function randomB64(size) {
  return Buffer.from(randomBytes(size)).toString('base64');
}

function makeUser(runId, suffix) {
  return {
    username: `cover_${runId}_${suffix}`,
    authToken: randomBytes(32).toString('hex'),
    argon2Salt: randomB64(32),
    classicalPublicKey: randomB64(32),
    pqPublicKey: randomB64(32),
    classicalPrivEncrypted: randomB64(48),
    pqPrivEncrypted: randomB64(48),
  };
}

async function fetchJson(baseUrl, path, init = {}, timeoutMs = DEFAULT_TIMEOUT_MS) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(joinUrl(baseUrl, path), {
      ...init,
      signal: controller.signal,
      headers: {
        accept: 'application/json',
        ...(init.headers ?? {}),
      },
    });

    const text = await response.text();
    let body;
    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = text;
    }

    return { response, body, raw: text };
  } finally {
    clearTimeout(timer);
  }
}

async function runCase(results, baseUrl, timeoutMs, spec) {
  const { name, method, path, expectedStatus, body, headers, asBytes } = spec;
  const init = { method, headers: { ...(headers ?? {}) } };

  if (body !== undefined) {
    if (asBytes) {
      init.body = body;
    } else {
      init.headers['content-type'] = 'application/json';
      init.body = JSON.stringify(body);
    }
  }

  const outcome = await fetchJson(baseUrl, path, init, timeoutMs);
  const actual = outcome.response.status;
  const ok = expectedStatus.includes(actual);

  results.push({
    name,
    method,
    path,
    status: actual,
    expectedStatus,
    ok,
    body: outcome.body,
  });

  if (!ok) {
    throw new Error(`${name} failed: expected [${expectedStatus.join(', ')}], got ${actual}, body=${JSON.stringify(outcome.body)}`);
  }

  return outcome;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const baseUrl = normalizeBaseUrl(options.baseUrl);
  const runId = randomUUID().slice(0, 8);
  const results = [];

  const userA = makeUser(runId, 'a');
  const userB = makeUser(runId, 'b');

  let accessToken = '';
  let refreshToken = '';
  let userAId = 'missing-user-a-id';
  let userBId = 'missing-user-b-id';
  let mapId = 'missing-map-id';
  let attachmentId = 'missing-attachment-id';
  let feedbackId = 'missing-feedback-id';

  const bearer = () => ({ authorization: `Bearer ${accessToken}` });
  const adminBearer = () => ({ authorization: `Bearer ${options.adminToken}` });

  console.log(JSON.stringify({ event: 'start', baseUrl, runId }, null, 2));

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'health',
    method: 'GET',
    path: '/health',
    expectedStatus: [200],
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'admin-redirect',
    method: 'GET',
    path: '/admin',
    expectedStatus: [200, 301, 302, 307, 308],
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-register-a',
    method: 'POST',
    path: '/api/auth/register',
    expectedStatus: [200],
    body: {
      username: userA.username,
      auth_token: userA.authToken,
      argon2_salt: userA.argon2Salt,
      argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
      classical_public_key: userA.classicalPublicKey,
      pq_public_key: userA.pqPublicKey,
      classical_priv_encrypted: userA.classicalPrivEncrypted,
      pq_priv_encrypted: userA.pqPrivEncrypted,
    },
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-register-b',
    method: 'POST',
    path: '/api/auth/register',
    expectedStatus: [200],
    body: {
      username: userB.username,
      auth_token: userB.authToken,
      argon2_salt: userB.argon2Salt,
      argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
      classical_public_key: userB.classicalPublicKey,
      pq_public_key: userB.pqPublicKey,
      classical_priv_encrypted: userB.classicalPrivEncrypted,
      pq_priv_encrypted: userB.pqPrivEncrypted,
    },
  });

  const login = await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-login-a',
    method: 'POST',
    path: '/api/auth/login',
    expectedStatus: [200],
    body: {
      username: userA.username,
      auth_token: userA.authToken,
    },
  });
  accessToken = login.body.access_token;
  refreshToken = login.body.refresh_token;

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-salt',
    method: 'GET',
    path: `/api/auth/salt?username=${encodeURIComponent(userA.username)}`,
    expectedStatus: [200],
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-refresh',
    method: 'POST',
    path: '/api/auth/refresh',
    expectedStatus: [200],
    body: {
      refresh_token: refreshToken,
    },
  });

  const mapCreated = await runCase(results, baseUrl, options.timeoutMs, {
    name: 'mindmaps-create',
    method: 'POST',
    path: '/api/mindmaps',
    expectedStatus: [200],
    headers: bearer(),
    body: {
      title_encrypted: randomB64(48),
      eph_classical_public: randomB64(32),
      eph_pq_ciphertext: randomB64(64),
      wrapped_dek: randomB64(32),
    },
  });
  mapId = mapCreated.body.id;

  const attachmentInit = await runCase(results, baseUrl, options.timeoutMs, {
    name: 'mindmaps-attachments-init',
    method: 'POST',
    path: `/api/mindmaps/${mapId}/attachments/init`,
    expectedStatus: [200],
    headers: bearer(),
    body: {
      name: 'coverage.bin',
      content_type: 'application/octet-stream',
      size: 64,
      node_id: null,
      encrypted: true,
      encryption_meta: {},
    },
  });
  attachmentId = attachmentInit.body.attachment_id;

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'public-feedback-create',
    method: 'POST',
    path: '/api/public/marketing/feedback',
    expectedStatus: [200],
    body: {
      name: 'Endpoint Coverage',
      email: 'coverage@example.test',
      subject: 'coverage check',
      message: `run ${runId}`,
      page_url: 'https://example.test/coverage',
    },
  });

  const overview = await runCase(results, baseUrl, options.timeoutMs, {
    name: 'admin-overview',
    method: 'GET',
    path: '/api/admin/overview',
    expectedStatus: [200],
    headers: adminBearer(),
  });

  const users = Array.isArray(overview.body.users) ? overview.body.users : [];
  const userARecord = users.find((item) => item.username === userA.username);
  const userBRecord = users.find((item) => item.username === userB.username);
  if (userARecord?.id) userAId = userARecord.id;
  if (userBRecord?.id) userBId = userBRecord.id;

  const feedback = Array.isArray(overview.body.feedback) ? overview.body.feedback : [];
  if (feedback[0]?.public_id) {
    feedbackId = feedback[0].public_id;
  }

  const endpointCases = [
    { name: 'auth-keys', method: 'GET', path: '/api/auth/keys', expectedStatus: [200], headers: bearer() },
    { name: 'auth-rotate-credentials', method: 'POST', path: '/api/auth/rotate-credentials', expectedStatus: [400], headers: bearer(), body: {} },
    { name: 'auth-subscription', method: 'GET', path: '/api/auth/subscription', expectedStatus: [200], headers: bearer() },
    { name: 'auth-capabilities', method: 'GET', path: '/api/auth/capabilities', expectedStatus: [200], headers: bearer() },
    { name: 'auth-storage', method: 'GET', path: '/api/auth/storage', expectedStatus: [200], headers: bearer() },
    { name: 'auth-settings-get', method: 'GET', path: '/api/auth/settings', expectedStatus: [200], headers: bearer() },
    { name: 'auth-settings-patch', method: 'PATCH', path: '/api/auth/settings', expectedStatus: [200], headers: bearer(), body: { locale: 'en', date_format: 'iso', default_map_layout: 'mindmap' } },
    { name: 'auth-profile-get', method: 'GET', path: '/api/auth/profile', expectedStatus: [200], headers: bearer() },
    { name: 'auth-profile-put', method: 'PUT', path: '/api/auth/profile', expectedStatus: [200], headers: bearer(), body: { first_name: 'Endpoint', last_name: 'Coverage', email: `${userA.username}@example.test` } },

    { name: 'admin-user-lock', method: 'POST', path: `/api/admin/users/${userAId}/account-lock`, expectedStatus: [200], headers: adminBearer(), body: { locked: false, reason: null } },
    { name: 'admin-user-details', method: 'POST', path: `/api/admin/users/${userAId}/admin-details`, expectedStatus: [200], headers: adminBearer(), body: { admin_note: 'coverage', locked_reason: null } },
    { name: 'admin-user-access-grants', method: 'POST', path: `/api/admin/users/${userAId}/access-grants`, expectedStatus: [200], headers: adminBearer(), body: { access_grants: userARecord?.access_grants ?? [] } },
    { name: 'admin-user-plan-override', method: 'POST', path: `/api/admin/users/${userAId}/plan-override`, expectedStatus: [200], headers: adminBearer(), body: { manual_subscription_tier: null, manual_subscription_expires_at: null, reason: null } },
    { name: 'admin-feedback-archive', method: 'POST', path: `/api/admin/feedback/${feedbackId}/archive`, expectedStatus: [200], headers: adminBearer(), body: { archived: true } },
    { name: 'admin-feedback-delete', method: 'POST', path: `/api/admin/feedback/${feedbackId}/delete`, expectedStatus: [200], headers: adminBearer(), body: {} },

    { name: 'mindmaps-list', method: 'GET', path: '/api/mindmaps/', expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-storage', method: 'GET', path: '/api/mindmaps/storage', expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-my-storage', method: 'GET', path: '/api/mindmaps/my/storage', expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-get', method: 'GET', path: `/api/mindmaps/${mapId}`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-update', method: 'PUT', path: `/api/mindmaps/${mapId}`, expectedStatus: [200], headers: bearer(), body: { title_encrypted: randomB64(48), eph_classical_public: randomB64(32), eph_pq_ciphertext: randomB64(64), wrapped_dek: randomB64(32) } },
    { name: 'mindmaps-meta', method: 'PUT', path: `/api/mindmaps/${mapId}/meta`, expectedStatus: [200], headers: bearer(), body: { vault_color: '#7C3AED', vault_sharing_mode: 'private', vault_encryption_mode: 'standard', max_versions: 50, title_encrypted: randomB64(48), vault_labels: ['coverage'] } },
    { name: 'mindmaps-upload', method: 'POST', path: `/api/mindmaps/${mapId}/upload`, expectedStatus: [200, 400], headers: { ...bearer(), 'content-type': 'application/octet-stream' }, body: randomBytes(32), asBytes: true },
    { name: 'mindmaps-blob', method: 'GET', path: `/api/mindmaps/${mapId}/blob`, expectedStatus: [200, 404, 500], headers: bearer() },
    { name: 'mindmaps-upload-url', method: 'POST', path: `/api/mindmaps/${mapId}/upload-url`, expectedStatus: [200], headers: bearer(), body: {} },
    { name: 'mindmaps-confirm-upload', method: 'POST', path: `/api/mindmaps/${mapId}/confirm-upload`, expectedStatus: [400], headers: bearer(), body: { version_id: 'invalid-version-id' } },
    { name: 'mindmaps-download-url', method: 'GET', path: `/api/mindmaps/${mapId}/download-url`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-attachments-list', method: 'GET', path: `/api/mindmaps/${mapId}/attachments`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-attachment-get', method: 'GET', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-attachment-patch', method: 'PATCH', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}`, expectedStatus: [200], headers: bearer(), body: { node_id: 'node-1' } },
    { name: 'mindmaps-attachment-upload', method: 'POST', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}/upload`, expectedStatus: [200, 400], headers: { ...bearer(), 'content-type': 'application/octet-stream' }, body: randomBytes(32), asBytes: true },
    { name: 'mindmaps-attachment-complete', method: 'POST', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}/complete`, expectedStatus: [400], headers: bearer(), body: { version_id: 'invalid-version-id', checksum_sha256: null } },
    { name: 'mindmaps-attachment-download', method: 'GET', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}/download`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-attachment-blob', method: 'GET', path: `/api/mindmaps/${mapId}/attachments/${attachmentId}/blob`, expectedStatus: [200, 404, 500], headers: bearer() },
    { name: 'mindmaps-versions-list', method: 'GET', path: `/api/mindmaps/${mapId}/versions`, expectedStatus: [200], headers: bearer() },
    { name: 'mindmaps-version-delete', method: 'DELETE', path: `/api/mindmaps/${mapId}/versions/invalid-version-id`, expectedStatus: [400, 404], headers: bearer() },
    { name: 'mindmaps-maintenance-allocator-stats', method: 'GET', path: '/api/mindmaps/maintenance/allocator-stats', expectedStatus: [200, 401, 404], headers: bearer() },
  ];

  for (const spec of endpointCases) {
    await runCase(results, baseUrl, options.timeoutMs, spec);
  }

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'mindmaps-attachment-delete',
    method: 'DELETE',
    path: `/api/mindmaps/${mapId}/attachments/${attachmentId}`,
    expectedStatus: [200],
    headers: bearer(),
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'mindmaps-delete',
    method: 'DELETE',
    path: `/api/mindmaps/${mapId}`,
    expectedStatus: [200],
    headers: bearer(),
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'admin-user-delete-account',
    method: 'POST',
    path: `/api/admin/users/${userBId}/delete-account`,
    expectedStatus: [200],
    headers: adminBearer(),
    body: { delete_all_data: true },
  });

  await runCase(results, baseUrl, options.timeoutMs, {
    name: 'auth-profile-delete-a',
    method: 'DELETE',
    path: '/api/auth/profile',
    expectedStatus: [200],
    headers: bearer(),
  });

  const failed = results.filter((item) => !item.ok);
  const summary = {
    event: 'summary',
    baseUrl,
    runId,
    totalCases: results.length,
    passed: results.length - failed.length,
    failed: failed.length,
    failures: failed.slice(0, 10).map((item) => ({
      name: item.name,
      method: item.method,
      path: item.path,
      status: item.status,
      expectedStatus: item.expectedStatus,
      body: item.body,
    })),
  };

  console.log(JSON.stringify(summary, null, 2));

  if (failed.length > 0) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});
