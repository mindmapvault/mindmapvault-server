import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import {
  Engine, Scene, Interaction, TextCardObj, ImageCardObj, PdfCardObj, ConnectionObj,
  CARD_COLORS, CONN_COLOR_KEYS, CONN_COLORS, BOARD_BACKGROUNDS,
  type BoardTool, type BoardNodeObj,
} from '../board/BoardEngine';
import type { BoardBackground, BoardCardColor, BoardData } from '../board/BoardTypes';
import { useThemeStore } from '../store/theme';
import './BoardEditor.css';

export interface BoardEditorHandle {
  captureCanvas(): Promise<Blob | null>;
}

interface BoardEditorProps {
  title: string;
  versionLabel?: string;
  data: BoardData;
  isDirty: boolean;
  saving: boolean;
  saveMsg: string;
  error: string;
  resolvedImageSrcs?: Record<string, string>;
  onPickImage: (file: File) => Promise<{ storedSrc: string; displaySrc: string }>;
  onPickPdf: (file: File) => Promise<{ storedSrc: string; thumbnailSrc: string; pageCount: number }>;
  onBack?: () => void;
  onSave: (data: BoardData) => void;
}

interface EditModal {
  node: BoardNodeObj;
  title: string;
  body: string;
  color: BoardCardColor;
  storedSrc: string;
  displaySrc: string;
  label: string;
}

interface CtxMenu {
  x: number;
  y: number;
  node: BoardNodeObj | null;
  conn: ConnectionObj | null;
  wp: { x: number; y: number };
}

interface ConnEditModal {
  conn: ConnectionObj;
  label: string;
  color: string;
}

type PendingImageAction =
  | { type: 'add'; x: number; y: number }
  | { type: 'replace'; node: ImageCardObj; label: string };

const CARD_COLOR_OPTS: BoardCardColor[] = ['default', 'red', 'yellow', 'blue', 'green', 'purple'];

export const BoardEditor = forwardRef<BoardEditorHandle, BoardEditorProps>(function BoardEditor({
  title, versionLabel, data, isDirty, saving, saveMsg, error,
  resolvedImageSrcs = {},
  onPickImage, onPickPdf,
  onBack, onSave,
}: BoardEditorProps, ref) {
  const themeMode = useThemeStore((s) => s.mode);
  const toggleThemeMode = useThemeStore((s) => s.toggleMode);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const engineRef = useRef<Engine | null>(null);
  const sceneRef = useRef<Scene | null>(null);
  const interRef = useRef<Interaction | null>(null);

  const imageInputRef = useRef<HTMLInputElement>(null);
  const pendingImageRef = useRef<PendingImageAction | null>(null);
  const pdfInputRef = useRef<HTMLInputElement>(null);
  const pendingPdfRef = useRef<{ x: number; y: number } | null>(null);

  // Background is tracked independently from scene data
  const bgRef = useRef<BoardBackground | null>(data.background ?? null);

  const [tool, setTool] = useState<BoardTool>('select');
  const [connColor, setConnColor] = useState(CONN_COLOR_KEYS[0]);
  const [zoom, setZoom] = useState(1);
  const [imageUploading, setImageUploading] = useState(false);
  const [bgPickerOpen, setBgPickerOpen] = useState(false);
  const [customColor, setCustomColor] = useState('#1e3a2f');

  const [editModal, setEditModal] = useState<EditModal | null>(null);
  const [connEditModal, setConnEditModal] = useState<ConnEditModal | null>(null);
  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);

  useImperativeHandle(ref, () => ({
    captureCanvas(): Promise<Blob | null> {
      const canvas = canvasRef.current;
      if (!canvas) return Promise.resolve(null);
      return new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, 'image/webp', 0.75));
    },
  }));

  // ── Serialize current canvas state + background ───────────────────────────
  const notifyChanged = useCallback(() => {
    const scene = sceneRef.current;
    const engine = engineRef.current;
    if (!scene || !engine) return;
    const { cards, connections } = scene.toData();
    const { x: panX, y: panY, zoom: z } = engine.camera;
    onSave({ version: 'board', cards, connections, view: { panX, panY, zoom: z }, background: bgRef.current ?? undefined });
  }, [onSave]);

  // ── Mount engine ──────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const engine = new Engine(canvas, themeMode === 'light');
    const scene = new Scene();
    const inter = new Interaction(canvas, engine, scene);

    engineRef.current = engine;
    sceneRef.current = scene;
    interRef.current = inter;

    scene.loadData(data);
    engine.background = bgRef.current;

    if (data.view) {
      engine.camera.x = data.view.panX;
      engine.camera.y = data.view.panY;
      engine.camera.zoom = data.view.zoom;
      setZoom(data.view.zoom);
    } else {
      engine.fitToContent(scene.nodes, 80, true);
    }

    engine.onDraw = (ctx, cam) => scene.draw(ctx, cam.zoom);
    engine.onDrawUI = (ctx) => inter.drawOverlay(ctx);
    engine.onZoomChange = (z) => setZoom(z);

    inter.onToolChange = (t) => setTool(t);
    inter.onChanged = () => notifyChanged();

    inter.onAddCard = (x, y) => {
      const node = new TextCardObj({ x, y, title: 'New card', body: '', color: 'default' });
      scene.add(node);
      notifyChanged();
      setEditModal({ node, title: node.title, body: node.body, color: node.color, storedSrc: '', displaySrc: '', label: '' });
    };

    inter.onAddImage = (x, y) => {
      pendingImageRef.current = { type: 'add', x, y };
      imageInputRef.current?.click();
    };

    inter.onAddPdf = (x, y) => {
      pendingPdfRef.current = { x, y };
      pdfInputRef.current?.click();
    };

    inter.onEditNode = (node) => {
      if (node instanceof TextCardObj) {
        setEditModal({ node, title: node.title, body: node.body, color: node.color, storedSrc: '', displaySrc: '', label: '' });
      } else if (node instanceof ImageCardObj) {
        const displaySrc = resolvedImageSrcs[node.src] ?? node.src;
        setEditModal({ node, title: '', body: '', color: 'default', storedSrc: node.src, displaySrc, label: node.label });
      } else if (node instanceof PdfCardObj) {
        setEditModal({ node, title: '', body: '', color: 'default', storedSrc: node.src, displaySrc: '', label: node.label });
      }
    };

    inter.onEditConnection = (conn) => {
      // Deselect all others, select this one
      scene.connections.forEach((c) => { c.selected = false; });
      conn.selected = true;
      setConnEditModal({ conn, label: conn.label, color: conn.color });
    };

    inter.onContextMenu = (cx, cy, hit, conn, wp) => {
      setCtxMenu({ x: cx, y: cy, node: hit, conn, wp });
    };

    return () => {
      engine.destroy();
      engineRef.current = null;
      sceneRef.current = null;
      interRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Patch resolved image URLs whenever the map changes
  useEffect(() => {
    sceneRef.current?.resolveImages(resolvedImageSrcs);
  }, [resolvedImageSrcs]);

  useEffect(() => {
    if (engineRef.current) engineRef.current.lightMode = themeMode === 'light';
  }, [themeMode]);

  useEffect(() => {
    if (interRef.current) interRef.current.connColor = connColor;
  }, [connColor]);

  // ── Background change ─────────────────────────────────────────────────────
  const applyBackground = useCallback((bg: BoardBackground | null) => {
    bgRef.current = bg;
    if (engineRef.current) engineRef.current.background = bg;
    notifyChanged();
  }, [notifyChanged]);

  // ── Image file picker ─────────────────────────────────────────────────────
  const handleImageFileChosen = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) { pendingImageRef.current = null; return; }

    const action = pendingImageRef.current;
    pendingImageRef.current = null;
    if (!action) return;

    setImageUploading(true);
    try {
      const { storedSrc, displaySrc } = await onPickImage(file);
      const scene = sceneRef.current;
      if (!scene) return;

      if (action.type === 'add') {
        const node = new ImageCardObj({ x: action.x - 100, y: action.y - 90, src: storedSrc, label: file.name.replace(/\.[^.]+$/, '') });
        node._loadImg(displaySrc);
        scene.add(node);
        notifyChanged();
        setEditModal({ node, title: '', body: '', color: 'default', storedSrc, displaySrc, label: node.label });
      } else {
        const { node, label } = action;
        node.src = storedSrc;
        node._img = null;
        node._loadImg(displaySrc);
        notifyChanged();
        setEditModal({ node, title: '', body: '', color: 'default', storedSrc, displaySrc, label });
      }
    } catch (err) {
      console.error('Image upload failed', err);
    } finally {
      setImageUploading(false);
    }
  }, [onPickImage, notifyChanged]);

  // ── PDF file picker ───────────────────────────────────────────────────────
  const handlePdfFileChosen = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) { pendingPdfRef.current = null; return; }

    const pos = pendingPdfRef.current;
    pendingPdfRef.current = null;
    if (!pos) return;

    setImageUploading(true);
    try {
      const { storedSrc, thumbnailSrc, pageCount } = await onPickPdf(file);
      const scene = sceneRef.current;
      if (!scene) return;

      const node = new PdfCardObj({ x: pos.x, y: pos.y, src: storedSrc, thumbnailSrc, label: file.name.replace(/\.[^.]+$/, ''), pageCount });
      scene.add(node);
      notifyChanged();
      setEditModal({ node, title: '', body: '', color: 'default', storedSrc, displaySrc: thumbnailSrc, label: node.label });
    } catch (err) {
      console.error('PDF upload failed', err);
    } finally {
      setImageUploading(false);
    }
  }, [onPickPdf, notifyChanged]);

  const handleToolClick = (t: BoardTool) => {
    interRef.current?.setTool(t);
    setTool(t);
  };

  const handleFit = () => {
    const scene = sceneRef.current;
    const engine = engineRef.current;
    if (scene && engine) engine.fitToContent(scene.nodes);
  };

  // ── Edit modal ────────────────────────────────────────────────────────────
  const commitEdit = () => {
    if (!editModal) return;
    const { node } = editModal;
    if (node instanceof TextCardObj) {
      node.title = editModal.title;
      node.body = editModal.body;
      node.color = editModal.color;
    } else if (node instanceof ImageCardObj) {
      node.label = editModal.label;
    } else if (node instanceof PdfCardObj) {
      node.label = editModal.label;
    }
    setEditModal(null);
    notifyChanged();
  };

  // ── Connection edit modal ────────────────────────────────────────────────
  const commitConnEdit = () => {
    if (!connEditModal) return;
    const { conn, label, color } = connEditModal;
    conn.label = label;
    conn.color = color;
    conn.selected = false;
    setConnEditModal(null);
    notifyChanged();
  };

  const cancelConnEdit = () => {
    if (connEditModal) connEditModal.conn.selected = false;
    setConnEditModal(null);
  };

  // ── Context menu ──────────────────────────────────────────────────────────
  const handleCtxDelete = () => {
    if (!ctxMenu?.node) return;
    sceneRef.current?.remove(ctxMenu.node);
    setCtxMenu(null);
    notifyChanged();
  };

  const handleCtxDuplicate = () => {
    if (!ctxMenu?.node) return;
    sceneRef.current?.duplicate(ctxMenu.node);
    setCtxMenu(null);
    notifyChanged();
  };

  const handleCtxAddCard = () => {
    if (!ctxMenu) return;
    const node = new TextCardObj({ x: ctxMenu.wp.x - 120, y: ctxMenu.wp.y - 80, title: 'New card', body: '', color: 'default' });
    sceneRef.current?.add(node);
    setCtxMenu(null);
    notifyChanged();
    setEditModal({ node, title: node.title, body: node.body, color: node.color, storedSrc: '', displaySrc: '', label: '' });
  };

  const handleCtxAddImage = () => {
    if (!ctxMenu) return;
    pendingImageRef.current = { type: 'add', x: ctxMenu.wp.x, y: ctxMenu.wp.y };
    setCtxMenu(null);
    imageInputRef.current?.click();
  };

  const handleCtxEditConn = () => {
    if (!ctxMenu?.conn) return;
    const conn = ctxMenu.conn;
    sceneRef.current?.connections.forEach((c) => { c.selected = false; });
    conn.selected = true;
    setConnEditModal({ conn, label: conn.label, color: conn.color });
    setCtxMenu(null);
  };

  const handleCtxDeleteConn = () => {
    if (!ctxMenu?.conn) return;
    sceneRef.current?.removeConnection(ctxMenu.conn);
    setCtxMenu(null);
    notifyChanged();
  };

  // Current background label for the button tooltip
  const bgLabel = bgRef.current
    ? bgRef.current.type === 'color'
      ? 'Custom colour'
      : (BOARD_BACKGROUNDS.find((b) => b.id === (bgRef.current as { type: 'texture'; id: string }).id)?.label ?? 'Texture')
    : 'Grid (default)';

  return (
    <div className="board-root" onClick={() => {
      setCtxMenu(null);
      setBgPickerOpen(false);
      // Deselect connections on blank click
      if (sceneRef.current) {
        sceneRef.current.connections.forEach((c) => { c.selected = false; });
      }
    }}>
      {/* Hidden image file input */}
      <input ref={imageInputRef} type="file" accept="image/*" style={{ display: 'none' }} onChange={handleImageFileChosen} />
      {/* Hidden PDF file input */}
      <input ref={pdfInputRef} type="file" accept=".pdf,application/pdf" style={{ display: 'none' }} onChange={handlePdfFileChosen} />

      {imageUploading && (
        <div className="board-upload-overlay">
          <svg className="board-spin" width="28" height="28" viewBox="0 0 24 24" fill="none">
            <circle opacity=".2" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"/>
            <path opacity=".8" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z"/>
          </svg>
          <span>Encrypting &amp; uploading image…</span>
        </div>
      )}

      {/* ── Toolbar ─────────────────────────────────────────────────── */}
      <div className="board-toolbar">
        <div className="board-toolbar-left">
          {onBack && (
            <button className="board-btn" onClick={onBack} title="Back to lobby">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M15 19l-7-7 7-7"/></svg>
            </button>
          )}
          <span className="board-title-text" title={title}>{title}</span>
          {versionLabel && <span className="board-version">{versionLabel}</span>}
        </div>

        <div className="board-toolbar-tools">
          {/* Tool buttons */}
          {([
            ['select',  'V', 'Select / move'],
            ['pan',     'H', 'Pan canvas'],
            ['card',    'C', 'Add card'],
            ['image',   'I', 'Add image'],
            ['pdf',     'P', 'Add PDF'],
            ['connect', 'E', 'Connect'],
          ] as const).map(([t, key, label]) => (
            <button
              key={t}
              className={`board-btn board-tool-btn${tool === t ? ' board-tool-btn--active' : ''}`}
              onClick={() => handleToolClick(t as BoardTool)}
              title={`${label} (${key})`}
            >
              {t === 'select'  && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z"/></svg>}
              {t === 'pan'     && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M5 9l-3 3 3 3M9 5l3-3 3 3M15 19l-3 3-3-3M19 9l3 3-3 3M12 12v.01"/><path strokeLinecap="round" strokeLinejoin="round" d="M2 12h20M12 2v20"/></svg>}
              {t === 'card'    && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><rect x="2" y="5" width="20" height="14" rx="2"/><line x1="2" y1="10" x2="22" y2="10"/></svg>}
              {t === 'image'   && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline strokeLinecap="round" strokeLinejoin="round" points="21 15 16 10 5 21"/></svg>}
              {t === 'pdf'     && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><polyline strokeLinecap="round" strokeLinejoin="round" points="14 2 14 8 20 8"/><line x1="9" y1="13" x2="15" y2="13"/><line x1="9" y1="17" x2="15" y2="17"/></svg>}
              {t === 'connect' && <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" d="M5 12h14M12 5l7 7-7 7"/></svg>}
              <span>{t === 'select' ? 'Select' : t === 'pan' ? 'Pan' : t === 'card' ? 'Card' : t === 'image' ? 'Image' : t === 'pdf' ? 'PDF' : 'Connect'}</span>
            </button>
          ))}

          <div className="board-toolbar-sep" />

          {/* Connection colour swatches */}
          <div className="board-conn-colors" title="Connection colour">
            {CONN_COLOR_KEYS.map((k) => (
              <button
                key={k}
                className={`board-conn-color-swatch${connColor === k ? ' board-conn-color-swatch--active' : ''}`}
                style={{ background: CONN_COLORS[k] }}
                onClick={() => setConnColor(k)}
                title={k}
              />
            ))}
          </div>

          <div className="board-toolbar-sep" />

          {/* Fit */}
          <button className="board-btn" onClick={handleFit} title="Fit to content (F)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M4 8V4m0 0h4M4 4l5 5m11-1V4m0 0h-4m4 0l-5 5M4 16v4m0 0h4m-4 0l5-5m11 5v-4m0 4h-4m4 0l-5-5"/></svg>
          </button>

          <span className="board-zoom-label">{Math.round(zoom * 100)}%</span>

          <div className="board-toolbar-sep" />

          {/* Background picker */}
          <div className="board-bg-picker-wrap" style={{ position: 'relative' }}>
            <button
              className={`board-btn board-bg-btn${bgPickerOpen ? ' board-tool-btn--active' : ''}`}
              onClick={(e) => { e.stopPropagation(); setBgPickerOpen((o) => !o); }}
              title={`Background: ${bgLabel}`}
            >
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
                <circle cx="12" cy="12" r="3"/>
                <path strokeLinecap="round" d="M12 1v3M12 20v3M4.22 4.22l2.12 2.12M17.66 17.66l2.12 2.12M1 12h3M20 12h3M4.22 19.78l2.12-2.12M17.66 6.34l2.12-2.12"/>
              </svg>
              <span>Background</span>
            </button>

            {bgPickerOpen && (
              <div className="board-bg-popover" onClick={(e) => e.stopPropagation()}>
                <div className="board-bg-popover-label">Canvas texture</div>
                <div className="board-bg-texture-grid">
                  {BOARD_BACKGROUNDS.map((b) => {
                    const active = bgRef.current?.type === 'texture' && (bgRef.current as { id: string }).id === b.id;
                    return (
                      <button
                        key={b.id}
                        className={`board-bg-swatch${active ? ' board-bg-swatch--active' : ''}`}
                        style={{ background: b.preview }}
                        title={b.label}
                        onClick={() => { applyBackground({ type: 'texture', id: b.id }); setBgPickerOpen(false); }}
                      >
                        <span className="board-bg-swatch-label">{b.label}</span>
                      </button>
                    );
                  })}
                </div>

                <div className="board-bg-popover-label" style={{ marginTop: 10 }}>Custom colour</div>
                <div className="board-bg-color-row">
                  <input
                    type="color"
                    className="board-bg-color-input"
                    value={customColor}
                    onChange={(e) => setCustomColor(e.target.value)}
                  />
                  <button
                    className={`board-bg-color-apply${bgRef.current?.type === 'color' ? ' board-bg-swatch--active' : ''}`}
                    onClick={() => { applyBackground({ type: 'color', color: customColor }); setBgPickerOpen(false); }}
                  >
                    Apply colour
                  </button>
                </div>

                <div className="board-bg-popover-sep" />
                <button
                  className={`board-bg-reset${!bgRef.current ? ' board-bg-reset--active' : ''}`}
                  onClick={() => { applyBackground(null); setBgPickerOpen(false); }}
                >
                  Grid (default)
                </button>
              </div>
            )}
          </div>
        </div>

        <div className="board-toolbar-right">
          <button className="board-btn" onClick={toggleThemeMode} title={themeMode === 'dark' ? 'Light mode' : 'Dark mode'}>
            {themeMode === 'dark'
              ? <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/><line x1="12" y1="21" x2="12" y2="23"/><line x1="4.22" y1="4.22" x2="5.64" y2="5.64"/><line x1="18.36" y1="18.36" x2="19.78" y2="19.78"/><line x1="1" y1="12" x2="3" y2="12"/><line x1="21" y1="12" x2="23" y2="12"/><line x1="4.22" y1="19.78" x2="5.64" y2="18.36"/><line x1="18.36" y1="5.64" x2="19.78" y2="4.22"/></svg>
              : <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" strokeLinejoin="round" d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z"/></svg>
            }
          </button>
          <button
            className={`board-btn board-save-btn${isDirty ? ' board-save-btn--dirty' : ''}${saving ? ' board-save-btn--saving' : ''}${error ? ' board-save-btn--error' : ''}${saveMsg ? ' board-save-btn--ok' : ''}`}
            onClick={notifyChanged}
            disabled={saving || (!isDirty && !error)}
            title={saving ? 'Saving…' : error || (isDirty ? 'Unsaved changes (Ctrl+S)' : 'All saved')}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}>
              <path strokeLinecap="round" strokeLinejoin="round" d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/>
              <polyline points="17 21 17 13 7 13 7 21"/>
              <polyline points="7 3 7 8 15 8"/>
            </svg>
            <span>{saving ? 'Saving…' : saveMsg || (isDirty ? 'Save' : 'Saved')}</span>
          </button>
        </div>
      </div>

      {/* ── Canvas ───────────────────────────────────────────────────── */}
      <div className="board-canvas-wrap">
        <canvas ref={canvasRef} className="board-canvas" />
      </div>

      {/* ── Context menu ─────────────────────────────────────────────── */}
      {ctxMenu && (
        <div
          className="board-ctx-menu"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          {!ctxMenu.node && !ctxMenu.conn && (
            <>
              <button className="board-ctx-item" onClick={handleCtxAddCard}>Add card</button>
              <button className="board-ctx-item" onClick={handleCtxAddImage}>Add image…</button>
              <button className="board-ctx-item" onClick={() => {
                if (!ctxMenu) return;
                pendingPdfRef.current = { x: ctxMenu.wp.x, y: ctxMenu.wp.y };
                setCtxMenu(null);
                pdfInputRef.current?.click();
              }}>Add PDF…</button>
            </>
          )}
          {ctxMenu.node && (
            <>
              <button className="board-ctx-item" onClick={() => {
                if (ctxMenu.node instanceof TextCardObj) {
                  setEditModal({ node: ctxMenu.node, title: ctxMenu.node.title, body: ctxMenu.node.body, color: ctxMenu.node.color, storedSrc: '', displaySrc: '', label: '' });
                } else if (ctxMenu.node instanceof ImageCardObj) {
                  const displaySrc = resolvedImageSrcs[ctxMenu.node.src] ?? ctxMenu.node.src;
                  setEditModal({ node: ctxMenu.node, title: '', body: '', color: 'default', storedSrc: ctxMenu.node.src, displaySrc, label: ctxMenu.node.label });
                } else if (ctxMenu.node instanceof PdfCardObj) {
                  setEditModal({ node: ctxMenu.node, title: '', body: '', color: 'default', storedSrc: ctxMenu.node.src, displaySrc: '', label: ctxMenu.node.label });
                }
                setCtxMenu(null);
              }}>Edit</button>
              <button className="board-ctx-item" onClick={handleCtxDuplicate}>Duplicate</button>
              <div className="board-ctx-sep" />
              <button className="board-ctx-item board-ctx-item--danger" onClick={handleCtxDelete}>Delete</button>
            </>
          )}
          {ctxMenu.conn && (
            <>
              <button className="board-ctx-item" onClick={handleCtxEditConn}>Edit connection…</button>
              <div className="board-ctx-sep" />
              <button className="board-ctx-item board-ctx-item--danger" onClick={handleCtxDeleteConn}>Delete connection</button>
            </>
          )}
        </div>
      )}

      {/* ── Edit modal ────────────────────────────────────────────────── */}
      {editModal && (
        <div className="board-modal-overlay" onClick={() => setEditModal(null)}>
          <div className="board-modal" onClick={(e) => e.stopPropagation()}>
            <div className="board-modal-header">
              <span>{editModal.node instanceof PdfCardObj ? 'Edit PDF card' : editModal.node instanceof ImageCardObj ? 'Edit image card' : 'Edit card'}</span>
              <button className="board-btn-icon" onClick={() => setEditModal(null)}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>

            {editModal.node instanceof TextCardObj ? (
              <>
                <label className="board-modal-label">Title</label>
                <input className="board-modal-input" value={editModal.title} onChange={(e) => setEditModal((m) => m ? { ...m, title: e.target.value } : m)} autoFocus />
                <label className="board-modal-label">Notes</label>
                <textarea className="board-modal-textarea" value={editModal.body} onChange={(e) => setEditModal((m) => m ? { ...m, body: e.target.value } : m)} rows={4} />
                <label className="board-modal-label">Color</label>
                <div className="board-modal-colors">
                  {CARD_COLOR_OPTS.map((c) => (
                    <button
                      key={c}
                      className={`board-modal-color-btn${editModal.color === c ? ' board-modal-color-btn--active' : ''}`}
                      style={{ background: CARD_COLORS[c] }}
                      onClick={() => setEditModal((m) => m ? { ...m, color: c } : m)}
                      title={c}
                    />
                  ))}
                </div>
              </>
            ) : editModal.node instanceof PdfCardObj ? (
              <>
                <div className="board-modal-img-preview board-modal-pdf-preview">
                  {editModal.displaySrc
                    ? <img src={editModal.displaySrc} alt="PDF preview" />
                    : <div className="board-modal-img-empty" style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6 }}>
                        <span style={{ fontSize: 32 }}>📄</span>
                        <span style={{ fontSize: 12, opacity: 0.6 }}>PDF — {(editModal.node as PdfCardObj).pageCount} page{(editModal.node as PdfCardObj).pageCount !== 1 ? 's' : ''}</span>
                      </div>
                  }
                </div>
                <label className="board-modal-label">Label</label>
                <input className="board-modal-input" value={editModal.label} onChange={(e) => setEditModal((m) => m ? { ...m, label: e.target.value } : m)} autoFocus />
              </>
            ) : (
              <>
                {editModal.displaySrc ? (
                  <div className="board-modal-img-preview"><img src={editModal.displaySrc} alt="preview" /></div>
                ) : (
                  <div className="board-modal-img-empty">No image yet</div>
                )}
                <button
                  className="board-modal-btn board-modal-btn--secondary board-modal-replace-btn"
                  onClick={() => {
                    if (editModal.node instanceof ImageCardObj) {
                      pendingImageRef.current = { type: 'replace', node: editModal.node, label: editModal.label };
                      imageInputRef.current?.click();
                    }
                  }}
                >
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2}><path strokeLinecap="round" d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline strokeLinecap="round" strokeLinejoin="round" points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
                  {editModal.displaySrc ? 'Replace image' : 'Choose image'}
                </button>
                <label className="board-modal-label">Label</label>
                <input className="board-modal-input" value={editModal.label} onChange={(e) => setEditModal((m) => m ? { ...m, label: e.target.value } : m)} autoFocus={!editModal.displaySrc} />
              </>
            )}

            <div className="board-modal-actions">
              <button className="board-modal-btn board-modal-btn--secondary" onClick={() => setEditModal(null)}>Cancel</button>
              <button className="board-modal-btn board-modal-btn--primary" onClick={commitEdit}>Save</button>
            </div>
          </div>
        </div>
      )}

      {/* ── Connection edit modal ─────────────────────────────────────── */}
      {connEditModal && (
        <div className="board-modal-overlay" onClick={cancelConnEdit}>
          <div className="board-modal" onClick={(e) => e.stopPropagation()}>
            <div className="board-modal-header">
              <span>Edit connection</span>
              <button className="board-btn-icon" onClick={cancelConnEdit}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>

            <label className="board-modal-label">Tag / label</label>
            <input
              className="board-modal-input"
              value={connEditModal.label}
              onChange={(e) => setConnEditModal((m) => m ? { ...m, label: e.target.value } : m)}
              placeholder="e.g. connected to, evidence of…"
              autoFocus
              onKeyDown={(e) => { if (e.key === 'Enter') commitConnEdit(); }}
            />

            <label className="board-modal-label">Colour</label>
            <div className="board-conn-edit-colors">
              {CONN_COLOR_KEYS.map((k) => (
                <button
                  key={k}
                  className={`board-conn-color-swatch board-conn-color-swatch--lg${connEditModal.color === k ? ' board-conn-color-swatch--active' : ''}`}
                  style={{ background: CONN_COLORS[k] }}
                  onClick={() => setConnEditModal((m) => m ? { ...m, color: k } : m)}
                  title={k}
                />
              ))}
              {/* Custom hex colour */}
              <label className="board-conn-color-custom" title="Custom colour">
                <span
                  className="board-conn-color-swatch board-conn-color-swatch--lg"
                  style={{ background: CONN_COLORS[connEditModal.color] ? 'transparent' : connEditModal.color, border: '2px dashed rgba(255,255,255,0.4)' }}
                >
                  {!CONN_COLORS[connEditModal.color] && (
                    <span style={{ display: 'block', width: '100%', height: '100%', background: connEditModal.color, borderRadius: 4 }} />
                  )}
                </span>
                <input
                  type="color"
                  className="sr-only"
                  value={CONN_COLORS[connEditModal.color] ?? connEditModal.color}
                  onChange={(e) => setConnEditModal((m) => m ? { ...m, color: e.target.value } : m)}
                />
              </label>
            </div>

            <div className="board-modal-actions">
              <button className="board-modal-btn board-modal-btn--secondary" onClick={cancelConnEdit}>Cancel</button>
              <button className="board-modal-btn board-modal-btn--primary" onClick={commitConnEdit}>Save</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
