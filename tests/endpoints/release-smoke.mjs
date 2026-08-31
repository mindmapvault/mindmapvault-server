#!/usr/bin/env node
// Smoke test for release-critical behavior against a running backend.
//
// Covers: request-id header, register/login, generic JWT error message,
// vault + attachment upload above 2 MB, the upload body-limit ceiling, and
// the CORS origin allowlist.
//
// Needs a backend at --base-url with PostgreSQL and an S3 store that returns
// version ids (Garage does by default; MinIO needs bucket versioning enabled).
import { randomBytes, randomUUID } from 'node:crypto';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const BASE = flag('--base-url', 'http://127.0.0.1:8090').replace(/\/$/, '');

const b64 = (n) => Buffer.from(randomBytes(n)).toString('base64');
const runId = randomUUID().slice(0, 8);
let failures = 0;

function check(name, cond, detail = '') {
  const ok = Boolean(cond);
  if (!ok) failures += 1;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
}

async function json(path, init = {}) {
  const res = await fetch(`${BASE}${path}`, {
    ...init,
    headers: { 'content-type': 'application/json', ...(init.headers ?? {}) },
    body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
  });
  let body = null;
  try { body = await res.json(); } catch { /* non-JSON */ }
  return { res, body };
}

{
  const res = await fetch(`${BASE}/health`);
  check('x-request-id header present', res.headers.get('x-request-id'));
}

const user = {
  username: `smoke_${runId}`,
  auth_token: randomBytes(32).toString('hex'),
  argon2_salt: b64(32),
  argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
  classical_public_key: b64(32),
  pq_public_key: b64(32),
  classical_priv_encrypted: b64(48),
  pq_priv_encrypted: b64(48),
};
{
  const { res } = await json('/api/auth/register', { method: 'POST', body: user });
  check('register 200', res.status === 200, `status ${res.status}`);
}
let token = '';
{
  const { res, body } = await json('/api/auth/login', {
    method: 'POST',
    body: { username: user.username, auth_token: user.auth_token },
  });
  check('login 200', res.status === 200, `status ${res.status}`);
  token = body?.access_token ?? '';
}
const bearer = { authorization: `Bearer ${token}` };

// A malformed token must produce a generic message, not the JWT library's own.
{
  const res = await fetch(`${BASE}/api/mindmaps`, {
    headers: { authorization: 'Bearer not.a.token' },
  });
  const body = await res.json().catch(() => null);
  check('bad JWT returns 401', res.status === 401, `status ${res.status}`);
  check(
    'bad JWT error message is generic',
    body && body.error === 'invalid or expired token',
    `error=${JSON.stringify(body?.error)}`,
  );
}

let mapId = '';
{
  const { res, body } = await json('/api/mindmaps', {
    method: 'POST',
    headers: bearer,
    body: {
      title_encrypted: b64(48),
      eph_classical_public: b64(32),
      eph_pq_ciphertext: b64(64),
      wrapped_dek: b64(32),
    },
  });
  check('vault create 200', res.status === 200, `status ${res.status}`);
  mapId = body?.id ?? '';
}

// Uploads above axum's 2 MB default must pass; the derived ceiling must hold.
const THREE_MB = 3 * 1024 * 1024;
let attachmentId = '';
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/attachments/init`, {
    method: 'POST',
    headers: bearer,
    body: {
      name: 'smoke-3mb.bin',
      content_type: 'application/octet-stream',
      size: THREE_MB,
      node_id: null,
      encrypted: true,
      encryption_meta: {},
    },
  });
  check('attachment init (3 MB declared) 200', res.status === 200, `status ${res.status}`);
  attachmentId = body?.attachment_id ?? '';
}
{
  const res = await fetch(`${BASE}/api/mindmaps/${mapId}/attachments/${attachmentId}/upload`, {
    method: 'POST',
    headers: { ...bearer, 'content-type': 'application/octet-stream' },
    body: randomBytes(THREE_MB),
  });
  check('attachment upload 3 MB accepted', res.status === 200, `status ${res.status}`);
}
{
  const res = await fetch(`${BASE}/api/mindmaps/${mapId}/upload`, {
    method: 'POST',
    headers: { ...bearer, 'content-type': 'application/octet-stream' },
    body: randomBytes(THREE_MB),
  });
  check('vault blob upload 3 MB not blocked by body limit', res.status !== 413, `status ${res.status}`);
}
{
  const res = await fetch(`${BASE}/api/mindmaps/${mapId}/attachments/${attachmentId}/upload`, {
    method: 'POST',
    headers: { ...bearer, 'content-type': 'application/octet-stream' },
    body: randomBytes(60 * 1024 * 1024),
  });
  check('attachment upload 60 MB rejected 413', res.status === 413, `status ${res.status}`);
}

// Default CORS allowlist: local dev yes, hosted domains no.
{
  const probe = async (origin) => {
    const res = await fetch(`${BASE}/health`, { headers: { origin } });
    return res.headers.get('access-control-allow-origin');
  };
  check('CORS allows http://localhost:5173', (await probe('http://localhost:5173')) === 'http://localhost:5173');
  check('CORS does not allow https://app.mindmapvault.com', (await probe('https://app.mindmapvault.com')) === null);
}

console.log(failures === 0 ? '\nAll smoke checks passed.' : `\n${failures} check(s) FAILED.`);
process.exit(failures === 0 ? 0 : 1);
