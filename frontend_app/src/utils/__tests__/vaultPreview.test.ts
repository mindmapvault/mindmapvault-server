import { describe, expect, it } from 'vitest';
import { fitToFrame } from '../vaultPreview';

/**
 * The mind map and board previews scale and centre their scenes with this, and
 * used to do it with two copies of the same arithmetic.
 */

const FRAME = { w: 760, h: 260 };
const fit = (b: Parameters<typeof fitToFrame>[0]) => fitToFrame(b, FRAME.w, FRAME.h);

describe('fitToFrame', () => {
  it('scales to whichever axis runs out of room first', () => {
    // Wide and short: width is the binding constraint (760 - 40) / 1440.
    const wide = fit({ minX: 0, minY: 0, maxX: 1440, maxY: 100 });
    expect(wide.scale).toBeCloseTo(0.5);

    // Tall and narrow: height binds, (260 - 40) / 440.
    const tall = fit({ minX: 0, minY: 0, maxX: 100, maxY: 440 });
    expect(tall.scale).toBeCloseTo(0.5);
  });

  it('centres the scene in the frame', () => {
    const { scale, offsetX, offsetY } = fit({ minX: 0, minY: 0, maxX: 1440, maxY: 100 });
    const drawnW = 1440 * scale;
    const drawnH = 100 * scale;
    expect(offsetX).toBeCloseTo((FRAME.w - drawnW) / 2);
    expect(offsetY).toBeCloseTo((FRAME.h - drawnH) / 2);
  });

  it('shifts a scene that does not start at the origin back into view', () => {
    const at0 = fit({ minX: 0, minY: 0, maxX: 400, maxY: 200 });
    const moved = fit({ minX: 1000, minY: -500, maxX: 1400, maxY: -300 });
    // Same size scene, so the same scale and the same on-screen placement.
    expect(moved.scale).toBeCloseTo(at0.scale);
    expect(moved.offsetX).toBeCloseTo(at0.offsetX - 1000 * at0.scale);
    expect(moved.offsetY).toBeCloseTo(at0.offsetY + 500 * at0.scale);
  });

  /** A one-node map has zero width; without the floor this divides by zero. */
  it('survives a scene with no extent', () => {
    const { scale, offsetX, offsetY } = fit({ minX: 50, minY: 50, maxX: 50, maxY: 50 });
    expect(Number.isFinite(scale)).toBe(true);
    expect(Number.isFinite(offsetX)).toBe(true);
    expect(Number.isFinite(offsetY)).toBe(true);
  });

  it('leaves the frame padding clear on the binding axis', () => {
    const { scale } = fit({ minX: 0, minY: 0, maxX: 1440, maxY: 100 });
    expect(1440 * scale).toBeCloseTo(FRAME.w - 40);
  });
});
