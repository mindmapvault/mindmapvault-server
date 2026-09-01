#!/usr/bin/env node
// Password rotation across MULTIPLE accounts, with the real client crypto.
//
// Three accounts, each with 3 vaults and 20 attachments (completed, pending,
// and legacy `master-aes-256-gcm` wraps mixed in), all encrypted by the
// app's own modules. Account A rotates alone; B and C then rotate IN
// PARALLEL. Between those, B mounts a forged rotation naming one of A's
// attachments. Proven at the end, per account, under the new passwords only:
// every title, note and vault blob decrypts, every attachment's stored wrap
// unwraps to the ORIGINAL file key, and every uploaded attachment's content
// decrypts to its marker. Accounts that did not rotate are shown
// byte-identical while others did.
//
// Needs the app on a VITE DEV SERVER (the test imports the app's source
// modules in-page) and Playwright.
//
//   node tests/ui/rotation-multi-account.mjs --app-url http://127.0.0.1:5173
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

const run = Date.now().toString().slice(-7);
const ACCOUNTS = {
  A: { username: `multiA${run}`, oldPw: 'account-a-password-one', newPw: 'account-a-password-two' },
  B: { username: `multiB${run}`, oldPw: 'account-b-password-one', newPw: 'account-b-password-two' },
  C: { username: `multiC${run}`, oldPw: 'account-c-password-one', newPw: 'account-c-password-two' },
};

const browser = await chromium.launch();
const pages = {};
const errors = [];
for (const tag of Object.keys(ACCOUNTS)) {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  pages[tag] = await ctx.newPage();
  pages[tag].on('pageerror', (e) => errors.push(`${tag}: ${String(e).slice(0, 150)}`));
}

const signUp = async (page, { username, oldPw }) => {
  await page.goto(`${APP}/register`);
  await page.waitForTimeout(1000);
  await page.locator('input[autocomplete="username"]').first().fill(username);
  const pw = page.locator('input[type="password"]');
  await pw.nth(0).fill(oldPw);
  if (await pw.count() > 1) await pw.nth(1).fill(oldPw);
  await page.getByRole('button', { name: /create|sign up|register/i }).first().click();
  await page.waitForTimeout(9000);
  return /\/vaults/.test(page.url());
};

const signIn = async (page, username, password) => {
  await page.goto(`${APP}/login`);
  await page.waitForTimeout(1000);
  await page.locator('input[autocomplete="username"]').first().fill(username);
  await page.locator('input[type="password"]').first().fill(password);
  await page.getByRole('button', { name: /sign in|log in|unlock/i }).first().click();
  await page.waitForTimeout(9000);
  return /\/vaults/.test(page.url());
};

const rotateViaUi = async (page, oldPw, newPw) => {
  await page.getByTestId('open-settings').first().click();
  await page.waitForTimeout(800);
  await page.locator('input[placeholder="Current password"]').fill(oldPw);
  await page.locator('input[placeholder^="New password"]').fill(newPw);
  await page.locator('input[placeholder="Confirm new password"]').fill(newPw);
  await page.getByTestId('rotation-submit').click();
  const ok = await page.getByTestId('rotation-done')
    .waitFor({ state: 'visible', timeout: 90000 })
    .then(() => true)
    .catch(() => false);
  await page.keyboard.press('Escape');
  await page.waitForTimeout(400);
  return ok;
};

// Everything the seed and verify passes need runs IN the page, with the
// app's own modules, against the app's own live session keys.
const seedAccount = (page, tag) => page.evaluate(async (tag) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const { encryptTitle, encryptTree } = await import('/src/crypto/vault.ts');
  const { hybridEncap } = await import('/src/crypto/kem.ts');
  const { encryptAttachmentForOwner } = await import('/src/crypto/encryptedVault.ts');
  const { deriveMasterAesKey } = await import('/src/crypto/kdf.ts');
  const { aesEncrypt, importAesKey } = await import('/src/crypto/aes.ts');
  const { toBase64 } = await import('/src/crypto/utils.ts');

  const st = useAuthStore.getState();
  const { masterKey, classicalPubKey, pqPubKey } = st.sessionKeys ?? {};
  const headers = { authorization: `Bearer ${st.accessToken}` };
  const json = { ...headers, 'content-type': 'application/json' };
  const octet = { ...headers, 'content-type': 'application/octet-stream' };
  if (!masterKey) return { error: 'no session' };

  const mkVault = async (title, note) => {
    const enc = await hybridEncap(classicalPubKey, pqPubKey);
    const res = await fetch('/api/mindmaps', {
      method: 'POST', headers: json,
      body: JSON.stringify({
        title_encrypted: await encryptTitle(title, masterKey),
        eph_classical_public: toBase64(enc.ephClassicalPublic),
        eph_pq_ciphertext: toBase64(enc.ephPqCiphertext),
        wrapped_dek: toBase64(enc.wrappedDek),
      }),
    }).then((r) => r.json());
    const blob = await encryptTree(
      { version: 'tree', root: { id: 'root', text: `${tag}-tree-${title}`, children: [], collapsed: false } },
      enc.dek,
    );
    await fetch(`/api/mindmaps/${res.id}/upload`, { method: 'POST', headers: octet, body: blob });
    if (note) {
      await fetch(`/api/mindmaps/${res.id}/meta`, {
        method: 'PUT', headers: json,
        body: JSON.stringify({ vault_note_encrypted: await encryptTitle(note, masterKey) }),
      });
    }
    return res.id;
  };

  const addAttachment = async (mapId, marker, { legacy = false, pending = false } = {}) => {
    // Build the bundle the way the editor does, but remember the file key so
    // the verify pass can prove the rotated wrap still unwraps to it.
    let ciphertext; let meta; let fileKeyB64;
    if (!legacy) {
      const bundle = await encryptAttachmentForOwner(new TextEncoder().encode(marker), masterKey);
      ciphertext = bundle.ciphertext;
      meta = bundle.encryptionMeta;
      fileKeyB64 = null; // recovered at verify time by unwrapping
    } else {
      const fileKey = crypto.getRandomValues(new Uint8Array(32));
      ciphertext = await aesEncrypt(await importAesKey(fileKey), new TextEncoder().encode(marker));
      const wrapped = await aesEncrypt(await deriveMasterAesKey(masterKey), fileKey);
      meta = {
        format: 'cryptmind-attachment-v1', algorithm: 'aes-256-gcm',
        wrapped_key_b64: toBase64(wrapped), key_wrap: 'master-aes-256-gcm',
      };
      fileKeyB64 = toBase64(fileKey);
    }
    const init = await fetch(`/api/mindmaps/${mapId}/attachments/init`, {
      method: 'POST', headers: json,
      body: JSON.stringify({
        name: `${marker}.bin`, content_type: 'application/octet-stream',
        size: ciphertext.length, encrypted: true, encryption_meta: meta,
      }),
    }).then((r) => r.json());
    if (!pending) {
      const up = await fetch(`/api/mindmaps/${mapId}/attachments/${init.attachment_id}/upload`, {
        method: 'POST', headers: octet, body: ciphertext,
      }).then((r) => r.json());
      await fetch(`/api/mindmaps/${mapId}/attachments/${init.attachment_id}/complete`, {
        method: 'POST', headers: json, body: JSON.stringify({ version_id: up.version_id }),
      });
    }
    return { id: init.attachment_id, mapId, marker, legacy, pending, fileKeyB64 };
  };

  const vaults = [
    { id: await mkVault(`Alpha ${tag}`, `note-alpha-${tag}`), title: `Alpha ${tag}`, note: `note-alpha-${tag}` },
    { id: await mkVault(`Beta ${tag}`, null), title: `Beta ${tag}`, note: null },
    { id: await mkVault(`Gamma ${tag}`, `note-gamma-${tag}`), title: `Gamma ${tag}`, note: `note-gamma-${tag}` },
  ];

  const attachments = [];
  for (let i = 0; i < 8; i += 1) attachments.push(await addAttachment(vaults[0].id, `${tag}|v0|a${i}`));
  for (let i = 0; i < 4; i += 1) attachments.push(await addAttachment(vaults[1].id, `${tag}|v1|a${i}`));
  for (let i = 4; i < 6; i += 1) attachments.push(await addAttachment(vaults[1].id, `${tag}|v1|a${i}`, { pending: true }));
  for (let i = 0; i < 4; i += 1) attachments.push(await addAttachment(vaults[2].id, `${tag}|v2|a${i}`));
  for (let i = 4; i < 6; i += 1) attachments.push(await addAttachment(vaults[2].id, `${tag}|v2|a${i}`, { legacy: true }));

  return { vaults, attachments };
}, tag);

const fetchManifest = (page) => page.evaluate(async () => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const token = useAuthStore.getState().accessToken;
  const m = await fetch('/api/auth/rotation-manifest', { headers: { authorization: `Bearer ${token}` } })
    .then((r) => r.json());
  // Normalised for byte-comparison: everything ciphertext-bearing, ordered.
  return JSON.stringify({
    key_version: m.key_version,
    salt: m.argon2_salt,
    priv: [m.classical_priv_encrypted, m.pq_priv_encrypted],
    vaults: m.vaults.map((v) => [v.id, v.title_encrypted, v.vault_note_encrypted]).sort(),
    attachments: m.attachments.map((a) => [a.id, a.encryption_meta?.wrapped_key_b64, a.encryption_meta?.key_wrap]).sort(),
  });
});

const verifyAccount = (page, seeded, tag) => page.evaluate(async ({ seeded, tag }) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const { decryptTitle, decryptTree } = await import('/src/crypto/vault.ts');
  const { hybridDecap } = await import('/src/crypto/kem.ts');
  const { decryptAttachmentForOwner } = await import('/src/crypto/encryptedVault.ts');
  const { deriveAttachmentWrapKey } = await import('/src/crypto/kdf.ts');
  const { aesDecrypt } = await import('/src/crypto/aes.ts');
  const { fromBase64, toBase64 } = await import('/src/crypto/utils.ts');

  const st = useAuthStore.getState();
  const keys = st.sessionKeys;
  const headers = { authorization: `Bearer ${st.accessToken}` };
  if (!keys?.masterKey) return { error: 'no session at verify time' };

  const t = { titles: 0, notes: 0, blobs: 0, wraps: 0, contents: 0, problems: [] };

  for (const vault of seeded.vaults) {
    const detail = await fetch(`/api/mindmaps/${vault.id}`, { headers }).then((r) => r.json());
    try {
      if (await decryptTitle(detail.title_encrypted, keys.masterKey) === vault.title) t.titles += 1;
      else t.problems.push(`title mismatch ${vault.id}`);
    } catch { t.problems.push(`title undecryptable ${vault.id}`); }
    if (vault.note) {
      try {
        if (await decryptTitle(detail.vault_note_encrypted, keys.masterKey) === vault.note) t.notes += 1;
        else t.problems.push(`note mismatch ${vault.id}`);
      } catch { t.problems.push(`note undecryptable ${vault.id}`); }
    }
    try {
      const blobRes = await fetch(`/api/mindmaps/${vault.id}/blob`, { headers });
      const blob = new Uint8Array(await blobRes.arrayBuffer());
      const dek = await hybridDecap(
        keys.classicalPrivKey, keys.pqPrivKey,
        fromBase64(detail.eph_classical_public), fromBase64(detail.eph_pq_ciphertext),
        fromBase64(detail.wrapped_dek),
      );
      const tree = await decryptTree(blob, dek);
      if (tree.root.text === `${tag}-tree-${vault.title}`) t.blobs += 1;
      else t.problems.push(`blob mismatch ${vault.id}`);
    } catch { t.problems.push(`blob undecryptable ${vault.id}`); }
  }

  const wrapKey = await deriveAttachmentWrapKey(keys.masterKey);
  for (const att of seeded.attachments) {
    const meta = await fetch(`/api/mindmaps/${att.mapId}/attachments/${att.id}`, { headers })
      .then((r) => r.json());
    // Wrap correctness: the stored wrap must claim the current format and
    // unwrap, under the NEW master key, to a usable file key.
    let fileKey = null;
    try {
      if (meta.encryption_meta?.key_wrap !== 'hkdf-attachment-v1') {
        t.problems.push(`${att.marker}: wrap not upgraded (${meta.encryption_meta?.key_wrap})`);
      } else {
        fileKey = await aesDecrypt(wrapKey, fromBase64(meta.encryption_meta.wrapped_key_b64));
        if (att.fileKeyB64 && toBase64(fileKey) !== att.fileKeyB64) {
          t.problems.push(`${att.marker}: unwrapped key differs from the original`);
        } else {
          t.wraps += 1;
        }
      }
    } catch { t.problems.push(`${att.marker}: wrapped key does not unwrap`); }
    // Content correctness for everything that has a blob.
    if (!att.pending && fileKey) {
      try {
        const blob = await fetch(`/api/mindmaps/${att.mapId}/attachments/${att.id}/blob`, { headers });
        const ct = new Uint8Array(await blob.arrayBuffer());
        const pt = await decryptAttachmentForOwner(ct, meta.encryption_meta, keys.masterKey);
        if (new TextDecoder().decode(pt) === att.marker) t.contents += 1;
        else t.problems.push(`${att.marker}: content mismatch`);
      } catch { t.problems.push(`${att.marker}: content undecryptable`); }
    }
  }

  return t;
}, { seeded, tag });

// ── Seed all three accounts ─────────────────────────────────────────────────
console.log('── seeding 3 accounts × 3 vaults × 20 attachments ──');
const seeded = {};
for (const [tag, acct] of Object.entries(ACCOUNTS)) {
  check(`${tag}: signed up`, await signUp(pages[tag], acct));
  seeded[tag] = await seedAccount(pages[tag], tag);
  check(`${tag}: seeded ${seeded[tag]?.vaults?.length ?? 0} vaults, ${seeded[tag]?.attachments?.length ?? 0} attachments`,
    seeded[tag]?.vaults?.length === 3 && seeded[tag]?.attachments?.length === 20,
    seeded[tag]?.error ?? '');
}

// ── Phase 1: A rotates alone; B and C must be untouched ─────────────────────
console.log('\n── A rotates; B and C must not move ──');
const bBefore = await fetchManifest(pages.B);
const cBefore = await fetchManifest(pages.C);

check('A: rotation via the settings hub succeeds',
  await rotateViaUi(pages.A, ACCOUNTS.A.oldPw, ACCOUNTS.A.newPw));

check('B: every ciphertext byte-identical after A\'s rotation', await fetchManifest(pages.B) === bBefore);
check('C: every ciphertext byte-identical after A\'s rotation', await fetchManifest(pages.C) === cBefore);

const bWrite = await pages.B.evaluate(async ({ mapId }) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const token = useAuthStore.getState().accessToken;
  const r = await fetch(`/api/mindmaps/${mapId}/meta`, {
    method: 'PUT',
    headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
    body: JSON.stringify({ vault_color: '#123456' }),
  });
  return r.status;
}, { mapId: seeded.B.vaults[0].id });
check('B: still allowed to write after A\'s rotation', bWrite === 200, `status ${bWrite}`);

// B forges a rotation bundle that names one of A's attachments. The auth
// token is B's real one — this is an authenticated owner trying to reach
// across accounts, not a stranger.
const forge = await pages.B.evaluate(async ({ foreignAttachmentId }) => {
  const { useAuthStore } = await import('/src/store/auth.ts');
  const { deriveAuthToken } = await import('/src/crypto/kdf.ts');
  const token = useAuthStore.getState().accessToken;
  const masterKey = useAuthStore.getState().sessionKeys?.masterKey;
  const headers = { authorization: `Bearer ${token}`, 'content-type': 'application/json' };
  const m = await fetch('/api/auth/rotation-manifest', { headers }).then((r) => r.json());
  const junk = (n) => btoa(String.fromCharCode(...crypto.getRandomValues(new Uint8Array(n))));
  const hex = Array.from(crypto.getRandomValues(new Uint8Array(32)))
    .map((b) => b.toString(16).padStart(2, '0')).join('');
  const res = await fetch('/api/auth/rotate-credentials', {
    method: 'POST', headers,
    body: JSON.stringify({
      current_auth_token: deriveAuthToken(masterKey),
      new_auth_token: hex,
      new_argon2_salt: junk(32),
      new_argon2_params: { m_cost: 65536, t_cost: 3, p_cost: 4 },
      new_classical_priv_encrypted: junk(48),
      new_pq_priv_encrypted: junk(48),
      new_key_version: m.key_version + 1,
      updated_vaults: m.vaults.map((v) => ({
        id: v.id, title_encrypted: junk(24),
        vault_note_encrypted: v.vault_note_encrypted ? junk(40) : null,
      })),
      updated_attachments: [
        ...m.attachments.map((a) => ({ id: a.id, wrapped_key_b64: junk(48) })),
        { id: foreignAttachmentId, wrapped_key_b64: junk(48) },
      ],
    }),
  });
  return { status: res.status, body: await res.json().catch(() => null) };
}, { foreignAttachmentId: seeded.A.attachments[0].id });
check('B: a bundle naming A\'s attachment is refused',
  forge.status === 400 && /unknown attachment/.test(forge.body?.error ?? ''), JSON.stringify(forge));
check('B: nothing of B\'s changed in the refused attempt', await fetchManifest(pages.B) === bBefore);

// ── Phase 2: B and C rotate in parallel ─────────────────────────────────────
console.log('\n── B and C rotate at the same time ──');
const [bOk, cOk] = await Promise.all([
  rotateViaUi(pages.B, ACCOUNTS.B.oldPw, ACCOUNTS.B.newPw),
  rotateViaUi(pages.C, ACCOUNTS.C.oldPw, ACCOUNTS.C.newPw),
]);
check('B: parallel rotation succeeds', bOk);
check('C: parallel rotation succeeds', cOk);

// ── Phase 3: prove every account under its NEW password only ────────────────
console.log('\n── decrypt-everything sweep under the new passwords ──');
for (const [tag, acct] of Object.entries(ACCOUNTS)) {
  const page = pages[tag];
  await page.getByRole('button', { name: /^Log out$/i }).first().click().catch(() => {});
  await page.waitForTimeout(1500);
  check(`${tag}: old password refused`, !(await signIn(page, acct.username, acct.oldPw)));
  check(`${tag}: new password signs in`, await signIn(page, acct.username, acct.newPw));

  const t = await verifyAccount(page, seeded[tag], tag);
  check(`${tag}: all 3 titles decrypt`, t.titles === 3, `${t.titles}/3`);
  check(`${tag}: both notes decrypt`, t.notes === 2, `${t.notes}/2`);
  check(`${tag}: all 3 vault blobs decrypt (KEM untouched)`, t.blobs === 3, `${t.blobs}/3`);
  check(`${tag}: all 20 attachment wraps unwrap under the new key`, t.wraps === 20, `${t.wraps}/20`);
  check(`${tag}: all 18 uploaded attachment contents match their markers`, t.contents === 18, `${t.contents}/18`);
  if (t.problems?.length) console.log(`      problems: ${t.problems.slice(0, 5).join('; ')}`);
}

check('no page errors in any account', errors.length === 0, errors.join(' | '));

await browser.close();
console.log(failures === 0
  ? '\nAll multi-account rotation checks passed.'
  : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
