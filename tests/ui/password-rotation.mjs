#!/usr/bin/env node
// Password rotation with the REAL client crypto, end to end in a browser.
//
// The endpoint test (tests/endpoints/password-rotation.mjs) proves the server
// contract with opaque ciphertexts; this one proves the crypto: it signs up
// through the UI, stores two attachments encrypted by the app's own modules —
// one current `hkdf-attachment-v1` wrap, one hand-built legacy
// `master-aes-256-gcm` wrap — changes the password through the settings hub,
// signs back in with the new password, and decrypts both attachments and the
// vault title under the new keys. Needs the app served by the VITE DEV
// SERVER (module imports drive the in-page crypto), plus Playwright.
//
//   node tests/ui/password-rotation.mjs --app-url http://127.0.0.1:5173
import { chromium } from 'playwright';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const APP = flag('--app-url', 'http://127.0.0.1:5173').replace(/\/$/, '');

let failures = 0;
const check = (n, c, d = '') => {
  if (!c) failures += 1;
  console.log(`${c ? 'PASS' : 'FAIL'}  ${n}${d ? `  (${d})` : ''}`);
};

const username = `rotui${Date.now().toString().slice(-8)}`;
const oldPassword = 'the-original-password-123';
const newPassword = 'the-rotated-password-456';
const marker = `attachment-rotation-proof-${Date.now()}`;
const legacyMarker = `legacy-wrap-proof-${Date.now()}`;

const browser = await chromium.launch();
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(String(e).slice(0, 200)));

const signIn = async (password) => {
  await page.goto(`${APP}/login`);
  await page.waitForTimeout(1200);
  await page.locator('input[autocomplete="username"]').first().fill(username);
  await page.locator('input[type="password"]').first().fill(password);
  await page.getByRole('button', { name: /sign in|log in|unlock/i }).first().click();
  await page.waitForTimeout(9000);
};

// ── Sign up and make a vault ────────────────────────────────────────────────
await page.goto(`${APP}/register`);
await page.waitForTimeout(1200);
await page.locator('input[autocomplete="username"]').first().fill(username);
const pw = page.locator('input[type="password"]');
await pw.nth(0).fill(oldPassword);
if (await pw.count() > 1) await pw.nth(1).fill(oldPassword);
await page.getByRole('button', { name: /create|sign up|register/i }).first().click();
await page.waitForTimeout(9000);
if (/\/login/.test(page.url())) await signIn(oldPassword);
check('signed up and reached the vault list', /\/vaults/.test(page.url()), page.url());

await page.getByRole('button', { name: /^New/ }).first().click();
await page.waitForTimeout(500);
await page.getByText('Mind map', { exact: true }).first().click();
await page.waitForTimeout(500);
await page.locator('input[placeholder="Vault name..."]').fill('Rotation proof');
await page.getByRole('button', { name: /^Create$/ }).click();
await page.waitForTimeout(7000);
const mapId = (page.url().match(/\/vaults\/([0-9a-f-]{36})/) ?? [])[1];
check('created a vault', !!mapId, page.url());

// ── Store two attachments with the app's own crypto ─────────────────────────
// Both wrapped keys derive from the master key, which is exactly what a
// password change replaces. The second wrap is hand-built in the legacy
// format so the rotation's upgrade path is exercised with real keys.
const stored = await page.evaluate(async ({ mapId, marker, legacyMarker }) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const { encryptAttachmentForOwner, sha256Hex } = await import('/src/crypto/encryptedVault.ts');
  const { deriveMasterAesKey } = await import('/src/crypto/kdf.ts');
  const { aesEncrypt, importAesKey } = await import('/src/crypto/aes.ts');
  const { toBase64 } = await import('/src/crypto/utils.ts');

  const st = useAuthStore.getState();
  const token = st.accessToken;
  const masterKey = st.sessionKeys?.masterKey;
  if (!token || !masterKey) return { error: 'no session in the page' };
  const headers = { authorization: `Bearer ${token}` };

  const put = async (name, ciphertext, encryptionMeta) => {
    const init = await fetch(`/api/mindmaps/${mapId}/attachments/init`, {
      method: 'POST',
      headers: { ...headers, 'content-type': 'application/json' },
      body: JSON.stringify({
        name,
        content_type: 'application/octet-stream',
        size: ciphertext.length,
        encrypted: true,
        encryption_meta: encryptionMeta,
      }),
    }).then((r) => r.json());
    const up = await fetch(`/api/mindmaps/${mapId}/attachments/${init.attachment_id}/upload`, {
      method: 'POST',
      headers: { ...headers, 'content-type': 'application/octet-stream' },
      body: ciphertext,
    }).then((r) => r.json());
    const done = await fetch(`/api/mindmaps/${mapId}/attachments/${init.attachment_id}/complete`, {
      method: 'POST',
      headers: { ...headers, 'content-type': 'application/json' },
      body: JSON.stringify({ version_id: up.version_id }),
    });
    return done.ok ? init.attachment_id : null;
  };

  // Current-format attachment, exactly as the editor stores one.
  const modern = await encryptAttachmentForOwner(new TextEncoder().encode(marker), masterKey);
  const modernId = await put('proof.bin', modern.ciphertext, modern.encryptionMeta);

  // Legacy-format attachment: raw master key as the wrap key, the pre-0.3.22 shape.
  const fileKey = crypto.getRandomValues(new Uint8Array(32));
  const legacyCt = await aesEncrypt(await importAesKey(fileKey), new TextEncoder().encode(legacyMarker));
  const legacyWrapKey = await deriveMasterAesKey(masterKey);
  const wrapped = await aesEncrypt(legacyWrapKey, fileKey);
  const legacyId = await put('legacy.bin', legacyCt, {
    format: 'cryptmind-attachment-v1',
    algorithm: 'aes-256-gcm',
    wrapped_key_b64: toBase64(wrapped),
    key_wrap: 'master-aes-256-gcm',
  });

  return { modernId, legacyId, checksum: await sha256Hex(modern.ciphertext) };
}, { mapId, marker, legacyMarker });
check('stored a modern-wrap attachment with the app\'s crypto', !!stored.modernId, JSON.stringify(stored));
check('stored a hand-built legacy-wrap attachment', !!stored.legacyId);

// ── Change the password through the settings hub ────────────────────────────
await page.goBack();
await page.waitForTimeout(2500);
await page.getByTestId('open-settings').first().click();
await page.waitForTimeout(1000);
check('the Account tab carries the rotation form',
  await page.getByTestId('rotation-submit').isVisible().catch(() => false));

await page.locator('input[placeholder="Current password"]').fill(oldPassword);
await page.locator('input[placeholder^="New password"]').fill(newPassword);
await page.locator('input[placeholder="Confirm new password"]').fill(newPassword);
await page.getByTestId('rotation-submit').click();
const doneMsg = page.getByTestId('rotation-done');
await doneMsg.waitFor({ state: 'visible', timeout: 60000 }).catch(() => {});
check('the form reports the rotation done', await doneMsg.isVisible().catch(() => false),
  await page.getByTestId('rotation-error').innerText().catch(() => 'no error shown'));

// The rotating session must keep working without a reload: its store now
// holds the new master key and the new tokens.
await page.keyboard.press('Escape');
await page.waitForTimeout(600);
const liveTitle = await page.getByText('Rotation proof').first().isVisible().catch(() => false);
check('the rotating session still shows the vault by name', liveTitle);

// ── The old password is dead; the new one opens everything ──────────────────
await page.getByRole('button', { name: /^Log out$/i }).first().click();
await page.waitForTimeout(2000);

await signIn(oldPassword);
check('the old password no longer signs in', !/\/vaults/.test(page.url()), page.url());

await signIn(newPassword);
check('the new password signs in', /\/vaults/.test(page.url()), page.url());
check('the vault title decrypts under the new key',
  await page.getByText('Rotation proof').first().isVisible().catch(() => false));

const decrypted = await page.evaluate(async ({ mapId, modernId, legacyId }) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const { decryptAttachmentForOwner } = await import('/src/crypto/encryptedVault.ts');
  const st = useAuthStore.getState();
  const headers = { authorization: `Bearer ${st.accessToken}` };
  const masterKey = st.sessionKeys?.masterKey;
  if (!masterKey) return { error: 'no master key after re-login' };

  const read = async (attId) => {
    const meta = await fetch(`/api/mindmaps/${mapId}/attachments/${attId}`, { headers }).then((r) => r.json());
    const blob = await fetch(`/api/mindmaps/${mapId}/attachments/${attId}/blob`, { headers });
    const ct = new Uint8Array(await blob.arrayBuffer());
    const pt = await decryptAttachmentForOwner(ct, meta.encryption_meta, masterKey);
    return { text: new TextDecoder().decode(pt), keyWrap: meta.encryption_meta?.key_wrap };
  };

  try {
    return { modern: await read(modernId), legacy: await read(legacyId) };
  } catch (e) {
    return { error: String(e).slice(0, 300) };
  }
}, { mapId, modernId: stored.modernId, legacyId: stored.legacyId });

check('the modern attachment decrypts under the NEW password',
  decrypted.modern?.text === marker, JSON.stringify(decrypted).slice(0, 200));
check('the legacy attachment decrypts too', decrypted.legacy?.text === legacyMarker);
check('…and its wrap was upgraded to hkdf-attachment-v1',
  decrypted.legacy?.keyWrap === 'hkdf-attachment-v1', decrypted.legacy?.keyWrap);
check('no page errors', errors.length === 0, errors.join(' | '));

await browser.close();
console.log(failures === 0 ? '\nAll rotation-crypto checks passed.' : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
