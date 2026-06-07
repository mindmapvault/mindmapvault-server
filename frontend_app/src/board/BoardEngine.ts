/**
 * BoardEngine — Canvas 2D rendering engine for the evidence / detective board.
 * TypeScript port of the frontend_app_v4 prototype.
 *
 * Exports: Engine, Scene, BoardNodeObj, TextCardObj, ImageCardObj,
 *          ConnectionObj, Interaction, CONN_COLORS
 */

import type {
  BoardBackground,
  BoardBackgroundTextureId,
  BoardCardColor,
  BoardConnection,
  BoardData,
  BoardNode,
  BoardTextCard,
  BoardImageCard,
  BoardPdfCard,
} from './BoardTypes';

// ── Constants ────────────────────────────────────────────────────────────────

const GRID = 40;
const BG_DARK = '#0f0f13';
const BG_LIGHT = '#f1f5f9';
const GRID_DARK = 'rgba(255,255,255,0.035)';
const GRID_LIGHT = 'rgba(0,0,0,0.055)';
const GRID_ACCENT_DARK = 'rgba(255,255,255,0.07)';
const GRID_ACCENT_LIGHT = 'rgba(0,0,0,0.11)';

// ── Background texture catalogue ──────────────────────────────────────────────

export const BOARD_BACKGROUNDS: Array<{ id: BoardBackgroundTextureId; label: string; preview: string }> = [
  { id: 'cork',        label: 'Cork board',  preview: '#c49a6c' },
  { id: 'cork-dark',   label: 'Dark cork',   preview: '#8b5e3c' },
  { id: 'chalkboard',  label: 'Chalkboard',  preview: '#263d2b' },
  { id: 'aged-paper',  label: 'Aged paper',  preview: '#f2e8d0' },
  { id: 'blueprint',   label: 'Blueprint',   preview: '#0c1e3a' },
];

// ── Seeded PRNG (LCG) for deterministic procedural textures ──────────────────

function seedRng(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (Math.imul(1664525, s) + 1013904223) >>> 0;
    return s / 0x100000000;
  };
}

export const CARD_COLORS: Record<BoardCardColor, string> = {
  default: '#f5f0e8',
  red:     '#ffd5d5',
  yellow:  '#fff3b0',
  blue:    '#d0e8ff',
  green:   '#d0ffdc',
  purple:  '#ead5ff',
};

export const CONN_COLORS: Record<string, string> = {
  red:    '#ef4444',
  blue:   '#3b82f6',
  green:  '#22c55e',
  yellow: '#f59e0b',
  purple: '#a855f7',
  white:  '#d1d5db',
};
export const CONN_COLOR_KEYS = Object.keys(CONN_COLORS);

const PIN_COLORS = ['#e74c3c', '#e67e22', '#3498db', '#2ecc71', '#9b59b6', '#f39c12'];

// ── Utilities ────────────────────────────────────────────────────────────────

let _uid = 0;
function newId(prefix = 'n'): string {
  return `${prefix}-${++_uid}-${Math.random().toString(36).slice(2, 6)}`;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const R = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + R, y);
  ctx.arcTo(x + w, y, x + w, y + h, R);
  ctx.arcTo(x + w, y + h, x, y + h, R);
  ctx.arcTo(x, y + h, x, y, R);
  ctx.arcTo(x, y, x + w, y, R);
  ctx.closePath();
}

function clipText(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (!text) return '';
  if (ctx.measureText(text).width <= maxW) return text;
  let lo = 0, hi = text.length;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (ctx.measureText(text.slice(0, mid) + '…').width <= maxW) lo = mid;
    else hi = mid - 1;
  }
  return text.slice(0, lo) + '…';
}

function wrapText(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, maxW: number, lineH: number, maxLines = 6) {
  if (!text) return;
  const words = text.split(/\s+/);
  let line = '';
  let lc = 0;
  for (let i = 0; i <= words.length; i++) {
    const w = words[i] ?? '';
    const test = line ? line + ' ' + w : w;
    if ((ctx.measureText(test).width > maxW || i === words.length) && line) {
      if (lc >= maxLines - 1 && i < words.length) {
        ctx.fillText(clipText(ctx, line, maxW), x, y + lc * lineH);
        return;
      }
      ctx.fillText(line, x, y + lc * lineH);
      lc++;
      line = w;
    } else {
      line = test;
    }
  }
}

function nearestSide(
  nx: number, ny: number, nw: number, nh: number,
  tx: number, ty: number,
): { x: number; y: number } {
  const anchors = [
    { x: nx + nw / 2, y: ny },
    { x: nx + nw,     y: ny + nh / 2 },
    { x: nx + nw / 2, y: ny + nh },
    { x: nx,          y: ny + nh / 2 },
  ];
  let best = anchors[0], bd = Infinity;
  for (const a of anchors) {
    const d = Math.hypot(a.x - tx, a.y - ty);
    if (d < bd) { bd = d; best = a; }
  }
  return best;
}

function drawPin(ctx: CanvasRenderingContext2D, px: number, py: number, color: string) {
  ctx.save();
  ctx.shadowColor = 'rgba(0,0,0,0.5)';
  ctx.shadowBlur = 5;
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(px, py, 6, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = 'rgba(255,255,255,0.35)';
  ctx.beginPath();
  ctx.arc(px - 2, py - 2, 2.2, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
}

// ── Node objects ─────────────────────────────────────────────────────────────

export class TextCardObj {
  id: string;
  type: 'card' = 'card';
  x: number;
  y: number;
  width = 240;
  height = 160;
  title: string;
  body: string;
  color: BoardCardColor;
  selected = false;
  readonly _pin: string;

  constructor(d: Partial<BoardTextCard> = {}) {
    this.id = d.id ?? newId('card');
    this.x = d.x ?? 0;
    this.y = d.y ?? 0;
    this.title = d.title ?? 'Untitled';
    this.body = d.body ?? '';
    this.color = d.color ?? 'default';
    this._pin = PIN_COLORS[Math.floor(Math.random() * PIN_COLORS.length)];
  }

  draw(ctx: CanvasRenderingContext2D) {
    const { x, y, width, height, title, body, color, selected } = this;
    const bg = CARD_COLORS[color] ?? CARD_COLORS.default;
    const PAD = 13;
    const TITLE_H = 34;

    ctx.save();
    ctx.shadowColor = 'rgba(0,0,0,0.55)';
    ctx.shadowBlur = selected ? 22 : 14;
    ctx.shadowOffsetX = 2;
    ctx.shadowOffsetY = 5;
    ctx.fillStyle = bg;
    roundRect(ctx, x, y, width, height, 10);
    ctx.fill();
    ctx.restore();

    if (selected) {
      ctx.save();
      ctx.strokeStyle = '#60a5fa';
      ctx.lineWidth = 2;
      ctx.shadowColor = 'rgba(96,165,250,0.6)';
      ctx.shadowBlur = 10;
      roundRect(ctx, x - 2, y - 2, width + 4, height + 4, 12);
      ctx.stroke();
      ctx.restore();
    }

    // Title bar tint
    ctx.save();
    ctx.fillStyle = 'rgba(0,0,0,0.09)';
    ctx.beginPath();
    ctx.moveTo(x + 10, y);
    ctx.lineTo(x + width - 10, y);
    ctx.arcTo(x + width, y, x + width, y + 10, 10);
    ctx.lineTo(x + width, y + TITLE_H);
    ctx.lineTo(x, y + TITLE_H);
    ctx.lineTo(x, y + 10);
    ctx.arcTo(x, y, x + 10, y, 10);
    ctx.closePath();
    ctx.fill();
    ctx.restore();

    ctx.save();
    ctx.fillStyle = '#1a1a1a';
    ctx.font = 'bold 12px system-ui,-apple-system,sans-serif';
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    ctx.fillText(clipText(ctx, title, width - PAD * 2 - 4), x + PAD, y + TITLE_H / 2 + 1);
    ctx.restore();

    ctx.save();
    ctx.fillStyle = '#3a3535';
    ctx.font = '11px system-ui,-apple-system,sans-serif';
    ctx.textBaseline = 'top';
    wrapText(ctx, body, x + PAD, y + TITLE_H + 9, width - PAD * 2, 15, 6);
    ctx.restore();

    drawPin(ctx, x + width / 2, y + 5, this._pin);
  }

  hitTest(wx: number, wy: number): boolean {
    return wx >= this.x && wx <= this.x + this.width && wy >= this.y && wy <= this.y + this.height;
  }

  nearestAnchor(tx: number, ty: number) {
    return nearestSide(this.x, this.y, this.width, this.height, tx, ty);
  }

  pinAnchor() {
    return { x: this.x + this.width / 2, y: this.y + 5 };
  }

  toJSON(): BoardTextCard {
    return { type: this.type, id: this.id, x: this.x, y: this.y, title: this.title, body: this.body, color: this.color };
  }

  static fromJSON(d: BoardTextCard): TextCardObj {
    const n = new TextCardObj(d);
    n.id = d.id;
    return n;
  }
}

export class ImageCardObj {
  id: string;
  type: 'image' = 'image';
  x: number;
  y: number;
  width: number;
  height: number;
  _imgH: number;
  src: string;
  label: string;
  selected = false;
  readonly _pin: string;
  _img: HTMLImageElement | null = null;

  constructor(d: Partial<BoardImageCard> = {}) {
    this.id = d.id ?? newId('img');
    this.x = d.x ?? 0;
    this.y = d.y ?? 0;
    this._imgH = d.height ?? 180;
    this.width = d.width ?? 200;
    this.height = this._imgH + 32;
    this.src = d.src ?? '';
    this.label = d.label ?? '';
    this._pin = PIN_COLORS[Math.floor(Math.random() * PIN_COLORS.length)];
    // Only auto-load if src is a real displayable URL; 'attachment:' IDs are resolved
    // later by Scene.resolveImages() once the encrypted attachment has been decrypted.
    if (this.src && !this.src.startsWith('attachment:')) this._loadImg(this.src);
  }

  _loadImg(src: string) {
    const img = new Image();
    if (!src.startsWith('data:')) img.crossOrigin = 'anonymous';
    img.onload = () => { this._img = img; };
    img.src = src;
  }

  draw(ctx: CanvasRenderingContext2D) {
    const { x, y, width, height, _imgH, label, selected } = this;
    const BORDER = 8;

    ctx.save();
    ctx.shadowColor = 'rgba(0,0,0,0.65)';
    ctx.shadowBlur = selected ? 24 : 16;
    ctx.shadowOffsetX = 3;
    ctx.shadowOffsetY = 6;
    ctx.fillStyle = '#f0ece0';
    roundRect(ctx, x, y, width, height, 4);
    ctx.fill();
    ctx.restore();

    if (selected) {
      ctx.save();
      ctx.strokeStyle = '#60a5fa';
      ctx.lineWidth = 2;
      ctx.shadowColor = 'rgba(96,165,250,0.6)';
      ctx.shadowBlur = 10;
      roundRect(ctx, x - 2, y - 2, width + 4, height + 4, 6);
      ctx.stroke();
      ctx.restore();
    }

    const ix = x + BORDER, iy = y + BORDER;
    const iw = width - BORDER * 2, ih = _imgH - BORDER;
    ctx.save();
    ctx.beginPath();
    ctx.rect(ix, iy, iw, ih);
    ctx.clip();
    if (this._img) {
      ctx.drawImage(this._img, ix, iy, iw, ih);
    } else {
      ctx.fillStyle = '#4b5563';
      ctx.fillRect(ix, iy, iw, ih);
      ctx.fillStyle = 'rgba(255,255,255,0.3)';
      ctx.font = '32px system-ui';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText('📷', ix + iw / 2, iy + ih / 2);
    }
    ctx.restore();

    ctx.save();
    ctx.fillStyle = '#1a1a1a';
    ctx.font = '10px system-ui,sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(clipText(ctx, label || 'Evidence', width - 16), x + width / 2, y + _imgH + 16);
    ctx.restore();

    drawPin(ctx, x + width / 2, y + 5, this._pin);
  }

  hitTest(wx: number, wy: number): boolean {
    return wx >= this.x && wx <= this.x + this.width && wy >= this.y && wy <= this.y + this.height;
  }

  nearestAnchor(tx: number, ty: number) {
    return nearestSide(this.x, this.y, this.width, this.height, tx, ty);
  }

  pinAnchor() {
    return { x: this.x + this.width / 2, y: this.y + 5 };
  }

  toJSON(): BoardImageCard {
    return { type: this.type, id: this.id, x: this.x, y: this.y, src: this.src, label: this.label, width: this.width, height: this._imgH };
  }

  static fromJSON(d: BoardImageCard): ImageCardObj {
    const n = new ImageCardObj(d);
    n.id = d.id;
    return n;
  }
}

export class PdfCardObj {
  id: string;
  type: 'pdf' = 'pdf';
  x: number;
  y: number;
  width: number;
  height: number;
  _displayH: number;
  src: string;
  thumbnailSrc: string;
  label: string;
  pageCount: number;
  selected = false;
  readonly _pin: string;
  _img: HTMLImageElement | null = null;

  constructor(d: Partial<BoardPdfCard> = {}) {
    this.id = d.id ?? newId('pdf');
    this.x = d.x ?? 0;
    this.y = d.y ?? 0;
    this._displayH = d.height ?? 180;
    this.width = d.width ?? 200;
    this.height = this._displayH + 32;
    this.src = d.src ?? '';
    this.thumbnailSrc = d.thumbnailSrc ?? '';
    this.label = d.label ?? '';
    this.pageCount = d.pageCount ?? 0;
    this._pin = PIN_COLORS[Math.floor(Math.random() * PIN_COLORS.length)];
    if (this.thumbnailSrc) {
      this._loadImg(this.thumbnailSrc);
    } else if (this.src && !this.src.startsWith('attachment:')) {
      this._loadImg(this.src);
    }
  }

  _loadImg(src: string) {
    const img = new Image();
    if (!src.startsWith('data:')) img.crossOrigin = 'anonymous';
    img.onload = () => { this._img = img; };
    img.src = src;
  }

  draw(ctx: CanvasRenderingContext2D) {
    const { x, y, width, height, _displayH, label, selected, pageCount } = this;
    const BORDER = 8;

    ctx.save();
    ctx.shadowColor = 'rgba(0,0,0,0.65)';
    ctx.shadowBlur = selected ? 24 : 16;
    ctx.shadowOffsetX = 3;
    ctx.shadowOffsetY = 6;
    ctx.fillStyle = '#fdf6f0';
    roundRect(ctx, x, y, width, height, 4);
    ctx.fill();
    ctx.restore();

    if (selected) {
      ctx.save();
      ctx.strokeStyle = '#60a5fa';
      ctx.lineWidth = 2;
      ctx.shadowColor = 'rgba(96,165,250,0.6)';
      ctx.shadowBlur = 10;
      roundRect(ctx, x - 2, y - 2, width + 4, height + 4, 6);
      ctx.stroke();
      ctx.restore();
    }

    const ix = x + BORDER, iy = y + BORDER;
    const iw = width - BORDER * 2, ih = _displayH - BORDER;

    ctx.save();
    ctx.beginPath();
    ctx.rect(ix, iy, iw, ih);
    ctx.clip();
    if (this._img) {
      ctx.drawImage(this._img, ix, iy, iw, ih);
    } else {
      // Placeholder: light grey with PDF icon
      ctx.fillStyle = '#e8e0d8';
      ctx.fillRect(ix, iy, iw, ih);
      ctx.font = '36px system-ui';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillStyle = '#b0522a';
      ctx.fillText('📄', ix + iw / 2, iy + ih / 2);
    }
    ctx.restore();

    // PDF badge (top-right corner)
    ctx.save();
    const badgeX = x + width - BORDER - 2;
    const badgeY = y + BORDER + 2;
    const badgeW = 36;
    const badgeH = 16;
    ctx.fillStyle = '#b0522a';
    roundRect(ctx, badgeX - badgeW, badgeY, badgeW, badgeH, 3);
    ctx.fill();
    ctx.fillStyle = '#ffffff';
    ctx.font = 'bold 9px system-ui,sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(pageCount > 0 ? `PDF ${pageCount}p` : 'PDF', badgeX - badgeW / 2, badgeY + badgeH / 2);
    ctx.restore();

    // Label bar
    ctx.save();
    ctx.fillStyle = '#1a1a1a';
    ctx.font = '10px system-ui,sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(clipText(ctx, label || 'Document', width - 16), x + width / 2, y + _displayH + 16);
    ctx.restore();

    drawPin(ctx, x + width / 2, y + 5, this._pin);
  }

  hitTest(wx: number, wy: number): boolean {
    return wx >= this.x && wx <= this.x + this.width && wy >= this.y && wy <= this.y + this.height;
  }

  nearestAnchor(tx: number, ty: number) {
    return nearestSide(this.x, this.y, this.width, this.height, tx, ty);
  }

  pinAnchor() {
    return { x: this.x + this.width / 2, y: this.y + 5 };
  }

  toJSON(): BoardPdfCard {
    return { type: this.type, id: this.id, x: this.x, y: this.y, src: this.src, thumbnailSrc: this.thumbnailSrc || undefined, label: this.label, width: this.width, height: this._displayH, pageCount: this.pageCount };
  }

  static fromJSON(d: BoardPdfCard): PdfCardObj {
    const n = new PdfCardObj(d);
    n.id = d.id;
    return n;
  }
}

export type BoardNodeObj = TextCardObj | ImageCardObj | PdfCardObj;

// ── Connection ────────────────────────────────────────────────────────────────

export class ConnectionObj {
  id: string;
  sourceId: string;
  targetId: string;
  color: string;
  label: string;
  selected = false;

  constructor(d: Partial<BoardConnection> = {}) {
    this.id = d.id ?? newId('c');
    this.sourceId = d.sourceId ?? '';
    this.targetId = d.targetId ?? '';
    this.color = d.color ?? 'red';
    this.label = d.label ?? '';
  }

  draw(ctx: CanvasRenderingContext2D, src: BoardNodeObj, tgt: BoardNodeObj, zoom: number) {
    const s = src.pinAnchor();
    const e = tgt.pinAnchor();

    const hex = CONN_COLORS[this.color] ?? this.color;
    const lw = Math.max(0.5, this.selected ? 2.5 / zoom : 1.5 / zoom);
    const dx = e.x - s.x, dy = e.y - s.y;
    const len = Math.hypot(dx, dy);
    const sag = Math.min(len * 0.18, 55);
    const cx1 = s.x + dx * 0.25, cy1 = s.y + dy * 0.25 + sag;
    const cx2 = s.x + dx * 0.75, cy2 = s.y + dy * 0.75 + sag;

    ctx.save();
    ctx.strokeStyle = hex;
    ctx.lineCap = 'round';

    // Glow (stronger when selected)
    ctx.lineWidth = lw * (this.selected ? 7 : 4);
    ctx.globalAlpha = this.selected ? 0.35 : 0.18;
    ctx.shadowColor = hex;
    ctx.shadowBlur = (this.selected ? 18 : 10) / zoom;
    ctx.beginPath();
    ctx.moveTo(s.x, s.y);
    ctx.bezierCurveTo(cx1, cy1, cx2, cy2, e.x, e.y);
    ctx.stroke();

    // String
    ctx.globalAlpha = 0.9;
    ctx.lineWidth = lw;
    ctx.shadowBlur = 0;
    ctx.beginPath();
    ctx.moveTo(s.x, s.y);
    ctx.bezierCurveTo(cx1, cy1, cx2, cy2, e.x, e.y);
    ctx.stroke();

    // Arrowhead
    const angle = Math.atan2(e.y - cy2, e.x - cx2);
    const al = 9 / zoom;
    ctx.beginPath();
    ctx.moveTo(e.x, e.y);
    ctx.lineTo(e.x - al * Math.cos(angle - 0.38), e.y - al * Math.sin(angle - 0.38));
    ctx.moveTo(e.x, e.y);
    ctx.lineTo(e.x - al * Math.cos(angle + 0.38), e.y - al * Math.sin(angle + 0.38));
    ctx.stroke();

    // Midpoint edit handle (visible when selected or label exists)
    const mx = (s.x + e.x) / 2, my = (s.y + e.y) / 2 + sag * 0.55;
    const fs = Math.max(9, 11 / zoom);

    if (this.label) {
      ctx.font = `${fs}px system-ui,sans-serif`;
      ctx.globalAlpha = 1;
      const tw = ctx.measureText(this.label).width + 10 / zoom;
      const th = fs * 1.6;
      ctx.fillStyle = this.selected ? hex : 'rgba(15,15,19,0.82)';
      roundRect(ctx, mx - tw / 2, my - th / 2, tw, th, 3 / zoom);
      ctx.fill();
      if (this.selected) {
        ctx.strokeStyle = 'rgba(255,255,255,0.6)';
        ctx.lineWidth = 0.8 / zoom;
        roundRect(ctx, mx - tw / 2, my - th / 2, tw, th, 3 / zoom);
        ctx.stroke();
      }
      ctx.fillStyle = this.selected ? '#fff' : hex;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(this.label, mx, my);
    } else if (this.selected) {
      // Show a small dot at midpoint when selected without label
      ctx.globalAlpha = 0.85;
      ctx.fillStyle = hex;
      ctx.beginPath();
      ctx.arc(mx, my, 5 / zoom, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = 'rgba(255,255,255,0.7)';
      ctx.lineWidth = 1 / zoom;
      ctx.beginPath();
      ctx.arc(mx, my, 5 / zoom, 0, Math.PI * 2);
      ctx.stroke();
    }

    ctx.restore();
  }

  midPoint(src: BoardNodeObj, tgt: BoardNodeObj) {
    const s = src.pinAnchor();
    const e = tgt.pinAnchor();
    const sag = Math.min(Math.hypot(e.x - s.x, e.y - s.y) * 0.18, 55);
    return { x: (s.x + e.x) / 2, y: (s.y + e.y) / 2 + sag * 0.55 };
  }

  hitTest(wx: number, wy: number, src: BoardNodeObj, tgt: BoardNodeObj): boolean {
    const mp = this.midPoint(src, tgt);
    return Math.hypot(wx - mp.x, wy - mp.y) < 24;
  }

  toJSON(): BoardConnection {
    return { id: this.id, sourceId: this.sourceId, targetId: this.targetId, color: this.color, label: this.label };
  }

  static fromJSON(d: BoardConnection): ConnectionObj {
    const c = new ConnectionObj(d);
    c.id = d.id;
    return c;
  }
}

// ── Scene ────────────────────────────────────────────────────────────────────

export class Scene {
  nodes: BoardNodeObj[] = [];
  connections: ConnectionObj[] = [];
  private _colorIdx = 0;

  add(node: BoardNodeObj) { this.nodes.push(node); return node; }

  remove(node: BoardNodeObj) {
    this.nodes = this.nodes.filter((n) => n !== node);
    this.connections = this.connections.filter((c) => c.sourceId !== node.id && c.targetId !== node.id);
  }

  duplicate(node: BoardNodeObj): BoardNodeObj {
    const { id: _drop, ...rest } = node.toJSON();
    let copy: BoardNodeObj;
    if (node.type === 'image') {
      copy = new ImageCardObj({ ...rest, x: node.x + 24, y: node.y + 24 } as Partial<BoardImageCard>);
      if (node._img) (copy as ImageCardObj)._img = node._img;
    } else if (node.type === 'pdf') {
      copy = new PdfCardObj({ ...rest, x: node.x + 24, y: node.y + 24 } as Partial<BoardPdfCard>);
      if (node._img) (copy as PdfCardObj)._img = node._img;
    } else {
      copy = new TextCardObj({ ...rest, x: node.x + 24, y: node.y + 24 } as Partial<BoardTextCard>);
    }
    this.add(copy);
    return copy;
  }

  bringToFront(node: BoardNodeObj) {
    this.nodes = [...this.nodes.filter((n) => n !== node), node];
  }

  connect(src: BoardNodeObj, tgt: BoardNodeObj, color?: string, label = ''): ConnectionObj {
    const exists = this.connections.find(
      (c) => (c.sourceId === src.id && c.targetId === tgt.id) ||
              (c.sourceId === tgt.id && c.targetId === src.id),
    );
    if (exists) return exists;
    const col = color ?? CONN_COLOR_KEYS[this._colorIdx++ % CONN_COLOR_KEYS.length];
    const conn = new ConnectionObj({ sourceId: src.id, targetId: tgt.id, color: col, label });
    this.connections.push(conn);
    return conn;
  }

  removeConnection(conn: ConnectionObj) {
    this.connections = this.connections.filter((c) => c !== conn);
  }

  hitTestNode(wx: number, wy: number): BoardNodeObj | null {
    for (let i = this.nodes.length - 1; i >= 0; i--) {
      if (this.nodes[i].hitTest(wx, wy)) return this.nodes[i];
    }
    return null;
  }

  hitTestConnection(wx: number, wy: number): ConnectionObj | null {
    for (let i = this.connections.length - 1; i >= 0; i--) {
      const c = this.connections[i];
      const src = this.getNode(c.sourceId);
      const tgt = this.getNode(c.targetId);
      if (src && tgt && c.hitTest(wx, wy, src, tgt)) return c;
    }
    return null;
  }

  clearSelection() { this.nodes.forEach((n) => (n.selected = false)); }
  getSelected() { return this.nodes.filter((n) => n.selected); }

  selectInRect(x1: number, y1: number, x2: number, y2: number) {
    const minX = Math.min(x1, x2), maxX = Math.max(x1, x2);
    const minY = Math.min(y1, y2), maxY = Math.max(y1, y2);
    for (const n of this.nodes) {
      const cx = n.x + n.width / 2, cy = n.y + n.height / 2;
      if (cx >= minX && cx <= maxX && cy >= minY && cy <= maxY) n.selected = true;
    }
  }

  deleteSelected() { for (const n of this.getSelected()) this.remove(n); }

  getNode(id: string): BoardNodeObj | undefined { return this.nodes.find((n) => n.id === id); }

  draw(ctx: CanvasRenderingContext2D, zoom: number) {
    for (const n of this.nodes) n.draw(ctx);
    for (const c of this.connections) {
      const src = this.getNode(c.sourceId);
      const tgt = this.getNode(c.targetId);
      if (src && tgt) c.draw(ctx, src, tgt, zoom);
    }
  }

  toData(): Pick<BoardData, 'cards' | 'connections'> {
    return {
      cards: this.nodes.map((n) => n.toJSON() as BoardNode),
      connections: this.connections.map((c) => c.toJSON()),
    };
  }

  loadData(data: Pick<BoardData, 'cards' | 'connections'>) {
    this.nodes = [];
    this.connections = [];
    for (const d of data.cards ?? []) {
      if (d.type === 'image') this.nodes.push(ImageCardObj.fromJSON(d as BoardImageCard));
      else if (d.type === 'pdf') this.nodes.push(PdfCardObj.fromJSON(d as BoardPdfCard));
      else this.nodes.push(TextCardObj.fromJSON(d as BoardTextCard));
    }
    for (const d of data.connections ?? []) {
      this.connections.push(ConnectionObj.fromJSON(d));
    }
  }

  // Patches display URLs for image/pdf cards whose stored src is an opaque reference (e.g. "attachment:ID").
  // Call after loadData() and whenever resolved URLs become available.
  resolveImages(displaySrcMap: Record<string, string>) {
    for (const n of this.nodes) {
      if (n.type === 'image' || n.type === 'pdf') {
        const card = n as ImageCardObj | PdfCardObj;
        const resolved = displaySrcMap[card.src];
        if (resolved) {
          card._loadImg(resolved);
          if (n.type === 'pdf') {
            (card as PdfCardObj).thumbnailSrc = resolved;
          }
        }
      }
    }
  }
}

// ── Engine ────────────────────────────────────────────────────────────────────

export class Engine {
  canvas: HTMLCanvasElement;
  ctx: CanvasRenderingContext2D;
  dpr: number;
  width = 0;
  height = 0;
  camera = { x: 0, y: 0, zoom: 1 };
  lightMode = false;
  background: BoardBackground | null = null;

  onDraw: ((ctx: CanvasRenderingContext2D, cam: Engine['camera']) => void) | null = null;
  onDrawUI: ((ctx: CanvasRenderingContext2D, cam: Engine['camera']) => void) | null = null;
  onZoomChange: ((z: number) => void) | null = null;

  private _raf: number | null = null;
  private _ro: ResizeObserver;
  private _textures = new Map<string, CanvasPattern>();

  // ── Smooth camera animation ───────────────────────────────────────────────
  // Wheel/pinch zoom: spring physics that keeps the focal point fixed in world space
  private _tz = 1;          // target zoom
  private _tzv = 0;         // zoom spring velocity
  private _zfx = 0;         // focal world-x (stays fixed during zoom animation)
  private _zfy = 0;         // focal world-y
  private _zsx = 0;         // screen-x of focal point
  private _zsy = 0;         // screen-y of focal point
  // Fit-to-content: animates zoom + pan simultaneously
  private _flyTo: { z: number; x: number; y: number } | null = null;
  private _flyXV = 0;
  private _flyYV = 0;

  constructor(canvas: HTMLCanvasElement, lightMode = false) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d')!;
    this.dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.lightMode = lightMode;
    this._ro = new ResizeObserver(() => this._resize());
    this._ro.observe(canvas.parentElement!);
    this._resize();
    this._loop();
  }

  private _resize() {
    const el = this.canvas.parentElement!;
    const w = el.clientWidth;
    const h = el.clientHeight;
    const bw = Math.round(w * this.dpr);
    const bh = Math.round(h * this.dpr);

    this.width = w;
    this.height = h;
    this.canvas.style.width = w + 'px';
    this.canvas.style.height = h + 'px';

    // Assigning canvas.width/height clears the backing store to transparent,
    // which produces a one-frame blank flash before the next RAF redraw. Only
    // touch them when the pixel dimensions actually changed, then redraw
    // synchronously so the resized canvas is never shown empty.
    if (bw !== this.canvas.width || bh !== this.canvas.height) {
      this.canvas.width = bw;
      this.canvas.height = bh;
      this._draw();
    }
  }

  private _loop() {
    this._raf = requestAnimationFrame(() => this._loop());
    this._animCamera();
    this._draw();
  }

  private _animCamera() {
    const CAM = this.camera;
    const DT = 1 / 60;

    // Fly-to animation (fitToContent): springs for zoom + pan simultaneously
    if (this._flyTo) {
      const { z, x, y } = this._flyTo;
      const K = 140, D = 23;

      const dz = z - CAM.zoom;
      this._tzv += (K * dz - D * this._tzv) * DT;
      CAM.zoom = Math.max(0.08, Math.min(6, CAM.zoom + this._tzv * DT));

      const dx = x - CAM.x;
      this._flyXV += (K * dx - D * this._flyXV) * DT;
      CAM.x += this._flyXV * DT;

      const dy = y - CAM.y;
      this._flyYV += (K * dy - D * this._flyYV) * DT;
      CAM.y += this._flyYV * DT;

      const settled = Math.abs(dz) < 4e-4 && Math.abs(dx) < 0.4 && Math.abs(dy) < 0.4
        && Math.abs(this._tzv) < 4e-4 && Math.abs(this._flyXV) < 0.4 && Math.abs(this._flyYV) < 0.4;
      if (settled) {
        CAM.zoom = z; CAM.x = x; CAM.y = y;
        this._flyTo = null; this._tzv = 0; this._flyXV = 0; this._flyYV = 0;
        // Sync wheel-zoom target so next wheel event starts from correct baseline
        this._tz = z;
      }
      this.onZoomChange?.(CAM.zoom);
      return;
    }

    // Wheel/pinch zoom: spring that keeps world focal point fixed in screen space
    const dz = this._tz - CAM.zoom;
    if (Math.abs(dz) < 4e-5 && Math.abs(this._tzv) < 4e-5) {
      if (CAM.zoom !== this._tz) {
        CAM.zoom = this._tz;
        CAM.x = this._zsx - this._zfx * this._tz;
        CAM.y = this._zsy - this._zfy * this._tz;
        this.onZoomChange?.(this._tz);
      }
      this._tzv = 0;
      return;
    }

    // Spring: k=100, d=19 → ζ ≈ 0.95 (near-critically damped, quick & crisp)
    this._tzv += (100 * dz - 19 * this._tzv) * DT;
    const newZoom = Math.max(0.08, Math.min(6, CAM.zoom + this._tzv * DT));
    CAM.zoom = newZoom;
    CAM.x = this._zsx - this._zfx * newZoom;
    CAM.y = this._zsy - this._zfy * newZoom;
    this.onZoomChange?.(newZoom);
  }

  private _draw() {
    const { ctx, dpr, width, height, camera } = this;
    ctx.clearRect(0, 0, width * dpr, height * dpr);
    ctx.save();
    ctx.scale(dpr, dpr);

    this._drawBackground();

    ctx.save();
    ctx.translate(camera.x, camera.y);
    ctx.scale(camera.zoom, camera.zoom);
    if (this.onDraw) this.onDraw(ctx, camera);
    ctx.restore();

    if (this.onDrawUI) this.onDrawUI(ctx, camera);
    ctx.restore();
  }

  private _drawBackground() {
    const { ctx, width, height, camera, lightMode, background } = this;

    if (!background) {
      // Default: solid colour + dot-grid
      ctx.fillStyle = lightMode ? BG_LIGHT : BG_DARK;
      ctx.fillRect(0, 0, width, height);
      this._drawGrid();
      return;
    }

    if (background.type === 'color') {
      ctx.fillStyle = background.color;
      ctx.fillRect(0, 0, width, height);
      this._drawGrid();
      return;
    }

    // Texture: fill with panning + zooming tiled pattern
    const pat = this._getTexture(background.id);
    if (!pat) {
      ctx.fillStyle = lightMode ? BG_LIGHT : BG_DARK;
      ctx.fillRect(0, 0, width, height);
      return;
    }
    // Scale the pattern with camera.zoom so the texture feels physical — zooming
    // in magnifies the grain, zooming out shows more of it.
    pat.setTransform(new DOMMatrix([camera.zoom, 0, 0, camera.zoom, camera.x, camera.y]));
    ctx.fillStyle = pat;
    ctx.fillRect(0, 0, width, height);
  }

  private _drawGrid() {
    const { ctx, width, height, camera, lightMode, background } = this;
    const z = camera.zoom;
    const size = GRID * z;
    const ox = ((camera.x % size) + size) % size;
    const oy = ((camera.y % size) + size) % size;

    // For custom colour backgrounds, tint the grid to match
    const isMidtone = background?.type === 'color';
    const gridCol = isMidtone ? 'rgba(128,128,128,0.18)' : (lightMode ? GRID_LIGHT : GRID_DARK);
    const gridAccent = isMidtone ? 'rgba(128,128,128,0.32)' : (lightMode ? GRID_ACCENT_LIGHT : GRID_ACCENT_DARK);

    ctx.beginPath();
    ctx.strokeStyle = gridCol;
    ctx.lineWidth = 1;
    for (let x = ox - size; x < width + size; x += size) { ctx.moveTo(x, 0); ctx.lineTo(x, height); }
    for (let y = oy - size; y < height + size; y += size) { ctx.moveTo(0, y); ctx.lineTo(width, y); }
    ctx.stroke();

    const major = size * 4;
    const mox = ((camera.x % major) + major) % major;
    const moy = ((camera.y % major) + major) % major;
    ctx.beginPath();
    ctx.strokeStyle = gridAccent;
    for (let x = mox - major; x < width + major; x += major) { ctx.moveTo(x, 0); ctx.lineTo(x, height); }
    for (let y = moy - major; y < height + major; y += major) { ctx.moveTo(0, y); ctx.lineTo(width, y); }
    ctx.stroke();
  }

  private _getTexture(id: string): CanvasPattern | null {
    if (this._textures.has(id)) return this._textures.get(id)!;
    const off = this._buildTexture(id as BoardBackgroundTextureId);
    if (!off) return null;
    const pat = this.ctx.createPattern(off, 'repeat');
    if (pat) this._textures.set(id, pat);
    return pat;
  }

  private _buildTexture(id: BoardBackgroundTextureId): HTMLCanvasElement | null {
    const sz = 120;
    const off = document.createElement('canvas');
    off.width = sz; off.height = sz;
    const c = off.getContext('2d')!;

    switch (id) {
      case 'cork': {
        const rng = seedRng(101);
        c.fillStyle = '#c49a6c';
        c.fillRect(0, 0, sz, sz);
        for (let i = 0; i < 380; i++) {
          const x = rng() * sz, y = rng() * sz;
          const rx = rng() * 2.5 + 0.4, ry = rng() * 1.4 + 0.4;
          c.save(); c.translate(x, y); c.rotate(rng() * Math.PI);
          c.beginPath(); c.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
          const d = rng() * 45 | 0;
          c.fillStyle = `rgba(${80 - d},${48 - d},18,${0.3 + rng() * 0.4})`; c.fill();
          c.restore();
        }
        for (let i = 0; i < 90; i++) {
          const x = rng() * sz, y = rng() * sz;
          c.beginPath(); c.arc(x, y, rng() * 1.2 + 0.3, 0, Math.PI * 2);
          c.fillStyle = `rgba(222,178,108,${0.18 + rng() * 0.22})`; c.fill();
        }
        break;
      }
      case 'cork-dark': {
        const rng = seedRng(202);
        c.fillStyle = '#8b5e3c';
        c.fillRect(0, 0, sz, sz);
        for (let i = 0; i < 380; i++) {
          const x = rng() * sz, y = rng() * sz;
          const rx = rng() * 2.5 + 0.4, ry = rng() * 1.4 + 0.4;
          c.save(); c.translate(x, y); c.rotate(rng() * Math.PI);
          c.beginPath(); c.ellipse(0, 0, rx, ry, 0, 0, Math.PI * 2);
          const d = rng() * 30 | 0;
          c.fillStyle = `rgba(${50 - d},${28 - d},8,${0.4 + rng() * 0.4})`; c.fill();
          c.restore();
        }
        for (let i = 0; i < 90; i++) {
          const x = rng() * sz, y = rng() * sz;
          c.beginPath(); c.arc(x, y, rng() * 1.2 + 0.3, 0, Math.PI * 2);
          c.fillStyle = `rgba(160,108,68,${0.18 + rng() * 0.22})`; c.fill();
        }
        break;
      }
      case 'chalkboard': {
        const rng = seedRng(303);
        c.fillStyle = '#263d2b';
        c.fillRect(0, 0, sz, sz);
        for (let i = 0; i < 240; i++) {
          c.beginPath(); c.arc(rng() * sz, rng() * sz, rng() * 0.9 + 0.2, 0, Math.PI * 2);
          c.fillStyle = `rgba(240,240,234,${0.03 + rng() * 0.11})`; c.fill();
        }
        for (let i = 0; i < 7; i++) {
          const x1 = rng() * sz, y1 = rng() * sz;
          c.beginPath(); c.moveTo(x1, y1);
          c.lineTo(x1 + (rng() - 0.5) * 55, y1 + (rng() - 0.5) * 18);
          c.strokeStyle = `rgba(240,240,234,${0.04 + rng() * 0.06})`;
          c.lineWidth = rng() * 2 + 0.4; c.stroke();
        }
        break;
      }
      case 'aged-paper': {
        const rng = seedRng(404);
        c.fillStyle = '#f2e8d0';
        c.fillRect(0, 0, sz, sz);
        c.strokeStyle = 'rgba(176,148,96,0.12)';
        c.lineWidth = 0.6;
        for (let y = 20; y < sz; y += 20) {
          c.beginPath(); c.moveTo(0, y); c.lineTo(sz, y); c.stroke();
        }
        for (let i = 0; i < 90; i++) {
          const x = rng() * sz, y = rng() * sz;
          c.beginPath(); c.arc(x, y, rng() * 3 + 0.5, 0, Math.PI * 2);
          const hi = rng() > 0.5;
          c.fillStyle = hi
            ? `rgba(242,212,148,${0.05 + rng() * 0.08})`
            : `rgba(155,128,78,${0.04 + rng() * 0.07})`;
          c.fill();
        }
        break;
      }
      case 'blueprint': {
        c.fillStyle = '#0c1e3a';
        c.fillRect(0, 0, sz, sz);
        const step = 40;
        c.strokeStyle = 'rgba(100,162,255,0.22)';
        c.lineWidth = 0.6;
        c.beginPath();
        for (let x = 0; x <= sz; x += step) { c.moveTo(x, 0); c.lineTo(x, sz); }
        for (let y = 0; y <= sz; y += step) { c.moveTo(0, y); c.lineTo(sz, y); }
        c.stroke();
        const cs = 3.5;
        c.strokeStyle = 'rgba(155,205,255,0.38)';
        c.lineWidth = 0.7;
        for (let x = 0; x <= sz; x += step) {
          for (let y = 0; y <= sz; y += step) {
            c.beginPath();
            c.moveTo(x - cs, y); c.lineTo(x + cs, y);
            c.moveTo(x, y - cs); c.lineTo(x, y + cs);
            c.stroke();
          }
        }
        break;
      }
      default:
        return null;
    }
    return off;
  }

  screenToWorld(sx: number, sy: number) {
    return { x: (sx - this.camera.x) / this.camera.zoom, y: (sy - this.camera.y) / this.camera.zoom };
  }

  worldToScreen(wx: number, wy: number) {
    return { x: wx * this.camera.zoom + this.camera.x, y: wy * this.camera.zoom + this.camera.y };
  }

  zoomAt(sx: number, sy: number, factor: number) {
    // Target zoom is based on the CURRENT target (not rendered) zoom so rapid scroll
    // events accumulate correctly even while the spring is still moving.
    const newTarget = Math.max(0.08, Math.min(6, this._tz * factor));
    // World point under cursor computed from the RENDERED camera so it matches
    // what the user actually sees.
    const wx = (sx - this.camera.x) / this.camera.zoom;
    const wy = (sy - this.camera.y) / this.camera.zoom;
    this._tz = newTarget;
    this._zfx = wx; this._zfy = wy;
    this._zsx = sx; this._zsy = sy;
    // Cancel any fly-to animation so wheel zoom takes over
    this._flyTo = null; this._flyXV = 0; this._flyYV = 0;
  }

  // Call when the canvas is panned by (dx, dy) so the zoom focal point stays
  // coherent with the shifted camera position.
  shiftZoomFocus(dx: number, dy: number) {
    this._zsx += dx;
    this._zsy += dy;
  }

  fitToContent(nodes: BoardNodeObj[], padding = 80, immediate = false) {
    if (!nodes.length) return;
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const n of nodes) {
      minX = Math.min(minX, n.x); minY = Math.min(minY, n.y);
      maxX = Math.max(maxX, n.x + n.width); maxY = Math.max(maxY, n.y + n.height);
    }
    const cw = this.width - padding * 2, ch = this.height - padding * 2;
    const ww = Math.max(maxX - minX, 1), wh = Math.max(maxY - minY, 1);
    const z = Math.min(cw / ww, ch / wh, 2);
    const x = this.width / 2 - (minX + ww / 2) * z;
    const y = this.height / 2 - (minY + wh / 2) * z;

    if (immediate) {
      this.camera.zoom = z; this.camera.x = x; this.camera.y = y;
      this._tz = z; this._tzv = 0;
      this._flyTo = null; this._flyXV = 0; this._flyYV = 0;
      this.onZoomChange?.(z);
      return;
    }

    // Animated fly-to: spring all three camera values toward the fit view
    this._flyTo = { z, x, y };
    this._flyXV = 0; this._flyYV = 0; this._tzv = 0;
    this._tz = z;  // keep in sync so next wheel event starts from correct baseline
  }

  destroy() {
    if (this._raf) cancelAnimationFrame(this._raf);
    this._ro.disconnect();
  }
}

// ── Interaction ───────────────────────────────────────────────────────────────

export type BoardTool = 'select' | 'pan' | 'card' | 'image' | 'pdf' | 'connect';

export class Interaction {
  private canvas: HTMLCanvasElement;
  private engine: Engine;
  private scene: Scene;

  tool: BoardTool = 'select';
  connColor = 'red';

  private _state: 'idle' | 'panning' | 'dragging' | 'box-select' | 'connecting' = 'idle';
  private _last = { x: 0, y: 0 };
  private _dragItems: Array<{ node: BoardNodeObj; ox: number; oy: number }> = [];
  private _boxStart: { x: number; y: number } | null = null;
  private _boxEnd: { x: number; y: number } | null = null;
  private _connSrc: BoardNodeObj | null = null;
  private _connPreview: { x: number; y: number } | null = null;
  private _spaceDown = false;

  onAddCard: ((x: number, y: number) => void) | null = null;
  onAddImage: ((x: number, y: number) => void) | null = null;
  onAddPdf: ((x: number, y: number) => void) | null = null;
  onContextMenu: ((cx: number, cy: number, hit: BoardNodeObj | null, conn: ConnectionObj | null, wp: { x: number; y: number }) => void) | null = null;
  onEditNode: ((node: BoardNodeObj) => void) | null = null;
  onEditConnection: ((conn: ConnectionObj) => void) | null = null;
  onToolChange: ((tool: BoardTool) => void) | null = null;
  onChanged: (() => void) | null = null;

  constructor(canvas: HTMLCanvasElement, engine: Engine, scene: Scene) {
    this.canvas = canvas;
    this.engine = engine;
    this.scene = scene;
    this._bind();
  }

  private _bind() {
    const c = this.canvas;
    c.addEventListener('mousedown', (e) => this._down(e));
    c.addEventListener('mousemove', (e) => this._move(e));
    c.addEventListener('mouseup', (e) => this._up(e));
    c.addEventListener('wheel', (e) => this._wheel(e), { passive: false });
    c.addEventListener('contextmenu', (e) => this._ctx(e));
    c.addEventListener('dblclick', (e) => this._dbl(e));
    window.addEventListener('keydown', (e) => this._kdown(e));
    window.addEventListener('keyup', (e) => this._kup(e));
  }

  private _pos(e: MouseEvent) {
    const r = this.canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private _down(e: MouseEvent) {
    if (e.button === 1 || (e.button === 0 && this._spaceDown) || (e.button === 0 && this.tool === 'pan')) {
      this._state = 'panning';
      this._last = this._pos(e);
      this._setCursor('grabbing');
      e.preventDefault();
      return;
    }
    if (e.button !== 0) return;

    const sp = this._pos(e);
    const wp = this.engine.screenToWorld(sp.x, sp.y);

    if (this.tool === 'connect') {
      const hit = this.scene.hitTestNode(wp.x, wp.y);
      if (hit) {
        if (!this._connSrc) {
          this._connSrc = hit;
          this._connPreview = wp;
          this._state = 'connecting';
        } else if (hit !== this._connSrc) {
          this.scene.connect(this._connSrc, hit, this.connColor);
          this._finishConnect();
          this.onChanged?.();
        }
      } else {
        this._finishConnect();
      }
      return;
    }

    const hit = this.scene.hitTestNode(wp.x, wp.y);
    if (hit) {
      this.scene.bringToFront(hit);
      if (!e.shiftKey && !hit.selected) this.scene.clearSelection();
      hit.selected = true;
      this._dragItems = this.scene.getSelected().map((n) => ({ node: n, ox: wp.x - n.x, oy: wp.y - n.y }));
      this._state = 'dragging';
      this._setCursor('grabbing');
      return;
    }

    if (this.tool === 'card') {
      this.onAddCard?.(wp.x - 120, wp.y - 80);
      this.setTool('select');
      return;
    }
    if (this.tool === 'image') {
      this.onAddImage?.(wp.x - 100, wp.y - 116);
      this.setTool('select');
      return;
    }
    if (this.tool === 'pdf') {
      this.onAddPdf?.(wp.x - 100, wp.y - 116);
      this.setTool('select');
      return;
    }

    this.scene.clearSelection();
    this._boxStart = sp;
    this._boxEnd = { ...sp };
    this._state = 'box-select';
  }

  private _move(e: MouseEvent) {
    const sp = this._pos(e);
    const wp = this.engine.screenToWorld(sp.x, sp.y);

    if (this._state === 'panning') {
      const dx = sp.x - this._last.x;
      const dy = sp.y - this._last.y;
      this.engine.camera.x += dx;
      this.engine.camera.y += dy;
      // Keep the zoom focal point in sync so any ongoing zoom animation
      // stays anchored to the right world position after the pan.
      this.engine.shiftZoomFocus(dx, dy);
      this._last = sp;
      return;
    }
    if (this._state === 'dragging') {
      for (const { node, ox, oy } of this._dragItems) { node.x = wp.x - ox; node.y = wp.y - oy; }
      return;
    }
    if (this._state === 'box-select') { this._boxEnd = sp; return; }
    if (this._state === 'connecting') { this._connPreview = wp; return; }

    const hit = this.scene.hitTestNode(wp.x, wp.y);
    if (this._spaceDown || this.tool === 'pan') this._setCursor('grab');
    else if (this.tool === 'connect') this._setCursor(hit ? 'crosshair' : 'default');
    else if (this.tool === 'card' || this.tool === 'image' || this.tool === 'pdf') this._setCursor('crosshair');
    else this._setCursor(hit ? 'grab' : 'default');
  }

  private _up(e: MouseEvent) {
    if (this._state === 'box-select' && this._boxStart && this._boxEnd) {
      const w1 = this.engine.screenToWorld(Math.min(this._boxStart.x, this._boxEnd.x), Math.min(this._boxStart.y, this._boxEnd.y));
      const w2 = this.engine.screenToWorld(Math.max(this._boxStart.x, this._boxEnd.x), Math.max(this._boxStart.y, this._boxEnd.y));
      this.scene.selectInRect(w1.x, w1.y, w2.x, w2.y);
    }
    if (this._state === 'dragging') this.onChanged?.();
    if (this._state !== 'connecting') this._state = 'idle';
    this._dragItems = [];
    this._boxStart = null;
    this._boxEnd = null;
    if (this.tool === 'pan') this._setCursor('grab');
    else if (!this._spaceDown && this._state !== 'connecting') this._setCursor('default');
    void e;
  }

  private _wheel(e: WheelEvent) {
    e.preventDefault();
    const sp = this._pos(e);

    let factor: number;
    if (e.ctrlKey) {
      // Trackpad pinch-to-zoom: browser reports as ctrl+wheel with pixel deltas.
      // Use exponential mapping for a natural, continuous feel.
      factor = Math.exp(-e.deltaY / 120);
    } else if (e.deltaMode === 0 /* DOM_DELTA_PIXEL */) {
      // High-res trackpad two-finger scroll used as zoom (non-pinch).
      factor = Math.exp(-e.deltaY / 600);
    } else {
      // Standard mouse wheel: discrete line/page steps → snappy fixed factor.
      factor = e.deltaY < 0 ? 1.14 : 1 / 1.14;
    }

    this.engine.zoomAt(sp.x, sp.y, factor);
  }

  private _ctx(e: MouseEvent) {
    e.preventDefault();
    const sp = this._pos(e);
    const wp = this.engine.screenToWorld(sp.x, sp.y);
    const hit = this.scene.hitTestNode(wp.x, wp.y);
    const conn = hit ? null : this.scene.hitTestConnection(wp.x, wp.y);
    this.onContextMenu?.(e.clientX, e.clientY, hit, conn, wp);
  }

  private _dbl(e: MouseEvent) {
    const sp = this._pos(e);
    const wp = this.engine.screenToWorld(sp.x, sp.y);
    const hit = this.scene.hitTestNode(wp.x, wp.y);
    if (hit) {
      this.onEditNode?.(hit);
      return;
    }
    const conn = this.scene.hitTestConnection(wp.x, wp.y);
    if (conn) this.onEditConnection?.(conn);
  }

  private _kdown(e: KeyboardEvent) {
    if ((e.target as HTMLElement).matches('input,textarea,[contenteditable]')) return;
    if (e.code === 'Space') { this._spaceDown = true; this._setCursor('grab'); e.preventDefault(); }
    if (e.code === 'Delete' || e.code === 'Backspace') {
      const had = this.scene.getSelected().length > 0;
      this.scene.deleteSelected();
      if (had) this.onChanged?.();
    }
    if (e.code === 'KeyF') this.engine.fitToContent(this.scene.nodes);
    if (e.code === 'Escape') { this._finishConnect(); this.scene.clearSelection(); this.setTool('select'); }
    if (e.code === 'KeyV') this.setTool('select');
    if (e.code === 'KeyH' && !e.ctrlKey && !e.metaKey) this.setTool('pan');
    if (e.code === 'KeyC' && !e.ctrlKey && !e.metaKey) this.setTool('card');
    if (e.code === 'KeyI' && !e.ctrlKey && !e.metaKey) this.setTool('image');
    if (e.code === 'KeyP' && !e.ctrlKey && !e.metaKey) this.setTool('pdf');
    if (e.code === 'KeyE' && !e.ctrlKey && !e.metaKey) this.setTool('connect');
  }

  private _kup(e: KeyboardEvent) {
    if (e.code === 'Space') { this._spaceDown = false; this._setCursor('default'); }
  }

  private _finishConnect() {
    this._connSrc = null;
    this._connPreview = null;
    this._state = 'idle';
  }

  private _setCursor(c: string) { this.canvas.style.cursor = c; }

  setTool(tool: BoardTool) {
    this.tool = tool;
    if (tool !== 'connect') this._finishConnect();
    this.onToolChange?.(tool);
  }

  drawOverlay(ctx: CanvasRenderingContext2D) {
    if (this._state === 'box-select' && this._boxStart && this._boxEnd) {
      const x = Math.min(this._boxStart.x, this._boxEnd.x);
      const y = Math.min(this._boxStart.y, this._boxEnd.y);
      const w = Math.abs(this._boxEnd.x - this._boxStart.x);
      const h = Math.abs(this._boxEnd.y - this._boxStart.y);
      ctx.save();
      ctx.strokeStyle = '#60a5fa';
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 3]);
      ctx.strokeRect(x, y, w, h);
      ctx.fillStyle = 'rgba(96,165,250,0.07)';
      ctx.fillRect(x, y, w, h);
      ctx.restore();
    }

    if (this._state === 'connecting' && this._connSrc && this._connPreview) {
      const s = this._connSrc.pinAnchor();
      const ss = this.engine.worldToScreen(s.x, s.y);
      const se = this.engine.worldToScreen(this._connPreview.x, this._connPreview.y);
      ctx.save();
      ctx.strokeStyle = CONN_COLORS[this.connColor] ?? '#ef4444';
      ctx.lineWidth = 1.5;
      ctx.setLineDash([6, 4]);
      ctx.globalAlpha = 0.75;
      ctx.beginPath();
      ctx.moveTo(ss.x, ss.y);
      ctx.lineTo(se.x, se.y);
      ctx.stroke();
      ctx.restore();
    }
  }
}
