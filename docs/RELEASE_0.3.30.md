# Release 0.3.30 — Evidence Board, Voice Recording & PWA Offline Sync

**Date:** 2026-06-07

## Overview

This release introduces three major new capabilities: an encrypted Evidence Board canvas for free-form investigation work, in-browser voice recording that attaches encrypted audio clips to mind-map nodes, and a full PWA offline sync layer so the installed app works without a connection and syncs automatically on reconnect. A round of mobile UX improvements and attachment fixes is also included.

---

## Evidence Board

A new vault type — the **Evidence Board** — gives users a free-form, zoomable canvas for pinning cards, images, and connectors. Boards share the same end-to-end encrypted storage pipeline as mind maps (hybrid KEM + AES-256-GCM), so all content is encrypted before it leaves the browser.

### Creating a board

The **Create** button in the Vaults lobby now opens a dropdown with two options:

| Option | Result |
|--------|--------|
| New Mind Map | Creates a standard mind map vault (existing behaviour) |
| New Evidence Board | Creates an encrypted board vault, navigates to `/boards/:id` |

Board vaults are internally tagged with a `__board__` system label that is stripped from all visible label chips and table rows.

### Board editor

The board editor (`BoardEditor`) supports:

- **Text cards** — resizable sticky-note-style cards with a title, body, and six colour options (default, red, yellow, blue, green, purple)
- **Image cards** — pinned images with a label field and resize handles
- **Connectors** — lines drawn between any two cards to express relationships
- **Infinite canvas** — zoom and pan with the same gesture model as the mind-map editor (wheel, pinch, drag)
- **Auto-save** — board state is encrypted and uploaded on every meaningful edit, matching the mind-map save flow

### Routing

Vault cards and table rows automatically detect the `__board__` tag and link to `/boards/:id` instead of `/vaults/:id`. Existing mind-map links are unaffected.

---

## Voice Recording

Users can now record audio directly from the mobile canvas and attach it to any node as an encrypted voice note.

### Record button in the action bar

A **Record** button (microphone icon) has been added to the mobile bottom action bar between **Delete** and **Fit**. It is disabled when the root node is selected.

### Record Audio option in the attach-files sheet

The mobile file-upload bottom sheet now has a fourth option — **Record Audio** — alongside Camera, Photo Library, and Browse Files. Tapping it closes the file sheet and opens the recording UI.

### Recording bottom sheet

The recording UI has three states:

| State | What the user sees |
|-------|--------------------|
| **Idle** | Pulsing microphone button, "Tap to start recording" |
| **Recording** | MM:SS timer, pulsing red stop button |
| **Recorded** | Inline `<audio>` playback preview, filename input, **Save & Upload** / **Re-record** / **Discard** actions |

Pressing **Save & Upload** wraps the audio blob in a `File` object and routes it through the existing encrypted attachment pipeline — the recording is encrypted before upload with no plaintext ever reaching the server.

### Technical details

- Codec priority: `audio/webm;codecs=opus` → `audio/webm` → platform default
- Blob URL is managed via `useMemo` + `useEffect` cleanup (`URL.revokeObjectURL`) to prevent memory leaks
- Recording timer interval is cleared on component unmount

---

## PWA Offline Sync

The installed PWA now works without an internet connection. Changes made offline are persisted locally (encrypted) and synced to the server automatically when the connection is restored.

### How it works

The new **`OfflineStorageAdapter`** wraps the existing `ServerStorageAdapter` transparently. The editor never changes how it saves or loads — the adapter layer handles the online/offline split.

| Scenario | Behaviour |
|----------|-----------|
| Online read | Fetches from server, writes result to IndexedDB cache |
| Online write | Writes to server and updates the local cache simultaneously |
| Offline read | Serves encrypted data from IndexedDB |
| Offline write | Saves to IndexedDB, enqueues operation for later upload |
| Reconnect | Drains the sync queue automatically; retries up to 3 times per op |

### IndexedDB schema (`mmv-offline-v1`)

| Store | Contents |
|-------|---------|
| `vault_list` | Cached vault list |
| `vault_meta` | Per-vault `MindMapDetail` (KEM envelope + metadata) |
| `vault_blobs` | Encrypted tree blobs with `minio_version_id` for conflict detection |
| `sync_queue` — | Pending operations: `updateVault`, `uploadBlob`, `updateMeta` |

Blobs are written to `vault_blobs` **before** the sync queue entry is created, so the user's work is never lost even if the subsequent queue flush fails.

### Conflict detection

If another session saves to the server while the app is offline, the server's `minio_version_id` will differ from the locally stored `base_version_id`. The adapter detects this during the sync drain and surfaces a **conflict banner** instead of silently overwriting.

The user can choose:

- **My offline edits** — force-uploads the local version, overwriting the server
- **Server version** — discards pending local ops and keeps the server state

### Offline banner

A contextual status banner appears above the editor whenever the offline adapter is active:

| Banner state | Appearance |
|-------------|-----------|
| Offline with pending changes | Red dot · "Offline — N unsaved changes" |
| Syncing on reconnect | Spinner · "Syncing N changes…" |
| Conflict detected | Warning icon · "Server has newer changes" + resolution buttons |
| Sync failed after retries | Red dot · "Sync failed" + Retry button |
| Sync complete | Green dot · "All changes synced" (disappears after 3 s) |

### Activation

Offline mode activates automatically when the app is running as an **installed PWA** (`display-mode: standalone` or iOS `navigator.standalone`). No user action or settings toggle is required. Regular browser sessions continue to use the direct server adapter.

---

## Mobile canvas improvements

### Props toolbar restructured

The mobile props sheet now groups actions into two rows:

| Row | Buttons |
|-----|---------|
| Top | Notes · Labels · Files |
| Bottom | Date · Checkbox · Icons |

The **Files** button was not previously present on mobile.

### Labels as a sub-view

Tapping **Labels** in the props sheet now opens a sub-view *inside* the same bottom panel instead of launching a separate `position: fixed` overlay. The canvas remains visible above the panel throughout.

The sub-view has ← Back and × navigation, library items render one-per-row with full-width tap targets, and the **Add** / **Save to lib** buttons wrap to a second line when needed.

### File upload bottom sheet

The **Files** button opens a PWA-capable bottom sheet with four distinct options, each backed by a separately hidden `<input type="file">` with appropriate `accept` and `capture` attributes:

| Option | Behaviour |
|--------|-----------|
| Camera | Opens camera with `capture="environment"` |
| Photo Library | Image/video picker without capture |
| Browse Files | Generic file picker |
| Record Audio | Opens the voice recording sheet |

### Checkbox fix

The mobile checkbox cycle no longer includes a `null` / "Checkbox" intermediate label. The button now toggles directly between **Unchecked** and **Checked**.

---

## Attachment improvements

### Audio playback

Audio attachments (content-type `audio/*` or extensions `.webm`, `.m4a`, `.mp3`, `.ogg`, `.wav`, `.aac`, `.flac`, `.opus`) now open in the attachment preview modal with a centred `<audio controls>` player and a large microphone icon, instead of falling through to the system file handler.

Audio attachment cards in the notes dialog display a red microphone icon in the thumbnail area instead of the generic **FILE** label.

### Image thumbnail fix

Image attachments in the notes dialog were never loading their thumbnail preview. The root cause was a guard in `loadAttachmentPreview` that returned early when `preview_attachment_id` was absent — but images use their own `attachment_id` as the preview source and do not have a separate preview file. The guard now checks `isImage` first and only falls back to `preview_attachment_id` for non-image files.

### Attachment preview modal

The attachment preview modal was completely unstyled, causing it to render in document flow behind the notes modal. A full CSS block has been added:

- `position: fixed`, `z-index: 801`, flex-column layout
- Image and PDF body sections with proper scaling
- Audio player section
- Mobile override: full-screen (no border radius, 100 vw × 100 vh)
- Header with title and close button; footer with Download and Close actions

---

## Bug fixes

### Backend: S3 404 correctly propagated for board content

`MinioClient::get_object` in `minio.rs` previously mapped all AWS SDK errors to a generic `AppError::Storage`. The error mapper now checks the HTTP status code and AWS service error code; a `404` or `NoSuchKey` / `NotFound` / `NoSuchVersion` response is mapped to `AppError::NotFound`, which the API layer converts to a proper HTTP 404 response. This prevents a confusing 500 error when opening a board whose content blob has not yet been uploaded.

---

## Validation

- `pnpm exec tsc --noEmit` in `frontend_app` → clean
