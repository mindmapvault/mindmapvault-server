# Tests

This folder contains repository test helpers that are not tied to a single build system.

Current layout:

- `tests/performance/load-test.mjs` - JavaScript load test for the backend server
- `tests/endpoints/release-smoke.mjs` - release-critical behaviour against a running backend
- `tests/endpoints/share-flow.mjs` - end-to-end encrypted share flow, owner and recipient sides
- `tests/ui/settings-hub.mjs` - the in-app settings hub, driven in a real browser

Both endpoint scripts take `--base-url` (default `http://127.0.0.1:8090`) and exit non-zero on
failure. They need PostgreSQL and an S3 store that returns version ids — Garage does by default,
MinIO needs bucket versioning enabled (`mc version enable <alias>/<bucket>`).

```bash
node tests/endpoints/release-smoke.mjs --base-url http://127.0.0.1:8090
node tests/endpoints/share-flow.mjs --base-url http://127.0.0.1:8090
```

`tests/ui/settings-hub.mjs` drives the app in Chromium, so it needs both a running backend and the
app being served, plus Playwright, which is not a dependency of this repository:

```bash
npm i -D playwright && npx playwright install chromium
node tests/ui/settings-hub.mjs --app-url http://127.0.0.1:5173 --api-url http://127.0.0.1:8090
```

It signs up a throwaway account and creates a vault, so point it at a disposable instance. Pass
`--screenshot-dir <dir>` to keep screenshots of each step; without it none are written.

Performance test defaults target a local server at `http://127.0.0.1:8090` and model 200 concurrent users.

Run it from the repository root with Node.js 20+:

```powershell
node tests/performance/load-test.mjs --users 200 --concurrency 200 --no-cleanup
```

The load test covers auth, profile, settings, notifications, and vault metadata flows. Use `--cleanup` when the backend cleanup path is available and you want the test data removed after the run.