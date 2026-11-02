import { useCallback, useMemo, useRef, useState, type SetStateAction } from 'react';
import {
  clampPinchZoom,
  clampZoom,
  readViewState,
  toCanvasPoint,
  touchDistance,
  touchMidpoint,
  zoomAround,
  type Point,
  type ViewState,
} from './viewport';

/** Two touches, relative to the canvas element's top-left corner. */
export interface TouchPair {
  a: Point;
  b: Point;
}

/**
 * Where the canvas is, and the gestures that move it.
 *
 * Pan and zoom are one piece of state: zooming around a point changes both,
 * and two separate setters would let a fast run of wheel or touch events read
 * one that is a render behind the other. Whether a pan is in progress is a
 * ref: it changes on every mouse move and nothing renders differently for it,
 * so putting it in state would re-render the whole canvas on each pointer event.
 *
 * This is per user and per device — nothing here is in the document or shared
 * with anyone else looking at the same map.
 */
export function useViewport(saved?: { pan_x?: number; pan_y?: number; zoom?: number } | null) {
  const [view, setView] = useState<ViewState>(() => readViewState(saved));
  const { pan, zoom } = view;

  const panning = useRef(false);
  const lastPoint = useRef<Point>({ x: 0, y: 0 });
  const pinch = useRef<{ distance: number; mid: Point; view: ViewState } | null>(null);

  const setPan = useCallback((next: SetStateAction<Point>) => {
    setView((v) => ({ ...v, pan: typeof next === 'function' ? next(v.pan) : next }));
  }, []);

  const setZoom = useCallback((next: SetStateAction<number>) => {
    setView((v) => ({ ...v, zoom: typeof next === 'function' ? next(v.zoom) : next }));
  }, []);

  /** Jump straight to a view — opening a map, or restoring a saved one. */
  const applyView = useCallback((next: ViewState) => {
    setView({ pan: next.pan, zoom: clampZoom(next.zoom) });
  }, []);

  /**
   * Zoom by a step, keeping `anchor` (canvas-element coordinates) still on
   * screen — the cursor for the wheel, the middle of the canvas for buttons.
   */
  const zoomBy = useCallback((delta: number, anchor: Point) => {
    setView((v) => zoomAround(v, clampZoom(v.zoom + delta), anchor));
  }, []);

  const nudgePan = useCallback((dx: number, dy: number) => {
    if (dx === 0 && dy === 0) return;
    setPan((p) => ({ x: p.x + dx, y: p.y + dy }));
  }, [setPan]);

  // ── Pan ───────────────────────────────────────────────────────

  const beginPan = useCallback((x: number, y: number) => {
    panning.current = true;
    lastPoint.current = { x, y };
  }, []);

  /** Continue a pan. False when none is in progress, so callers can fall through. */
  const continuePan = useCallback((x: number, y: number): boolean => {
    if (!panning.current) return false;
    const dx = x - lastPoint.current.x;
    const dy = y - lastPoint.current.y;
    lastPoint.current = { x, y };
    setPan((p) => ({ x: p.x + dx, y: p.y + dy }));
    return true;
  }, [setPan]);

  const endPan = useCallback(() => {
    panning.current = false;
  }, []);

  const isPanning = useCallback(() => panning.current, []);

  // ── Pinch ─────────────────────────────────────────────────────

  const beginPinch = useCallback((touches: TouchPair) => {
    // A second finger ends the pan it started as, or the canvas slides while
    // it scales.
    panning.current = false;
    pinch.current = {
      distance: touchDistance(touches.a, touches.b),
      mid: touchMidpoint(touches.a, touches.b),
      view,
    };
  }, [view]);

  /**
   * Scale relative to the gap the pinch started at, around the point between
   * the fingers, and follow the fingers if they move together. The map point
   * that started under the fingers stays under them. False when none is active.
   */
  const continuePinch = useCallback((touches: TouchPair): boolean => {
    const start = pinch.current;
    if (!start) return false;
    if (start.distance <= 0) return true;
    const distance = touchDistance(touches.a, touches.b);
    const mid = touchMidpoint(touches.a, touches.b);
    const nextZoom = clampPinchZoom(start.view.zoom * (distance / start.distance));
    const scaled = zoomAround(start.view, nextZoom, start.mid);
    setView({
      zoom: nextZoom,
      pan: { x: scaled.pan.x + mid.x - start.mid.x, y: scaled.pan.y + mid.y - start.mid.y },
    });
    return true;
  }, []);

  const endGesture = useCallback(() => {
    panning.current = false;
    pinch.current = null;
  }, []);

  /** A point on screen, in the map's own coordinates. */
  const toCanvas = useCallback(
    (clientX: number, clientY: number, rect: { left: number; top: number }): Point =>
      toCanvasPoint(clientX, clientY, rect, view),
    [view],
  );

  // Memoised because the editor's event handlers take this whole object as a
  // dependency. A fresh object each render would rebuild every one of them on
  // every render, which for a canvas of five hundred nodes is not free.
  return useMemo(() => ({
    pan,
    zoom,
    view,
    setPan,
    setZoom,
    applyView,
    zoomBy,
    nudgePan,
    beginPan,
    continuePan,
    endPan,
    isPanning,
    beginPinch,
    continuePinch,
    endGesture,
    toCanvas,
  }), [
    pan, zoom, view, setPan, setZoom, applyView, zoomBy, nudgePan, beginPan, continuePan,
    endPan, isPanning, beginPinch, continuePinch, endGesture, toCanvas,
  ]);
}
