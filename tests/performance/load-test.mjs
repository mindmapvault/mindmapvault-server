#!/usr/bin/env node

import { performance } from 'node:perf_hooks';
import { randomBytes, randomUUID } from 'node:crypto';

const DEFAULT_BASE_URL = 'http://127.0.0.1:8090';
const DEFAULT_USERS = 200;
const DEFAULT_CONCURRENCY = 200;
const DEFAULT_BLOB_BYTES = 16 * 1024;
const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_ARGON2_PARAMS = { m_cost: 65_536, t_cost: 3, p_cost: 4 };

function parseArgs(argv) {
  const options = {
    baseUrl: DEFAULT_BASE_URL,
    users: DEFAULT_USERS,
    concurrency: DEFAULT_CONCURRENCY,
    blobBytes: DEFAULT_BLOB_BYTES,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    cleanup: true,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = argv[i + 1];

    switch (arg) {
      case '--base-url':
        if (!next) throw new Error('--base-url requires a value');
        options.baseUrl = next;
        i += 1;
        break;
      case '--users':
        if (!next) throw new Error('--users requires a value');
        options.users = Number(next);
        i += 1;
        break;
      case '--concurrency':
        if (!next) throw new Error('--concurrency requires a value');
        options.concurrency = Number(next);
        i += 1;
        break;
      case '--blob-kb':
        if (!next) throw new Error('--blob-kb requires a value');
        options.blobBytes = Number(next) * 1024;
        i += 1;
        break;
      case '--timeout-ms':
        if (!next) throw new Error('--timeout-ms requires a value');
        options.timeoutMs = Number(next);
        i += 1;
        break;
      case '--cleanup':
        options.cleanup = true;
        break;
      case '--no-cleanup':
        options.cleanup = false;
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!Number.isFinite(options.users) || options.users < 1) {
    throw new Error('--users must be a positive number');
  }
  if (!Number.isFinite(options.concurrency) || options.concurrency < 1) {
    throw new Error('--concurrency must be a positive number');
  }
  if (!Number.isFinite(options.blobBytes) || options.blobBytes < 1) {
    throw new Error('--blob-kb must be a positive number');
  }
  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs < 1) {
    throw new Error('--timeout-ms must be a positive number');
  }

  options.users = Math.floor(options.users);
  options.concurrency = Math.floor(options.concurrency);
  options.blobBytes = Math.floor(options.blobBytes);
  options.timeoutMs = Math.floor(options.timeoutMs);

  return options;
}

function normalizeBaseUrl(url) {
  return url.trim().replace(/\/$/, '').replace(/\/api$/, '');
}

function joinUrl(baseUrl, path) {
  return `${baseUrl}${path.startsWith('/') ? path : `/${path}`}`;
}

function base64(bytes) {
  return Buffer.from(bytes).toString('base64');
}

function randomB64(size) {
  return base64(randomBytes(size));
}

function randomHex(size = 32) {
  return randomBytes(size).toString('hex');
}

function makeUser(runId, index) {
  const suffix = String(index).padStart(4, '0');
  return {
    username: `perf_${runId}_${suffix}`,
    authToken: randomHex(32),
    argon2Salt: randomB64(32),
    classicalPublicKey: randomB64(32),
    pqPublicKey: randomB64(32),
    classicalPrivEncrypted: randomB64(48),
    pqPrivEncrypted: randomB64(48),
  };
}

function makeVaultPayload(user) {
  return {
    title_encrypted: randomB64(48),
    eph_classical_public: randomB64(32),
    eph_pq_ciphertext: randomB64(64),
    wrapped_dek: randomB64(32),
  };
}

function makeAccountSettingsPayload(index) {
  return {
    locale: index % 2 === 0 ? 'en' : 'en-US',
    timezone: index % 3 === 0 ? 'UTC' : 'Europe/London',
    date_format: index % 2 === 0 ? 'iso' : 'us',
    accessibility_reduce_motion: index % 2 === 1,
    sync_appearance_across_devices: index % 3 === 0,
    default_map_layout: index % 2 === 0 ? 'mindmap' : 'tree',
    default_map_theme: index % 2 === 0 ? 'system' : 'focus',
    default_export_format: index % 2 === 0 ? 'cryptmind' : 'json',
    default_node_style_preset: index % 2 === 0 ? 'default' : 'compact',
    user_labels_json: JSON.stringify([
      { name: `Load ${index}`, color: '#7C3AED' },
    ]),
  };
}

async function fetchJson(url, init = {}) {
  const start = performance.now();
  const response = await fetch(url, {
    ...init,
    headers: {
      accept: 'application/json',
      ...(init.headers ?? {}),
    },
  });
  const elapsedMs = performance.now() - start;
  const text = await response.text();
  let parsed;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    parsed = text;
  }
  return { response, elapsedMs, body: parsed, raw: text };
}

async function fetchBytes(url, init = {}) {
  const start = performance.now();
  const response = await fetch(url, init);
  const elapsedMs = performance.now() - start;
  const text = await response.text();
  let parsed;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    parsed = text;
  }
  return { response, elapsedMs, body: parsed, raw: text };
}

function summarize(values) {
  if (!values.length) {
    return { count: 0, avg: 0, min: 0, p50: 0, p95: 0, max: 0 };
  }

  const sorted = [...values].sort((a, b) => a - b);
  const sum = sorted.reduce((acc, value) => acc + value, 0);
  const percentile = (p) => sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * p))];

  return {
    count: sorted.length,
    avg: sum / sorted.length,
    min: sorted[0],
    p50: percentile(0.5),
    p95: percentile(0.95),
    max: sorted[sorted.length - 1],
  };
}

async function runPool(items, limit, worker) {
  const results = new Array(items.length);
  let cursor = 0;

  async function next() {
    const current = cursor;
    cursor += 1;
    if (current >= items.length) return;
    results[current] = await worker(items[current], current);
    return next();
  }

  const runners = Array.from({ length: Math.min(limit, items.length) }, () => next());
  await Promise.all(runners);
  return results;
}

async function runUserFlow(baseUrl, user, options) {
  const timings = {};
  const headers = { 'content-type': 'application/json' };
  let accessToken = '';
  let refreshToken = '';
  let createdVaultId = '';
  let cleanupError = null;
  let primaryError = null;

  const measure = async (name, fn) => {
    const start = performance.now();
    const value = await fn();
    timings[name] = performance.now() - start;
    return value;
  };

  try {
    await measure('register', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/register'), {
        method: 'POST',
        headers,
        body: JSON.stringify({
          username: user.username,
          auth_token: user.authToken,
          argon2_salt: user.argon2Salt,
          argon2_params: DEFAULT_ARGON2_PARAMS,
          classical_public_key: user.classicalPublicKey,
          pq_public_key: user.pqPublicKey,
          classical_priv_encrypted: user.classicalPrivEncrypted,
          pq_priv_encrypted: user.pqPrivEncrypted,
        }),
      });

      if (!result.response.ok) {
        throw new Error(`register failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    const loginBody = await measure('login', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/login'), {
        method: 'POST',
        headers,
        body: JSON.stringify({
          username: user.username,
          auth_token: user.authToken,
        }),
      });

      if (!result.response.ok) {
        throw new Error(`login failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }

      return result.body;
    });

    accessToken = loginBody.access_token;
    refreshToken = loginBody.refresh_token;

    await measure('profile', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/profile'), {
        headers: { authorization: `Bearer ${accessToken}` },
      });
      if (!result.response.ok) {
        throw new Error(`profile failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    await measure('capabilities', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/capabilities'), {
        headers: { authorization: `Bearer ${accessToken}` },
      });
      if (!result.response.ok) {
        throw new Error(`capabilities failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    await measure('storage', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/storage'), {
        headers: { authorization: `Bearer ${accessToken}` },
      });
      if (!result.response.ok) {
        throw new Error(`storage failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    await measure('update-profile', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/profile'), {
        method: 'PUT',
        headers: {
          ...headers,
          authorization: `Bearer ${accessToken}`,
        },
        body: JSON.stringify({
          first_name: `Perf${String(user.username.slice(-4))}`,
          last_name: `Run${String(user.username.slice(-4))}`,
          email: `${user.username}@example.test`,
        }),
      });
      if (!result.response.ok) {
        throw new Error(`update profile failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    await measure('update-settings', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/auth/settings'), {
        method: 'PATCH',
        headers: {
          ...headers,
          authorization: `Bearer ${accessToken}`,
        },
        body: JSON.stringify(makeAccountSettingsPayload(user.username.length)),
      });
      if (!result.response.ok) {
        throw new Error(`update settings failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    const vault = await measure('create-vault', async () => {
      const result = await fetchJson(joinUrl(baseUrl, '/api/mindmaps'), {
        method: 'POST',
        headers: {
          ...headers,
          authorization: `Bearer ${accessToken}`,
        },
        body: JSON.stringify(makeVaultPayload(user)),
      });

      if (!result.response.ok) {
        throw new Error(`create vault failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }

      return result.body;
    });

    createdVaultId = vault.id;

    await measure('update-vault-meta', async () => {
      const result = await fetchJson(joinUrl(baseUrl, `/api/mindmaps/${createdVaultId}/meta`), {
        method: 'PUT',
        headers: {
          ...headers,
          authorization: `Bearer ${accessToken}`,
        },
        body: JSON.stringify({
          vault_color: '#7C3AED',
          vault_note_encrypted: randomB64(64),
          vault_sharing_mode: 'private',
          vault_encryption_mode: 'standard',
          max_versions: 50,
          title_encrypted: randomB64(48),
          vault_labels: ['perf', 'load-test'],
        }),
      });
      if (!result.response.ok) {
        throw new Error(`update vault meta failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

    await measure('get-vault', async () => {
      const result = await fetchJson(joinUrl(baseUrl, `/api/mindmaps/${createdVaultId}`), {
        headers: { authorization: `Bearer ${accessToken}` },
      });
      if (!result.response.ok) {
        throw new Error(`get vault failed (${result.response.status}): ${JSON.stringify(result.body)}`);
      }
      return result.body;
    });

  } catch (error) {
    primaryError = error instanceof Error ? error : new Error(String(error));
  } finally {
    if (options.cleanup && accessToken && createdVaultId) {
      try {
        await measure('cleanup-vault', async () => {
          const result = await fetchJson(joinUrl(baseUrl, `/api/mindmaps/${createdVaultId}`), {
            method: 'DELETE',
            headers: { authorization: `Bearer ${accessToken}` },
          });
          if (!result.response.ok) {
            throw new Error(`delete vault failed (${result.response.status}): ${JSON.stringify(result.body)}`);
          }
          return result.body;
        });
      } catch (error) {
        cleanupError = error instanceof Error ? error.message : String(error);
      }
    }

    if (options.cleanup && accessToken) {
      try {
        await measure('cleanup-profile', async () => {
          const result = await fetchJson(joinUrl(baseUrl, '/api/auth/profile'), {
            method: 'DELETE',
            headers: { authorization: `Bearer ${accessToken}` },
          });
          if (!result.response.ok) {
            throw new Error(`delete profile failed (${result.response.status}): ${JSON.stringify(result.body)}`);
          }
          return result.body;
        });
      } catch (error) {
        cleanupError = error instanceof Error ? error.message : String(error);
      }
    }
  }

  if (primaryError) {
    if (cleanupError) {
      primaryError.message = `${primaryError.message}; cleanup error: ${cleanupError}`;
    }
    throw primaryError;
  }

  if (cleanupError) {
    throw new Error(cleanupError);
  }

  return { ok: true, timings };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const baseUrl = normalizeBaseUrl(options.baseUrl);
  const runId = randomUUID().slice(0, 8);
  const users = Array.from({ length: options.users }, (_, index) => makeUser(runId, index + 1));
  const startedAt = performance.now();

  console.log(JSON.stringify({
    event: 'start',
    baseUrl,
    users: options.users,
    concurrency: options.concurrency,
    blobBytes: options.blobBytes,
    cleanup: options.cleanup,
    runId,
  }, null, 2));

  const results = await runPool(users, options.concurrency, async (user) => {
    try {
      return await runUserFlow(baseUrl, user, options);
    } catch (error) {
      return {
        ok: false,
        error: error instanceof Error ? error.message : String(error),
        timings: {},
      };
    }
  });

  const durationMs = performance.now() - startedAt;
  const okResults = results.filter((result) => result?.ok);
  const failedResults = results.filter((result) => !result?.ok);
  const stepTimings = new Map();

  for (const result of okResults) {
    for (const [step, elapsed] of Object.entries(result.timings)) {
      if (!stepTimings.has(step)) {
        stepTimings.set(step, []);
      }
      stepTimings.get(step).push(elapsed);
    }
  }

  const summary = {
    event: 'summary',
    baseUrl,
    runId,
    users: options.users,
    concurrency: options.concurrency,
    cleanup: options.cleanup,
    durationMs,
    throughputUsersPerSec: options.users / (durationMs / 1000),
    successCount: okResults.length,
    failureCount: failedResults.length,
    stepStats: Object.fromEntries(
      [...stepTimings.entries()].map(([step, values]) => [step, summarize(values)]),
    ),
    failures: failedResults.slice(0, 10).map((result, index) => ({
      index: index + 1,
      error: result?.error ?? 'unknown error',
    })),
  };

  console.log(JSON.stringify(summary, null, 2));

  if (failedResults.length > 0) {
    process.exitCode = 1;
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});