import { beforeAll, describe, expect, it } from 'vitest';
import type { MindMapTreeNode } from '../../types';

/**
 * Characterisation tests for the layout engine.
 *
 * These do not assert that the numbers are *right* — they assert that they are
 * what they are today, so the geometry extraction that follows can be shown to
 * change nothing. Read a failure here as "the layout moved", not "the layout is
 * wrong".
 *
 * `measureText` asks a canvas for a width. Real font metrics differ between
 * machines, which would make every expected number a property of the runner, so
 * a stub gives every character a width of 7. The arithmetic around the text is
 * what these tests are for.
 */

const CHAR_W = 7;

type Layout = typeof import('../MindMapLayout');
let layout: Layout;

beforeAll(async () => {
  (globalThis as unknown as { document: unknown }).document = {
    createElement: () => ({
      getContext: () => ({
        font: '',
        measureText: (t: string) => ({ width: (t ?? '').length * CHAR_W }),
      }),
    }),
  };
  layout = await import('../MindMapLayout');
});

const node = (over: Partial<MindMapTreeNode> = {}): MindMapTreeNode =>
  ({ id: 'n', text: 'hello', children: [], ...over }) as MindMapTreeNode;

describe('measureNodeSize', () => {
  const measure = (...args: Parameters<Layout['measureNodeSize']>) =>
    layout.measureNodeSize(...args);

  it('gives a plain one-line node the minimum height', () => {
    const { w, h, lines } = measure('hello', null, 0, false, 0, false);
    // 'hello' is 5 chars = 35px, + 18px padding each side = 71, under MIN_W 80
    expect(w).toBe(80);
    expect(h).toBe(36); // NODE_MIN_H wins over 1 * 20 + 2 * 8
    expect(lines).toEqual(['hello']);
  });

  it('grows by a line height once the text needs more than two lines', () => {
    const three = measure('a\nb\nc', null, 0, false, 0, false);
    expect(three.lines).toHaveLength(3);
    expect(three.h).toBe(3 * 20 + 8 * 2); // 76, past the 36 minimum
  });

  it('widens for icons, the checkbox and the progress pie', () => {
    const plain = measure('wide enough to matter', null, 0, false, 0, false);
    const decorated = measure('wide enough to matter', null, 2, true, 0, true);
    // 2 icons (16+4 each, +2) + checkbox (16+6) + pie (32+6) = 102
    expect(decorated.w - plain.w).toBe(102);
    expect(decorated.h).toBe(plain.h);
  });

  it('adds one strip for notes or attachments, however many there are', () => {
    const base = measure('x', null, 0, false, 0, false).h;
    expect(measure('x', null, 0, false, 0, false, true).h).toBe(base + 18);
    expect(measure('x', null, 0, false, 0, false, false, 3).h).toBe(base + 18);
    expect(measure('x', null, 0, false, 0, false, true, 3).h).toBe(base + 18);
  });

  it('adds a strip per footer link, and one for tags', () => {
    const base = measure('x', null, 0, false, 0, false).h;
    expect(measure('x', 'lnk', 0, false, 0, false).h).toBe(base + 18);
    expect(measure('x', null, 0, false, 2, false).h).toBe(base + 36);
    expect(measure('x', 'lnk', 0, false, 2, false).h).toBe(base + 54);
    expect(measure('x', null, 0, false, 0, false, false, 0, 4).h).toBe(base + 18);
  });

  it('takes the image into account on both axes', () => {
    const base = measure('x', null, 0, false, 0, false);
    const withImage = measure('x', null, 0, false, 0, false, false, 0, 0, 200, 120);
    expect(withImage.h).toBe(base.h + 120 + 6); // image + NODE_IMAGE_PAD
    expect(withImage.w).toBe(200 + 18 * 2);     // image + NODE_PAD_X either side
  });

  it('ignores attachment markdown lines when measuring text', () => {
    const withMarkdown = measure('visible\n[Attachment: notes.pdf](attachment://abc)', null, 0, false, 0, false);
    expect(withMarkdown.lines).toEqual(['visible']);
  });
});

describe('layoutTree', () => {
  it('places the root at the origin it was given', () => {
    const pos = layout.layoutTree(node({ id: 'root' }), 100, 50);
    expect(pos.root.x).toBe(100);
    expect(pos.root.y).toBe(50 - pos.root.h / 2);
    expect(pos.root.direction).toBe('right');
  });

  it('puts children to the right, one node width plus the gap away', () => {
    const root = node({ id: 'root', children: [node({ id: 'a' })] });
    const pos = layout.layoutTree(root, 0, 0);
    expect(pos.a.x).toBe(pos.root.x + pos.root.w + 40); // H_GAP
    expect(pos.a.direction).toBe('right');
  });

  it('sends nodes marked side:left to the other side', () => {
    const root = node({
      id: 'root',
      children: [node({ id: 'r' }), node({ id: 'l', side: 'left' } as Partial<MindMapTreeNode>)],
    });
    const pos = layout.layoutTree(root, 0, 0);
    expect(pos.l.direction).toBe('left');
    expect(pos.l.x).toBe(pos.root.x - pos.l.w - 40);
    expect(pos.r.x).toBeGreaterThan(pos.root.x);
  });

  it('stacks siblings with the vertical gap between them', () => {
    const root = node({ id: 'root', children: [node({ id: 'a' }), node({ id: 'b' })] });
    const pos = layout.layoutTree(root, 0, 0);
    expect(pos.b.y - (pos.a.y + pos.a.h)).toBe(8); // V_GAP
  });

  it('lays out nothing under a collapsed node', () => {
    const root = node({ id: 'root', collapsed: true, children: [node({ id: 'hidden' })] });
    const pos = layout.layoutTree(root, 0, 0);
    expect(pos.hidden).toBeUndefined();
  });

  it('honours a dragged node\'s own position', () => {
    const root = node({
      id: 'root',
      children: [node({ id: 'a', customX: 500, customY: 250 } as Partial<MindMapTreeNode>)],
    });
    const pos = layout.layoutTree(root, 0, 0);
    expect(pos.a.x).toBe(500);
    expect(pos.a.y).toBe(250);
  });

  it('reserves room above a node carrying a date badge', () => {
    const plain = layout.layoutTree(node({ id: 'root' }), 0, 0);
    const dated = layout.layoutTree(
      node({ id: 'root', startDate: '2026-01-01' } as Partial<MindMapTreeNode>), 0, 0,
    );
    expect(plain.root.visualTopExtra).toBe(0);
    expect(dated.root.visualTopExtra).toBe(34); // DATE_BADGE_OFFSET_H
    // the badge pushes the box down by half of what it adds
    expect(dated.root.y - plain.root.y).toBe(17);
  });

  it('sizes a subtree by its children when they are taller than the parent', () => {
    const root = node({
      id: 'root',
      children: [node({ id: 'a' }), node({ id: 'b' }), node({ id: 'c' })],
    });
    const pos = layout.layoutTree(root, 0, 0);
    const children = pos.a.h + pos.b.h + pos.c.h + 8 * 2; // three boxes, two gaps
    expect(pos.root.subtreeH).toBe(children);
    expect(pos.root.subtreeH).toBeGreaterThan(pos.root.h);
  });
});

describe('bezierPath', () => {
  it('bends through the horizontal midpoint', () => {
    expect(layout.bezierPath(0, 0, 100, 50)).toBe('M 0,0 C 50,0 50,50 100,50');
  });
});
