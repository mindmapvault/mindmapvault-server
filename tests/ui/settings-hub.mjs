#!/usr/bin/env node
// Drives the settings hub in a real browser: signs a fresh account up, opens
// the gear from the vault list, and exercises every tab. The profile save is
// checked against the backend rather than the form, so a save that only
// updated local state would fail here.
//
// Needs Playwright, which is not a dependency of this repository:
//
//   npm i -D playwright && npx playwright install chromium
//
// Point --app-url at a dev server or a built app, and --api-url at the backend
// it talks to. Signs up a throwaway account and creates one vault, so run it
// against a disposable instance.
//
//   node tests/ui/settings-hub.mjs --app-url http://127.0.0.1:5173 --api-url http://127.0.0.1:8090
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 && args[i + 1] ? args[i + 1] : fallback;
};
const APP = flag('--app-url', 'http://127.0.0.1:5173').replace(/\/$/, '');
const API = flag('--api-url', 'http://127.0.0.1:8090').replace(/\/$/, '');
// Screenshots are the quickest way to see why a layout check failed.
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

const username = `settings${Date.now().toString().slice(-7)}`;
const password = 'a-long-enough-test-password-123';

const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(String(e).slice(0, 200)));

// ── sign up ─────────────────────────────────────────────────────────────────
await page.goto(`${APP}/register`);
await page.waitForTimeout(1200);
await page.locator('input[autocomplete="username"]').first().fill(username);
const pw = page.locator('input[type="password"]');
await pw.nth(0).fill(password);
if (await pw.count() > 1) await pw.nth(1).fill(password);
await page.getByRole('button', { name: /create|sign up|register/i }).first().click();
await page.waitForTimeout(9000);

// Registration may land on the vault list or bounce to sign-in; handle both.
if (/\/login/.test(page.url())) {
  await page.locator('input[autocomplete="username"]').first().fill(username);
  await page.locator('input[type="password"]').first().fill(password);
  await page.getByRole('button', { name: /sign in|log in|unlock/i }).first().click();
  await page.waitForTimeout(9000);
}
check('signed in and reached the vault list', /\/vaults/.test(page.url()), page.url());
await shot(page, 'vaults');

// ── the gear replaces the old theme popover ─────────────────────────────────
const gear = page.getByTestId('open-settings').first();
check('the vault list has a settings button', await gear.isVisible().catch(() => false));
await gear.click();
await page.waitForTimeout(1200);

const modal = page.getByTestId('settings-modal');
check('clicking it opens the settings hub', await modal.isVisible().catch(() => false));
await shot(page, 'account-tab');

// ── tabs ────────────────────────────────────────────────────────────────────
for (const [id, needle] of [
  ['account', /Profile/i],
  ['appearance', /Accent colour/i],
  ['support', /Getting help/i],
]) {
  await page.getByTestId(`settings-tab-${id}`).click();
  await page.waitForTimeout(500);
  const text = await modal.innerText();
  check(`the ${id} tab renders its own content`, needle.test(text));
}

// Billing must not have followed the port across.
await page.getByTestId('settings-tab-account').click();
await page.waitForTimeout(400);
let allText = '';
for (const id of ['account', 'appearance', 'support']) {
  await page.getByTestId(`settings-tab-${id}`).click();
  await page.waitForTimeout(400);
  allText += await modal.innerText();
}
check('no billing, plan or subscription copy anywhere in the hub',
  !/subscri|billing|invoice|payment|upgrade|\bplan\b/i.test(allText));
check('no notifications tab', !/notification/i.test(allText));
check('it says plainly that nobody can reset the password',
  /nobody[\s\S]{0,80}can reset it/i.test(allText));

// ── appearance actually changes the app ─────────────────────────────────────
await page.getByTestId('settings-tab-appearance').click();
await page.waitForTimeout(400);
const before = await page.evaluate(() => document.documentElement.classList.contains('light'));
await page.getByTestId('settings-theme-toggle').click();
await page.waitForTimeout(700);
const after = await page.evaluate(() => document.documentElement.classList.contains('light'));
check('the theme toggle flips the document theme', before !== after, `${before} → ${after}`);
await shot(page, 'appearance-toggled');

await page.locator('button[aria-label="Use accent colour #22c55e"]').click();
await page.waitForTimeout(600);
const accent = await page.evaluate(() =>
  document.documentElement.style.getPropertyValue('--accent').trim());
check('picking an accent colour updates the CSS variable', accent === '#22c55e', accent);

// Put the theme back so the screenshots below are comparable.
await page.getByTestId('settings-theme-toggle').click();
await page.waitForTimeout(500);

// ── profile round-trip, verified against the API ────────────────────────────
await page.getByTestId('settings-tab-account').click();
await page.waitForTimeout(600);
await page.locator('input[autocomplete="given-name"]').fill('Anna');
await page.locator('input[autocomplete="family-name"]').fill('Novak');
await page.locator('input[autocomplete="email"]').fill('anna@example.com');
await page.waitForTimeout(300);
await page.getByTestId('profile-save').click();
await page.waitForTimeout(2500);
check('the form reports the save', /Profile saved/i.test(await modal.innerText()));
await shot(page, 'profile-saved');

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
if (token) {
  const res = await fetch(`${API}/api/auth/profile`, { headers: { authorization: `Bearer ${token}` } });
  const body = await res.json();
  check('the server stored the profile, not just the form',
    body.first_name === 'Anna' && body.last_name === 'Novak' && body.email === 'anna@example.com',
    JSON.stringify({ f: body.first_name, l: body.last_name, e: body.email }));
}

// An invalid email must be refused before it reaches the server.
await page.locator('input[autocomplete="email"]').fill('not-an-email');
await page.waitForTimeout(400);
check('an address with no @ is refused', /valid email address/i.test(await modal.innerText()));
check('and Save is disabled while it is invalid',
  await page.getByTestId('profile-save').isDisabled());

// ── closing ─────────────────────────────────────────────────────────────────
await page.keyboard.press('Escape');
await page.waitForTimeout(600);
check('Escape closes the hub', !(await modal.isVisible().catch(() => false)));

// ── the editor toolbar, the other place the gear replaced the popover ───────
await page.getByRole('button', { name: /^New/ }).first().click();
await page.waitForTimeout(600);
await page.getByText('Mind map', { exact: true }).first().click();
await page.waitForTimeout(600);
await page.locator('input[placeholder="Vault name..."]').fill('Settings hub check');
await page.getByRole('button', { name: /^Create$/ }).click();
await page.waitForTimeout(7000);
check('opened the editor', /\/vaults\/[0-9a-f-]{8}/.test(page.url()), page.url());
await shot(page, 'editor-toolbar');

const editorGear = page.getByTestId('open-settings').first();
check('the editor toolbar has a settings button',
  await editorGear.isVisible().catch(() => false));
const gearBox = await editorGear.boundingBox();
check('the gear is sized like the toolbar buttons beside it',
  !!gearBox && gearBox.height > 24 && gearBox.height < 44 && gearBox.width < 60,
  gearBox ? `${Math.round(gearBox.width)}×${Math.round(gearBox.height)}` : 'no box');
await editorGear.click();
await page.waitForTimeout(1200);
check('and opens the same hub from the editor',
  await modal.isVisible().catch(() => false));
await shot(page, 'editor-hub');
await page.keyboard.press('Escape');
await page.waitForTimeout(600);

// ── the hub at phone width ──────────────────────────────────────────────────
// Same page, resized: a fresh context would lose the in-memory session keys
// and land on the unlock prompt instead of the vault list.
// History, not goto: a full reload drops the session keys and lands on the
// unlock prompt instead of the vault list.
await page.goBack();
await page.waitForTimeout(3000);
await page.setViewportSize({ width: 390, height: 844 });
await page.waitForTimeout(800);
await page.getByTestId('open-settings').first().click();
await page.waitForTimeout(1200);
check('the hub opens on a phone', await modal.isVisible().catch(() => false));
const overflow = await page.evaluate(() =>
  document.documentElement.scrollWidth - document.documentElement.clientWidth);
check('and does not scroll sideways on a phone', overflow <= 0, `${overflow}px overflow`);
const phoneTabs = await page.getByTestId('settings-tab-support').isVisible().catch(() => false);
check('every tab is still reachable on a phone', phoneTabs);
await shot(page, 'phone-hub');

// ── the editor on a real touch device ───────────────────────────────────────
// The vault list is the only place a phone can reach settings from: the
// editor's mobile branch has no settings entry point, and never had one —
// the popover this replaced was in the desktop toolbar only. What is checked
// here is that opening a vault on a phone still works after the swap.
const { devices } = await import('playwright');
const touchCtx = await browser.newContext({ ...devices['iPhone 13'] });
const touch = await touchCtx.newPage();
const touchErrors = [];
touch.on('pageerror', (e) => touchErrors.push(String(e).slice(0, 200)));
await touch.goto(`${APP}/login`);
await touch.waitForTimeout(1500);
await touch.locator('input[autocomplete="username"]').first().fill(username);
await touch.locator('input[type="password"]').first().fill(password);
await touch.getByRole('button', { name: /sign in|log in|unlock/i }).first().click();
await touch.waitForTimeout(9000);
check('the vault list reachable on a real phone', /\/vaults/.test(touch.url()), touch.url());
check('and the phone vault list has the settings gear',
  await touch.getByTestId('open-settings').first().isVisible().catch(() => false));
await touch.getByText('Settings hub check').first().click();
await touch.waitForTimeout(7000);
check('opening a vault on a phone still works',
  /\/vaults\/[0-9a-f-]{8}/.test(touch.url()), touch.url());
await shot(touch, 'touch-editor');
check('no page errors on the phone', touchErrors.length === 0, touchErrors.join(' | '));

check('no page errors', errors.length === 0, errors.join(' | '));

await browser.close();
console.log(failures === 0 ? '\nAll settings-hub checks passed.' : `\n${failures} check(s) failed.`);
process.exit(failures === 0 ? 0 : 1);
