# Tests

This folder contains repository test helpers that are not tied to a single build system.

Current layout:

- `tests/performance/load-test.mjs` - JavaScript load test for the backend server

Performance test defaults target a local server at `http://127.0.0.1:8090` and model 200 concurrent users.

Run it from the repository root with Node.js 20+:

```powershell
node tests/performance/load-test.mjs --users 200 --concurrency 200 --no-cleanup
```

The load test covers auth, profile, settings, notifications, and vault metadata flows. Use `--cleanup` when the backend cleanup path is available and you want the test data removed after the run.