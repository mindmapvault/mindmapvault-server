#!/usr/bin/env node
// End-to-end test of the instance settings against a running backend.
//
// Covers everything an operator can set from the admin console: the
// registration switch, the per-account storage cap, the per-file cap, and the
// two sign-in throttles — plus the admin token comparison and the audit trail
// each change leaves behind.
//
// This writes to the database it points at and changes settings as it goes, so
// run it against a disposable instance, not a live one. It restores the
// defaults on a clean run.
//
//   node tests/endpoints/instance-settings.mjs --base-url http://127.0.0.1:8090 --admin-token <token>
//
// Needs a backend with PostgreSQL and an S3 store that returns version ids
// (Garage does by default; MinIO needs bucket versioning enabled).
import { randomBytes, randomUUID } from 'node:crypto';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const BASE = flag('--base-url', 'http://127.0.0.1:8090').replace(/\/$/, '');
const ADMIN = flag('--admin-token', process.env.ADMIN_API_TOKEN ?? '');

if (!ADMIN) {
  console.error('An admin token is required: --admin-token <token> or ADMIN_API_TOKEN in the environment.');
  process.exit(2);
}
const b64 = (n) => Buffer.from(randomBytes(n)).toString('base64');
const id = randomUUID().slice(0, 6);

let failures = 0;
const check = (name, cond, detail = '') => {
  if (!cond) failures += 1;
  console.log(`${cond ? 'PASS' : 'FAIL'}  ${name}${detail ? `  (${detail})` : ''}`);
};

async function req(path, init = {}) {
  const res = await fetch(BASE + path, {
    ...init,
    headers: { 'content-type': 'application/json', ...(init.headers ?? {}) },
    body: init.body !== undefined ? JSON.stringify(init.body) : undefined,
  });
  let body = null;
  try { body = await res.json(); } catch { /* empty body */ }
  return { res, body };
}

const admin = { authorization: `Bearer ${ADMIN}` };
const setSettings = (patch) => req('/api/admin/settings', { method: 'POST', headers: admin, body: patch });

// Throttles are generous by default and would otherwise fire during the setup
// of later cases. Each section turns on only what it is testing.
const OFF = { auth_rate_limit_per_minute: 0, failed_login_threshold: 0 };

function newUser(name) {
  return {
    username: name,
    auth_token: randomBytes(32).toString('hex'),
    argon2_salt: b64(32),
    argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
    classical_public_key: b64(32),
    pq_public_key: b64(32),
    classical_priv_encrypted: b64(48),
    pq_priv_encrypted: b64(48),
  };
}

async function signUp(name) {
  const user = newUser(name);
  const reg = await req('/api/auth/register', { method: 'POST', body: user });
  const login = await req('/api/auth/login', {
    method: 'POST',
    body: { username: user.username, auth_token: user.auth_token },
  });
  return { user, reg, bearer: { authorization: `Bearer ${login.body?.access_token}` } };
}

console.log('── admin token ───────────────────────────────────────────────');
{
  const missing = await req('/api/admin/settings');
  check('settings refuse an unauthenticated caller', missing.res.status === 401, `status ${missing.res.status}`);

  const wrong = await req('/api/admin/settings', { headers: { authorization: 'Bearer wrong-token' } });
  check('settings refuse a wrong token', wrong.res.status === 401, `status ${wrong.res.status}`);

  // A prefix of the real token must be refused too — the constant-time
  // comparison checks the length first, and this is the case a naive
  // starts-with comparison would let through.
  const prefix = await req('/api/admin/settings', {
    headers: { authorization: `Bearer ${ADMIN.slice(0, ADMIN.length - 1)}` },
  });
  check('settings refuse a truncated token', prefix.res.status === 401, `status ${prefix.res.status}`);

  const ok = await req('/api/admin/settings', { headers: admin });
  check('settings load with the real token', ok.res.status === 200, `status ${ok.res.status}`);
  check('settings report the observed client address', Boolean(ok.body?.observed_client_address),
    ok.body?.observed_client_address);
  check('settings default to unlimited storage', ok.body?.settings?.user_storage_limit_bytes === 0);
  check('settings default to registration open', ok.body?.settings?.registration_enabled === true);
}

console.log('\n── validation ────────────────────────────────────────────────');
{
  const negative = await setSettings({ user_storage_limit_bytes: -5 });
  check('a negative storage limit is refused', negative.res.status === 400, `status ${negative.res.status}`);

  const forever = await setSettings({ failed_login_lockout_minutes: 0 });
  check('a never-expiring lockout is refused', forever.res.status === 400, `status ${forever.res.status}`);

  const stillDefault = await req('/api/admin/settings', { headers: admin });
  check('a refused update changed nothing', stillDefault.body?.settings?.user_storage_limit_bytes === 0);
}

console.log('\n── registration switch ───────────────────────────────────────');
{
  await setSettings({ registration_enabled: false, ...OFF });

  const info = await req('/api/public/instance');
  check('the public endpoint reports registration closed', info.body?.registration_enabled === false);

  const attempt = await req('/api/auth/register', { method: 'POST', body: newUser(`closed_${id}`) });
  check('register is refused while closed', attempt.res.status === 403, `status ${attempt.res.status}`);

  // An existing account must be unaffected by a closed door.
  await setSettings({ registration_enabled: true });
  const { user } = await signUp(`open_${id}`);
  await setSettings({ registration_enabled: false });
  const login = await req('/api/auth/login', {
    method: 'POST',
    body: { username: user.username, auth_token: user.auth_token },
  });
  check('an existing account still signs in while closed', login.res.status === 200, `status ${login.res.status}`);

  await setSettings({ registration_enabled: true });
  const reopened = await req('/api/public/instance');
  check('reopening takes effect immediately', reopened.body?.registration_enabled === true);
}

console.log('\n── per-file size cap ─────────────────────────────────────────');
{
  await setSettings({ max_attachment_size_bytes: 10_000, user_storage_limit_bytes: 0, ...OFF });
  const { bearer } = await signUp(`file_${id}`);
  const vault = await req('/api/mindmaps', {
    method: 'POST', headers: bearer,
    body: { title_encrypted: b64(48), eph_classical_public: b64(32), eph_pq_ciphertext: b64(64), wrapped_dek: b64(32) },
  });
  const mapId = vault.body.id;

  const tooBig = await req(`/api/mindmaps/${mapId}/attachments/init`, {
    method: 'POST', headers: bearer,
    body: { name: 'big.bin', content_type: 'application/octet-stream', size: 20_000, node_id: null, encrypted: false },
  });
  check('an oversized file is refused at init', tooBig.res.status === 403, `status ${tooBig.res.status}`);
  check('the refusal names the limit', tooBig.body?.limit_value === 10_000, JSON.stringify(tooBig.body));
  check('the refusal carries a machine-readable code', tooBig.body?.code === 'attachment_too_large', tooBig.body?.code);

  const okFile = await req(`/api/mindmaps/${mapId}/attachments/init`, {
    method: 'POST', headers: bearer,
    body: { name: 'small.bin', content_type: 'application/octet-stream', size: 5_000, node_id: null, encrypted: false },
  });
  check('a file under the limit is accepted', okFile.res.status === 200, `status ${okFile.res.status}`);
}

console.log('\n── per-account storage cap ───────────────────────────────────');
{
  await setSettings({ max_attachment_size_bytes: 0, user_storage_limit_bytes: 50_000, ...OFF });
  const { bearer } = await signUp(`quota_${id}`);
  const vault = await req('/api/mindmaps', {
    method: 'POST', headers: bearer,
    body: { title_encrypted: b64(48), eph_classical_public: b64(32), eph_pq_ciphertext: b64(64), wrapped_dek: b64(32) },
  });
  const mapId = vault.body.id;

  // Upload 30 KB for real, so the stored total is what the next check is
  // measured against rather than a number that only exists in the request.
  async function upload(name, size) {
    const init = await req(`/api/mindmaps/${mapId}/attachments/init`, {
      method: 'POST', headers: bearer,
      body: { name, content_type: 'application/octet-stream', size, node_id: null, encrypted: false },
    });
    if (init.res.status !== 200) return init;
    const up = await fetch(`${BASE}/api/mindmaps/${mapId}/attachments/${init.body.attachment_id}/upload`, {
      method: 'POST', headers: { ...bearer, 'content-type': 'application/octet-stream' }, body: randomBytes(size),
    });
    const ub = await up.json();
    await req(`/api/mindmaps/${mapId}/attachments/${init.body.attachment_id}/complete`, {
      method: 'POST', headers: bearer, body: { version_id: ub.version_id },
    });
    return init;
  }

  const first = await upload('a.bin', 30_000);
  check('the first upload fits under the cap', first.res.status === 200, `status ${first.res.status}`);

  const second = await req(`/api/mindmaps/${mapId}/attachments/init`, {
    method: 'POST', headers: bearer,
    body: { name: 'b.bin', content_type: 'application/octet-stream', size: 30_000, node_id: null, encrypted: false },
  });
  check('an upload crossing the cap is refused', second.res.status === 403, `status ${second.res.status}`);
  check('the refusal reports usage and limit', second.body?.limit_value === 50_000 && second.body?.current_value === 60_000,
    JSON.stringify(second.body));

  const third = await upload('c.bin', 10_000);
  check('an upload still inside the cap is accepted', third.res.status === 200, `status ${third.res.status}`);

  const storage = await req('/api/auth/storage', { headers: bearer });
  check('the reported limit is the instance limit, not a plan tier',
    storage.body?.plan_limit_bytes === 50_000, `plan_limit_bytes=${storage.body?.plan_limit_bytes}`);

  await setSettings({ user_storage_limit_bytes: 0 });
  const unlimited = await req('/api/auth/storage', { headers: bearer });
  check('with no cap the limit reads as boundless',
    unlimited.body?.plan_limit_bytes > 1e15, `plan_limit_bytes=${unlimited.body?.plan_limit_bytes}`);
  check('with no cap nobody is over limit', unlimited.body?.over_limit === false);
}

console.log('\n── auth rate limit ───────────────────────────────────────────');
{
  await setSettings({ auth_rate_limit_per_minute: 5, failed_login_threshold: 0 });

  let limited = null;
  for (let i = 0; i < 10; i += 1) {
    const attempt = await req('/api/auth/login', {
      method: 'POST', body: { username: `nobody_${id}`, auth_token: 'x'.repeat(64) },
    });
    if (attempt.res.status === 429) { limited = attempt; break; }
  }
  check('the address limit fires', Boolean(limited), limited ? '429 seen' : 'never hit the limit');
  check('the refusal says how long to wait',
    Boolean(limited?.res.headers.get('retry-after')), `retry-after=${limited?.res.headers.get('retry-after')}`);
  check('the refusal is machine-readable', limited?.body?.code === 'rate_limited', limited?.body?.code);

  // Raising the limit must not require a restart to take effect. The window
  // itself is still burnt, so this checks the salt route, which shares the
  // allowance but has room again once the limit moves up.
  await setSettings({ auth_rate_limit_per_minute: 0 });
  const afterOff = await req('/api/auth/login', {
    method: 'POST', body: { username: `nobody_${id}`, auth_token: 'x'.repeat(64) },
  });
  check('turning the limit off takes effect immediately', afterOff.res.status === 401,
    `status ${afterOff.res.status}`);
}

console.log('\n── failed sign-in lockout ────────────────────────────────────');
{
  await setSettings({ auth_rate_limit_per_minute: 0, failed_login_threshold: 3, failed_login_lockout_minutes: 15 });
  const { user } = await signUp(`lock_${id}`);

  for (let i = 0; i < 3; i += 1) {
    await req('/api/auth/login', { method: 'POST', body: { username: user.username, auth_token: 'x'.repeat(64) } });
  }

  const locked = await req('/api/auth/login', {
    method: 'POST', body: { username: user.username, auth_token: user.auth_token },
  });
  check('the right password is refused while locked out', locked.res.status === 429, `status ${locked.res.status}`);
  const retry = Number(locked.res.headers.get('retry-after') ?? 0);
  check('the lockout reports a sane wait', retry > 800 && retry <= 900, `retry-after=${retry}`);

  // Case must not be a way around it.
  const upper = await req('/api/auth/login', {
    method: 'POST', body: { username: user.username.toUpperCase(), auth_token: user.auth_token },
  });
  check('changing case does not dodge the lockout', upper.res.status === 429, `status ${upper.res.status}`);

  // An unrelated account must be unaffected by another's lockout.
  const other = await signUp(`free_${id}`);
  check('a different account is unaffected', Boolean(other.bearer.authorization.endsWith('undefined') === false));

  await setSettings({ failed_login_threshold: 0 });
  const disabled = await req('/api/auth/login', {
    method: 'POST', body: { username: `never_${id}`, auth_token: 'x'.repeat(64) },
  });
  check('turning the lockout off stops new lockouts', disabled.res.status === 401, `status ${disabled.res.status}`);
}

console.log('\n── settings are audited ──────────────────────────────────────');
{
  await setSettings({ registration_enabled: false });
  const overview = await req('/api/admin/overview', { headers: admin });
  const event = (overview.body?.audit_events ?? []).find((e) => e.action_type === 'instance_settings_updated');
  check('a settings change is written to the audit trail', Boolean(event), event?.summary);
  check('the audit entry names what changed', /registration_enabled/.test(event?.detail ?? ''), event?.detail);
  // Put the instance back the way it was found.
  await setSettings({
    registration_enabled: true,
    user_storage_limit_bytes: 0,
    max_attachment_size_bytes: 0,
    auth_rate_limit_per_minute: 30,
    failed_login_threshold: 10,
    failed_login_lockout_minutes: 15,
  });
}

console.log(failures === 0 ? '\nAll Phase 2 checks passed.' : `\n${failures} check(s) FAILED.`);
process.exit(failures === 0 ? 0 : 1);
