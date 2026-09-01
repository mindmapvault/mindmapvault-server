# Changelog

All notable changes to this repository are documented here.

The format is based on Keep a Changelog and this project follows Semantic Versioning.

## [Unreleased]

### Fixed
- **The recommended install pulled a four-month-old image.** `scripts/publish_dockerhub/setup.sh` and the default `docker-compose.yml` both pull from Docker Hub, but Docker Hub publishing was removed from the build workflow in May, so everything since went to GHCR alone and the Docker Hub tag stopped moving. Releases now publish the same digests to both registries; a fork without the Docker Hub secrets still publishes to GHCR alone rather than failing. `docs/DEPLOYMENT.md` documents both registries, what each tag means, and how to republish a release to a registry it did not reach.

## [0.3.33] - 2026-08-31

### Added
- **The admin console is rebuilt around the person who runs the server.** Four screens: **Status**, **People** (accounts and invite codes), **Settings** (the rules below), and **Maintenance** (run the cleanup, what to back up, and a log of every change made from the console). Written for someone with a box in a cupboard and a few friends on it, not for a billing department, and styled as an operations tool — one neutral surface, one accent, colour reserved for state that means something.
- **A light theme for the admin console**, following the operating system by default with an Auto/Light/Dark control that overrides it and is remembered per browser.
- **Status reports what a self-hoster actually needs to know.** Free space on the filesystem the container sits on, with warnings at 80% and 92% full, since running out of disk is the failure that loses data rather than merely annoying someone (`STATUS_DISK_PATH` points it at another filesystem when the volumes live elsewhere). Whether Postgres and the object store answer and how long they take, the PostgreSQL version and the database's size on disk, how many objects the bucket holds and their total size, and the server's own memory use where the platform reports it. The bucket total normally exceeds what the accounts add up to, and the page explains why: every vault keeps older versions to roll back to, and revoked shares wait there for the daily cleanup.
- **Invite codes.** Closing sign-ups used to leave no way to add anyone, and there cannot be a "create user" button: registration is zero-knowledge, so the browser derives the account's keys from the password and the server never sees it — nobody but the new user can make their account. The console generates single-use codes instead, shaped `MMV-XXXX-XXXX-XXXX-XXXX` from an alphabet with no look-alike characters because these get read down a phone. Optional expiry, revocable before use, and the link fills the code in for whoever follows it. Typing it by hand works in any casing, with or without the dashes. A sign-up that fails for another reason — a taken username — hands the code back rather than burning it, and while sign-ups are open codes are neither required nor spent.
- **Instance settings, editable from the admin console.** A self-hosted instance had no way to say no: registration was always open, every account had unlimited storage, and the sign-in form could be hit as fast as anyone liked. `/admin/` now has a **Settings** screen for all of it, stored in the database and applied immediately without a restart.
- **Sign-ups can be turned off.** `POST /api/auth/register` is refused and the app hides both the sign-up form and the "create one" link on the sign-in page, so the door is visibly shut instead of opening onto a refusal after a minute of key derivation. Existing accounts are unaffected. A new `GET /api/public/instance` tells the app which it is.
- **A per-account storage limit and a largest-single-file limit.** An upload that would cross either is refused when the client asks to upload, before the bytes are sent, with the usage and the limit in the response body. The storage limit counts what the database records — attachments, share copies, and the files inside them; map blobs live in object storage with no recorded size and are outside the total, and a map's version history is bounded separately by its own version limit.
- **Sign-in throttling.** A per-address limit on sign-in, sign-up and the salt lookup (30 a minute by default), plus a lockout after repeated failed sign-ins against one username (10 attempts, then 15 minutes). Both answer `429` with `Retry-After`, and either can be switched off. A wrong username counts the same as a wrong password — skipping it would make a wrong username measurably cheaper, which is a way to ask the server which accounts exist. The lockout expires on its own because anyone who knows a username can trigger it deliberately.
- **`X-Forwarded-For` is honoured only when the operator turns it on.** Both throttles count per client address, and behind a reverse proxy every request arrives from the proxy; the header carries the real address but anyone can write anything in it. The Settings screen shows which address the current request was attributed to, so a wrong setting is visible rather than silent.
- **Environment seeds for a first boot** — `REGISTRATION_ENABLED`, `USER_STORAGE_LIMIT_BYTES`, `MAX_ATTACHMENT_SIZE_BYTES` and `TRUST_PROXY_HEADERS` set the initial values when the server starts against an empty database, so a new deployment can come up already closed. They are ignored once the settings row exists, leaving the admin console as the single authority rather than two places that can disagree. Effective values are logged at startup, with a warning when registration is open and no storage limit is set.
- Settings changes are recorded in the admin audit trail with their before and after values.
- **`tests/endpoints/instance-settings.mjs`** exercises the settings against a live backend: the switch, both caps, both throttles, the token comparison, and the audit entries. **`tests/endpoints/invites.mjs`** covers the invite lifecycle and the status endpoint.

### Removed
- **Plans, Stripe and access grants are gone from the admin surface**, along with the `/users/{id}/plan-override` and `/users/{id}/access-grants` endpoints and the store methods behind them. None of it did anything on this build: the Stripe fields were never written, and no route read `access_grants` for authorization. The console showed "paid users" and "active subscriptions" that were structurally always zero, and offered grants across `shared_plaintext`, `realtime_collaboration` and `kanban` surfaces this product does not have.

### Fixed
- **The admin console never loaded.** It expected a `feedback` array in the overview response — a hosted-product surface this backend has no endpoints for — and threw on `undefined.filter` immediately after sign-in. Nobody running this build had a working console.
- **The admin console called the wrong server.** With no build-time `VITE_ADMIN_API_BASE` it fell back to `127.0.0.1:8090` and then to the hosted SaaS API, so on any self-hosted install reached by its real hostname the console was asking a domain the operator does not run. It now defaults to the same origin, which is where this image serves both the console and the API.
- **Storage was reported against a plan that does not exist here.** `/api/auth/storage` returned the hosted free tier's 25 MB cap, so a self-hosted user holding more than that was told they were over a limit nothing enforced. Both storage endpoints now report the instance's own limit, or effectively boundless when none is set.
- **The admin bearer token was compared with `!=`**, which returns as soon as it finds a difference. It is now compared in constant time.
- **The app's service worker was swallowing `/admin/`.** Its navigation fallback had no denylist and its scope is the whole origin, so once a browser had opened the app, every later navigation — the admin console included — was answered with the app's own shell, and the operator got the app's sign-in page instead of the console. `/admin/`, `/api/`, `/share/` and `/health` are now excluded, and `release-smoke.mjs` asserts it.

## [0.3.32] - 2026-08-31

### Added
- **Encrypted share links now work.** The web app has shipped the share UI since 0.3.27 — a share tab in the editor, a `/shared/:id` page, and an API client calling `/api/mindmaps/{id}/shares…` — but the backend had no share routes at all, so every share action failed and every share link led to a dead page. The routes now exist: create, upload, complete, list and revoke a share, the same for share attachments, and an unauthenticated `/share/{id}` surface for recipients. A share is a second, independently-keyed copy of the vault, encrypted client-side under a passphrase the server never sees; the server stores ciphertext and serves it to whoever holds the link.
- **The recipient sees the map on a canvas**, drawn with the same renderer the vault previews use, instead of only a nested text outline. The outline is still there, one click away, as a text fallback. The SVG is handed to an `<img>` as a data URI rather than injected into the page — node text is someone else's content and this page is opened by people with no account here.
- **A share can be set to never expire.** The dialog offers it explicitly, with a note that the link then lives until it is revoked; previously an empty or invalid expiry silently produced the same result with nothing saying so.
- **The share dialog was reorganised** into what is shared, the passphrase, and access. The passphrase and its confirmation are masked with a single Show/Hide toggle for both, and pressing Generate reveals them — a generated passphrase you cannot read is a passphrase you cannot send on.
- **Share passphrases can be generated** — the dialog offers a passphrase of roughly 100 bits from an alphabet that drops look-alike characters, refuses anything under 12 characters, and warns that the passphrase must travel separately from the link, must not be the account password, and that the hint is stored unencrypted.
- **A sharing overview in the lobby.** Two cards summarise both directions and filter the vault list when clicked: **I'm sharing** counts the active shares across your vaults, and **Shared with me** counts the vaults you imported from someone's share. Each card appears only when it has something to show. Note what the incoming side can honestly be: a share is a link plus a passphrase with no recipient — the server never learns who opened one — so the only durable record that something was shared with you is a vault you chose to import. Imported vaults are now tagged with a reserved `__imported__` label (the same mechanism as `__board__`, and filtered out of the label chips) to make that grouping possible.
- **Ways to actually reach sharing.** The share panel previously had no entry point on the desktop at all — it opened only from the mobile props sheet or by typing `?secure=shares` on the editor URL. The desktop canvas toolbar now has **Vault files** (F7) and **Share exports** (F8) buttons, and each vault in the lobby gains a share action in both the card and table views, which opens the editor with the share panel already on.
- **`tests/endpoints/share-flow.mjs`** exercises the whole flow against a live backend: the owner path, the recipient path unauthenticated, ownership isolation between accounts, and that revoking removes the stored bytes.

### Fixed
- **Shares carried the vault's own preview thumbnails.** When the editor had no user attachments loaded, creating a share with "include files" fell back to the raw attachment list, which still contains the internal `__vault_preview_*` images — so recipients were handed an artefact that is not theirs to receive and that nobody asked to send. Both code paths now filter preview-role attachments, and both apply the available-status filter.
- **A wrong share passphrase gave no feedback at all.** The unlock error read `err.message`, but a failed AES-GCM decryption throws `OperationError` — an `Error` whose `message` is empty — so the error state was set to an empty string and the banner, which only renders a non-empty message, stayed hidden. Entering the wrong passphrase simply did nothing. All four error paths on the shared-vault page now fall back to a real message when the caught error carries none.

### Changed
- **File extensions now match the product name.** Share exports are `.mmvshare` (was `.cmvshare`) and the desktop vault export is `.mmv` (was `.cmvault`); the share blob's media type is `application/vnd.mindmapvault.share+json`. The `cmv`/`cryptmind` prefixes were left over from the project's former name, CryptMind Vault. Only names users see changed: the stored crypto format identifiers (`cryptmind-share-v1`, `cryptmind-attachment-v1`) and the `cryptmind_role` marker are unchanged, because existing shares and attachments carry them and the backend matches on `cryptmind_role` to keep preview thumbnails out of user-facing counts. Existing shares keep working and keep their original media type; only newly created ones use the new one.
- **Revoking a share deletes the encrypted copy** rather than only setting a flag, and a daily sweep clears shares that have passed their expiry. Without this a revoked share's ciphertext would sit in object storage indefinitely, still counted against the owner's storage. An expired share stops being readable immediately; the sweep is what reclaims the bytes.
- **Share links are built from the request host.** A self-hosted server serves the app from its own origin, so the link points wherever the request arrived, honouring `X-Forwarded-Host`/`-Proto` for installs behind a reverse proxy.

## [0.3.31] - 2026-08-31

### Security
- **Updated `h2` to 0.4.19** for RUSTSEC-2026-0258 — a remote peer could make the HTTP/2 stack buffer unbounded empty DATA frames.
- **Raised the admin console's `nanoid` override to 3.3.18** for GHSA-2v37-7h3g-55p8; the previous pin sat one patch below the fix. The app bundle already resolved a patched version.

### Changed
- **Compose binds Postgres and Garage ports to `127.0.0.1`** — Docker's published ports bypass ufw-style host firewalls, so the previous `5432:5432` / `9000:3900` mappings exposed the database and object store to the network on a stock host. Only `8090` is meant to be public. Documented in `docs/DEPLOYMENT.md`, including how to expose presigned URLs through a reverse proxy instead.
- **Compiled-in CORS default trimmed to local origins** — the binary's fallback `CORS_ALLOWED_ORIGINS` no longer includes hosted production domains; it now covers only localhost development and the desktop (tauri) origins. Deployments that need more set the env var, as the compose file already does. An empty list logs a startup warning.
- **Request ID per request** — every HTTP request now gets a UUID injected as a tracing span field and echoed back in the `X-Request-ID` response header, so all log lines for one request can be correlated.
- **Request/response logging at INFO** — `TraceLayer` now logs HTTP method, path, status code, and latency at `INFO` level (was `DEBUG`, so effectively silent in production). The compose default `RUST_LOG` drops from `backend=debug` to `backend=info` accordingly.
- **Database errors no longer log PII** — previously the full PostgreSQL error message was logged, which can include column values from constraint violations (e.g. `DETAIL: Key (username)=(alice) already exists`). Now only the PostgreSQL error code (e.g. `23505`) is logged.
- **JWT errors return a generic client message** — the raw `jsonwebtoken` error string (which can reveal algorithm details) is no longer sent to clients. Clients receive `"invalid or expired token"`; the error kind is logged server-side. This covers both the error type itself and the auth middleware, which was formatting the library error into its own message.
- **Usernames removed from log lines** — registration, login, and account-deletion events no longer log the username. Login and deletion log the opaque user UUID instead.
- **Structured error logging across all variants** — every `AppError` variant now emits a log event at the right severity, carrying an `error_kind` field for log-based filtering.

### Fixed
- **PNG and PDF exports came out empty (background and watermark only)** — `renderSvgToCanvas` serialized the live `<svg class="mm-canvas">` into a `data:` URI, but that element is sized purely by CSS and carries no `width`, `height` or `viewBox` attribute. In a standalone SVG image none of that CSS applies, so the image had no intrinsic size and rasterized at the SVG default of 300×150; a centred mind map sits outside that box and was cropped away entirely. The clone is now stamped with the measured viewport size (plus a matching `viewBox` when absent) before serialization.
- **Export filenames picked up the day of the month as a fake version** — a date fallback version label (`v 6. 8. 2026`) matched an unanchored `/v\s*(\d+)/i` and exported `MyMap` as `MyMap-v6.png`. The match is now anchored to the whole label, so only a genuine sequential label (`v12`) contributes a token. The mobile and page-level filename builders also gained the existing dedupe guard, so a vault titled `guide-v3` at version `v3` no longer exports as `guide-v3-v3`.
- **Markdown import turned flat lists into descending chains** — each sibling list item became a child of the previous one, because the list's base level was read from the top of the parse stack instead of the enclosing heading level.
- **`VITE_BACKEND_URL` in `.env` was ignored** — both `vite.config.ts` files read `process.env`, which Vite does not populate from `.env` files. They now use `loadEnv`, so the dev proxy can target a non-local backend without code changes.
- **Uploads over ~2 MB failed with a bare `413`.** `POST .../attachments/init` accepted the full declared size first, so the plan checks reported the file as allowed and only the upload that followed failed. Two body limits were stacked: axum's 2 MB default, which the upload routes never overrode, and a global 10 MiB `RequestBodyLimitLayer` that silently clipped anything set above it. The vault and attachment upload routes now carry an explicit limit derived from the largest per-plan attachment cap, and the global ceiling clears it.

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
