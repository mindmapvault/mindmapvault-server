/**
 * The canvas viewport: how far it is panned, how far zoomed, and how to get
 * between screen coordinates and map coordinates.
 *
 * Pure, so the awkward parts are testable: that a saved view state can hold
 * anything at all and has to be validated on the way in, and that the two ways
 * of zooming do not agree about the limits.
 */

/** Wheel and keyboard zoom. */
export const ZOOM_MIN = 0.3;
export const ZOOM_MAX = 3;
/**
 * Pinch zoom, which has always allowed a wider range than the wheel does. Kept
 * as it was rather than unified — a touch gesture overshooting and springing
 * back is normal, and tightening it here would change how the canvas feels on
 * a tablet. Worth deciding on deliberately sometime.
 */
export const PINCH_ZOOM_MIN = 0.2;
export const PINCH_ZOOM_MAX = 4;

/** Where a map opens when it has no saved view. */
export const DEFAULT_PAN = { x: 160, y: 300 };

export interface Point {
  x: number;
  y: number;
}

export interface ViewState {
  pan: Point;
  zoom: number;
}

export const clampZoom = (zoom: number): number =>
  Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));

export const clampPinchZoom = (zoom: number): number =>
  Math.min(PINCH_ZOOM_MAX, Math.max(PINCH_ZOOM_MIN, zoom));

const finiteOr = (value: unknown, fallback: number): number =>
  typeof value === 'number' && Number.isFinite(value) ? value : fallback;

/**
 * Read a saved view, defaulting anything missing or nonsensical.
 *
 * The view state comes out of the map file, so it can hold a NaN zoom from an
 * old bug or a hand-edited export — and a NaN zoom makes every coordinate on
 * the canvas NaN, which renders as an empty screen with no error.
 */
export const readViewState = (saved: {
  pan_x?: number;
  pan_y?: number;
  zoom?: number;
} | undefined | null): ViewState => ({
  pan: {
    x: finiteOr(saved?.pan_x, DEFAULT_PAN.x),
    y: finiteOr(saved?.pan_y, DEFAULT_PAN.y),
  },
  zoom: clampZoom(finiteOr(saved?.zoom, 1)),
});

/** A point on screen, in the map's own coordinates. */
export const toCanvasPoint = (
  clientX: number,
  clientY: number,
  rect: { left: number; top: number },
  view: ViewState,
): Point => ({
  x: (clientX - rect.left - view.pan.x) / view.zoom,
  y: (clientY - rect.top - view.pan.y) / view.zoom,
});

/** The distance between two touches, for pinch zoom. */
export const touchDistance = (a: Point, b: Point): number =>
  Math.sqrt((b.x - a.x) ** 2 + (b.y - a.y) ** 2);
