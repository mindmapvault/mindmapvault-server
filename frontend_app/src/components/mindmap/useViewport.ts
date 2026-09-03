import { useCallback, useMemo, useRef, useState } from 'react';
import {
  clampPinchZoom,
  clampZoom,
  readViewState,
  toCanvasPoint,
  touchDistance,
  type Point,
  type ViewState,
} from './viewport';

/** Two touches, as the browser reports them. */
export interface TouchPair {
  a: Point;
  b: Point;
}

/**
 * Where the canvas is, and the gestures that move it.
 *
 * Pan and zoom are state, because everything drawn depends on them. Whether a
 * pan is in progress is a ref: it changes on every mouse move and nothing
 * renders differently for it, so putting it in state would re-render the whole
 * canvas on each pointer event.
 *
 * This is per user and per device — nothing here is in the document or shared
 * with anyone else looking at the same map.
 */
export function useViewport(saved?: { pan_x?: number; pan_y?: number; zoom?: number } | null) {
  const initial = readViewState(saved);
  const [pan, setPan] = useState<Point>(initial.pan);
  const [zoom, setZoom] = useState(initial.zoom);

  const panning = useRef(false);
  const lastPoint = useRef<Point>({ x: 0, y: 0 });
  const pinch = useRef<{ distance: number; zoom: number } | null>(null);

  /** Jump straight to a view — opening a map, or restoring a saved one. */
  const applyView = useCallback((next: ViewState) => {
    setPan(next.pan);
    setZoom(clampZoom(next.zoom));
  }, []);

  const zoomBy = useCallback((delta: number) => {
    setZoom((z) => clampZoom(z + delta));
  }, []);

  const nudgePan = useCallback((dx: number, dy: number) => {
    if (dx === 0 && dy === 0) return;
    setPan((p) => ({ x: p.x + dx, y: p.y + dy }));
  }, []);

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
  }, []);

  const endPan = useCallback(() => {
    panning.current = false;
  }, []);

  const isPanning = useCallback(() => panning.current, []);

  // ── Pinch ─────────────────────────────────────────────────────

  const beginPinch = useCallback((touches: TouchPair) => {
    // A second finger ends the pan it started as, or the canvas slides while
    // it scales.
    panning.current = false;
    pinch.current = { distance: touchDistance(touches.a, touches.b), zoom };
  }, [zoom]);

  /** Scale relative to the gap the pinch started at. False when none is active. */
  const continuePinch = useCallback((touches: TouchPair): boolean => {
    const start = pinch.current;
    if (!start) return false;
    const distance = touchDistance(touches.a, touches.b);
    setZoom(clampPinchZoom(start.zoom * (distance / start.distance)));
    return true;
  }, []);

  const endGesture = useCallback(() => {
    panning.current = false;
    pinch.current = null;
  }, []);

  /** A point on screen, in the map's own coordinates. */
  const toCanvas = useCallback(
    (clientX: number, clientY: number, rect: { left: number; top: number }): Point =>
      toCanvasPoint(clientX, clientY, rect, { pan, zoom }),
    [pan, zoom],
  );

  // Memoised because the editor's event handlers take this whole object as a
  // dependency. A fresh object each render would rebuild every one of them on
  // every render, which for a canvas of five hundred nodes is not free.
  return useMemo(() => ({
    pan,
    zoom,
    view: { pan, zoom } as ViewState,
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
    pan, zoom, applyView, zoomBy, nudgePan, beginPan, continuePan, endPan,
    isPanning, beginPinch, continuePinch, endGesture, toCanvas,
  ]);
}
