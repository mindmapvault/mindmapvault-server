# Refactoring plan: MindMapEditor

Written 2026-09-04, after the same split was done in `mindmapvault-live` and
proved out there. **Steps 1-6 are done**, step 6 with a deliberate exception
that is explained below. Step 7 is deliberately not being done; the reason is
at the bottom.

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
| 6 | Extract hooks: ~~viewport (pan/zoom/pinch)~~, selection, drag, ~~history~~ | Viewport and history are out, and so is the drag/marquee geometry; **the selection and drag state stays — see below** |
| 7 | ~~Split the CSS by area, keeping every `mm-*` name~~ | **Not doing it — see below** |

Steps 1 and 2 are worth doing even if nothing else happens.

## Why the selection and drag *state* stays

Viewport and history came out as `viewport.ts` + `useViewport.ts` and
`history.ts` + `useMindMapHistory.ts`, each split pure-logic-plus-hook so the
awkward parts are testable without rendering. Listing selection and drag on
the same row was the plan guessing they were the same shape of job. They are
not, and the difference is worth writing down.

Viewport and history each had a **pure core** that could come out and be
tested, and their state came along with it. Selection and drag have no such
core in their state. `selectedId` is referenced 93 times in the editor,
`multiSelect` 15, `rectSel` 14. A hook wrapping those `useState` calls leaves
all 122 call sites exactly as they are and moves three lines to another file.
The drag-end handler alone needs `layout`, `zoom`, `multiSelect`, `svgRef`,
`root`, `mutate` and `reparentNode`; a hook taking all seven is the same code
with a longer parameter list, still untestable without a DOM. That is motion,
not improvement.

What they *did* have is pure geometry buried in the pointer handlers, and that
is now `dragSelection.ts`: the drag threshold and drop radius, which were
inline magic numbers; `dragDelta`, which divides by zoom so a drag feels the
same however far you are zoomed in; `findDropTarget`; and `nodesInMarquee`,
which encodes a real decision nobody had written down — a node is caught when
its **centre** is inside the rectangle, not when it overlaps, which is what
makes sweeping a dense branch predictable.

Two longstanding quirks in `findDropTarget` are preserved exactly and now have
tests asserting them, so changing either has to be deliberate:

- It returns the **first** candidate within the radius in layout order, not
  the nearest, so two overlapping candidates resolve arbitrarily by tree order.
- It measures from the dragged node's **top-left** to the target's centre, not
  centre to centre, so the effective radius is offset by half the dragged
  node's size and a wide node drops differently from a narrow one.

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

## Open questions

Three things this work found and deliberately did not decide, because each one
moves pixels or data and the call is the user's. Written down here because
otherwise they exist only in the heads of whoever did the refactor.

~~**The footer strip nothing fills, for a field nothing sets.**~~ — resolved by
building the feature. `node.link` had been unset and undrawn since the initial
import, reserving 18px for nothing. It now links a node to another vault:
picked from a searchable dialog, drawn as the first footer strip, and clicked
to open that vault. The vault's title is stored on the node beside its id,
because titles are encrypted and a node cannot resolve one on its own.

~~**`duplicateNode` inserts into the tree it started with.**~~ — fixed, and it
was not just `duplicateNode`. Six async callbacks rebuilt the tree from a
closure captured before an upload: `duplicateNode`, `uploadFilesIntoNotes`,
`deleteNotesAttachment`, `attachNodeImage`, `onDropSvg` and
`attachFilesToSelectedNode`. Any edit made while a file was uploading was
discarded when the upload finished. All six now read `rootRef.current`, which
`mutate` sets in the same tick rather than waiting for a re-render.

**The two stylesheets have diverged.** See "Why step 7 is not being done". This
one blocks the other repo, not this one.

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
  operations~~ — done: 84 tests over the geometry, layout, tree operations,
  history, viewport and the drag/marquee geometry. The selection and drag
  *state* has none, because it stays in the component on purpose — see above.
- ~~The band arithmetic exists once~~ — done. It was in three places and one of
  them, the vault preview, was measurably wrong.
- **No component over ~300 lines** — not met, and the bar was drawn in the
  wrong place. See below.
- `-server` and `-saas` import the same `packages/mindmap-core` — half done.
  The package exists here and `-saas` is unchanged, because `packages/` is
  per-repo rather than a workspace. Copying it across is a separate job in
  that repo.

### On the 300-line bar

`MindMapEditor.tsx` went 3,794 → 3,337, which is not the number this bar was
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
