/**
 * The bands a node is drawn from.
 *
 * A node has a vertical structure that `@mindmapvault/mindmap-core` already
 * knows about — a date badge above the box, then the meta strip, the tags, the
 * picture, the text body, and one footer strip per link. These components are
 * that structure, one per band.
 *
 * Splitting by band rather than by field is deliberate. A component per
 * decoration would be twenty of them, each working out for itself where the
 * band it lives in starts; a component per band takes the geometry it is given
 * and every offset inside it is relative to that. Each one receives the `box`
 * the layout produced, the `parts` it was measured from, and the `geom`
 * derived from both — never the raw numbers to redo the arithmetic with.
 */

import type { JSX } from 'react';
import type { LayoutEntry, NodeGeometry, NodeParts } from '@mindmapvault/mindmap-core';
import {
  CHECKBOX_SIZE,
  ICON_SIZE,
  LINK_STRIP_H,
  NODE_LINE_H,
  NODE_PAD_X,
  PROGRESS_PIE_SIZE,
  TAG_STRIP_H,
} from '../MindMapConstants';
import DynamicLucideIcon from '../DynamicLucideIcon.tsx';
import type { MindMapTreeNode, UrlEntry } from '../../types';
import type { UserLabel } from '../../hooks/useUserLabels';
import { openExternalUrl } from '../../utils/openExternal';

export type NodeBox = LayoutEntry<MindMapTreeNode>;

/** How this node is painted. Worked out once, in the editor, and passed down. */
export interface NodeVisual {
  /** The node's own explicit colour, or null. Nothing is inherited. */
  ownColor: string | null;
  fillColor: string;
  strokeColor: string;
  textColor: string;
  fontSize: number;
  fontWeight: 'bold' | 'normal';
}

interface BandProps {
  box: NodeBox;
  geom: NodeGeometry;
  parts: NodeParts;
  visual: NodeVisual;
}

/** The hairline that separates one band from the next. */
function BandDivider({
  box,
  y,
  ownColor,
  inset = 6,
}: {
  box: NodeBox;
  y: number;
  ownColor: string | null;
  inset?: number;
}): JSX.Element {
  return (
    <line
      x1={box.x + inset}
      y1={y}
      x2={box.x + box.w - inset}
      y2={y}
      stroke={ownColor ? '#ffffff22' : 'var(--mm-node-stroke)'}
      strokeWidth={0.5}
    />
  );
}

// ── Above the box ───────────────────────────────────────────────

const formatDate = (value: string): string => {
  const d = new Date(value);
  const pad = (n: number) => n.toString().padStart(2, '0');
  return `${pad(d.getDate())}.${pad(d.getMonth() + 1)}.${String(d.getFullYear()).slice(2)} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
};

/**
 * Start and end dates, drawn *above* the node rather than inside it — which is
 * what `parts.visualTopExtra` reserves room for, so the badge never overlaps
 * whatever sits above.
 */
export function DateBadge({ box, node }: { box: NodeBox; node: MindMapTreeNode }): JSX.Element {
  const startLabel = node.startDate ? formatDate(node.startDate) : '–';
  const endLabel = node.endDate ? formatDate(node.endDate) : '–';
  return (
    <g className="mm-date-badge">
      <svg x={box.x + box.w / 2 - 52} y={box.y - 32} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" strokeWidth={2}>
        <rect x="3" y="4" width="18" height="18" rx="2" ry="2" />
        <line x1="16" y1="2" x2="16" y2="6" />
        <line x1="8" y1="2" x2="8" y2="6" />
        <line x1="3" y1="10" x2="21" y2="10" />
      </svg>
      <text x={box.x + box.w / 2 - 33} y={box.y - 25} fontSize={11} fill="var(--accent)" fontWeight="500">{startLabel}</text>
      <text x={box.x + box.w / 2 - 33} y={box.y - 12} fontSize={11} fill="var(--mm-statusbar-text)">{endLabel}</text>
    </g>
  );
}

// ── Meta strip: what the node carries that is not its text ──────

/** The paperclip pill, with a count when there is more than one file. */
export function AttachmentIndicator({
  x,
  y,
  count,
  ownColor,
}: {
  x: number;
  y: number;
  count: number;
  ownColor: string | null;
}): JSX.Element {
  const indicatorWidth = count > 1 ? 28 : 18;
  const iconX = x - indicatorWidth / 2 + 5;
  const textX = x + indicatorWidth / 2 - 6;
  const stroke = ownColor ? '#ffffffcc' : '#cbd5e1';
  const fill = ownColor ? 'rgba(15, 23, 42, 0.34)' : 'rgba(15, 23, 42, 0.82)';
  return (
    <g className="mm-attachment-indicator">
      <rect x={x - indicatorWidth / 2} y={y - 7} width={indicatorWidth} height={14} rx={7} fill={fill} stroke={stroke} strokeWidth={1} />
      <path
        d={`M ${iconX} ${y + 1.5} l 4.1 -4.1 a 2.2 2.2 0 1 1 3.1 3.1 l -4.8 4.8 a 3.3 3.3 0 1 1 -4.7 -4.7 l 4.2 -4.2`}
        fill="none"
        stroke={stroke}
        strokeWidth={1.1}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {count > 1 && (
        <text x={textX} y={y + 0.5} textAnchor="middle" dominantBaseline="middle" fontSize={8.5} fontWeight="700" fill={ownColor ? '#ffffff' : '#f8fafc'}>
          {count}
        </text>
      )}
    </g>
  );
}

/**
 * The note dot and the attachment pill, on the strip that was reserved for
 * them. Both are gated on the same `parts` the measurement used, so the strip
 * is never drawn into space nothing set aside — and never omitted from space
 * that was.
 */
export function MetaBand({ box, geom, parts, visual }: BandProps): JSX.Element | null {
  if (parts.topMetaH === 0) return null;
  const { hasNote, attachmentCount } = parts;
  return (
    <>
      <BandDivider box={box} y={geom.tagTopY} ownColor={visual.ownColor} />
      {attachmentCount > 0 && (
        <AttachmentIndicator
          x={box.x + box.w - (hasNote ? 26 : 11)}
          y={geom.metaCentreY}
          count={attachmentCount}
          ownColor={visual.ownColor}
        />
      )}
      {hasNote && (
        <circle cx={box.x + box.w - 7} cy={geom.metaCentreY} r={5} fill="#f59e0b" className="mm-indicator" />
      )}
    </>
  );
}

// ── Tag strip ───────────────────────────────────────────────────

const MAX_TAGS_DRAWN = 5;
const TAG_H = 13;
const TAG_GAP = 3;

export function TagBand({
  box,
  geom,
  parts,
  visual,
  labels,
}: BandProps & { labels: UserLabel[] }): JSX.Element | null {
  if (parts.topTagH === 0) return null;

  const tagY = geom.tagTopY + (TAG_STRIP_H - TAG_H) / 2;
  const pills = parts.tags.slice(0, MAX_TAGS_DRAWN).map((tag) => {
    const txt = tag.length > 14 ? `${tag.slice(0, 13)}…` : tag;
    return {
      tag,
      txt,
      width: Math.min(box.w - 8, Math.max(18, 8 + txt.length * 5.5)),
      color: labels.find((l) => l.name === tag)?.color ?? 'var(--accent)',
    };
  });

  const totalW = pills.reduce((sum, p) => sum + p.width, 0) + (pills.length - 1) * TAG_GAP;
  let cursorX = box.x + Math.max(4, (box.w - totalW) / 2);

  return (
    <>
      <BandDivider box={box} y={geom.tagBottomY} ownColor={visual.ownColor} />
      {pills.map((pill) => {
        const x = cursorX;
        cursorX += pill.width + TAG_GAP;
        return (
          <g key={pill.tag} pointerEvents="none">
            <rect x={x} y={tagY} width={pill.width} height={TAG_H} rx={6.5} fill={pill.color} opacity={0.92} />
            <text x={x + pill.width / 2} y={tagY + TAG_H / 2 + 0.5} textAnchor="middle" dominantBaseline="middle" fontSize={8.5} fontWeight={700} fill="#fff">
              {pill.txt}
            </text>
          </g>
        );
      })}
    </>
  );
}

// ── Picture ─────────────────────────────────────────────────────

/**
 * An SVG <image>, deliberately not a foreignObject: the PDF export strips
 * every foreignObject before serializing, and a data: URI in an <image>
 * survives into the standalone SVG and rasterizes. The bitmap was encoded at
 * exactly these dimensions, so it maps 1:1 and there is no crop-versus-
 * letterbox question to answer.
 */
export function ImageBand({
  box,
  geom,
  parts,
  node,
  onOpen,
}: Omit<BandProps, 'visual'> & {
  node: MindMapTreeNode;
  onOpen: (node: MindMapTreeNode) => void;
}): JSX.Element | null {
  // Derived from the parts, not re-read from the node, so the glyph and the
  // band that makes room for it cannot disagree about whether there is one.
  if (!parts.image) return null;
  const image = node.image!;
  return (
    <image
      href={image.thumb}
      x={box.x + (box.w - image.w) / 2}
      y={geom.imageY}
      width={image.w}
      height={image.h}
      className="mm-node-image"
      // Inline, not in the stylesheet: the export serializes this element into
      // a standalone SVG where no class rule follows it, and a glyph with
      // square corners in the PDF would not match the canvas.
      style={{ clipPath: 'inset(0 round 5px)' }}
      onClick={(e) => { e.stopPropagation(); onOpen(node); }}
    >
      <title>{image.name ?? 'Image'}</title>
    </image>
  );
}

// ── Body: everything that sits on the text's centre line ────────

export function ProgressPie({
  cx,
  cy,
  pct,
  size,
  onClickPie,
}: {
  cx: number;
  cy: number;
  pct: number;
  size: number;
  onClickPie?: () => void;
}): JSX.Element {
  const r = size / 2 - 2;
  const inner = pct >= 100 ? (
    <g>
      <circle cx={cx} cy={cy} r={r} fill="#16a34a" />
      <path d={`M ${cx - 4} ${cy} l 3 3 5 -5`} fill="none" stroke="#fff" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
    </g>
  ) : (() => {
    const angle = (pct / 100) * 360;
    const rad = (angle - 90) * (Math.PI / 180);
    const ex = cx + r * Math.cos(rad);
    const ey = cy + r * Math.sin(rad);
    const large = angle > 180 ? 1 : 0;
    const piePath = pct > 0 ? `M ${cx} ${cy} L ${cx} ${cy - r} A ${r} ${r} 0 ${large} 1 ${ex} ${ey} Z` : '';
    return (
      <g>
        <circle cx={cx} cy={cy} r={r} fill="var(--mm-node-fill)" stroke="var(--mm-node-stroke)" strokeWidth={1} />
        {piePath && <path d={piePath} fill="var(--accent)" opacity={0.8} />}
        <text x={cx} y={cy + 1} textAnchor="middle" dominantBaseline="middle" fontSize={11} fontWeight="bold" fill="var(--mm-node-text)">{pct}%</text>
      </g>
    );
  })();

  if (!onClickPie) return inner;
  return (
    <g style={{ cursor: 'pointer' }} onClick={(e) => { e.stopPropagation(); onClickPie(); }}>
      {inner}
      {/* A transparent disc over the whole pie, so the gaps between the slice
          and the ring are clickable too. */}
      <circle cx={cx} cy={cy} r={r} fill="transparent" />
    </g>
  );
}

export interface BodyActions {
  onToggleCheckbox: (nodeId: string) => void;
  onCycleProgress: (nodeId: string) => void;
}

/**
 * The checkbox, icons and progress dial, all on `geom.centreY`, and the text
 * itself — or the textarea, while the node is being edited.
 */
export function BodyBand({
  box,
  geom,
  parts,
  visual,
  node,
  actions,
  isSearchHit,
  editor,
  checkedInfo,
}: BandProps & {
  node: MindMapTreeNode;
  actions: BodyActions;
  isSearchHit: boolean;
  /** Present only while this node is the one being edited. */
  editor: JSX.Element | null;
  checkedInfo: { checked: number; total: number } | null;
}): JSX.Element {
  const { hasCheckbox, hasProgress, iconCount, lines } = parts;
  const iconsX = box.x + NODE_PAD_X + (hasCheckbox ? CHECKBOX_SIZE + 6 : 0) - 2;

  return (
    <>
      {hasCheckbox && (
        <g className="mm-checkbox-g" onClick={(e) => { e.stopPropagation(); actions.onToggleCheckbox(node.id); }} style={{ cursor: 'pointer' }}>
          <rect
            x={box.x + NODE_PAD_X - 2}
            y={geom.centreY - CHECKBOX_SIZE / 2}
            width={CHECKBOX_SIZE}
            height={CHECKBOX_SIZE}
            rx={3}
            fill={node.checked ? 'var(--accent)' : 'transparent'}
            stroke={node.checked ? 'var(--accent)' : (visual.ownColor ? '#ffffff88' : 'var(--mm-node-stroke)')}
            strokeWidth={1.5}
          />
          {node.checked && (
            <path d={`M ${box.x + NODE_PAD_X + 2} ${geom.centreY} l 3 3 5 -6`} fill="none" stroke="#fff" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" />
          )}
        </g>
      )}

      {iconCount > 0 && !editor && (
        <g transform={`translate(${iconsX}, ${geom.centreY - ICON_SIZE / 2})`} style={{ pointerEvents: 'none' }}>
          {(node.icons ?? []).map((iconName, ii) => (
            <g key={`${iconName}-${ii}`} transform={`translate(${ii * (ICON_SIZE + 4)}, 0)`}>
              <DynamicLucideIcon name={iconName} size={ICON_SIZE} color={visual.textColor} />
            </g>
          ))}
        </g>
      )}

      {hasProgress && (
        <ProgressPie
          cx={iconsX + 2 + (iconCount > 0 ? (ICON_SIZE + 4) * iconCount + 2 : 0) + PROGRESS_PIE_SIZE / 2}
          cy={geom.centreY}
          pct={node.progress!}
          size={PROGRESS_PIE_SIZE}
          onClickPie={() => actions.onCycleProgress(node.id)}
        />
      )}

      {editor ?? lines.map((line, li) => (
        <text
          key={li}
          x={geom.textCentreX}
          y={geom.lineStartY + li * NODE_LINE_H}
          textAnchor="middle"
          dominantBaseline="middle"
          fontSize={visual.fontSize}
          fontWeight={visual.fontWeight}
          fill={visual.textColor}
          className={`mm-node-text${isSearchHit ? ' mm-search-highlight' : ''}`}
        >
          {line}
        </text>
      ))}

      {checkedInfo && checkedInfo.total > 0 && (
        <text
          x={box.x + box.w - 8}
          y={geom.bodyTopY + geom.bodyH - 6}
          textAnchor="end"
          fontSize={9}
          fill={visual.ownColor ? '#ffffff99' : 'var(--mm-statusbar-text)'}
        >
          {checkedInfo.checked}/{checkedInfo.total}
        </text>
      )}
    </>
  );
}

// ── Footer: one strip per link ──────────────────────────────────

export function FooterBand({ box, geom, parts, visual, urls }: BandProps & { urls: UrlEntry[] }): JSX.Element | null {
  if (parts.footerH === 0) return null;
  return (
    <>
      {urls.map((urlItem, ui) => {
        const fy = geom.footerTopY + ui * LINK_STRIP_H;
        const rawUrl = (urlItem.url ?? '').trim();
        const openUrl = /^https?:\/\//i.test(rawUrl) ? rawUrl : `https://${rawUrl}`;
        return (
          <g key={`url-${ui}`}>
            <BandDivider box={box} y={fy} ownColor={visual.ownColor} inset={4} />
            <text
              x={box.x + 8}
              y={fy + LINK_STRIP_H / 2 + 1.5}
              fontSize={10.5}
              fontWeight={600}
              fill={visual.ownColor ? '#ffffff' : 'var(--accent)'}
              dominantBaseline="middle"
              className={`mm-url-link${visual.ownColor ? ' mm-url-link--on-color' : ''}`}
              style={{ cursor: 'pointer', textDecoration: 'underline' }}
              onMouseDown={(e) => { e.stopPropagation(); }}
              onClick={(e) => { e.stopPropagation(); void openExternalUrl(openUrl); }}
            >
              {urlItem.label || rawUrl}
            </text>
          </g>
        );
      })}
    </>
  );
}

// ── Collapse controls, on the box edge ──────────────────────────

function CollapseBubble({
  x,
  y,
  label,
  onClick,
}: {
  x: number;
  y: number;
  label: string;
  onClick: () => void;
}): JSX.Element {
  return (
    <g
      className="mm-collapse-btn"
      transform={`translate(${x}, ${y})`}
      onClick={(e) => { e.stopPropagation(); onClick(); }}
    >
      <circle r={8} fill="var(--mm-collapse-fill)" stroke="var(--mm-collapse-stroke)" strokeWidth={1.5} />
      <text textAnchor="middle" dominantBaseline="middle" fontSize={11} fill="var(--mm-collapse-text)" fontWeight="bold" y={0.5}>
        {label}
      </text>
    </g>
  );
}

/**
 * One bubble for an ordinary node, on whichever side its children are; two for
 * the root, which fans out both ways and collapses each side separately.
 */
export function CollapseControls({
  box,
  node,
  isRoot,
  rootLeftCollapsed,
  rootRightCollapsed,
  onToggle,
  onToggleRootLeft,
  onToggleRootRight,
}: {
  box: NodeBox;
  node: MindMapTreeNode;
  isRoot: boolean;
  rootLeftCollapsed: boolean;
  rootRightCollapsed: boolean;
  onToggle: (nodeId: string) => void;
  onToggleRootLeft: () => void;
  onToggleRootRight: () => void;
}): JSX.Element | null {
  const midY = box.y + box.h / 2;
  const leftX = box.x - 1;
  const rightX = box.x + box.w + 1;

  if (!isRoot) {
    if (node.children.length === 0) return null;
    return (
      <CollapseBubble
        x={box.direction === 'left' ? leftX : rightX}
        y={midY}
        label={node.collapsed ? `+${node.children.length}` : '−'}
        onClick={() => onToggle(node.id)}
      />
    );
  }

  const leftChildren = node.children.filter((ch) => ch.side === 'left');
  const rightChildren = node.children.filter((ch) => ch.side !== 'left');
  return (
    <>
      {leftChildren.length > 0 && (
        <CollapseBubble
          x={leftX}
          y={midY}
          label={rootLeftCollapsed ? `+${leftChildren.length}` : '−'}
          onClick={onToggleRootLeft}
        />
      )}
      {rightChildren.length > 0 && (
        <CollapseBubble
          x={rightX}
          y={midY}
          label={rootRightCollapsed ? `+${rightChildren.length}` : '−'}
          onClick={onToggleRootRight}
        />
      )}
    </>
  );
}
