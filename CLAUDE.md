# Working in this repo

Rules earned from real bugs in this codebase. Each one names the incident, so
it can be argued with rather than obeyed.

## Derive once

**If two places compute the same thing from the same fields, they will drift.**
Not "might" — every instance of this we have found had already drifted.

- The node's band arithmetic lived in `measureNodeSize`, again inline in the
  editor's renderer, and a third time in `utils/vaultPreview.ts`. All three
  disagreed. The renderer treated whitespace-only notes as a note and counted
  attachments the layout could not see, so both stole 18px from the text body;
  the preview had the tag strip at 16px instead of 18 and ignored the meta
  strip entirely, so thumbnails put labels where the editor did not.
- `buildExportFileBaseName` existed in `EditorPage` and in `MindMapEditor`,
  identical except that one fell back to `vault` and the other to `mindmap`.
- `VaultCard` and `VaultTableRow` each recomputed a vault's route and sharing
  state from the same record.
- The tree snapshot with its `view_state` was written out **seven times**.

Put the derivation in one function, give it a test, and have both sides call
it. `packages/mindmap-core/src/geometry.ts` and
`pages/vaults/vaultState.ts` are the pattern.

## A table, not N branches

Four import handlers differed in nine lines out of thirty-eight: the parser,
the extension, and which of three state variables to set. Five export handlers
differed in the serializer and the extension. Adding a format meant touching
nine places.

When you are about to copy a handler and change two things in it, write the
two things down as data instead — `pages/vaults/importFormats.ts`,
`utils/exportFormats.ts`. One entry per format, one code path.

Watch for the list that is *not* one-to-one: the import menu has five entries
for four formats, because FreeMind and Freeplane are separate names for the
same `.mm` reader. Model the menu separately rather than losing an entry.

## Keep the pure core out of React

`frontend_app` has no DOM testing library, and adding one to justify testing
code you wrote the same day turns a refactor into a rewrite. So:

**Extract the arithmetic, test that, leave the component alone.** Viewport and
history split cleanly into `viewport.ts` + `useViewport.ts` because they had a
pure core. Selection and drag did not — their state is read from ninety-odd
places and a hook around it would move three lines — but the *geometry* buried
in their pointer handlers did, and that became `dragSelection.ts`.

A hook that takes seven parameters is the same code with a longer signature.
If extracting it does not make something testable, it is motion, not progress.

## Read state after an `await` through a ref

Six callbacks rebuilt the tree from a closure captured before a file upload, so
anything typed while the upload was in flight was discarded when it finished.
Uploads take seconds; this was reachable in ordinary use.

Any callback that mutates shared state *after* an `await` must read that state
as of now — `rootRef.current`, not `root`. `mutate` sets the ref in the same
tick rather than waiting for a re-render.

## Do not measure what nothing draws

`node.link` was counted as a footer strip by the layout and drawn by nothing,
reserving 18px on every node that had one. It had never been set since the
initial import.

If a field affects layout, something must render it. If nothing renders it,
it should not affect layout. (That field is now a real feature — a link to
another vault — which is the other way to resolve it.)

## Persisted strings are data

Renaming these silently reclassifies or orphans existing records:

- `localStorage` keys: `vault-color-<id>`, `vault-labels-<id>`,
  `mindmapvault-theme`
- reserved vault labels: `__board__`, `__imported__`
- wire names pinned by `#[serde(rename)]` in the backend, and the
  `minio_object_key` / `minio_version_id` JSON field names
- the `mm-*` CSS class names, which are the seam between the three apps

Keep them in one place with a comment saying they are persisted:
`utils/vaultLabels.ts`, `vaultState.ts`.

## Split by structure, not by line count

A 3,400-line file is not automatically a problem, and a 300-line target is not
automatically an improvement. What made `MindMapEditor` hard to work on was the
duplicated arithmetic, not its length.

- Split the node renderer by **band** — date badge, meta, tags, image, body,
  footer, controls — because the geometry already knows that structure.
  Splitting by field would have produced twenty components each recomputing
  where its band starts.
- `NodeBands.tsx` is 534 lines and `treeOps.ts` is 304. Both are one coherent
  thing. Cutting them to hit a number would be the mistake above.
- Do not merge two components that share data and behaviour but not markup.
  `VaultCard` and `VaultTableRow` share `vaultState`, not a `variant` prop.

## Tests

`pnpm test` in `frontend_app` runs vitest over `src/` and `packages/*/src/`.
Backend tests are `cargo test`.

- Before changing behaviour-carrying arithmetic, characterise it: assert what
  it does today so the change can be shown to move only what it meant to.
- Stub `measureText`; real font metrics make every expected number a property
  of the machine running the tests.
- Test the operations that can lose data hardest. `reparentNode` refusing to
  drop a node into its own subtree is the difference between a move and a
  silent delete.
- A test that pins a known defect is fine — say so in the test, and record the
  defect in the relevant plan under `docs/`.

## When you find a defect mid-refactor

Write it down, do not fix it in passing. A refactor whose diff also changes
behaviour cannot be reviewed as either. `docs/*_REFACTORING_PLAN.md` has an
"Open questions" section for this; two live ones are an export that can be
named `.md` with no stem, and a corrupt local label list failing the whole
vault list.

## Commits

Short, in the style of the surrounding history — usually one `type: summary`
line. A body only when the reason is not obvious from the diff. No AI
attribution or `Co-Authored-By` trailers.
