# Refactoring plan: MindMapEditor

Written 2026-09-04, after the same split was done in `mindmapvault-live` and
proved out there. **Steps 1-5 are done, and step 6 in part.** Step 7 is
deliberately not being done; both remainders are explained at the bottom.

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
| 6 | Extract hooks: ~~viewport (pan/zoom/pinch)~~, selection, drag, ~~history~~ | Viewport and history are out; **selection and drag are not — see below** |
| 7 | ~~Split the CSS by area, keeping every `mm-*` name~~ | **Not doing it — see below** |

Steps 1 and 2 are worth doing even if nothing else happens.

## Why half of step 6 is not being done

Viewport and history came out as `viewport.ts` + `useViewport.ts` and
`history.ts` + `useMindMapHistory.ts`, each split pure-logic-plus-hook so the
awkward parts are testable without rendering. Selection and drag did not, and
listing all four on one row was the plan guessing that they were the same
shape of job. They are not.

Viewport and history each own their state and are asked questions by the rest
of the editor. Selection and drag are the opposite: `selectedId`,
`multiSelect`, `rectSel` and `dragRef` are read and written across the pointer
handlers, the marquee, the keyboard shortcuts, the context menus, the toolbar
and the layout, and dragging commits through the same `mutate` as every tree
edit. Pulling them out is not the same move again — it is a bigger job than
the other two together, and it changes the editor's control flow rather than
relocating a slice of it.

Worth doing, with its own tests and its own commit. Not worth doing as the
tail of another step.

## Why step 7 is not being done

`MindMapEditor.css` is 3,409 lines here, and `-saas`'s copy of it has diverged:
roughly fifty ported rule blocks plus several fixes live there and not here.
Whoever reconciles the two is already facing a cross-repo diff of rule bodies.
Splitting this side into separate files first would put a file-boundary diff on
top of that, to buy structure that nothing is currently blocked on.

That is this plan's own warning about the CSS, applied to the end of the
sequence rather than the start: it is the safest change and therefore the one
that feels like progress. Doing six steps and explaining the seventh is better
than doing seven and taxing the port.

Do it *after* the two stylesheets are reconciled, not before.

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

- ~~`npm test` runs in `frontend_app` and covers layout, geometry and the tree
  operations~~ — done: 74 tests over the geometry, layout, tree operations,
  history and viewport. Selection and drag have none, because they are still
  in the component.
- ~~The band arithmetic exists once~~ — done. It was in three places and one of
  them, the vault preview, was measurably wrong.
- **No component over ~300 lines** — not met, and the bar was drawn in the
  wrong place. See below.
- `-server` and `-saas` import the same `packages/mindmap-core` — half done.
  The package exists here and `-saas` is unchanged, because `packages/` is
  per-repo rather than a workspace. Copying it across is a separate job in
  that repo.

### On the 300-line bar

`MindMapEditor.tsx` went 3,794 → 3,349, which is not the number this bar was
imagining. The bar was wrong in two ways.

It counted the wrong thing. What made the file hard to work on was not its
length, it was that the same arithmetic was written down three times and drifted
between the copies. That is fixed, and it would have been fixed by these changes
whether or not the line count moved.

And it assumed the leftovers would divide. What is left in `MindMapEditor.tsx`
is attachments, images, audio recording, notes, publishing, boards, exports,
Tauri paths, the command palette, the context menus and the toolbar — a dozen
features that share a selection and a tree and not much else. Splitting *those*
apart is a different project with a different justification, and doing it to hit
a line count would produce components that pass state through each other to no
one's benefit.

The parts that had a reason to come out have come out: geometry, layout, the
node's bands, the tree operations, history, viewport. `NodeBands.tsx` at 534
lines and `treeOps.ts` at 304 are over the bar and should stay as they are —
they are one coherent thing each, and cutting them to hit a number would be the
twenty-tiny-components mistake this plan already warns about in step 4.

## Reference

The same split, done and working: `mindmapvault-live`
`app/src/maps/geometry.ts`, `app/src/maps/canvas/`, and
`docs/refactoring-plan.md`. 175 tests passed before and after it, none edited.
