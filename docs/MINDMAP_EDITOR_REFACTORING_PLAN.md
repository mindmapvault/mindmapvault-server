# Refactoring plan: MindMapEditor

Written 2026-09-04, after the same split was done in `mindmapvault-live` and
proved out there. Not started — this file is the proposal.

## The blocker, first

**`frontend_app` has no tests.** No `test` script, no vitest or playwright
config, no `*.test.*` file anywhere under `src/`. Refactoring 3,794 lines with
no safety net is not a refactor; it is a rewrite that happens to reuse the old
code as a reference.

So the first commit is not a refactor at all. It is a test harness and a set of
characterisation tests over the pure logic, which is the part that can be
tested without a browser and the part everything else depends on.

Note that `mindmapvault-saas` *does* have vitest (`"test": "vitest run"`, with
tests under `src/crypto/__tests__` and `src/components/notes/__tests__`), so
the tooling choice is already made in the family — use the same.

## What we are dealing with

| File | Lines |
|---|---|
| `components/MindMapEditor.tsx` | 3,794 |
| `components/MindMapEditor.css` | 3,409 |
| `components/MindMapLayout.ts` | 200 |

`MindMapLayout.ts` is byte-identical to the one in `mindmapvault-saas`, and the
two editors are 3,794 and 3,693 lines of nearly the same thing. **Refactoring
one and not the other doubles the divergence that already exists.** Whatever is
extracted should go into `packages/` — the `@mindmapvault/connectors` pattern
(tsconfig `paths` plus a vite alias) is already established in both repos and
is the obvious home.

## The bug worth fixing first

The node's band arithmetic is computed twice from the same fields:

- in `measureNodeSize` (`MindMapLayout.ts`), to decide how big a node is
- inline in the render function (`MindMapEditor.tsx` around line 2245), to
  decide where inside it each part goes — `leftPad`, `topMetaH`, `topTagH`,
  `bodyTopY`, `bodyH`, `lineStartY`

They have to agree exactly, or text drifts out of the box it was measured for.
Every new node decoration has to be added to both, correctly, by hand.

In Live this became `geometry.ts`: `describeNode(node)` returns the parts and
band heights, `measureNodeSize` adds them up, and `nodeGeometry(box, parts)`
turns the laid-out box into positions. Both sides then use the same numbers by
construction. It took an afternoon and is the single highest-value change here.

## Order

Each step is a commit that leaves the app working.

| # | Step | Why here |
|---|---|---|
| 1 | Add vitest to `frontend_app`; characterise `layoutTree` and `measureNodeSize` | Nothing else is safe without it |
| 2 | Extract `geometry.ts`, used by both the layout and the renderer | Fixes the duplication above; covered by step 1's tests |
| 3 | Move `geometry.ts` + `MindMapLayout.ts` into `packages/mindmap-core` | Stops `-server` and `-saas` drifting further apart |
| 4 | Split the node renderer by **band** — date badge, meta strip, tags, body, footer, controls | The node has a vertical structure the geometry already knows; splitting by field instead produces twenty tiny components that all recompute offsets |
| 5 | Extract the pure tree operations (`addChild`, `deleteNode`, `moveNode`, `reparentNode`, `duplicateNode`, the bulk ones) into a tested module | They are the operations that can lose a subtree; they need tests more than anything else here |
| 6 | Extract hooks: viewport (pan/zoom/pinch), selection, drag, history | Removes most of the remaining state from the component |
| 7 | Split the CSS by area, keeping every `mm-*` name | Lowest risk, do it whenever |

Steps 1 and 2 are worth doing even if nothing else happens.

## What not to do

- **Do not port Live's editor back.** Live carries a fraction of what this does
  — no attachments, versions, publishing, boards, notes uploads, Tauri paths —
  and it drives a Yjs document rather than React state. Only the *pure* parts
  transfer: geometry, layout, tree operations.
- **Do not rename `mm-*` classes.** They are the seam between the three apps.
- **Do not remove `@xyflow/react` as part of this.** It is dead — only the
  orphaned `MindMapNode.tsx` imports it, and nothing imports that — so it
  should go, but as its own commit with its own justification.
- **Do not start with the CSS.** It is the safest change and therefore the one
  that feels like progress while the actual risk is untouched.

## Done when

- `npm test` runs in `frontend_app` and covers layout, geometry and the tree
  operations
- The band arithmetic exists once
- No component over ~300 lines
- `-server` and `-saas` import the same `packages/mindmap-core`, so the next
  fix lands in both

## Reference

The same split, done and working: `mindmapvault-live`
`app/src/maps/geometry.ts`, `app/src/maps/canvas/`, and
`docs/refactoring-plan.md`. 175 tests passed before and after it, none edited.
