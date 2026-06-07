# Changelog

All notable changes to this repository are documented here.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

## [Unreleased]

## [0.3.30] - 2026-06-07

### Added

#### Evidence Board
- **Evidence board canvas** — new vault type for free-form evidence/investigation boards. Cards, images, and connectors can be placed freely on a zoomable/pannable infinite canvas. Boards are end-to-end encrypted with the same hybrid KEM + AES-256-GCM pipeline as mind maps.
  - `src/board/BoardTypes.ts` — data model: `BoardTextCard`, `BoardImageCard`, `BoardConnector`, `BoardData`.
  - `src/board/BoardEngine.ts` — pure manipulation helpers (add, move, delete cards and connectors).
  - `src/components/BoardEditor.tsx` + `BoardEditor.css` — full board editor component with drag-to-move cards, draw connectors, resize, zoom/pan, and per-card color theming.
  - `src/pages/BoardPage.tsx` — page that loads, decrypts, edits, and saves boards using the existing `hybridDecap/Encap` flow. Includes version label, save status, attachment support, and back-navigation.
  - `src/app-core/AppRoot.tsx` — `/boards/:id` route added alongside `/vaults/:id`.
  - `src/crypto/vault.ts` — `encryptBoard` / `decryptBoard` helpers (AES-256-GCM via the same DEK pipeline as `encryptTree` / `decryptTree`).
- **Create board from lobby** — the Vaults page "Create" button now opens a dropdown with **New Mind Map** and **New Evidence Board** options. Boards are tagged with the internal `__board__` label that is filtered from all UI display.
- **Board routing in vault list** — vault cards and table rows detect the `__board__` label and navigate to `/boards/:id` instead of `/vaults/:id`. The `__board__` label is stripped from visible label chips.

#### Voice Recording
- **Voice recording attachment** — users can record audio directly on the mobile canvas and attach the encrypted recording to a node.
  - **Record button in mobile action bar** — mic icon button added between Delete and Fit; disabled on root node.
  - **Record Audio option in file upload sheet** — fourth option in the mobile attach-files bottom sheet opens the recording UI instead of a file picker.
  - **Recording bottom sheet** — three-state UI: *Idle* (pulsing mic button, "Tap to start recording"), *Recording* (MM:SS timer, pulsing stop button), *Recorded* (`<audio>` playback preview, filename input, Save & Upload / Re-record / Discard actions).
  - Uses `MediaRecorder` API with `audio/webm;codecs=opus` → `audio/webm` → platform default codec fallback chain.
  - On save, recording is wrapped in a `File` object and routed through the existing `attachFilesToSelectedNode` flow so it is encrypted before upload.
  - Blob URL memory managed via `useMemo` + `useEffect` cleanup (`URL.revokeObjectURL`); recording timer cleaned up on unmount.

#### PWA Offline Sync
- **`src/storage/idb.ts`** — `MmvIdb` class; IndexedDB database `mmv-offline-v1` with four object stores:
  - `vault_list` — cached vault list (single record, keyed `'list'`).
  - `vault_meta` — per-vault `MindMapDetail` metadata.
  - `vault_blobs` — encrypted blob bytes with `version_id` for conflict detection.
  - `sync_queue` — pending server operations (autoIncrement key); each entry records op type, vault ID, payload, `base_version_id`, timestamp, and attempt count.
- **`src/storage/offline.ts`** — `OfflineStorageAdapter implements StorageAdapter`:
  - **Read-through cache** — every `getVault`, `downloadBlob`, and `listVaults` call while online writes the result to IDB so the data is available offline.
  - **Write-through (online)** — `updateVault` / `uploadBlob` / `updateMeta` write to the server; `uploadBlob` additionally captures the new `minio_version_id` from a post-upload `getVault` call and stores it alongside the cached blob.
  - **Write-local (offline)** — the same calls cache to IDB and enqueue to `sync_queue`. The blob is persisted to `vault_blobs` **before** enqueue, so the user's work is never lost even if the sync queue flush fails.
  - **`drainSyncQueue()`** — processes ops in insertion order; before uploading a blob checks the server's current `minio_version_id` against the stored `base_version_id`. Emits `conflict` status if they diverge. Failed ops increment an attempt counter and are dropped after 3 failures.
  - **`resolveConflict(vaultId, 'local' | 'server')`** — "keep mine" force-uploads the cached blob overwriting the server; "use server" removes all pending ops for the vault from the queue.
  - Observable `SyncStatus` (`state: 'idle' | 'syncing' | 'conflict' | 'error'`, `pendingCount`, `lastSyncedAt`, `conflictVaultId`) via `onStatusChange()` subscribe/unsubscribe pattern.
- **`src/storage/index.ts`** — `getOfflineStorage()` singleton factory; `isPwa()` detection helper (`display-mode: standalone` media query + iOS `navigator.standalone`).
- **`src/components/OfflineBanner.tsx`** — contextual banner rendered above the editor when using the offline adapter: red dot + "X unsaved changes" when offline; spinner + count while syncing; conflict prompt with "My offline edits" / "Server version" buttons; error + Retry button; transient green "✓ All changes synced" flash on successful drain.
- **`src/pages/EditorPage.tsx`** — storage `useMemo` now selects `OfflineStorageAdapter` when `isPwa()` returns true; two `useEffect`s subscribe to status changes and listen to `window.online` / `window.offline` events to auto-drain the queue on reconnect; 3-second "synced" flash state managed independently of the adapter status.

#### Attachment Improvements
- **Audio attachment playback** — `previewOrOpenAttachment` detects `audio/*` content-type and file extensions (`.webm`, `.m4a`, `.mp3`, `.ogg`, `.wav`, `.aac`, `.flac`, `.opus`). The attachment preview modal renders a centered `<audio controls>` element with a large mic icon for audio files.
- **Audio card icon in notes dialog** — attachment cards with `audio/*` content-type show a red mic SVG icon in the thumbnail area instead of the generic "FILE" text.
- **PDF thumbnail utility** — `src/utils/pdfThumbnail.ts` renders the first page of a PDF to a JPEG data URL using `pdfjs-dist` (lazy-initialised worker).
- **`pruneVersionHistory` utility** — `src/api/mindmaps.ts` adds a fire-and-forget helper that deletes versions older than a configurable limit (default 30), skipping the current latest version; uses `Promise.allSettled` so individual delete failures are non-fatal.

### Changed

#### Mobile Canvas UX
- **Mobile props toolbar restructured** — top row: Notes / Labels / Files; bottom row: Date / Checkbox / Icons. Files button opens the new attach-files bottom sheet (previously absent).
- **Labels as props sub-view** — tapping Labels in the mobile props panel now opens a sub-view inside the same bottom panel (canvas remains visible above) rather than launching a separate `position: fixed` overlay. Sub-view has ← Back and × navigation; library items render one-per-row with full-width tap targets; Add / Save to lib buttons wrap to a second line.
- **Mobile file upload bottom sheet** — PWA-capable attach sheet with three distinct file input paths (Camera with `capture="environment"`, Photo Library, Browse Files) plus the new Record Audio option. Each input is a separate hidden `<input type="file">` with the appropriate `accept` and `capture` attributes.
- **Mobile checkbox fix** — checkbox cycle no longer includes a `null` / "Checkbox" label state; only "Unchecked" and "Checked" are shown.

#### Attachment Preview
- **Attachment preview modal fully styled** — the modal was previously unstyled (no CSS), causing it to render in document flow behind the notes modal. Added complete `position: fixed` layout (`z-index: 801`), flex column structure, mobile full-screen override, image / PDF / audio body sections, header + footer with Download and Close buttons.
- **Image thumbnail fix** — `loadAttachmentPreview` guarded on `preview_attachment_id` being truthy, which meant images (which use their own `attachment_id` as the preview source) never loaded a thumbnail. Guard updated to `if (!isImage && !attachment.preview_attachment_id) return`.

### Fixed
- **Backend: S3 get-object 404 now returns `AppError::NotFound`** — `MinioClient::get_object` in `backend/src/db/minio.rs` previously mapped all AWS SDK errors to `AppError::Storage`. The error mapper now inspects the HTTP status (404) and service code (`NoSuchKey`, `NotFound`, `NoSuchVersion`) and returns `AppError::NotFound("board content not found in storage")` for missing objects, enabling correct 404 HTTP responses for board content that has not been uploaded yet.

### Validation
- `pnpm exec tsc --noEmit` in `frontend_app` → clean.

## [0.3.29] - 2026-06-06

### Added
- **Dark / light mode toggle in canvas toolbar** — sun/moon button added to the editor toolbar so users can switch themes without leaving the canvas. State is persisted via `useThemeStore` (zustand/persist).
- **Mobile canvas experience** — when a viewport ≤ 768 px is detected, the full-screen canvas editor switches to a mobile chrome layout:
  - **Mobile top bar** — vault name (truncated), version label, save-status button, and theme toggle replace the desktop toolbar.
  - **Mobile bottom action bar** — five-button dock: Back (lobby), Add (child node), Delete (with confirmation step), Fit (zoom-to-fit all nodes), Props (opens the props sheet). The delete button requires a second tap on a Confirm button to execute, preventing accidental deletions.
  - **Mobile props sheet** — bottom sheet triggered by the Props button; contains: node color swatches (9 presets), Notes / Date / Labels action buttons, progress pill presets (✕ / 0% / 25% / 50% / 75% / 100%), Checkbox toggle, and an Icons button that opens the icon picker as a full-width bottom sheet.
  - **Mobile labels dialog** — the tags/labels dialog renders as a `position: fixed` bottom sheet with rounded top corners and larger touch targets instead of the desktop's small absolute popup.
  - **Mobile icon picker** — `MindMapIconPicker` overridden via CSS to render as a `position: fixed` bottom sheet (max 72 vh) when accessed from the mobile props sheet.
- **Pinch-to-zoom on touch canvas** — two-finger pinch gesture scales the canvas zoom (0.2 × – 4 ×) proportionally to the distance ratio between touchstart and touchmove. Single-finger pan is unaffected.
- **Responsive toolbar** — desktop toolbar restructured into a single flex row: vault name and version always visible in a left nav section; action buttons fill remaining space and wrap to a second row when the window is too narrow. The second-row separator appears only when wrapping occurs (`box-shadow: inset 0 1px 0`).
- **Close button on Labels dialog** — the labels / tags dialog now has an × button in the title bar, matching the existing Date Planning dialog pattern.
- **Lobby search** — instant search bar on the Vaults page filters the displayed vault list by name, vault note, and labels. Searches only metadata already loaded in the lobby; never reads vault contents. Shows a result count badge when filtering is active and a "Clear search" shortcut when no results are found.
- **Table / list view** — new compact table layout for the Vaults page, toggled by a grid/list icon pair next to the search bar. Each row shows a thumbnail, vault name with labels and note excerpt, last-updated date, node count, and version count. All row actions (open, rename, history, delete) available inline. Implemented as a memoised `VaultTableRow` component; view preference is session-local.
- **Clickable vault preview (card view)** — the preview image in card view is now a clickable button that navigates directly into the vault.

### Changed
- **Wheel zoom uses non-passive native listener** — `onWheel` React prop removed from the SVG canvas element. A `useEffect` now attaches a native `wheel` listener with `{ passive: false }` so `e.preventDefault()` works correctly for Ctrl+wheel zoom, eliminating the browser console warning "Unable to preventDefault inside passive event listener".
- **Appearance (ThemePanel) button hidden on mobile** — the settings cog is not shown in the mobile top bar; only save status and theme toggle are exposed.
- **Vault card re-render fix** — eliminated a cascade where editing any single vault's settings caused every vault card to re-render. Fixed by deriving a stable `mapMetaKey` and reading the maps array via a `useRef` (latest-ref pattern) inside effects.
- **Vault preview panel cleanup** — removed nested frame/shell divs that surrounded the preview screenshot in card view.
- **Table view tooltip fix** — the label/note hover tooltip now renders via a React portal at `document.body` with `position: fixed`, ensuring it always appears above the search bar.

### Fixed
- **Passive event listener violation** — `e.preventDefault()` on wheel events inside a React synthetic handler triggered repeated browser warnings. Resolved by switching to a native event listener with `{ passive: false }`.
- **Attached-file count double-counting** — `attachment_count` and `attachment_bytes` were counting each file twice (primary + auto-generated preview thumbnail). Both `load_map_attachment_storage` in `mindmaps_sql.rs` and the inline loop in `auth_sql.rs` now filter out `cryptmind_role: "preview"` records from user-facing counts.

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
