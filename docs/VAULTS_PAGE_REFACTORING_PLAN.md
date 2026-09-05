# Refactoring plan: VaultsPage

Written 2026-09-05, after the same discipline worked on `MindMapEditor`. See
`MINDMAP_EDITOR_REFACTORING_PLAN.md` — this follows its order and its warnings.

## What we are dealing with

| File | Lines |
|---|---|
| `pages/VaultsPage.tsx` | 2,259 |
| `pages/EditorPage.tsx` | 1,272 |

2,259 lines is not itself the problem. Three specific things are.

**The same import written four times.** `handleImportMarkdown`,
`handleImportFreemind`, `handleImportWisemapping` and `handleImportXmind` are
~38 lines each and differ in **nine meaningful lines**: which parser to call,
which extension to strip, and which of three state variables to set. Each
carries its own `…Importing` / `…ImportError` / `…ImportRef` triple — 49
references across twelve state variables doing one job. `EditorPage` holds the
matching five export handlers with the same shape. Adding a sixth format today
means touching nine places and getting all of them right.

**The same vault derived twice.** `VaultCard` (342 lines) and `VaultTableRow`
(236) are the grid and table views of one vault. They share **18 identical
props** and each recomputes, verbatim:

```ts
const persistedSharingMode = normalizeSharingMode(map.vault_sharing_mode);
const isBoard = map.vault_labels?.includes(BOARD_LABEL) ?? false;
const vaultPath = isBoard ? `/boards/${map.id}` : `/vaults/${map.id}`;
const isSharedVault = activeShareCount > 0 || persistedSharingMode === 'shared';
```

This is the editor's band arithmetic in a new place: one derivation, two
copies, nothing keeping them in step. Their `memo` comparators then repeat the
`sameRenameContext` rule — subtle enough to be worth writing once, since it
decides whether a vault re-renders on every keystroke while being renamed.

**One 263-line function.** `loadMaps` fetches, decrypts every title and note,
normalises colours, labels, sharing and encryption modes, and builds the draft
state each card edits from.

## The blocker, and why it differs from the editor

`frontend_app` now has tests, but only over `mindmap-core` and
`components/mindmap` — 87 of them. `VaultsPage` has none, and **there is no
DOM testing library in this repo**. Adding one mid-refactor to test components
written the same day is how a refactor becomes a rewrite.

So the rule that worked for step 6 of the editor applies here from the start:
**extract the pure logic and test that; leave the components alone.** Every
item above has a pure core — the parse-and-name step of an import, the
derivation of a vault's state, the rename-context rule — and the pure core is
where the bugs would hide.

## Order

Each step is a commit that leaves the app working.

| # | Step | Why here |
|---|---|---|
| 1 | ~~Extract the vault helpers and derivations into a tested module~~ | Done |
| 2 | ~~Collapse the four import handlers onto one helper~~ | Done |
| 3 | ~~Do the same for `EditorPage`'s five export handlers~~ | Done — and it was bigger than this row: the tree snapshot was written seven times, and the filename builder twice |
| 4 | ~~Share the `memo` comparator's rename rule~~ | Done as part of step 1 |
| 5 | ~~Break up `loadMaps`~~ | Done: 263 lines to 60 |
| 6 | ~~Move `VaultCard` and `VaultTableRow` into their own files~~ | Done |

**All six are done.** `VaultsPage.tsx` is 2,259 → 1,408 and `EditorPage.tsx`
1,272 → 1,234, with 44 new tests. What is left in `VaultsPage` is the page
itself: its state, its handlers and its layout.

Steps 1 and 2 are worth doing even if nothing else happens.

## What not to do

- **Do not merge `VaultCard` and `VaultTableRow` into one component.** They
  share their data and their behaviour, not their markup: a card is an
  `<article>` with a preview panel, a row is a `<tr>` with a portal menu.
  Merging them produces one component with a `variant` prop and two disjoint
  render paths, which is worse than two honest ones. Share the derivation, not
  the JSX.
- **Do not add `@testing-library/react` as part of this.** If the components
  should be tested, that is its own decision with its own commit — not a
  dependency added to justify a refactor.
- **Do not rename the reserved labels or the storage keys.** `__board__`,
  `__imported__` and `vault-color-<id>` are persisted in vault records and in
  `localStorage`; changing them silently reclassifies existing vaults.
- **Do not start with step 6.** Moving components between files is the change
  that feels like progress while the duplication is untouched.

## Open questions this surfaced

~~**An export can be named nothing.**~~ — fixed. `buildExportFileBaseName` takes the first
truthy of `baseTitle`, `title`, `fallback` and *then* trims — so a title that is
only whitespace wins over the fallback and trims to `''`, and the download is
called `.md`. It now trims before choosing, so a blank title falls
through to the next candidate.

~~**One corrupt label list fails the whole vault list.**~~ — fixed. Local mode keeps a
vault's labels in `localStorage`, read with an unguarded `JSON.parse` inside
the row builder. A single malformed entry throws, and because the read happens
while mapping every vault, the entire list fails to load rather than that one
row. `readLocalVaultLabels` now returns an empty list
for an unparseable or non-array value, costing that vault its labels rather
than the page.

## Done when

- ~~The four import paths are one, and adding a format is one entry~~ — done,
  and the five export paths with them.
- ~~A vault's state is derived in one place, with tests~~ — done.
- ~~`npm test` covers the vault helpers as well as the mindmap ones~~ — done:
  131 tests, 44 of them new here.
- ~~No behaviour has changed~~ — held. Two defects were found and deliberately
  left alone, both recorded above with tests pinning what they do today.
