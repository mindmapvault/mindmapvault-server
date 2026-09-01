#!/usr/bin/env node
// End-to-end test of the password-rotation contract, against a live backend.
//
// The server treats every ciphertext as opaque, so this test uses opaque
// stand-ins and checks the CONTRACT: the manifest lists exactly what must be
// rewritten, the transaction is all-or-nothing with complete coverage of
// vaults, notes and attachments, and every session issued before the
// rotation fails closed on writes and refresh. The real crypto path is
// covered by the browser test (tests/ui/), which drives the actual client.
//
// Design: docs/PASSWORD_ROTATION.md. Writes to the database it points at —
// run it against a disposable instance.
//
//   node tests/endpoints/password-rotation.mjs --base-url http://127.0.0.1:8090
import { randomBytes } from 'node:crypto';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const BASE = flag('--base-url', 'http://127.0.0.1:8090').replace(/\/$/, '');

const b64 = (n) => Buffer.from(randomBytes(n)).toString('base64');
const hex64 = () => randomBytes(32).toString('hex');

let failures = 0;
const check = (name, cond, detail = '') => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

async function req(path, { method = 'GET', token, body, raw } = {}) {
  const headers = {};
  if (token) headers.authorization = `Bearer ${token}`;
  if (body !== undefined && !raw) headers['content-type'] = 'application/json';
  const res = await fetch(BASE + path, {
    method,
    headers,
    body: raw ? body : body !== undefined ? JSON.stringify(body) : undefined,
  });
  let json = null;
  try { json = await res.json(); } catch { /* not json */ }
  return { status: res.status, body: json };
}

// ── Setup: an account with the awkward shapes on purpose ────────────────────

const username = `rotate${Date.now().toString().slice(-8)}`;
const T0 = hex64();                       // auth token under the "old password"
const S0 = b64(32);                       // old salt
const params = { m_cost: 65536, t_cost: 3, p_cost: 4 };

{
  const { status } = await req('/api/auth/register', {
    method: 'POST',
    body: {
      username,
      auth_token: T0,
      argon2_salt: S0,
      argon2_params: params,
      classical_public_key: b64(32),
      pq_public_key: b64(32),
      classical_priv_encrypted: b64(48),
      pq_priv_encrypted: b64(48),
    },
  });
  check('setup: account registered', status === 200, `status ${status}`);
}

const login = async (token) =>
  req('/api/auth/login', { method: 'POST', body: { username, auth_token: token } });

// Two sessions: session2 rotates; session1 plays the phone left signed in.
const s1 = (await login(T0)).body;
const s2 = (await login(T0)).body;
check('setup: two sessions signed in', !!s1?.access_token && !!s2?.access_token);
const auth2 = s2.access_token;

const mkVault = async (title) => {
  const { status, body } = await req('/api/mindmaps', {
    method: 'POST',
    token: auth2,
    body: {
      title_encrypted: title,
      eph_classical_public: b64(32),
      eph_pq_ciphertext: b64(64),
      wrapped_dek: b64(48),
    },
  });
  check(`setup: vault created`, status === 200, `status ${status}`);
  return body.id;
};

const TA = { v1: b64(24), v2: b64(24), v3: b64(24) };
const NA1 = b64(40);
const v1 = await mkVault(TA.v1);
const v2 = await mkVault(TA.v2);
const v3 = await mkVault(TA.v3);
await req(`/api/mindmaps/${v1}/meta`, { method: 'PUT', token: auth2, body: { vault_note_encrypted: NA1 } });

// Two blob uploads so v1 has version history to survive the rotation.
for (let i = 0; i < 2; i += 1) {
  const res = await fetch(`${BASE}/api/mindmaps/${v1}/upload`, {
    method: 'POST',
    headers: { authorization: `Bearer ${auth2}`, 'content-type': 'application/octet-stream' },
    body: randomBytes(256),
  });
  check(`setup: v1 blob upload ${i + 1}`, res.status === 200, `status ${res.status}`);
}

const attachmentMeta = (wrap, keyWrap) => ({
  format: 'cryptmind-attachment-v1',
  algorithm: 'aes-256-gcm',
  wrapped_key_b64: wrap,
  key_wrap: keyWrap,
});

const initAttachment = async (name, meta, encrypted = true) => {
  const { status, body } = await req(`/api/mindmaps/${v1}/attachments/init`, {
    method: 'POST',
    token: auth2,
    body: { name, content_type: 'application/octet-stream', size: 256, encrypted, encryption_meta: meta },
  });
  check(`setup: attachment ${name} init`, status === 200, `status ${status}`);
  return body.attachment_id;
};

const uploadAndComplete = async (attId, name) => {
  const up = await fetch(`${BASE}/api/mindmaps/${v1}/attachments/${attId}/upload`, {
    method: 'POST',
    headers: { authorization: `Bearer ${auth2}`, 'content-type': 'application/octet-stream' },
    body: randomBytes(256),
  });
  const upBody = await up.json();
  const done = await req(`/api/mindmaps/${v1}/attachments/${attId}/complete`, {
    method: 'POST',
    token: auth2,
    body: { version_id: upBody?.version_id },
  });
  check(`setup: attachment ${name} completed`, done.status === 200, `status ${done.status}`);
};

const W = { a1: b64(48), a2: b64(48), a3: b64(48), a4: b64(48) };
const a1 = await initAttachment('available.bin', attachmentMeta(W.a1, 'hkdf-attachment-v1'));
await uploadAndComplete(a1, 'available.bin');
const a2 = await initAttachment('pending.bin', attachmentMeta(W.a2, 'hkdf-attachment-v1'));
const a3 = await initAttachment('legacy.bin', attachmentMeta(W.a3, 'master-aes-256-gcm'));
await uploadAndComplete(a3, 'legacy.bin');
const a4 = await initAttachment('deleted.bin', attachmentMeta(W.a4, 'hkdf-attachment-v1'));
await req(`/api/mindmaps/${v1}/attachments/${a4}`, { method: 'DELETE', token: auth2 });
const a5 = await initAttachment('plain.bin', null, false);

// ── The manifest lists exactly what the transaction will demand ─────────────

console.log('\n── rotation manifest ──');
{
  const { status, body } = await req('/api/auth/rotation-manifest', { token: auth2 });
  check('manifest answers', status === 200, `status ${status}`);
  check('manifest: key_version 1', body?.key_version === 1);
  check('manifest: carries the live salt', body?.argon2_salt === S0);
  const vaultIds = (body?.vaults ?? []).map((v) => v.id).sort();
  check('manifest: all three vaults', JSON.stringify(vaultIds) === JSON.stringify([v1, v2, v3].sort()));
  check('manifest: v1 note included',
    body?.vaults?.find((v) => v.id === v1)?.vault_note_encrypted === NA1);
  const attIds = (body?.attachments ?? []).map((a) => a.id).sort();
  check('manifest: available + pending + legacy wraps listed',
    JSON.stringify(attIds) === JSON.stringify([a1, a2, a3].sort()), JSON.stringify(attIds));
  check('manifest: the deleted attachment is not demanded', !attIds.includes(a4));
  check('manifest: the unencrypted attachment is not demanded', !attIds.includes(a5));
  check('manifest: legacy wrap is labelled',
    body?.attachments?.find((a) => a.id === a3)?.encryption_meta?.key_wrap === 'master-aes-256-gcm');
}

// ── Failure cases: each must leave the database untouched ───────────────────

console.log('\n── rejected bundles ──');
const T1 = hex64();
const S1 = b64(32);
const TB = { v1: b64(24), v2: b64(24), v3: b64(24) };
const NB1 = b64(40);
const W2 = { a1: b64(48), a2: b64(48), a3: b64(48) };

const fullBundle = () => ({
  current_auth_token: T0,
  new_auth_token: T1,
  new_argon2_salt: S1,
  new_argon2_params: params,
  new_classical_priv_encrypted: b64(48),
  new_pq_priv_encrypted: b64(48),
  new_key_version: 2,
  updated_vaults: [
    { id: v1, title_encrypted: TB.v1, vault_note_encrypted: NB1 },
    { id: v2, title_encrypted: TB.v2, vault_note_encrypted: null },
    { id: v3, title_encrypted: TB.v3, vault_note_encrypted: null },
  ],
  updated_attachments: [
    { id: a1, wrapped_key_b64: W2.a1 },
    { id: a2, wrapped_key_b64: W2.a2 },
    { id: a3, wrapped_key_b64: W2.a3 },
  ],
});

const rotate = (bundle) =>
  req('/api/auth/rotate-credentials', { method: 'POST', token: auth2, body: bundle });

{
  const r = await rotate({ ...fullBundle(), current_auth_token: hex64() });
  check('wrong current password → 401', r.status === 401, `status ${r.status}`);
}
{
  const r = await rotate({ ...fullBundle(), new_key_version: 3 });
  check('stale key_version → 400 naming the expected value',
    r.status === 400 && /must be 2/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const b = fullBundle();
  b.updated_attachments = b.updated_attachments.filter((a) => a.id !== a2);
  const r = await rotate(b);
  check('missing pending attachment → 400',
    r.status === 400 && /attachment\(s\) missing/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const b = fullBundle();
  b.updated_attachments.push({ id: a4, wrapped_key_b64: b64(48) });
  const r = await rotate(b);
  check('deleted attachment in bundle → 400 unknown',
    r.status === 400 && /unknown attachment/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const b = fullBundle();
  b.updated_vaults = b.updated_vaults.filter((v) => v.id !== v2);
  const r = await rotate(b);
  check('missing vault → 400',
    r.status === 400 && /vault\(s\) missing/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const b = fullBundle();
  b.updated_vaults.push({ id: 'not-a-vault', title_encrypted: b64(24), vault_note_encrypted: null });
  const r = await rotate(b);
  check('unknown vault → 400',
    r.status === 400 && /unknown vault/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const b = fullBundle();
  b.updated_vaults = b.updated_vaults.map((v) =>
    v.id === v1 ? { ...v, vault_note_encrypted: null } : v);
  const r = await rotate(b);
  check('bundle that strands a note under the old key → 400',
    r.status === 400 && /note\(s\) under the old key/.test(r.body?.error ?? ''), JSON.stringify(r.body));
}

{
  // After all those rejections, nothing may have moved.
  const { body } = await req('/api/auth/rotation-manifest', { token: auth2 });
  check('after rejections: key_version still 1', body?.key_version === 1);
  check('after rejections: a1 wrap untouched',
    body?.attachments?.find((a) => a.id === a1)?.encryption_meta?.wrapped_key_b64 === W.a1);
  check('after rejections: titles untouched',
    body?.vaults?.find((v) => v.id === v1)?.title_encrypted === TA.v1);
  const relogin = await login(T0);
  check('after rejections: old password still signs in', relogin.status === 200);
}

// ── The rotation itself ─────────────────────────────────────────────────────

console.log('\n── committed rotation ──');
let rotatedTokens = null;
{
  const r = await rotate(fullBundle());
  check('full bundle commits', r.status === 200, JSON.stringify(r.body));
  check('rotation returns fresh tokens', !!r.body?.access_token && !!r.body?.refresh_token);
  rotatedTokens = r.body;
}

{
  const { body } = await req(`/api/auth/salt?username=${encodeURIComponent(username)}`);
  check('salt probe: live salt is the NEW salt', body?.argon2_salt === S1);
}
{
  const r = await login(T0);
  check('old password is dead', r.status === 401, `status ${r.status}`);
}
const s3 = await login(T1);
check('new password signs in', s3.status === 200, `status ${s3.status}`);
check('new session sees key_version 2', s3.body?.key_version === 2);
const auth3 = s3.body?.access_token;

{
  const { body } = await req('/api/auth/rotation-manifest', { token: auth3 });
  check('titles are the rotated ones',
    body?.vaults?.find((v) => v.id === v1)?.title_encrypted === TB.v1 &&
    body?.vaults?.find((v) => v.id === v3)?.title_encrypted === TB.v3);
  check('the note is the rotated one',
    body?.vaults?.find((v) => v.id === v1)?.vault_note_encrypted === NB1);
  const metaOf = (id) => body?.attachments?.find((a) => a.id === id)?.encryption_meta;
  check('a1 wrap replaced', metaOf(a1)?.wrapped_key_b64 === W2.a1);
  check('pending a2 wrap replaced', metaOf(a2)?.wrapped_key_b64 === W2.a2);
  check('legacy a3 upgraded to hkdf-attachment-v1',
    metaOf(a3)?.wrapped_key_b64 === W2.a3 && metaOf(a3)?.key_wrap === 'hkdf-attachment-v1');
  check('the rest of the attachment metadata survived the merge',
    metaOf(a1)?.format === 'cryptmind-attachment-v1' && metaOf(a1)?.algorithm === 'aes-256-gcm');
}
{
  const { status, body } = await req(`/api/mindmaps/${v1}/versions`, { token: auth3 });
  const count = Array.isArray(body?.versions) ? body.versions.length : (Array.isArray(body) ? body.length : 0);
  check('version history survived', status === 200 && count >= 2, `status ${status}, ${count} versions`);
}

// ── Every session from before the rotation fails closed ─────────────────────

console.log('\n── stale sessions ──');
{
  const r = await req('/api/auth/refresh', { method: 'POST', body: { refresh_token: s1.refresh_token } });
  check('stale device: refresh refused', r.status === 401, `status ${r.status}`);
}
{
  const r = await req(`/api/mindmaps/${v1}/attachments/init`, {
    method: 'POST',
    token: s1.access_token,
    body: { name: 'stale.bin', content_type: 'application/octet-stream', size: 16, encrypted: true, encryption_meta: attachmentMeta(b64(48), 'hkdf-attachment-v1') },
  });
  check('stale device: attachment write refused', r.status === 401, `status ${r.status}`);
  check('…with a message that says what to do', /sign in/i.test(r.body?.error ?? ''), JSON.stringify(r.body));
}
{
  const r = await req(`/api/mindmaps/${v1}/meta`, {
    method: 'PUT', token: s1.access_token, body: { vault_color: '#ff0000' },
  });
  check('stale device: vault write refused', r.status === 401, `status ${r.status}`);
}
{
  const r = await req('/api/mindmaps', { token: s1.access_token });
  check('stale device: reads still answer (harmless, and the app can show the sign-out)', r.status === 200, `status ${r.status}`);
}
{
  const r = await req('/api/auth/refresh', { method: 'POST', body: { refresh_token: s2.refresh_token } });
  check('the rotating session\'s OLD refresh token is dead too', r.status === 401, `status ${r.status}`);
}
{
  const r = await req(`/api/mindmaps/${v1}/meta`, {
    method: 'PUT', token: rotatedTokens.access_token, body: { vault_color: '#00ff00' },
  });
  check('the tokens returned by the rotation can write', r.status === 200, `status ${r.status}`);
}
{
  const r = await req('/api/auth/refresh', { method: 'POST', body: { refresh_token: rotatedTokens.refresh_token } });
  check('…and their refresh token renews', r.status === 200, `status ${r.status}`);
}

console.log(failures === 0 ? '\nAll password-rotation checks passed.' : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
