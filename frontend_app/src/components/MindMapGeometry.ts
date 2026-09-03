/**
 * MindMapGeometry
 *
 * A node's parts, how much room each needs, and where each one goes.
 *
 * This exists because the same arithmetic was being done three times: in
 * `measureNodeSize`, to decide how big a node is; inline in the editor's
 * render function, to decide where inside it each part goes; and again in
 * `utils/vaultPreview.ts`, to draw the thumbnail. All three derived
 * `leftPad`, `topMetaH`, `topTagH`, `bodyTopY` and `bodyH` from the same
 * fields, and they have to agree exactly or the text drifts out of the box it
 * was measured for. They had already drifted — see `describeNode` below.
 *
 * The split is the one proved out in `mindmapvault-live`
 * (`app/src/maps/geometry.ts`): `describeNode` reads the node once,
 * `measureNodeSize` adds the bands up, and `nodeGeometry` turns the box the
 * layout produced back into positions. Measuring and drawing then use the same
 * numbers by construction rather than by inspection.
 */

import type { MindMapTreeNode, NodeImage } from '../types';
import {
  NODE_LINE_H,
  NODE_MIN_H,
  NODE_PAD_X,
  NODE_PAD_Y,
  MIN_W,
  LINK_STRIP_H,
  TAG_STRIP_H,
  TOP_META_STRIP_H,
  DATE_BADGE_OFFSET_H,
  ICON_SIZE,
  CHECKBOX_SIZE,
  PROGRESS_PIE_SIZE,
  NODE_IMAGE_PAD,
} from './MindMapConstants';
import { getVisibleNodeTextLines } from '../utils/nodeAttachments';

// ── Text measurement ────────────────────────────────────────────

let measureContext: CanvasRenderingContext2D | null = null;
/**
 * Cached, because a headless environment has no 2d context and asking twice
 * only produces the same failure twice. The fallback estimate keeps the layout
 * usable outside a browser; the browser always has the real thing.
 */
let measureUnavailable = false;

export const measureText = (text: string, fontSize = 14): number => {
  if (!measureContext && !measureUnavailable) {
    try {
      measureContext = document.createElement('canvas').getContext('2d');
    } catch {
      measureContext = null;
    }
    if (!measureContext) measureUnavailable = true;
  }
  if (!measureContext) return (text?.length ?? 1) * fontSize * 0.6;

  measureContext.font = `${fontSize}px ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, sans-serif`;
  return measureContext.measureText(text || ' ').width;
};

// ── What a node is made of ──────────────────────────────────────

/**
 * Which parts a node has and how tall each band is. Derived from the node
 * alone, so measuring and drawing cannot disagree about it.
 *
 * Bands run top to bottom: the date badge sits *above* the box, then the meta
 * strip, the tags, the picture, the text body, and one footer strip per link.
 */
export interface NodeParts {
  lines: string[];
  iconCount: number;
  urlCount: number;
  tagCount: number;
  linkId: string | null;
  hasCheckbox: boolean;
  hasProgress: boolean;
  hasNote: boolean;
  attachmentCount: number;
  hasDate: boolean;
  /** The glyph to draw, or null. Dimensions come from the node JSON, so a node
   *  with a picture measures as fast as one without — no decode, no reflow. */
  image: NodeImage | null;
  /** Room taken by the checkbox, icons and progress dial, left of the text. */
  leftPad: number;
  /** Band above the body: note dot and attachment count. */
  topMetaH: number;
  /** Band below the meta strip: the tag pills. */
  topTagH: number;
  /** Band below the tags: the picture. */
  imageBandH: number;
  /** Band below the body: one strip per footer link. */
  footerH: number;
  /** Space reserved *above* the node for the date badge. */
  visualTopExtra: number;
}

export interface DescribeOptions {
  /**
   * Attachments resolved for this node, when the caller knows more than the
   * node does. The editor merges the node's own list with
   * `externalNodeAttachments`, so a node whose attachments live only outside
   * the tree used to get a meta strip drawn that the layout had reserved no
   * height for — 18px stolen from the text body. Pass the resolved count and
   * both sides agree again.
   */
  attachmentCount?: number;
}

export type DescribeNode = (node: MindMapTreeNode) => NodeParts;

export const describeNode = (
  node: MindMapTreeNode,
  options: DescribeOptions = {},
): NodeParts => {
  const lines = getVisibleNodeTextLines(node.text);
  const iconCount = node.icons?.length ?? 0;
  const urlCount = node.urls?.length ?? 0;
  const tagCount = node.tags?.length ?? 0;
  const linkId = node.link?.id || null;

  const hasCheckbox = node.checked != null;
  const hasProgress = node.progress != null;
  // Trimmed, deliberately: a node whose notes are a stray space has nothing to
  // show, and the renderer used to disagree with the layout about that.
  const hasNote = Boolean(node.notes?.trim());
  const attachmentCount = options.attachmentCount ?? node.attachments?.length ?? 0;
  const hasDate = Boolean(node.startDate || node.endDate);
  const image = node.image?.thumb ? node.image : null;

  return {
    lines,
    iconCount,
    urlCount,
    tagCount,
    linkId,
    hasCheckbox,
    hasProgress,
    hasNote,
    attachmentCount,
    hasDate,
    image,
    leftPad:
      (hasCheckbox ? CHECKBOX_SIZE + 6 : 0) +
      (iconCount > 0 ? (ICON_SIZE + 4) * iconCount + 2 : 0) +
      (hasProgress ? PROGRESS_PIE_SIZE + 6 : 0),
    topMetaH: hasNote || attachmentCount > 0 ? TOP_META_STRIP_H : 0,
    topTagH: tagCount > 0 ? TAG_STRIP_H : 0,
    imageBandH: image ? image.h + NODE_IMAGE_PAD : 0,
    footerH: ((linkId ? 1 : 0) + urlCount) * LINK_STRIP_H,
    visualTopExtra: hasDate ? DATE_BADGE_OFFSET_H : 0,
  };
};

// ── How big it has to be ────────────────────────────────────────

export interface NodeSize {
  w: number;
  h: number;
  lines: string[];
}

/** How big a node has to be to hold everything `describeNode` found in it. */
export const measureNodeSize = (
  node: MindMapTreeNode,
  parts: NodeParts = describeNode(node),
): NodeSize => {
  const linkW = parts.linkId ? measureText(parts.linkId, 10) + 24 : 0;
  const urlW = parts.urlCount > 0 ? 120 : 0;
  const maxW = Math.max(...parts.lines.map((line) => measureText(line || ' ')), linkW, urlW);

  const textW = Math.max(MIN_W, maxW + NODE_PAD_X * 2 + parts.leftPad);
  const imageW = parts.image ? parts.image.w + NODE_PAD_X * 2 : 0;
  const bodyH = Math.max(NODE_MIN_H, parts.lines.length * NODE_LINE_H + NODE_PAD_Y * 2);

  return {
    w: Math.max(textW, imageW),
    h: bodyH + parts.topMetaH + parts.topTagH + parts.imageBandH + parts.footerH,
    lines: parts.lines,
  };
};

// ── Where each part goes, once the node has been placed ─────────

export interface NodeBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface NodeGeometry {
  /** Centre of the meta strip: the note dot and the attachment badge. */
  metaCentreY: number;
  /** Top of the tag strip, and the divider under the meta strip. */
  tagTopY: number;
  /** Divider under the tag strip. */
  tagBottomY: number;
  /** Top of the picture glyph. */
  imageY: number;
  /** Top of the text body, below the meta, tag and image bands. */
  bodyTopY: number;
  /** Height of the text body — the number `measureNodeSize` started from. */
  bodyH: number;
  /** Vertical centre of the body: checkbox, icons and progress dial. */
  centreY: number;
  /** Left edge of the text, past the decorations. */
  textX: number;
  /** Horizontal centre of the text column. */
  textCentreX: number;
  /** Baseline of the first line of text. */
  lineStartY: number;
  /** Top of the first footer strip. */
  footerTopY: number;
}

/**
 * The counterpart to `measureNodeSize`: given the box the layout produced,
 * where does each band start. `bodyH` here is by construction the same number
 * `measureNodeSize` added the bands to.
 */
export const nodeGeometry = (box: NodeBox, parts: NodeParts): NodeGeometry => {
  const bandsAboveBody = parts.topMetaH + parts.topTagH + parts.imageBandH;
  const bodyTopY = box.y + bandsAboveBody;
  const bodyH = box.h - bandsAboveBody - parts.footerH;
  const textX = box.x + NODE_PAD_X + parts.leftPad;

  return {
    metaCentreY: box.y + parts.topMetaH / 2,
    tagTopY: box.y + parts.topMetaH,
    tagBottomY: box.y + parts.topMetaH + parts.topTagH,
    imageY: box.y + parts.topMetaH + parts.topTagH + NODE_IMAGE_PAD / 2,
    bodyTopY,
    bodyH,
    centreY: bodyTopY + bodyH / 2,
    textX,
    textCentreX: textX + (box.w - NODE_PAD_X * 2 - parts.leftPad) / 2,
    lineStartY: bodyTopY + bodyH / 2 - ((parts.lines.length - 1) * NODE_LINE_H) / 2,
    footerTopY: bodyTopY + bodyH,
  };
};
