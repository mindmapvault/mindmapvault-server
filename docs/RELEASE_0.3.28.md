# MindMapVault Server v0.3.28

This release adds broad interoperability with popular mind mapping tools and improves Obsidian mind map plugin compatibility for Markdown imports.

## Highlights

- **FreeMind / FreePlane import & export** — import `.mm` files from both FreeMind and FreePlane (auto-detected by format version). Export to either format from the editor menu.
- **WiseMapping import & export** — import and export `.wxml` files compatible with WiseMapping.
- **XMind import & export** — import `.xmind` files from XMind Zen (2020+, JSON format) and XMind 8/legacy (XML format). Export the active mind map as a `.xmind` archive readable by XMind.
- **Obsidian mind map plugin compatibility** — the Markdown importer now correctly handles the syntax used by the most popular Obsidian mind mapping plugins (Obsidian Mind Map, Markmap for Obsidian, Mindmap NextGen).
- **Unified import dropdown** — the Vaults page now has a single "Import" button with a format dropdown instead of separate per-format buttons.
- **WSL dev build script** — `scripts/dev-build.sh` automates container rebuilds in WSL dev environments.
- **Vault preview crash fix** — saving vaults with collapsed nodes no longer fails with a `TypeError` in `vaultPreview.ts`.

## What Changed

### New files

- `frontend_app/src/utils/freemindImport.ts`
  - Parses FreeMind (`.mm`, `version="1.0.1"`) and FreePlane (`.mm`, `version="freeplane 1.x"`) XML files.
  - Auto-detects format from the `<map version>` attribute.
  - Handles FreePlane nodes whose `TEXT` attribute contains raw HTML — tries `<richcontent TYPE="NODE">` first, falls back to `stripHtml()`.
  - Maps `COLOR`, `BACKGROUND_COLOR`, `FOLDED`, `POSITION`, `LINK`, and `<richcontent TYPE="NOTE">` to node fields.

- `frontend_app/src/utils/freemindExport.ts`
  - Exports the current mind map as a FreeMind-compatible `.mm` file (`<map version="1.0.1">`).

- `frontend_app/src/utils/freeplaneExport.ts`
  - Exports the current mind map as a FreePlane-compatible `.mm` file (`<map version="freeplane 1.9.0">`).
  - Generates `ID="ID_<timestamp><counter>"` attributes required by FreePlane.

- `frontend_app/src/utils/wisemappingImport.ts`
  - Parses WiseMapping `.wxml` XML files.
  - Resolves root via `topic[central="true"]` or the first `<topic>` child.
  - Maps `text`, `bgColor`, `position`, `<note>` (CDATA), `<link url>`, child `order`.

- `frontend_app/src/utils/wisemappingExport.ts`
  - Exports the current mind map as a WiseMapping-compatible `.wxml` file.
  - Uses `<note><![CDATA[...]]></note>` and `<link url="..."/>`.

- `frontend_app/src/utils/xmindImport.ts`
  - Reads a `.xmind` file (ZIP archive) passed in as `ArrayBuffer`.
  - Tries `content.json` first (XMind Zen / 2020+ format), falls back to `content.xml` (XMind 8 / legacy).
  - JSON format: maps `title`, `notes.plain.content`, `href`, `style.properties['background-color']`, and nested `children.attached`.
  - XML format: maps `<title>`, `<notes>/<plain>/<content>`, `xlink:href`, and `<children>/<topics type="attached">`.
  - Uses `fflate` (`unzipSync`) for ZIP decompression.

- `frontend_app/src/utils/xmindExport.ts`
  - Exports the current mind map as a `.xmind` ZIP archive in XMind Zen JSON format.
  - Produces `content.json` (mind map data) and `META-INF/manifest.xml` inside the archive.
  - Uses `fflate` (`zipSync`) for ZIP creation. Returns a `Blob` directly.

- `scripts/dev-build.sh`
  - WSL build script: builds the frontend, hot-swaps the server container via `docker compose`, creates a `mmvdev` Postgres user/database on first run.
  - Supports `--down` (bring stack down) and `--no-cache` (full rebuild) flags.

### Modified files

- `frontend_app/src/utils/markdownImport.ts`
  - **Obsidian mind map plugin compatibility** (Obsidian Mind Map / Markmap / Mindmap NextGen):
    - Task-list items `- [ ] text` and `- [x] text` now set the node's `checked` field.
    - Obsidian highlight syntax `==text==` is now stripped to plain text.
    - Images `![alt](url)` are replaced by their alt text.
    - Obsidian tags (`#tag`) are stripped from node labels.
    - HTML comment lines (`<!-- markmap: {...} -->` and similar) are skipped.
    - Obsidian callouts `> [!type] Title` have the `[!type]` marker stripped; the title is still appended to notes.
    - Tab indentation in nested lists is treated as 4 spaces (matches VSCode and Obsidian defaults).

- `frontend_app/src/components/MindMapEditor.types.ts`
  - Added optional `onExportFreemind`, `onExportFreeplane`, `onExportWisemapping`, `onExportXmind` props.

- `frontend_app/src/components/MindMapEditor.tsx`
  - Export context menu now includes FreeMind (.mm), FreePlane (.mm), WiseMapping (.wxml), and XMind (.xmind) entries when the corresponding handlers are provided.

- `frontend_app/src/pages/EditorPage.tsx`
  - Wires up `handleExportFreemind`, `handleExportFreeplane`, `handleExportWisemapping` and passes them to the editor.

- `frontend_app/src/pages/VaultsPage.tsx`
  - Replaces the separate import buttons with a single "Import" dropdown button.
  - Dropdown options: Markdown (.md), FreeMind (.mm), FreePlane (.mm), WiseMapping (.wxml), XMind (.xmind).
  - FreeMind and FreePlane share one file picker — parser auto-detects the format.
  - XMind uses a dedicated file picker (reads file as `ArrayBuffer` for ZIP parsing).
  - Click-outside handler closes the dropdown.

- `frontend_app/src/utils/vaultPreview.ts`
  - Fixed `TypeError: Cannot read properties of undefined (reading 'x')` crash in `walkConnectors`.
  - Root cause: nodes with `FOLDED="true"` are excluded from `layoutTree`'s output, but `walkConnectors` iterated their children anyway, causing `layout[child.id]` to be `undefined`.
  - Fix: added `if (!parentLayout) return` and `if (!childLayout) continue` guards. Saves for vaults with collapsed nodes now succeed (green diskette).

## Validation

- `pnpm --dir frontend_app install && pnpm --dir frontend_app build` → passes (installs `fflate`, then TypeScript + Vite).
- FreePlane files with `FOLDED="true"` nodes save without console errors.
- Markdown files exported from Obsidian with task lists, highlights, and callouts import cleanly.
- `.xmind` files from XMind Zen 24.x (JSON) and XMind 8 (XML) import correctly.

## Compatibility Notes

- FreeMind and FreePlane share the `.mm` extension; format is detected automatically from `<map version>`.
- WiseMapping exports use `.wxml` to avoid ambiguity with `.mm`.
- XMind export targets the Zen / 2020+ JSON format; the generated file opens in XMind 2020 and later.
- XMind import supports both the Zen JSON format and the legacy XML format (`content.xml`).
- **New dependency**: `fflate ^0.8.2` — run `pnpm --dir frontend_app install` before building.
- The `markdownImport.ts` changes are backwards-compatible — plain Markdown files without Obsidian-specific syntax are unaffected.
- No server-side or encryption changes in this release.
