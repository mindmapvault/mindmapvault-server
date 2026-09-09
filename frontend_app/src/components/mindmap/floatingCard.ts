/**
 * Dragging for the editor's floating cards — the Keyboard Shortcuts card and
 * the Labels dialog. Both are absolutely positioned inside the same
 * overflow-hidden parent and must stay within it, so the arithmetic is
 * identical; only the state it writes to differs.
 *
 * The geometry lives here as pure functions so it can be tested without a DOM,
 * with `beginCardDrag` left as the thin wiring over window listeners.
 */

export interface CardPos {
  x: number;
  y: number;
}

export interface CardDragState {
  /** Grab point inside the card. */
  offsetX: number;
  offsetY: number;
  /** Offset-parent origin, so viewport coords can be converted to the
   *  `left`/`top` the absolutely-positioned card actually needs. */
  originX: number;
  originY: number;
  /** Bounds that keep the card inside its parent. */
  maxX: number;
  maxY: number;
}

/** Gap kept between the card and every edge of its parent. */
export const CARD_DRAG_MARGIN = 8;

interface Rect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Capture where the card was grabbed and how far it may travel.
 *
 * The card is positioned against its offset parent rather than the viewport,
 * so the pointer's client coords have to be rebased onto that parent — without
 * it the card jumps by the parent's offset the moment it is grabbed.
 */
export function startCardDrag(
  card: Rect,
  parent: Rect | null,
  clientX: number,
  clientY: number,
  margin = CARD_DRAG_MARGIN,
): CardDragState {
  return {
    offsetX: clientX - card.left,
    offsetY: clientY - card.top,
    originX: parent?.left ?? 0,
    originY: parent?.top ?? 0,
    maxX: (parent?.width ?? 0) - card.width - margin,
    maxY: (parent?.height ?? 0) - card.height - margin,
  };
}

/**
 * Where the card should sit for a pointer at (clientX, clientY), clamped so it
 * cannot be dragged out of its parent.
 *
 * A card wider or taller than its parent gives `max < margin`; the clamp then
 * pins it at the margin rather than inverting, which would push it off the
 * opposite edge.
 */
export function clampCardPos(
  drag: CardDragState,
  clientX: number,
  clientY: number,
  margin = CARD_DRAG_MARGIN,
): CardPos {
  return {
    x: Math.max(margin, Math.min(clientX - drag.offsetX - drag.originX, Math.max(margin, drag.maxX))),
    y: Math.max(margin, Math.min(clientY - drag.offsetY - drag.originY, Math.max(margin, drag.maxY))),
  };
}

/**
 * Wire a header's mousedown up to a drag of the card it heads.
 *
 * `header` is the grabbed element; the card is taken to be its parent, matching
 * how both cards are built.
 */
export function beginCardDrag(
  header: HTMLElement,
  clientX: number,
  clientY: number,
  setPos: (pos: CardPos) => void,
): void {
  const card = header.parentElement as HTMLElement | null;
  if (!card) return;
  const parent = card.offsetParent as HTMLElement | null;
  const pRect = parent?.getBoundingClientRect();
  const drag = startCardDrag(
    card.getBoundingClientRect(),
    pRect
      ? { left: pRect.left, top: pRect.top, width: pRect.width, height: pRect.height }
      : { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight },
    clientX,
    clientY,
  );

  const onMove = (me: MouseEvent) => setPos(clampCardPos(drag, me.clientX, me.clientY));
  const onUp = () => {
    window.removeEventListener('mousemove', onMove);
    window.removeEventListener('mouseup', onUp);
  };
  window.addEventListener('mousemove', onMove);
  window.addEventListener('mouseup', onUp);
}
