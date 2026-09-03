import { describe, expect, it } from 'vitest';
import {
  DEFAULT_PAN,
  PINCH_ZOOM_MAX,
  PINCH_ZOOM_MIN,
  ZOOM_MAX,
  ZOOM_MIN,
  clampPinchZoom,
  clampZoom,
  readViewState,
  toCanvasPoint,
  touchDistance,
} from '../viewport';

describe('readViewState', () => {
  it('takes a saved view as it stands', () => {
    expect(readViewState({ pan_x: 10, pan_y: 20, zoom: 1.5 }))
      .toEqual({ pan: { x: 10, y: 20 }, zoom: 1.5 });
  });

  it('falls back to the opening view when there is nothing saved', () => {
    expect(readViewState(undefined)).toEqual({ pan: DEFAULT_PAN, zoom: 1 });
    expect(readViewState(null)).toEqual({ pan: DEFAULT_PAN, zoom: 1 });
    expect(readViewState({})).toEqual({ pan: DEFAULT_PAN, zoom: 1 });
  });

  /**
   * The view state comes out of the map file, so it can hold anything. A NaN
   * zoom makes every coordinate on the canvas NaN, which renders as a blank
   * screen and no error at all.
   */
  it('refuses a zoom or pan that is not a finite number', () => {
    expect(readViewState({ zoom: NaN }).zoom).toBe(1);
    expect(readViewState({ zoom: Infinity }).zoom).toBe(1);
    expect(readViewState({ pan_x: NaN, pan_y: 5 }).pan).toEqual({ x: DEFAULT_PAN.x, y: 5 });
    expect(readViewState({ zoom: '2' as unknown as number }).zoom).toBe(1);
  });

  it('clamps a saved zoom that is out of range', () => {
    expect(readViewState({ zoom: 99 }).zoom).toBe(ZOOM_MAX);
    expect(readViewState({ zoom: 0.01 }).zoom).toBe(ZOOM_MIN);
  });

  it('keeps a pan of zero rather than treating it as missing', () => {
    expect(readViewState({ pan_x: 0, pan_y: 0 }).pan).toEqual({ x: 0, y: 0 });
  });
});

describe('zoom limits', () => {
  it('clamps the wheel and the pinch to their own ranges', () => {
    expect(clampZoom(10)).toBe(ZOOM_MAX);
    expect(clampZoom(0)).toBe(ZOOM_MIN);
    expect(clampPinchZoom(10)).toBe(PINCH_ZOOM_MAX);
    expect(clampPinchZoom(0)).toBe(PINCH_ZOOM_MIN);
  });

  it('lets pinch go further than the wheel, as it always has', () => {
    expect(PINCH_ZOOM_MIN).toBeLessThan(ZOOM_MIN);
    expect(PINCH_ZOOM_MAX).toBeGreaterThan(ZOOM_MAX);
  });
});

describe('toCanvasPoint', () => {
  it('undoes the pan and the zoom', () => {
    const view = { pan: { x: 100, y: 50 }, zoom: 2 };
    expect(toCanvasPoint(300, 250, { left: 0, top: 0 }, view)).toEqual({ x: 100, y: 100 });
  });

  it('accounts for where the canvas sits in the page', () => {
    const view = { pan: { x: 0, y: 0 }, zoom: 1 };
    expect(toCanvasPoint(120, 80, { left: 20, top: 30 }, view)).toEqual({ x: 100, y: 50 });
  });
});

describe('touchDistance', () => {
  it('measures the gap between two fingers', () => {
    expect(touchDistance({ x: 0, y: 0 }, { x: 3, y: 4 })).toBe(5);
  });
});
