# Release 0.3.29 — Mobile Canvas & UX Polish

**Date:** 2026-06-06

## Overview

This release delivers a complete mobile canvas experience, responsive toolbar improvements, and several UX fixes for the editor. The app is now fully operable on touchscreen devices with native gesture support, a purpose-built bottom action bar, and mobile-optimised dialogs throughout.

---

## Mobile canvas experience

The editor now detects touch viewports (≤ 768 px) and switches into a mobile-specific chrome while keeping the same SVG canvas renderer.

### Mobile top bar
Replaces the desktop toolbar on mobile:
- Vault name and version label (left, truncated)
- Save-status button — coloured when dirty, error state, or saving
- Theme toggle (sun / moon)

### Mobile bottom action bar
Five-button dock fixed to the bottom of the screen:

| Button | Action |
|--------|--------|
| Back | Returns to the Vaults lobby |
| Add | Adds a child node to the selected node |
| Delete | First tap shows Confirm / Cancel — no accidental deletions |
| Fit | Zoom-to-fit all nodes in the viewport |
| Props | Opens the node properties bottom sheet |

### Mobile props sheet
Bottom sheet with all node-level options:
- **Color** — 9 colour swatches (default + 8 presets)
- **Notes / Date / Labels** — open their respective dialogs (each renders as a mobile bottom sheet)
- **Progress** — pill buttons: ✕ (clear), 0%, 25%, 50%, 75%, 100%
- **Checkbox** — toggle; label reflects current state (Checkbox / Unchecked / Checked)
- **Icons** — opens the icon picker as a full-width bottom sheet (max 72 vh)

### Mobile dialogs
- **Labels dialog** — rendered as `position: fixed` bottom sheet with rounded top corners, larger touch targets for chips, input, and buttons
- **Icon picker** — CSS override renders it as a bottom sheet when accessed from mobile

---

## Pinch-to-zoom

Two-finger pinch on the canvas scales zoom proportionally (0.2 × – 4 ×). The distance ratio between `touchstart` and `touchmove` is used to derive the next zoom level. Single-finger pan is unaffected.

---

## Responsive desktop toolbar

The desktop toolbar is now a single flex row that auto-wraps when the window is too narrow:
- Vault name and version are always visible in a left nav section with a right border separator
- Action buttons occupy the remaining width; they wrap to a second row when space runs out
- The second-row top separator is rendered via `box-shadow: inset 0 1px 0` so it only appears when the row has actually wrapped

---

## Vaults lobby improvements

- **Search** — filters vault list by name, note, and labels in real time; shows result count badge
- **Table / list view** — compact table layout toggled alongside the existing card view; each row shows thumbnail, name, labels, note excerpt, date, node count, version count, and inline actions
- **Clickable vault preview** — the preview image in card view now navigates directly into the vault

---

## Bug fixes

### Passive event listener violation
`e.preventDefault()` inside a React synthetic `onWheel` handler was silently ignored and logged repeated browser warnings. The `onWheel` prop is removed; a `useEffect` attaches a native `wheel` listener with `{ passive: false }` instead.

### Attached-file double-counting
`attachment_count` and `attachment_bytes` counted each attachment twice because auto-generated preview thumbnails (`cryptmind_role: "preview"`) were included in the loop. Both `mindmaps_sql.rs` and `auth_sql.rs` now filter them out.

### Vault card render cascade
Editing any single vault's metadata triggered a full re-render of all vault cards. Fixed with a stable `mapMetaKey` derivation and a latest-ref pattern on the maps array.

---

## Validation

- `pnpm exec tsc --noEmit` in `frontend_app` → clean
- `cargo check` in `backend` → clean
