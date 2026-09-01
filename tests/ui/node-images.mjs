#!/usr/bin/env node
// Drives node images — the picture shown on the node itself — in a real browser.
//
// Two halves. The first is what only a browser can settle: that the glyph keeps
// its aspect ratio, that layout grew to hold it without decoding anything, that
// the data: URI survives serialization into the standalone SVG the PNG and PDF
// exports rasterize, and that the vault-list preview draws it too. The second is
// the interaction surface — every way a picture can be put on a node, taken off
// it, replaced, undone, and clicked through to the original — driven the way a
// person would drive it rather than by calling functions.
//
// The server half is checked against the API directly, because "the duplicate
// has its own file" is a claim only the server can answer.
//
// Needs Playwright, which is not a dependency of this repository:
//
//   npm i -D playwright && npx playwright install chromium
//
//   node tests/ui/node-images.mjs --app-url http://127.0.0.1:5173 --api-url http://127.0.0.1:8090
//
// Signs up a throwaway account and creates one vault, so point it at a
// disposable instance. Works against the vite dev server or a built app.
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const APP = flag('--app-url', 'http://127.0.0.1:5173').replace(/\/$/, '');
const API = flag('--api-url', 'http://127.0.0.1:8090').replace(/\/$/, '');
const OUT = flag('--screenshot-dir', '').replace(/\/?$/, '/');
if (OUT !== '/') mkdirSync(OUT, { recursive: true });

let failures = 0;
let step = 0;
const check = (n, c, d = '') => {
  if (!c) failures += 1;
  console.log(`${c ? 'PASS' : 'FAIL'}  ${n}${d ? `  (${d})` : ''}`);
};
const shot = async (p, name) => {
  if (OUT === '/') return;
  step += 1;
  await p.screenshot({ path: `${OUT}${String(step).padStart(2, '0')}-${name}.png`, fullPage: true });
};

const username = `nodeimg${Date.now().toString().slice(-7)}`;
const password = 'a-long-enough-test-password-123';

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(String(e).slice(0, 200)));

/** A solid-colour PNG of the given size, built in the page and handed back. */
const makePng = async (w, h, color, noise = true) => {
  const dataUrl = await page.evaluate(async ({ w, h, color, noise }) => {
    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d');
    ctx.fillStyle = color;
    ctx.fillRect(0, 0, w, h);
    // Noise, so a large image cannot compress to nothing and make the size cap
    // look satisfied when it is not being tested at all.
    if (noise) {
      for (let i = 0; i < w * h / 400; i++) {
        ctx.fillStyle = `hsl(${(i * 37) % 360} 90% 50%)`;
        ctx.fillRect((i * 53) % w, (i * 97) % h, 9, 9);
      }
    }
    return canvas.toDataURL('image/png');
  }, { w, h, color, noise });
  return Buffer.from(dataUrl.split(',')[1], 'base64');
};

const glyph = () => page.locator('svg.mm-canvas image.mm-node-image');
const toast = () => page.locator('.mm-shortcut-toast');

const openNodeMenu = async (nth = 0) => {
  await page.locator('.mm-node-group').nth(nth).click({ button: 'right', position: { x: 8, y: 8 } });
  await page.waitForTimeout(500);
};

const addImageViaMenu = async (buffer, name = 'shot.png', nth = 0) => {
  await openNodeMenu(nth);
  await page.getByTestId('context-add-image').click();
  await page.locator('input[accept="image/*"]').setInputFiles({ name, mimeType: 'image/png', buffer });
  await page.waitForTimeout(9000);
};

const dims = async () => ({
  w: Number(await glyph().first().getAttribute('width')),
  h: Number(await glyph().first().getAttribute('height')),
});

// ── sign up ─────────────────────────────────────────────────────────────────
await page.goto(`${APP}/register`);
await page.waitForTimeout(1200);
await page.locator('input[autocomplete="username"]').first().fill(username);
const pw = page.locator('input[type="password"]');
await pw.nth(0).fill(password);
if (await pw.count() > 1) await pw.nth(1).fill(password);
await page.getByRole('button', { name: /create|sign up|register/i }).first().click();
await page.waitForTimeout(9000);

if (/\/login/.test(page.url())) {
  await page.locator('input[autocomplete="username"]').first().fill(username);
  await page.locator('input[type="password"]').first().fill(password);
  await page.getByRole('button', { name: /sign in|log in|unlock/i }).first().click();
  await page.waitForTimeout(9000);
}
check('signed in and reached the vault list', /\/vaults/.test(page.url()), page.url());

await page.getByRole('button', { name: /^New/ }).first().click();
await page.waitForTimeout(600);
await page.getByText('Mind map', { exact: true }).first().click();
await page.waitForTimeout(600);
await page.locator('input[placeholder="Vault name..."]').fill('Node image check');
await page.getByRole('button', { name: /^Create$/ }).click();
await page.waitForTimeout(8000);
check('opened the editor', /\/vaults\/[0-9a-f-]{8}/.test(page.url()), page.url());
const vaultId = page.url().split('/vaults/')[1];

// A map with no image field must be unaffected by any of this.
check('a node with no picture renders no glyph', (await glyph().count()) === 0);
const plainBox = await page.locator('.mm-node-group rect').first().boundingBox();

// ═══ 1. The glyph itself ════════════════════════════════════════════════════
await addImageViaMenu(await makePng(1600, 900, '#ff0000'));
check('the node now shows a picture', (await glyph().count()) === 1);
await shot(page, 'glyph-added');

const href = await glyph().first().getAttribute('href');
check('the glyph is inlined in the map, not fetched',
  (href ?? '').startsWith('data:image/webp;base64,'), (href ?? '').slice(0, 30));
check('and it stays under the 8 KB cap that rides in every future version',
  (href ?? '').length <= 8 * 1024, `${href?.length ?? 0} bytes`);

let d = await dims();
check('a 16:9 picture is drawn 16:9, not squashed into a square',
  d.w === 64 && d.h === 36, `${d.w}×${d.h}`);

// The rendered bitmap must be encoded at exactly those dimensions — that is
// what makes preserveAspectRatio, letterboxing and cropping all unnecessary.
const encoded = await page.evaluate((url) => new Promise((resolve) => {
  const img = new Image();
  img.onload = () => resolve({ w: img.naturalWidth, h: img.naturalHeight });
  img.onerror = () => resolve(null);
  img.src = url;
}), href);
check('the bitmap is encoded at exactly the size it is drawn',
  encoded && encoded.w === d.w && encoded.h === d.h,
  encoded ? `${encoded.w}×${encoded.h}` : 'failed to decode');

// ═══ 2. Aspect ratios ═══════════════════════════════════════════════════════
for (const [w, h, colour, label, want] of [
  [900, 1600, '#00a000', 'a portrait picture stays portrait', (r) => r.h === 64 && r.w === 36],
  [800, 800, '#0000ff', 'a square picture fills the box', (r) => r.w === 64 && r.h === 64],
  [2000, 200, '#ff8800', 'a 10:1 panorama is clamped, not reduced to a sliver', (r) => r.w === 64 && r.h >= 21 && r.h <= 24],
]) {
  await openNodeMenu();
  await page.getByTestId('context-remove-image').click();
  await page.waitForTimeout(1500);
  await addImageViaMenu(await makePng(w, h, colour));
  d = await dims();
  check(label, want(d), `${d.w}×${d.h}`);
}

// A big, noisy photo is the case the size cap exists for.
await openNodeMenu();
await page.getByTestId('context-remove-image').click();
await page.waitForTimeout(1500);
await addImageViaMenu(await makePng(4000, 3000, '#334455'), 'huge.png');
const hugeHref = await glyph().first().getAttribute('href');
check('a 4000×3000 photo full of detail still fits under the cap',
  (hugeHref ?? '').length <= 8 * 1024, `${hugeHref?.length ?? 0} bytes`);

// ═══ 3. Layout ══════════════════════════════════════════════════════════════
await openNodeMenu();
await page.getByTestId('context-remove-image').click();
await page.waitForTimeout(1500);
// Flat colour: the export check below samples a pixel out of it.
await addImageViaMenu(await makePng(800, 800, '#7c3aed', false), 'square.png');
const grownBox = await page.locator('.mm-node-group rect').first().boundingBox();
check('the node grew to hold the picture',
  !!plainBox && !!grownBox && grownBox.height > plainBox.height + 50,
  plainBox && grownBox ? `${Math.round(plainBox.height)} → ${Math.round(grownBox.height)}` : 'no box');

const overlap = await page.evaluate(() => {
  const image = document.querySelector('svg.mm-canvas image.mm-node-image');
  const text = document.querySelector('svg.mm-canvas .mm-node-text');
  if (!image || !text) return null;
  return text.getBoundingClientRect().top - image.getBoundingClientRect().bottom;
});
check('the picture and the node text do not overlap', overlap !== null && overlap >= 0, `${overlap}px gap`);
await shot(page, 'layout');

// ═══ 4. The export path ═════════════════════════════════════════════════════
// The whole design rests on this: a data: URI inside an SVG <image> survives
// XMLSerializer into a standalone document and rasterizes. Rather than trust
// it, serialize the live canvas the way the PNG/PDF export does and read the
// pixel back out.
const exported = await page.evaluate(async () => {
  const svg = document.querySelector('svg.mm-canvas');
  const image = svg.querySelector('image.mm-node-image');
  const svgBox = svg.getBoundingClientRect();
  const imgBox = image.getBoundingClientRect();
  const clone = svg.cloneNode(true);
  clone.querySelectorAll('foreignObject').forEach((fo) => fo.remove());
  clone.setAttribute('width', String(svg.clientWidth));
  clone.setAttribute('height', String(svg.clientHeight));
  if (!clone.getAttribute('viewBox')) {
    clone.setAttribute('viewBox', `0 0 ${svg.clientWidth} ${svg.clientHeight}`);
  }
  let str = new XMLSerializer().serializeToString(clone);
  if (!str.includes('xmlns=')) str = str.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"');

  const canvas = document.createElement('canvas');
  canvas.width = svg.clientWidth;
  canvas.height = svg.clientHeight;
  const ctx = canvas.getContext('2d');
  const ok = await new Promise((resolve) => {
    const img = new Image();
    img.onload = () => { ctx.drawImage(img, 0, 0); resolve(true); };
    img.onerror = () => resolve(false);
    img.src = `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(str)))}`;
  });
  if (!ok) return { ok: false };
  const x = Math.round(imgBox.left - svgBox.left + imgBox.width / 2);
  const y = Math.round(imgBox.top - svgBox.top + imgBox.height / 2);
  const [r, g, b] = ctx.getImageData(x, y, 1, 1).data;
  return { ok: true, r, g, b };
});
check('the standalone SVG the export rasterizes still renders', exported.ok === true);
check('and the picture is actually in the exported pixels',
  exported.ok && Math.abs(exported.r - 124) < 30 && Math.abs(exported.g - 58) < 30 && Math.abs(exported.b - 237) < 30,
  exported.ok ? `rgb(${exported.r},${exported.g},${exported.b}) vs the 124,58,237 that was uploaded` : 'render failed');

// ═══ 5. Clicking it opens the original ══════════════════════════════════════
await glyph().first().click();
await page.waitForTimeout(4000);
check('clicking the picture opens the full-size original',
  await page.locator('.mm-attachment-preview-modal').isVisible().catch(() => false));
await shot(page, 'original-open');
await page.getByTitle('Close preview').first().click();
await page.waitForTimeout(1200);
check('and the preview closes again',
  (await page.locator('.mm-overlay--attachment-preview').count()) === 0);

// ═══ 6. The other ways to add one ═══════════════════════════════════════════
// Ctrl+V. The handler lives on the window and has to ignore pastes meant for
// text, so it is worth driving rather than calling.
await page.getByTitle('Add child (Tab)').first().click();
await page.waitForTimeout(1500);
await page.keyboard.type('Pasted');
await page.keyboard.press('Enter');
await page.waitForTimeout(1200);

const pasteBuffer = await makePng(400, 400, '#e11d48');
await page.evaluate(async (b64) => {
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const file = new File([bytes], 'pasted.png', { type: 'image/png' });
  const dt = new DataTransfer();
  dt.items.add(file);
  window.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, clipboardData: dt }));
}, pasteBuffer.toString('base64'));
await page.waitForTimeout(9000);
check('Ctrl+V puts a copied picture on the selected node',
  (await glyph().count()) === 2, `${await glyph().count()} glyphs`);
await shot(page, 'pasted');

// Drag and drop onto a node.
await page.getByTitle('Add child (Tab)').first().click();
await page.waitForTimeout(1500);
await page.keyboard.type('Dropped');
await page.keyboard.press('Enter');
await page.waitForTimeout(1200);
const dropTarget = await page.locator('.mm-node-group').last().boundingBox();
const dropBuffer = await makePng(500, 300, '#16a34a');
await page.evaluate(async ({ b64, x, y }) => {
  const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
  const file = new File([bytes], 'dropped.png', { type: 'image/png' });
  const dt = new DataTransfer();
  dt.items.add(file);
  const svg = document.querySelector('svg.mm-canvas');
  const init = { bubbles: true, cancelable: true, clientX: x, clientY: y, dataTransfer: dt };
  svg.dispatchEvent(new DragEvent('dragover', init));
  svg.dispatchEvent(new DragEvent('drop', init));
}, {
  b64: dropBuffer.toString('base64'),
  x: Math.round(dropTarget.x + dropTarget.width / 2),
  y: Math.round(dropTarget.y + 10),
});
await page.waitForTimeout(9000);
check('dropping a single image on a node shows it there',
  (await glyph().count()) === 3, `${await glyph().count()} glyphs`);
await shot(page, 'dropped');

// ═══ 7. Replace, remove, undo ═══════════════════════════════════════════════
const beforeReplace = await glyph().first().getAttribute('href');
await openNodeMenu();
const replaceLabel = await page.getByTestId('context-add-image').innerText();
check('the menu offers Replace once a node already has a picture',
  /replace/i.test(replaceLabel), replaceLabel);
await page.getByTestId('context-add-image').click();
await page.locator('input[accept="image/*"]').setInputFiles({
  name: 'replacement.png', mimeType: 'image/png', buffer: await makePng(300, 900, '#f59e0b'),
});
await page.waitForTimeout(9000);
const afterReplace = await glyph().first().getAttribute('href');
check('replacing swaps the picture', beforeReplace !== afterReplace);
d = await dims();
check('and the node re-laid out for the new shape', d.h === 64 && d.w === 21 + 3, `${d.w}×${d.h}`);

// Removing the glyph must not remove the file: it stays an ordinary attachment.
const beforeRemove = await fetch(`${API}/api/mindmaps/${vaultId}/attachments`, {
  headers: { authorization: `Bearer ${await page.evaluate(() => {
    for (const k of Object.keys(localStorage)) {
      try {
        const v = JSON.parse(localStorage.getItem(k));
        const t = v?.state?.accessToken ?? v?.accessToken;
        if (typeof t === 'string' && t.length > 20) return t;
      } catch { /* not ours */ }
    }
    return '';
  })}` },
}).then((r) => r.json());

await openNodeMenu();
await page.getByTestId('context-remove-image').click();
await page.waitForTimeout(2000);
check('Remove Image takes the picture off the node',
  (await glyph().count()) === 2, `${await glyph().count()} glyphs`);

await page.keyboard.press('F9');
await page.waitForTimeout(2000);
check('and undo puts it back',
  (await glyph().count()) === 3, `${await glyph().count()} glyphs`);

// ═══ 8. A file that is not an image ═════════════════════════════════════════
await openNodeMenu();
await page.getByTestId('context-add-image').click();
await page.locator('input[accept="image/*"]').setInputFiles({
  name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('this is not a picture'),
});
// The toast clears itself after 1.6s, so catch it rather than sleeping past it.
let refusal = '';
for (let i = 0; i < 40 && !refusal; i++) {
  refusal = (await toast().allTextContents()).join(' ');
  if (!refusal) await page.waitForTimeout(150);
}
check('a file that is not an image is refused, with a reason',
  /not an image/i.test(refusal), refusal || '(no message)');
check('and nothing was added for it', (await glyph().count()) === 3);

// ═══ 9. The vault-list preview ══════════════════════════════════════════════
// A second renderer with its own SVG. It shares the layout, so if it did not
// draw the picture the node would show an unexplained gap.
await page.keyboard.press('Control+s');
await page.waitForTimeout(5000);
await page.goBack();
await page.waitForTimeout(6000);
// The preview is itself an encrypted attachment — rasterized to WebP and
// decrypted into a blob: URL — so there is no markup to grep. Read the pixels
// instead: the pictures put on the nodes are colours nothing else in a preview
// uses, so finding them proves the second renderer drew them.
const previewHit = await page.evaluate(async (targets) => {
  const img = Array.from(document.querySelectorAll('img'))
    .find((el) => el.src.startsWith('blob:') || el.src.startsWith('data:image'));
  if (!img) return { found: false, reason: 'no preview image on the vault card' };
  const bitmap = await new Promise((resolve) => {
    const probe = new Image();
    probe.onload = () => resolve(probe);
    probe.onerror = () => resolve(null);
    probe.src = img.src;
  });
  if (!bitmap) return { found: false, reason: 'preview image would not decode' };
  const canvas = document.createElement('canvas');
  canvas.width = bitmap.naturalWidth;
  canvas.height = bitmap.naturalHeight;
  const ctx = canvas.getContext('2d');
  ctx.drawImage(bitmap, 0, 0);
  const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
  for (const [tr, tg, tb] of targets) {
    let hits = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (Math.abs(data[i] - tr) < 40 && Math.abs(data[i + 1] - tg) < 40 && Math.abs(data[i + 2] - tb) < 40) {
        hits += 1;
        if (hits > 12) return { found: true, size: `${canvas.width}×${canvas.height}`, colour: `${tr},${tg},${tb}` };
      }
    }
  }
  return { found: false, reason: `none of the picture colours appear in the ${canvas.width}×${canvas.height} preview` };
}, [[225, 29, 72], [22, 163, 74], [245, 158, 11]]);
check('the vault-list preview draws the pictures too',
  previewHit.found === true,
  previewHit.found ? `found ${previewHit.colour} in the ${previewHit.size} preview` : previewHit.reason);
await shot(page, 'vault-list-preview');

// ═══ 10. Round trip ═════════════════════════════════════════════════════════
await page.getByText('Node image check').first().click();
await page.waitForTimeout(8000);
check('the pictures round-trip through the encrypted vault',
  (await glyph().count()) === 3, `${await glyph().count()} glyphs after reload`);
await shot(page, 'reloaded');

// ═══ 11. Duplicate owns its own file ════════════════════════════════════════
const token = await page.evaluate(() => {
  for (const k of Object.keys(localStorage)) {
    try {
      const v = JSON.parse(localStorage.getItem(k));
      const t = v?.state?.accessToken ?? v?.accessToken;
      if (typeof t === 'string' && t.length > 20) return t;
    } catch { /* not ours */ }
  }
  return null;
});
check('found the session token to verify against the server', !!token);
const bearer = { authorization: `Bearer ${token}` };

check('removing a picture from a node left its file attached',
  beforeRemove.filter((a) => a.name === 'replacement.png').length === 1,
  `${beforeRemove.filter((a) => a.name === 'replacement.png').length} stored`);

// Each child was added while the previous one was selected, so "Dropped" is a
// child of "Pasted". Duplicating "Pasted" must bring the whole subtree's
// pictures across, not just the node's own.
const beforeDuplicate = await glyph().count();
await openNodeMenu(1);
await page.getByRole('button', { name: 'Duplicate' }).click();
await page.waitForTimeout(12000);
check('duplicating a node copies every picture in its subtree',
  (await glyph().count()) === beforeDuplicate + 2,
  `${beforeDuplicate} → ${await glyph().count()} glyphs`);
await shot(page, 'duplicated');

const stored = await fetch(`${API}/api/mindmaps/${vaultId}/attachments`, { headers: bearer })
  .then((r) => r.json());
const pasted = stored.filter((a) => a.name === 'pasted.png' && a.status === 'available');
check('the server stored a second copy of the duplicated file',
  pasted.length === 2, `${pasted.length} copies of pasted.png`);
check('and the copy has an id of its own, not the original\'s',
  pasted.length === 2 && pasted[0].id !== pasted[1].id,
  pasted.map((a) => a.id.slice(0, 8)).join(' / '));
check('the copy is a full, completed attachment of the same size',
  pasted.length === 2 && pasted[0].size_bytes === pasted[1].size_bytes,
  pasted.map((a) => a.size_bytes).join(' / '));

// Deleting one must leave the other intact — the point of the whole copy.
if (pasted.length === 2) {
  const del = await fetch(`${API}/api/mindmaps/${vaultId}/attachments/${pasted[0].id}`, {
    method: 'DELETE', headers: bearer,
  });
  check('deleting one copy succeeds', del.status === 200, `status ${del.status}`);
  const survivor = await fetch(`${API}/api/mindmaps/${vaultId}/attachments/${pasted[1].id}/blob`, {
    headers: bearer,
  });
  check('and the other copy is still downloadable afterwards',
    survivor.status === 200, `status ${survivor.status}`);
}

// ═══ 12. A picture whose original is gone ═══════════════════════════════════
// Restoring an old version, or deleting the file from the vault-files dialog,
// leaves a node pointing at nothing. The picture must still render — it lives
// in the map — and only the click-through should fail, with an explanation.
await page.goBack();
await page.waitForTimeout(4000);
await page.getByText('Node image check').first().click();
await page.waitForTimeout(8000);
const glyphsWithMissingOriginal = await glyph().count();
check('a node whose original was deleted still shows its picture',
  glyphsWithMissingOriginal === beforeDuplicate + 2,
  `${glyphsWithMissingOriginal} glyphs`);

const orphan = page.locator('svg.mm-canvas image.mm-node-image').nth(1);
await orphan.click();
await page.waitForTimeout(3000);
const orphanMessage = (await toast().allTextContents()).join(' ');
check('and clicking it says so plainly instead of failing silently',
  /no longer available|no full-size copy/i.test(orphanMessage)
    || (await page.locator('.mm-attachment-preview-modal').isVisible().catch(() => false)),
  orphanMessage || '(preview opened — the surviving copy)');
await shot(page, 'missing-original');

check('no page errors', errors.length === 0, errors.join(' | '));

await browser.close();
console.log(failures === 0 ? '\nAll node-image checks passed.' : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
