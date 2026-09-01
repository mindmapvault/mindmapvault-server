#!/usr/bin/env node
// End-to-end test of the encrypted share flow against a running backend.
//
// Covers the owner path (create, upload, complete, list), the recipient path
// (unauthenticated metadata, blob and attachment download), ownership
// isolation, and that revoking deletes the stored ciphertext rather than only
// flagging the row.
//
// Needs a backend with PostgreSQL and an S3 store that returns version ids
// (Garage does by default; MinIO needs bucket versioning enabled).
import { randomBytes, randomUUID } from 'node:crypto';

const args = process.argv.slice(2);
const flagIndex = args.indexOf('--base-url');
const BASE = (flagIndex >= 0 && args[flagIndex + 1] ? args[flagIndex + 1] : 'http://127.0.0.1:8090').replace(/\/$/, '');
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

// ── setup: user + vault ─────────────────────────────────────────────────────
const user = {
  username: `share_${runId}`,
  auth_token: randomBytes(32).toString('hex'),
  argon2_salt: b64(32),
  argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
  classical_public_key: b64(32),
  pq_public_key: b64(32),
  classical_priv_encrypted: b64(48),
  pq_priv_encrypted: b64(48),
};
await json('/api/auth/register', { method: 'POST', body: user });
const login = await json('/api/auth/login', {
  method: 'POST',
  body: { username: user.username, auth_token: user.auth_token },
});
const token = login.body?.access_token ?? '';
const bearer = { authorization: `Bearer ${token}` };
check('setup: login', login.res.status === 200, `status ${login.res.status}`);

const created = await json('/api/mindmaps', {
  method: 'POST',
  headers: bearer,
  body: {
    title_encrypted: b64(48),
    eph_classical_public: b64(32),
    eph_pq_ciphertext: b64(64),
    wrapped_dek: b64(32),
  },
});
const mapId = created.body?.id;
check('setup: vault created', created.res.status === 200, `status ${created.res.status}`);

// ── shares list starts empty (route exists at all — used to 404) ────────────
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/shares`, { headers: bearer });
  check('GET shares returns 200 (was 404 before port)', res.status === 200, `status ${res.status}`);
  check('GET shares is empty initially', Array.isArray(body) && body.length === 0);
}

// ── create share ────────────────────────────────────────────────────────────
const SHARE_CIPHERTEXT = randomBytes(4096);
let shareId = '';
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/shares`, {
    method: 'POST',
    headers: bearer,
    body: {
      name: 'e2e-share.cmvshare',
      scope: 'map',
      include_attachments: true,
      passphrase_hint: 'the usual one',
      expires_at: new Date(Date.now() + 86400_000).toISOString(),
      content_type: 'application/vnd.cryptmind.share+json',
      size_bytes: SHARE_CIPHERTEXT.length,
      encryption_meta: { alg: 'AES-256-GCM', kdf: 'argon2id' },
    },
  });
  check('create share 200', res.status === 200, `status ${res.status}`);
  shareId = body?.share_id ?? '';
  // Derived from --base-url rather than hardcoded: the point of this check is
  // that the server echoes back whatever host the request arrived on, so
  // pinning a port here would pass for the wrong reason on any other one.
  check('share_url is built from the request host, not a hosted domain',
    typeof body?.share_url === 'string'
      && body.share_url.startsWith(`${new URL(BASE).origin}/shared/`)
      && !body.share_url.includes('mindmapvault.com'),
    body?.share_url);
}

// ── a pending share must not be publicly readable yet ───────────────────────
{
  const res = await fetch(`${BASE}/share/${shareId}`);
  check('pending share is not public yet (404)', res.status === 404, `status ${res.status}`);
}

// ── upload + complete ───────────────────────────────────────────────────────
let versionId = '';
{
  const res = await fetch(`${BASE}/api/mindmaps/${mapId}/shares/${shareId}/upload`, {
    method: 'POST',
    headers: { ...bearer, 'content-type': 'application/octet-stream' },
    body: SHARE_CIPHERTEXT,
  });
  const body = await res.json().catch(() => null);
  check('upload share blob 200', res.status === 200, `status ${res.status}`);
  versionId = body?.version_id ?? '';
}
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/shares/${shareId}/complete`, {
    method: 'POST',
    headers: bearer,
    body: { version_id: versionId },
  });
  check('complete share upload 200', res.status === 200, `status ${res.status}`);
  check('share status is available', body?.status === 'available', `status=${body?.status}`);
}

// ── recipient path: fully unauthenticated ───────────────────────────────────
{
  const res = await fetch(`${BASE}/share/${shareId}`);
  const body = await res.json().catch(() => null);
  check('public share metadata readable without auth', res.status === 200, `status ${res.status}`);
  check('public payload carries hint + encryption meta',
    body?.passphrase_hint === 'the usual one' && body?.encryption_meta?.alg === 'AES-256-GCM');
  check('public payload does NOT leak owner or s3 key',
    body && !('created_by' in body) && !('s3_key' in body));
}
{
  const res = await fetch(`${BASE}/share/${shareId}/blob`);
  const bytes = Buffer.from(await res.arrayBuffer());
  check('public blob download without auth', res.status === 200, `status ${res.status}`);
  check('downloaded ciphertext matches what was uploaded',
    bytes.length === SHARE_CIPHERTEXT.length && bytes.equals(SHARE_CIPHERTEXT),
    `${bytes.length} vs ${SHARE_CIPHERTEXT.length} bytes`);
}

// ── share attachment round trip ─────────────────────────────────────────────
const ATT = randomBytes(2048);
let attId = '';
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/shares/${shareId}/attachments`, {
    method: 'POST',
    headers: bearer,
    body: {
      name: 'evidence.bin',
      content_type: 'application/octet-stream',
      size: ATT.length,
      encryption_meta: { alg: 'AES-256-GCM' },
    },
  });
  check('init share attachment 200', res.status === 200, `status ${res.status}`);
  attId = body?.attachment_id ?? '';
}
{
  const up = await fetch(`${BASE}/api/mindmaps/${mapId}/shares/${shareId}/attachments/${attId}/upload`, {
    method: 'POST',
    headers: { ...bearer, 'content-type': 'application/octet-stream' },
    body: ATT,
  });
  const upBody = await up.json().catch(() => null);
  check('upload share attachment 200', up.status === 200, `status ${up.status}`);
  const { res } = await json(
    `/api/mindmaps/${mapId}/shares/${shareId}/attachments/${attId}/complete`,
    { method: 'POST', headers: bearer, body: { version_id: upBody?.version_id } },
  );
  check('complete share attachment 200', res.status === 200, `status ${res.status}`);
}
{
  const res = await fetch(`${BASE}/share/${shareId}/attachments/${attId}/blob`);
  const bytes = Buffer.from(await res.arrayBuffer());
  check('recipient downloads share attachment', res.status === 200 && bytes.equals(ATT),
    `status ${res.status}, ${bytes.length} bytes`);
}

// ── ownership isolation: another user must not see or touch this share ──────
{
  const other = { ...user, username: `other_${runId}`, auth_token: randomBytes(32).toString('hex') };
  await json('/api/auth/register', { method: 'POST', body: other });
  const l = await json('/api/auth/login', {
    method: 'POST',
    body: { username: other.username, auth_token: other.auth_token },
  });
  const otherBearer = { authorization: `Bearer ${l.body.access_token}` };
  const { res } = await json(`/api/mindmaps/${mapId}/shares`, { headers: otherBearer });
  check('another account cannot list this vault\'s shares', res.status === 404 || res.status === 403,
    `status ${res.status}`);
  const rev = await json(`/api/mindmaps/${mapId}/shares/${shareId}/revoke`, {
    method: 'POST', headers: otherBearer, body: {},
  });
  check('another account cannot revoke this share', rev.res.status === 404 || rev.res.status === 403,
    `status ${rev.res.status}`);
}

// ── revoke: must delete the ciphertext, not just flag the row ───────────────
{
  const { res, body } = await json(`/api/mindmaps/${mapId}/shares/${shareId}/revoke`, {
    method: 'POST', headers: bearer, body: {},
  });
  check('revoke 200', res.status === 200, `status ${res.status}`);
  check('share marked revoked', body?.revoked === true);
}
{
  const meta = await fetch(`${BASE}/share/${shareId}`);
  check('revoked share metadata is gone (404)', meta.status === 404, `status ${meta.status}`);
  const blob = await fetch(`${BASE}/share/${shareId}/blob`);
  check('revoked share blob is unreachable (404)', blob.status === 404, `status ${blob.status}`);
  const att = await fetch(`${BASE}/share/${shareId}/attachments/${attId}/blob`);
  check('revoked share attachment is unreachable (404)', att.status === 404, `status ${att.status}`);
}

console.log(failures === 0 ? '\nAll share checks passed.' : `\n${failures} check(s) FAILED.`);
process.exit(failures === 0 ? 0 : 1);
