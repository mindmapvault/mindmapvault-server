#!/usr/bin/env node
// End-to-end test of invite codes and the admin status endpoint.
//
// Invites are what keeps closing sign-ups from locking the operator out of
// their own server: an admin cannot create an account directly, because
// registration is zero-knowledge and the password never reaches the server.
// So this covers the whole path — generate a code, close sign-ups, sign up
// with it, and confirm it cannot be used twice.
//
// Writes to the database it points at and changes settings as it goes, so run
// it against a disposable instance. It restores the defaults on a clean run.
//
//   node tests/endpoints/invites.mjs --base-url http://127.0.0.1:8090 --admin-token <token>
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
const check = (n, c, d = '') => {
  if (!c) failures += 1;
  console.log(`${c ? 'PASS' : 'FAIL'}  ${n}${d ? `  (${d})` : ''}`);
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

function signUpBody(name, inviteCode) {
  return {
    username: name,
    auth_token: randomBytes(32).toString('hex'),
    argon2_salt: b64(32),
    argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
    classical_public_key: b64(32),
    pq_public_key: b64(32),
    classical_priv_encrypted: b64(48),
    pq_priv_encrypted: b64(48),
    ...(inviteCode === undefined ? {} : { invite_code: inviteCode }),
  };
}

// Throttles are generous by default but this script makes a lot of auth calls.
await setSettings({ auth_rate_limit_per_minute: 0, failed_login_threshold: 0 });

console.log('── status ────────────────────────────────────────────────────');
{
  const anon = await req('/api/admin/status');
  check('status needs the admin token', anon.res.status === 401, `status ${anon.res.status}`);

  const { res, body } = await req('/api/admin/status', { headers: admin });
  check('status loads', res.status === 200, `status ${res.status}`);
  check('it reports a version', typeof body?.version === 'string' && body.version.length > 0, body?.version);
  check('it reports uptime', Number.isFinite(body?.uptime_seconds), `${body?.uptime_seconds}`);
  check('the database reports reachable', body?.database?.reachable === true, JSON.stringify(body?.database));
  check('object storage reports reachable', body?.object_storage?.reachable === true, JSON.stringify(body?.object_storage));
  check('it names the storage bucket', Boolean(body?.storage_bucket), body?.storage_bucket);
  check('it counts what is stored', Number.isFinite(body?.totals?.accounts), JSON.stringify(body?.totals));

  // Disk. The single most useful figure on the page for someone running this
  // at home, so it has to be present and internally consistent.
  const disk = body?.server?.disk;
  check('it measures free disk space', Boolean(disk), JSON.stringify(body?.server));
  check('the disk figures are sane',
    disk && disk.total_bytes > 0 && disk.used_bytes <= disk.total_bytes
      && disk.available_bytes <= disk.total_bytes,
    JSON.stringify(disk));
  check('it says which filesystem it measured', Boolean(disk?.path), disk?.path);
  check('and reports it as a percentage',
    Number.isInteger(body?.server?.disk_used_percent)
      && body.server.disk_used_percent >= 0 && body.server.disk_used_percent <= 100,
    `${body?.server?.disk_used_percent}`);

  // Memory is Linux-only; on other platforms it must be absent rather than 0,
  // which would read as "using no memory".
  check('memory is a real figure or absent, never a misleading zero',
    body?.server?.memory_bytes === null || body?.server?.memory_bytes === undefined
      || body.server.memory_bytes > 0,
    `${body?.server?.memory_bytes}`);

  check('the database reports its version', Boolean(body?.database_stats?.version), body?.database_stats?.version);
  check('and its size on disk', body?.database_stats?.size_bytes > 0, `${body?.database_stats?.size_bytes}`);

  check('the bucket is measured', Boolean(body?.bucket_stats), JSON.stringify(body?.bucket_stats));
  check('the bucket scan completed rather than truncating',
    body?.bucket_stats?.truncated === false, `truncated=${body?.bucket_stats?.truncated}`);
  check('it counts objects and bytes',
    Number.isFinite(body?.bucket_stats?.object_count) && Number.isFinite(body?.bucket_stats?.size_bytes),
    JSON.stringify(body?.bucket_stats));

  // Sign-ups are open at this point, so the warning about that must be present:
  // an operator who never opens Settings should still be told.
  await setSettings({ registration_enabled: true, user_storage_limit_bytes: 0 });
  const open = await req('/api/admin/status', { headers: admin });
  const codes = (open.body?.warnings ?? []).map((w) => w.code);
  check('an open, unlimited instance is warned about', codes.includes('open_and_unlimited'), codes.join(','));

  // A proxy header that is being ignored is only worth raising when one is
  // actually arriving.
  const proxied = await req('/api/admin/status', { headers: { ...admin, 'x-forwarded-for': '203.0.113.9' } });
  const proxyCodes = (proxied.body?.warnings ?? []).map((w) => w.code);
  check('an ignored proxy header is flagged', proxyCodes.includes('proxy_header_ignored'), proxyCodes.join(','));
}

console.log('\n── creating invites ──────────────────────────────────────────');
let inviteCode = null;
let inviteId = null;
{
  const created = await req('/api/admin/invites', {
    method: 'POST', headers: admin, body: { label: `test ${id}`, expires_in_days: 7 },
  });
  check('an invite can be created', created.res.status === 200, `status ${created.res.status}`);

  const invite = (created.body?.invites ?? []).find((item) => item.label === `test ${id}`);
  check('the new invite comes back', Boolean(invite), JSON.stringify(created.body).slice(0, 120));
  inviteCode = invite?.code;
  inviteId = invite?.id;
  check('the code is in the readable MMV- shape', /^MMV(-[ACDEFGHJKMNPQRTWXY34679]{4}){4}$/.test(inviteCode ?? ''), inviteCode);
  check('a new invite is open', invite?.status === 'open', invite?.status);
  check('the response says where to send someone', /\/register$/.test(created.body?.register_url ?? ''), created.body?.register_url);

  const bad = await req('/api/admin/invites', {
    method: 'POST', headers: admin, body: { expires_in_days: 9999 },
  });
  check('an absurd expiry is refused', bad.res.status === 400, `status ${bad.res.status}`);
}

console.log('\n── redeeming an invite ───────────────────────────────────────');
{
  await setSettings({ registration_enabled: false });

  const info = await req('/api/public/instance');
  check('the app is told a code is needed', info.body?.invite_required === true, JSON.stringify(info.body));

  const noCode = await req('/api/auth/register', { method: 'POST', body: signUpBody(`nocode_${id}`) });
  check('signing up without a code is refused', noCode.res.status === 403, `status ${noCode.res.status}`);

  const wrongCode = await req('/api/auth/register', {
    method: 'POST', body: signUpBody(`wrong_${id}`, 'MMV-AAAA-AAAA-AAAA-AAAA'),
  });
  check('a wrong code is refused', wrongCode.res.status === 403, `status ${wrongCode.res.status}`);
  check('the refusal does not say why', /not valid/.test(wrongCode.body?.error ?? ''), wrongCode.body?.error);

  // Typed the way a person would after reading it off a screen.
  const retyped = inviteCode.toLowerCase().replace(/-/g, ' ');
  const ok = await req('/api/auth/register', { method: 'POST', body: signUpBody(`invited_${id}`, retyped) });
  check('a retyped code still works', ok.res.status === 200, `status ${ok.res.status} using "${retyped}"`);

  const reuse = await req('/api/auth/register', { method: 'POST', body: signUpBody(`reuse_${id}`, inviteCode) });
  check('the same code cannot be used twice', reuse.res.status === 403, `status ${reuse.res.status}`);

  const listed = await req('/api/admin/invites', { headers: admin });
  const used = (listed.body?.invites ?? []).find((item) => item.id === inviteId);
  check('the invite is marked used', used?.status === 'used', used?.status);
  check('it records who used it', used?.used_by_username === `invited_${id}`, used?.used_by_username);
}

console.log('\n── an invite is not burnt by a failed sign-up ────────────────');
{
  const created = await req('/api/admin/invites', {
    method: 'POST', headers: admin, body: { label: `clash ${id}` },
  });
  const invite = (created.body?.invites ?? []).find((item) => item.label === `clash ${id}`);
  check('a second invite exists', Boolean(invite?.code), invite?.code);
  check('no expiry when none was asked for', invite?.expires_at === null || invite?.expires_at === undefined,
    `expires_at=${invite?.expires_at}`);

  // Claim it for a username that is already taken: the sign-up fails, and the
  // invite has to survive for the person it was meant for.
  const clash = await req('/api/auth/register', {
    method: 'POST', body: signUpBody(`invited_${id}`, invite.code),
  });
  check('a taken username still fails', clash.res.status === 409, `status ${clash.res.status}`);

  const afterClash = await req('/api/admin/invites', { headers: admin });
  const stillOpen = (afterClash.body?.invites ?? []).find((item) => item.id === invite.id);
  check('the invite was handed back, not spent', stillOpen?.status === 'open', stillOpen?.status);

  const retry = await req('/api/auth/register', {
    method: 'POST', body: signUpBody(`retry_${id}`, invite.code),
  });
  check('so it still works for the right person', retry.res.status === 200, `status ${retry.res.status}`);
}

console.log('\n── revoking ──────────────────────────────────────────────────');
{
  const created = await req('/api/admin/invites', {
    method: 'POST', headers: admin, body: { label: `revoke ${id}` },
  });
  const invite = (created.body?.invites ?? []).find((item) => item.label === `revoke ${id}`);

  const revoked = await req(`/api/admin/invites/${invite.id}`, { method: 'DELETE', headers: admin });
  check('an invite can be revoked', revoked.res.status === 200, `status ${revoked.res.status}`);
  check('it is gone from the list', !(revoked.body?.invites ?? []).some((item) => item.id === invite.id));

  const used = await req('/api/auth/register', { method: 'POST', body: signUpBody(`revoked_${id}`, invite.code) });
  check('a revoked code no longer works', used.res.status === 403, `status ${used.res.status}`);

  const missing = await req(`/api/admin/invites/${invite.id}`, { method: 'DELETE', headers: admin });
  check('revoking it twice is a clean 404', missing.res.status === 404, `status ${missing.res.status}`);
}

console.log('\n── invites are ignored while sign-ups are open ───────────────');
{
  await setSettings({ registration_enabled: true });
  const created = await req('/api/admin/invites', {
    method: 'POST', headers: admin, body: { label: `unused ${id}` },
  });
  const invite = (created.body?.invites ?? []).find((item) => item.label === `unused ${id}`);

  const open = await req('/api/auth/register', { method: 'POST', body: signUpBody(`open_${id}`, invite.code) });
  check('a sign-up with a code succeeds while open', open.res.status === 200, `status ${open.res.status}`);

  const after = await req('/api/admin/invites', { headers: admin });
  const stillOpen = (after.body?.invites ?? []).find((item) => item.id === invite.id);
  check('but the code was not spent on it', stillOpen?.status === 'open', stillOpen?.status);

  await req(`/api/admin/invites/${invite.id}`, { method: 'DELETE', headers: admin });
}

console.log('\n── maintenance ───────────────────────────────────────────────');
{
  const anon = await req('/api/admin/maintenance/purge-shares', { method: 'POST' });
  check('the cleanup needs the admin token', anon.res.status === 401, `status ${anon.res.status}`);

  const run = await req('/api/admin/maintenance/purge-shares', { method: 'POST', headers: admin });
  check('the cleanup can be run on demand', run.res.status === 200, `status ${run.res.status}`);
  check('it reports how many it cleared', Number.isFinite(run.body?.cleared), `${run.body?.cleared}`);
  check('and when it last ran', Boolean(run.body?.purge?.last_run_at), run.body?.purge?.last_run_at);
}

console.log('\n── the removed plan endpoints are gone ───────────────────────');
{
  const overview = await req('/api/admin/overview', { headers: admin });
  const user = (overview.body?.users ?? [])[0];
  check('the overview still lists accounts', Boolean(user), JSON.stringify(overview.body?.metrics));
  check('no Stripe fields are exposed', user && !('stripe_customer_id' in user), Object.keys(user ?? {}).join(','));
  check('no plan tier is exposed', user && !('effective_subscription_tier' in user));
  check('metrics carry no billing counts', overview.body?.metrics && !('paid_users' in overview.body.metrics),
    Object.keys(overview.body?.metrics ?? {}).join(','));

  // A path the admin router no longer claims falls through the nest to the SPA
  // static service, which answers a POST with 405 rather than 404. Either way
  // nothing handles it any more, which is what this is checking.
  const gone = (status) => status === 404 || status === 405;

  const planOverride = await req(`/api/admin/users/${user.id}/plan-override`, {
    method: 'POST', headers: admin, body: { manual_subscription_tier: 'paid' },
  });
  check('the plan-override endpoint no longer exists', gone(planOverride.res.status), `status ${planOverride.res.status}`);

  const grants = await req(`/api/admin/users/${user.id}/access-grants`, {
    method: 'POST', headers: admin, body: { access_grants: [] },
  });
  check('the access-grants endpoint no longer exists', gone(grants.res.status), `status ${grants.res.status}`);
}

// Put the instance back the way it was found.
await setSettings({
  registration_enabled: true,
  user_storage_limit_bytes: 0,
  max_attachment_size_bytes: 0,
  auth_rate_limit_per_minute: 30,
  failed_login_threshold: 10,
  failed_login_lockout_minutes: 15,
});

console.log(failures === 0 ? '\nAll invite and status checks passed.' : `\n${failures} check(s) FAILED.`);
process.exit(failures === 0 ? 0 : 1);
