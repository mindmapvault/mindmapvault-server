#!/usr/bin/env node
// Drives the Obsidian-style note editor in a real browser.
//
// The live preview is decoration work that only exists once CodeMirror has
// measured and laid out a real document, so jsdom would prove nothing here: the
// checks below read what is actually on screen. Markers hidden means the DOM
// text no longer contains them; a checkbox rendered means an <input> widget
// replaced the source range.
//
// Needs Playwright, which is not a dependency of this repository:
//
//   npm i -D playwright && npx playwright install chromium
//
//   node tests/ui/note-editor.mjs --app-url http://127.0.0.1:5173 --api-url http://127.0.0.1:8090
//
// Signs up a throwaway account and creates one vault, so point it at a
// disposable instance.
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const APP = flag('--app-url', 'http://127.0.0.1:5173').replace(/\/$/, '');
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

const username = `notes${Date.now().toString().slice(-7)}`;
const password = 'a-long-enough-test-password-123';

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(String(e).slice(0, 200)));

// CodeMirror's own keymap follows the platform: Mod- is Cmd on macOS and Ctrl
// elsewhere, and so is "jump to the end of the document".
const MOD = process.platform === 'darwin' ? 'Meta' : 'Control';
const DOC_END = process.platform === 'darwin' ? 'Meta+ArrowDown' : 'Control+End';

// The rendered text of the writing surface: what the reader sees, with every
// hidden syntax marker already gone.
const surfaceText = () => page.locator('.mm-note-editor .cm-content').innerText();
/** Move the caret off the current element so its raw syntax is hidden again. */
const parkCaret = async () => {
  await page.keyboard.press(DOC_END);
  await page.waitForTimeout(250);
};

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

// ── a vault to write notes in ───────────────────────────────────────────────
await page.getByRole('button', { name: /^New/ }).first().click();
await page.waitForTimeout(600);
await page.getByText('Mind map', { exact: true }).first().click();
await page.waitForTimeout(600);
await page.locator('input[placeholder="Vault name..."]').fill('Note editor check');
await page.getByRole('button', { name: /^Create$/ }).click();
await page.waitForTimeout(8000);
check('opened the editor', /\/vaults\/[0-9a-f-]{8}/.test(page.url()), page.url());

// ── open the note ───────────────────────────────────────────────────────────
await page.getByTitle('Notes (F3)').first().click();
await page.waitForTimeout(2500);

const modal = page.locator('.mm-notes-modal');
check('the toolbar opens the note dialog', await modal.isVisible().catch(() => false));
check('the writing surface is CodeMirror, not a textarea',
  (await page.locator('.mm-note-editor .cm-content').count()) === 1
  && (await page.locator('.mm-notes-textarea').count()) === 0);
check('there is no Save button — notes autosave',
  !/save notes/i.test(await modal.innerText()));
check('the details disclosure starts collapsed',
  (await page.locator('.mm-notes-details-body').count()) === 0);
await shot(page, 'note-open');

// ── live preview: headings ──────────────────────────────────────────────────
await page.locator('.mm-note-editor .cm-content').click();
await page.waitForTimeout(300);
await page.keyboard.type('# Shopping list');
await page.keyboard.press('Enter');
await page.keyboard.press('Enter');
await page.waitForTimeout(400);

check('the heading marker is hidden once the caret leaves the line',
  !(await surfaceText()).includes('#'), JSON.stringify((await surfaceText()).slice(0, 40)));
check('and the line is styled as a heading',
  (await page.locator('.mm-note-editor .mm-cm-h1').count()) >= 1);

// ── live preview: emphasis ──────────────────────────────────────────────────
await page.keyboard.type('Buy **milk** today');
await parkCaret();
let text = await surfaceText();
check('bold markers are hidden while the text stays', !text.includes('**') && text.includes('milk'));
check('and the bold run is styled',
  (await page.locator('.mm-note-editor .mm-cm-strong').count()) >= 1);

// Putting the caret back inside must reveal the source again — that is the
// whole point of a live editor rather than a rendered preview.
await page.getByText('milk', { exact: false }).first().click();
await page.waitForTimeout(400);
check('putting the caret inside reveals the raw markers again',
  (await surfaceText()).includes('**'));
await parkCaret();

// ── task list ───────────────────────────────────────────────────────────────
await page.keyboard.press('Enter');
await page.keyboard.type('- [ ] pick up the parcel');
await parkCaret();
const box = page.locator('.mm-note-editor input.mm-cm-task');
check('a task item renders a real checkbox', (await box.count()) === 1);
check('which starts unchecked', !(await box.first().isChecked().catch(() => true)));
await box.first().click();
await page.waitForTimeout(500);
check('clicking it ticks the box', await box.first().isChecked().catch(() => false));
await shot(page, 'live-preview');

// ── list continuation ───────────────────────────────────────────────────────
await parkCaret();
await page.keyboard.press('Enter');
await page.keyboard.type('- one');
await page.keyboard.press('Enter');
await page.keyboard.type('two');
await parkCaret();
check('Enter continues the list without retyping the marker',
  (await surfaceText()).includes('two')
  && (await page.locator('.mm-note-editor .mm-cm-listmark').count()) >= 2);

// ── Ctrl+B on a selection ───────────────────────────────────────────────────
await page.getByText('parcel', { exact: false }).first().dblclick();
await page.waitForTimeout(300);
await page.keyboard.press(`${MOD}+b`);
await parkCaret();
check('Ctrl+B wraps the selection in bold',
  (await page.locator('.mm-note-editor .mm-cm-strong').count()) >= 2);

// ── an attachment, rendered inline ──────────────────────────────────────────
// The server-specific half of this editor: notes reference files as
// `attachment://<id>`, which means nothing to the DOM. The resolver has to map
// it to the decrypted blob URL, or the image silently renders as unavailable.
await parkCaret();
await page.keyboard.press('Enter');
await page.locator('.mm-notes-markdown-tools input[type="file"]').setInputFiles({
  name: 'dot.png',
  mimeType: 'image/png',
  // 1×1 red PNG.
  buffer: Buffer.from(
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
    'base64',
  ),
});
await page.waitForTimeout(12000);
// A newline below it, then park: an image whose own range the caret still
// touches shows its source, which is the point of a live editor.
await parkCaret();
await page.keyboard.press('Enter');
await page.waitForTimeout(400);
const inlineImage = page.locator('.mm-note-editor img.mm-cm-image');
check('an uploaded file is rendered inline in the note', (await inlineImage.count()) === 1);
const imageSrc = await inlineImage.first().getAttribute('src').catch(() => '');
check('and it resolves to a decrypted blob, not the attachment:// literal',
  (imageSrc ?? '').startsWith('blob:'), (imageSrc ?? '').slice(0, 40));
check('the raw attachment:// URL is not left on screen',
  !(await surfaceText()).includes('attachment://'));
await shot(page, 'attachment-inline');

// ── Write / Read ────────────────────────────────────────────────────────────
const readBtn = page.locator('.mm-notes-modeswitch button', { hasText: 'Read' });
const writeBtn = page.locator('.mm-notes-modeswitch button', { hasText: 'Write' });
await readBtn.click();
await page.waitForTimeout(600);
check('Read mode renders the note as HTML',
  (await page.locator('.mm-notes-preview--full h1').count()) >= 1);
check('and the editor stays mounted underneath rather than being torn down',
  (await page.locator('.mm-note-editor .cm-content').count()) === 1);
await shot(page, 'read-mode');

await writeBtn.click();
await page.waitForTimeout(600);
const afterRoundTrip = await surfaceText();
check('Write → Read → Write keeps every edit',
  afterRoundTrip.includes('Shopping list') && afterRoundTrip.includes('parcel'));

// Ctrl+E is what the mode buttons advertise, so it has to actually work.
await page.locator('.mm-note-editor .cm-content').click();
await page.keyboard.press('Control+e');
await page.waitForTimeout(500);
check('Ctrl+E switches to Read',
  (await page.locator('.mm-notes-preview--full').count()) === 1);
await page.keyboard.press('Control+e');
await page.waitForTimeout(500);
check('and Ctrl+E again switches back to Write',
  (await page.locator('.mm-notes-preview--full').count()) === 0);

// ── details disclosure ──────────────────────────────────────────────────────
await page.locator('.mm-notes-details-toggle').click();
await page.waitForTimeout(400);
check('the details disclosure opens to labels and files',
  (await page.locator('.mm-notes-details-body').count()) === 1);
check('and holds the delete-note action rather than the toolbar',
  /delete note/i.test(await modal.innerText()));
await page.locator('.mm-notes-details-toggle').click();
await page.waitForTimeout(300);

// ── autosave: Escape must not lose work ─────────────────────────────────────
await page.locator('.mm-note-editor .cm-content').click();
await parkCaret();
await page.keyboard.press('Enter');
await page.keyboard.type('written just before closing');
await page.waitForTimeout(1500);
check('the header reports the note as saved',
  /saved/i.test(await page.getByTestId('notes-savestate').innerText()));

await page.keyboard.press('Escape');
await page.waitForTimeout(800);
check('Escape closes the note', !(await modal.isVisible().catch(() => false)));

await page.keyboard.press('F3');
await page.waitForTimeout(2000);
check('and reopening shows the text typed right before Escape',
  (await surfaceText()).includes('written just before closing'));
await page.keyboard.press('Escape');
await page.waitForTimeout(600);

// ── it survives a save and a fresh load of the vault ────────────────────────
// goBack rather than goto: a full reload drops the in-memory session keys and
// lands on the unlock prompt instead of the vault list.
await page.keyboard.press(`${MOD}+s`);
await page.waitForTimeout(4000);
await page.goBack();
await page.waitForTimeout(3000);
await page.getByText('Note editor check').first().click();
await page.waitForTimeout(8000);
await page.keyboard.press('F3');
await page.waitForTimeout(2500);
const reloaded = await surfaceText();
check('the note round-trips through the encrypted vault',
  reloaded.includes('Shopping list')
  && reloaded.includes('pick up the parcel')
  && reloaded.includes('written just before closing'),
  JSON.stringify(reloaded.slice(0, 80)));
check('and the markdown is stored as markdown, not as rendered HTML',
  !reloaded.includes('<') && (await page.locator('.mm-note-editor .mm-cm-h1').count()) >= 1);
await shot(page, 'reloaded');

check('no page errors', errors.length === 0, errors.join(' | '));

await browser.close();
console.log(failures === 0 ? '\nAll note-editor checks passed.' : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
