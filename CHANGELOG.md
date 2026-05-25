# Changelog

All notable changes to this repository are documented here.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

## [Unreleased]

### Added

### Changed
- **Backend Dependency Security** — Updated backend dependency resolution to clear RustSec findings by moving to patched AWS SDK transitive crates (`aws-config`/`aws-sdk-s3` resolved forward), upgrading `pprof` to `0.15`, and removing `jemalloc-ctl` from the diagnostics path.
- **Diagnostics Endpoint Behavior** — `/api/mindmaps/maintenance/allocator-stats` now returns a deterministic "disabled in this build" response instead of querying allocator internals through `jemalloc-ctl`.
- **Docker Hub Release Tags** — Standardized published container version tags on the `v<version>` form (`v0.3.26`) and removed the stray bare `0.3.26` tag from Docker Hub so the registry matches Git tag naming and release references.
- **Deployment Installer Flow** — Verified the Docker Hub deployment installer can download the release bundle, write its config, and bring up a fresh stack successfully after removing the stale fixed-name containers from the previous run.
- **Public OSS Feature Status** — Added a public `docs/OSS_FEATURES.md` page that lists the shipped OSS server capabilities, marks them as done, and clearly separates the server-only OSS scope from hosted/enterprise out-of-scope features.
- **Server Scope Documentation Alignment** — Updated Server docs to consistently define the OSS Server product as online-only backend + web UI, and to keep sync/offline/collaboration and public API-spec promises out of scope for this repository.

### Removed

### Validation
- `cargo check --manifest-path backend/Cargo.toml` in WSL workspace → passed.
- `cargo audit -q --manifest-path backend/Cargo.toml` in WSL workspace → passed (`BACKEND_AUDIT_PASS`).
- `./scripts/publish_dockerhub/publish-to-dockerhub.sh v0.3.26` in WSL → passed; pushed `kornelko2/mindmapvault-server:v0.3.26` and `latest`, and packaged `dist/mindmapvault-server-v0.3.26-deploy.tar.gz`.
- Docker Hub API check → confirmed `0.3.26` tag removal and remaining `v0.3.26`, `latest`, and `sha-*` tags.
- `scripts/publish_dockerhub/setup.sh` in a clean WSL temp directory with the live stack stopped and removed → passed end-to-end and reported all services healthy.

## [0.3.26] - 2026-05-24

### Added
- **PWA Addon (Server UI)** — Added installable PWA support to the self-hosted `frontend_app`, including service worker registration, web app manifest generation, and install prompts for browser users.
- **Login Install Prompt UX** — Added a centered floating install panel on the login screen with explicit `Install app` and `Dismiss` actions and explanatory copy about PWA behavior.

### Changed
- **CI Policy** — Removed the server-repo offline parity workflow and clarified that offline parity checks are only required in the FOSS repo or for explicit shared offline-capability changes.
- **Security Procedure** — Documented the GitHub Actions security gate as the committed-secrets scan in `.github/workflows/security-guard.yml` and called out stable action pinning expectations in the Copilot instructions.
- **Hosted Runtime Reliability** — Hardened the hosted server routes so browser deep links (`/login`, `/register`) resolve through the SPA fallback instead of returning 404, and so storage/version listing endpoints degrade gracefully when Garage cannot provide object-version metadata.
- **Object Upload Compatibility** — Accepted opaque S3/Garage version IDs returned from uploads instead of requiring UUID-shaped IDs, which fixes hosted uploads against the current Garage backend.
- **Release Versioning** — Bumped release version from `0.3.24` to `0.3.26` across `backend`, `frontend_app`, and desktop Tauri metadata so build/version labels and release artifacts stay aligned.
- **PWA Install Prompt Behavior** — Login install prompt now appears once as a single floating panel; dismissing or declining the prompt hides it immediately and persists dismissal via local storage.
- **Brand Asset Consistency** — Synced server `frontend_app` favicon assets to the same canonical files used by `mindmapvault-www` (including `.ico` and `.png`) and updated login icon references accordingly.
- **Dev Experience** — Disabled PWA service worker generation in Vite dev mode to remove noisy Workbox glob warnings from `dev-dist` while preserving production PWA generation.

### Validation
- `cargo check --manifest-path backend/Cargo.toml` in native WSL workspace → clean.
- `cargo test --manifest-path backend/Cargo.toml db::minio::tests -- --nocapture` in native WSL workspace → passed.
- `pnpm run build` in `frontend_app` → passed.
- `docker build -f backend/Dockerfile -t mindmapvault-server:local .` → passed.
- `node tests/performance/load-test.mjs --base-url http://127.0.0.1:8090 --users 200 --concurrency 200 --cleanup` → passed.
- `wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server && docker build -f backend/Dockerfile -t mindmapvault-server:local .'` → passed.
- `wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/mindmapvault-server && docker compose up -d --force-recreate server && docker compose ps'` → passed.
- `pnpm build` in `frontend_app` → passed with generated PWA assets (`dist/manifest.webmanifest`, `dist/sw.js`).

## [0.3.25] - 2026-05-03

### Added
- **Security / Feature** — Server-mode password change (key rotation) via `POST /api/auth/rotate-credentials`.
  - Backend (`auth_sql.rs`): new authenticated route that re-verifies the current password (even over a live JWT session), enforces complete vault coverage, and executes all credential and vault-title updates in a single PostgreSQL transaction. Partial rotation is impossible — either every change commits or nothing changes.
  - Backend (`postgres.rs`): `rotate_user_credentials` implementation uses explicit `BEGIN`/`COMMIT`/`ROLLBACK` since `Arc<Client>` does not expose `client.transaction()`. Raw auth token is Argon2id-hashed server-side before the transaction; it never rests on disk.
  - Backend (`models/user.rs`): `KeyBundleResponse` now includes `argon2_salt` and `argon2_params` so the client can re-derive the master key for rotation without a second unauthenticated salt request. `RotateCredentialsRequest` and `RotateVaultApiEntry` request types added.
  - Backend route guards: server rejects bundles where `new_key_version != current + 1` (prevents replay) and rejects bundles missing any vault owned by the user (prevents unreadable vault titles after rotation).
  - `frontend_app/src/crypto/keyRotation.ts`: `buildPasswordRotationBundle` now also returns `currentAuthToken` and `newAuthToken` (HKDF-derived hex strings) for use by the server-mode caller.
  - `frontend_app/src/api/auth.ts`: `getKeyBundle()` and `rotateCredentials()` API methods added.
  - `frontend_app/src/pages/ChangePasswordPage.tsx`: server-mode branch added. Fetches key bundle + full vault list from the API, builds the rotation bundle entirely client-side, then calls `rotateCredentials`. On success, updates session with new master key and fresh JWT tokens. Vault blobs in object storage are never touched — they are KEM-encrypted to the keypair which is unchanged during rotation; all historical versions remain decryptable.
  - `frontend_app/src/pages/VaultsPage.tsx`: "Change password" button is now visible in both local and server modes.

### Changed
- **Editor UX Parity** — Verified server-mode `frontend_app` retains full node icon support parity with FOSS editor updates (`I` shortcut, toolbar/context icon picker access, and inline node icon rendering) so both products stay behaviorally aligned.

### Validation
- `pnpm exec tsc --noEmit` in `frontend_app` → clean.
- `node scripts/check_frontend_offline_parity.mjs` in repo root → passed.
- `node scripts/check_no_committed_secrets.mjs` in repo root → passed.

## [0.3.24] - 2026-05-03

### Changed
- **Local-Mode Auth Correctness** — Updated `frontend_app/src/store/auth.ts` so local unlocked sessions are treated as authenticated via in-memory session keys, even without server tokens.
- **Local Privacy Hardening** — Local-mode color preference now avoids plaintext persistence in desktop index metadata; local UI color state is handled client-side while index writes scrub plaintext color.
- **Local Cryptography Hardening** — Increased local/generated Argon2 salt size to 32 bytes in local unlock and rotation/share generation paths.
- **Desktop Index Reliability** — Added process-level mutex locking around local `index.json` read-modify-write command paths in `desktop/src-tauri/src/local_store.rs` to prevent concurrent lost updates.
- **Desktop Integrity Checks** — Added HMAC-SHA256 entry MAC stamping and local integrity verification command support in `desktop/src-tauri/src/local_store.rs`.

### Validation
- `pnpm exec tsc --noEmit` in `frontend_app` → clean.
- `cargo check` in `desktop/src-tauri` → clean.

## [0.3.23] - 2026-05-07

### Added
- **Security / Feature** — Local password change (key rotation) for desktop local mode, mirrored from FOSS. Users can now change their unlock password from the Vaults page via the new "Change password" button (visible only in `isLocalMode`). Rotation verifies current password, re-derives master key from a fresh salt, re-wraps private keys, and re-encrypts all vault titles and notes. Vault blobs are not touched.
- **Reliability** — Crash-safe two-phase rotation commit in `desktop/src-tauri/src/local_store.rs` with `recover_interrupted_rotation()` called from `migrate_if_needed()` on every startup.
- `frontend_app/src/crypto/keyRotation.ts` — same crypto module as FOSS. No server contact required for local-mode rotation.
- `frontend_app/src/pages/ChangePasswordPage.tsx` — same page as FOSS with progress states and per-field validation.
- `apply_local_password_rotation` Tauri command registered in `desktop/src-tauri/src/lib.rs`.
- Route `/change-password` added to `frontend_app/src/App.tsx` (desktop-only guard).

## [0.3.22] - 2026-05-04

### Changed
- **Security** — Added `deriveAttachmentWrapKey` to `frontend_app/src/crypto/kdf.ts`. New function derives a domain-separated 32-byte AES-GCM key via `HKDF-SHA256(master_key, info="crypt-mind-attachment-wrap-v1")`. New attachment encryptions (`encryptAttachmentForOwner`) now use this key and record `key_wrap: 'hkdf-attachment-v1'` in their metadata, ending the dual-role use of raw master key bytes for both HKDF IKM and direct AES-GCM encryption.
- **Security** — `decryptAttachmentForOwner` branches on `encryptionMeta.key_wrap`: records tagged `'hkdf-attachment-v1'` use `deriveAttachmentWrapKey`; older records tagged `'master-aes-256-gcm'` fall back to `deriveMasterAesKey` for backward compatibility. Existing encrypted attachments are unaffected.
- **Security** — `deriveShareKey` default `parallelism` raised from `1` to `4` to match `DEFAULT_ARGON2_PARAMS` (`p_cost: 4`). Share bundles store their Argon2id parameters in `encryptionMeta`, so existing share bundles decrypt using their stored value and are unaffected.
- `deriveMasterAesKey` comment updated to explicitly mark it as backward-compat-only for older attachment records and for Register/Login/Unlock private-key wrapping (migration of the latter is a documented follow-up).

### Validation
- TypeScript type-check (`tsc --noEmit`) in `frontend_app` - 2026-05-03

### Fixed
- **Reliability** — Eliminated a crash-window data-loss bug in `write_bytes_atomic` (`desktop/src-tauri/src/local_store.rs`). The previous implementation deleted the target file before renaming the temp file into place; a crash or power loss in that gap permanently destroyed the data. The fix removes the explicit `remove_file` entirely. `std::fs::rename` calls `rename(2)` on POSIX (atomic replace) and `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` on Windows — both replace the destination in a single step without a separate delete. A temp-file cleanup on rename error is now also handled explicitly.

### Validation
- `cargo check` in `desktop/src-tauri`

## [0.3.20] - 2026-05-03

### Changed
- **Security** — Added `validate_username` in `desktop/src-tauri/src/local_store.rs` to reject usernames containing path-separator characters (`/`, `\`, `..`, null byte). Previously a crafted username could escape the intended per-user storage directory (path traversal). The fix validates the username in both `local_dir` and `profile_path_for` and returns `LocalStoreError::InvalidUsername` on rejection.
- **Security** — Replaced `"csp": null` with an explicit Content Security Policy in `desktop/src-tauri/tauri.conf.json`. The policy restricts scripts to `'self'`, blocks inline script injection, and limits `connect-src` to Tauri IPC and the configured backend origin. This prevents a malicious vault file from executing injected JavaScript inside the WebView with access to all Tauri invoke commands.

### Validation
- `cargo check` in `desktop/src-tauri`

## [0.3.19] - prior
