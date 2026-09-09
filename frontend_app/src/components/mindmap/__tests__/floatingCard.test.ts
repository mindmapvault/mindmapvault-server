import { describe, it, expect } from 'vitest';
import { startCardDrag, clampCardPos, CARD_DRAG_MARGIN } from '../floatingCard';

const parent = { left: 100, top: 50, width: 1000, height: 600 };
/** Grabbed 20px in from the card's left edge and 10px down from its top. */
const card = { left: 300, top: 200, width: 340, height: 420 };

function grab(clientX = 320, clientY = 210) {
  return startCardDrag(card, parent, clientX, clientY);
}

describe('startCardDrag', () => {
  it('records the grab point relative to the card, not the page', () => {
    const d = grab();
    expect(d.offsetX).toBe(20);
    expect(d.offsetY).toBe(10);
  });

  it('rebases onto the offset parent so the card does not jump when grabbed', () => {
    const d = grab();
    expect(d.originX).toBe(100);
    expect(d.originY).toBe(50);
    // Dropping the pointer exactly where it was grabbed leaves the card where
    // it already sits, in parent coordinates.
    expect(clampCardPos(d, 320, 210)).toEqual({ x: 200, y: 150 });
  });

  it('bounds travel by the parent, less the card and the margin', () => {
    const d = grab();
    expect(d.maxX).toBe(1000 - 340 - CARD_DRAG_MARGIN);
    expect(d.maxY).toBe(600 - 420 - CARD_DRAG_MARGIN);
  });

  it('falls back to the viewport origin when there is no offset parent', () => {
    const d = startCardDrag(card, null, 320, 210);
    expect(d.originX).toBe(0);
    expect(d.originY).toBe(0);
  });
});

describe('clampCardPos', () => {
  it('follows the pointer between the bounds', () => {
    const d = grab();
    // 500 - 20 grab - 100 origin = 380; 200 - 10 - 50 = 140, both within range.
    expect(clampCardPos(d, 500, 200)).toEqual({ x: 380, y: 140 });
  });

  it('clamps a tall card to maxY well before the pointer runs out of room', () => {
    const d = grab();
    // The card is 420 of the parent's 600, so it stops at 172 even though the
    // pointer asks for 340.
    expect(clampCardPos(d, 500, 400)).toEqual({ x: 380, y: d.maxY });
  });

  it('holds the card at the margin against the top left', () => {
    const d = grab();
    expect(clampCardPos(d, -900, -900)).toEqual({ x: CARD_DRAG_MARGIN, y: CARD_DRAG_MARGIN });
  });

  it('holds the card at its maximum against the bottom right', () => {
    const d = grab();
    expect(clampCardPos(d, 99999, 99999)).toEqual({ x: d.maxX, y: d.maxY });
  });

  it('pins a card larger than its parent at the margin rather than inverting', () => {
    // maxX/maxY go negative here; without the guard the clamp would return a
    // negative position and push the card off the opposite edge.
    const d = startCardDrag({ left: 0, top: 0, width: 2000, height: 2000 }, parent, 10, 10);
    expect(d.maxX).toBeLessThan(0);
    expect(clampCardPos(d, 500, 500)).toEqual({ x: CARD_DRAG_MARGIN, y: CARD_DRAG_MARGIN });
  });
});
