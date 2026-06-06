# Changelog

All notable changes to this repository are documented here.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

## [Unreleased]

### Added
- **Lobby search** — instant search bar on the Vaults page filters the displayed vault list by name, vault note, and labels. Searches only metadata already loaded in the lobby; never reads vault contents. Shows a result count badge when filtering is active and a "Clear search" shortcut when no results are found.
- **Table / list view** — new compact table layout for the Vaults page, toggled by a grid/list icon pair next to the search bar. Each row shows: a 80 × 46 px thumbnail (blurred for shared vaults), vault name with labels and note excerpt, last-updated date, node count and version count. All row actions (open, rename, history, delete) are available inline. Implemented as a memoised `VaultTableRow` component. View preference is session-local.
- **Clickable vault preview (card view)** — the preview image in card view is now a clickable button that navigates directly into the vault. Shows a subtle hover opacity to signal interactivity.

### Changed
- **Vault card re-render fix** — eliminated a cascade where editing any single vault's settings (color, note, labels, max versions) caused every vault card to re-render and re-fetch share counts and preview images. Root cause: `useEffect` hooks in `VaultsPage` held direct references to the `maps` state array; any draft mutation produced a new array reference, re-firing all three effects. Fixed by deriving a stable string key (`mapMetaKey`) that only changes when vault identity or server-persisted `updated_at` changes, and reading the current maps array via a `useRef` (latest-ref pattern) inside effects so they never need to depend on the live reference.
- **Vault preview panel cleanup** — removed nested frame/shell divs that surrounded the preview screenshot in card view, resulting in a single clean rounded container instead of three stacked bordered rectangles.
- **Table view tooltip fix** — the label/note hover tooltip in table view now renders via a React portal at `document.body` with `position: fixed`, ensuring it always appears above the search bar and any other page elements. Previously the tooltip was clipped by the table wrapper's `overflow: hidden` and appeared behind the search input.

### Fixed
- **Attached-file count double-counting** — `attachment_count` and `attachment_bytes` in the storage summary (both `/mindmaps/my/storage` and the account storage endpoint) were counting each file twice: once for the primary attachment and once for the auto-generated encrypted preview thumbnail (`cryptmind_role: "preview"`). Both `load_map_attachment_storage` in `mindmaps_sql.rs` and the inline loop in `auth_sql.rs` now filter out preview records from the user-facing count and bytes. Total bytes (`total_bytes`) still includes preview storage for accurate disk accounting.

### Removed

### Validation
- `pnpm exec tsc --noEmit` in `frontend_app` → clean.
- `cargo check` in `backend` → clean.

## [0.3.28] - 2026-06-05

### Added
- **FreeMind / FreePlane import** — new `freemindImport.ts` utility parses both FreeMind (`.mm`, `version="1.0.1"`) and FreePlane (`.mm`, `version="freeplane 1.x"`) XML. Format is auto-detected from the `<map version>` attribute. Handles FreePlane nodes where the `TEXT` attribute contains a raw HTML document string — prefers `<richcontent TYPE="NODE">` content, falls back to `stripHtml()`. Maps `COLOR`, `BACKGROUND_COLOR`, `FOLDED`, `POSITION`, `LINK`, and `<richcontent TYPE="NOTE">` to the internal node model.
- **FreeMind export** — new `freemindExport.ts` exports the active mind map as a FreeMind-compatible `.mm` file.
- **FreePlane export** — new `freeplaneExport.ts` exports the active mind map as a FreePlane-compatible `.mm` file, including `ID="ID_<timestamp><counter>"` attributes required by FreePlane.
- **WiseMapping import** — new `wisemappingImport.ts` parses WiseMapping `.wxml` files. Resolves root via `topic[central="true"]` or first `<topic>` child. Maps `text`, `bgColor`, `position`, `<note>` (CDATA), `<link url>`, and child `order`.
- **WiseMapping export** — new `wisemappingExport.ts` exports the active mind map as a WiseMapping-compatible `.wxml` file with `<note><![CDATA[...]]></note>` and `<link url="..."/>`.
- **XMind import** — new `xmindImport.ts` reads `.xmind` files (ZIP archives). Supports XMind Zen / 2020+ (`content.json`) and XMind 8 / legacy (`content.xml`). Maps `title`, `notes.plain`, `href`, and `style.properties['background-color']` to the internal node model. Requires `fflate` for ZIP parsing.
- **XMind export** — new `xmindExport.ts` writes a `.xmind` ZIP archive in the XMind Zen JSON format with `content.json` and `META-INF/manifest.xml`. Requires `fflate` for ZIP creation.
- **WSL dev build script** — `scripts/dev-build.sh` automates container rebuilds in WSL dev environments. Builds the frontend, hot-swaps the server container via `docker compose`, and creates a dedicated `mmvdev` Postgres user/database on first run. Supports `--down` and `--no-cache` flags.

### Changed
- **Unified import dropdown (Vaults page)** — replaced separate import buttons with a single "Import ▾" dropdown in `VaultsPage.tsx`. Dropdown options: Markdown (.md), FreeMind (.mm), FreePlane (.mm), WiseMapping (.wxml). FreeMind and FreePlane share one file picker; the parser auto-detects the format. Includes a click-outside handler to close the menu.
- **Export menu additions (Editor)** — added FreeMind (.mm), FreePlane (.mm), WiseMapping (.wxml), and XMind (.xmind) entries to the editor export context menu via new optional props `onExportFreemind`, `onExportFreeplane`, `onExportWisemapping`, `onExportXmind` on `MindMapEditor`.
- **Markdown import — Obsidian mind map plugin compatibility** — `markdownImport.ts` updated to correctly handle the syntax used by Obsidian Mind Map (lynchjames), Markmap for Obsidian, and Mindmap NextGen:
  - Task-list items `- [ ] text` / `- [x] text` now set `node.checked`.
  - Obsidian highlight syntax `==text==` is stripped to plain text.
  - Images `![alt](url)` are replaced by their alt text.
  - Obsidian tags (`#tag`) are stripped from node labels.
  - HTML comment lines (`<!-- markmap: {...} -->` etc.) are skipped entirely.
  - Obsidian callouts `> [!type] Title` have the `[!type]` marker stripped; the title text is still appended to notes.
  - Tab indentation in nested lists is treated as 4 spaces (matches Obsidian and VSCode defaults).

### Fixed
- **Vault preview crash on collapsed nodes** — `walkConnectors` in `vaultPreview.ts` crashed with `TypeError: Cannot read properties of undefined (reading 'x')` when saving a vault that contained nodes with `FOLDED="true"` (e.g. imported from FreePlane). Root cause: `layoutTree` excludes collapsed children from its output map, but `walkConnectors` iterated them anyway. Fix: added `if (!parentLayout) return` and `if (!childLayout) continue` guards. Saves for vaults with collapsed nodes now complete successfully (green diskette indicator).

### Validation
- `pnpm --dir frontend_app build` → passed (after `pnpm --dir frontend_app install` to pull `fflate`).
- FreePlane files with `FOLDED="true"` nodes save without console errors.
- Markdown files exported from Obsidian with task lists, highlights, and callouts import cleanly.

## [0.3.27] - 2026-05-26

### Changed
- **Connector key canonicalization (Server frontend)** — Updated connector typing in `frontend_app/src/app-core/connectors/types.ts` to enforce canonical capability keys for feature and billing checks.
- **Connector capability defaults** — Updated `frontend_app/src/platform/bootstrap.ts` to use explicit capability handling and deterministic unsupported-feature behavior (`false` instead of permissive/implicit behavior).
- **Contributor policy alignment** — Added canonical connector capability-key naming guidance to `.github/copilot-instructions.md` for cross-repo consistency.
- **Release versioning** — Bumped release version from `0.3.26` to `0.3.27` across `backend`, `frontend_app`, and desktop Tauri metadata.

### Validation
- `pnpm --dir frontend_app build` → passed.

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
