/**
 * MindMapLayout
 *
 * Layout engine for the MindMap — computes node positions in a two-pass
 * (bottom-up height, then top-down position) tree layout algorithm.
 *
 * How big each node is, and what it is made of, lives in `MindMapGeometry`;
 * this file only decides where the boxes go. The parts each node was measured
 * from ride along in the layout entry, so the renderer draws inside exactly
 * the box that was measured rather than working the bands out a second time.
 */

import type { MindMapTreeNode } from '../types';
import { H_GAP, V_GAP } from './MindMapConstants';
import {
  describeNode,
  measureNodeSize,
  type DescribeNode,
  type NodeParts,
} from './MindMapGeometry';

export * from './MindMapGeometry';

// ── Layout types ────────────────────────────────────────────────

export interface LayoutEntry {
  x: number;
  y: number;
  w: number;
  h: number;
  visualTopExtra: number;
  subtreeH: number;
  direction: 'left' | 'right';
  node: MindMapTreeNode;
  /** What this node was measured from. The renderer draws from the same parts. */
  parts: NodeParts;
}

// ── Tree layout ─────────────────────────────────────────────────

export const layoutTree = (
  root: MindMapTreeNode,
  startX = 0,
  startY = 0,
  describe: DescribeNode = describeNode,
): Record<string, LayoutEntry> => {
  const pos: Record<string, Partial<LayoutEntry>> = {};

  // First pass: compute subtree heights (bottom-up)
  const computeHeight = (node: MindMapTreeNode): number => {
    const parts = describe(node);
    const { w, h } = measureNodeSize(node, parts);
    const visualTopExtra = parts.visualTopExtra;
    const visualH = h + visualTopExtra;

    if (!node.children || node.children.length === 0 || node.collapsed) {
      pos[node.id] = { w, h, visualTopExtra, subtreeH: visualH, node, parts };
      return visualH;
    }

    let childrenH = 0;
    node.children.forEach((ch, i) => {
      childrenH += computeHeight(ch);
      if (i > 0) childrenH += V_GAP;
    });

    const subtreeH = Math.max(visualH, childrenH);
    pos[node.id] = { w, h, visualTopExtra, subtreeH, node, parts };
    return subtreeH;
  };

  computeHeight(root);

  // Second pass: assign x, y positions (top-down)
  const assignPos = (
    node: MindMapTreeNode,
    x: number,
    yCenter: number,
    direction: 'left' | 'right' = 'right',
  ) => {
    const p = pos[node.id]!;
    p.direction = direction;

    // Use custom position if the node has been manually dragged
    if (node.customX != null && node.customY != null) {
      p.x = node.customX;
      p.y = node.customY;
    } else {
      const topExtra = p.visualTopExtra ?? 0;
      p.x = x;
      p.y = yCenter - ((p.h ?? 0) + topExtra) / 2 + topExtra;
    }

    if (!node.children || node.children.length === 0 || node.collapsed) return;

    const layoutGroup = (children: MindMapTreeNode[], dir: 'left' | 'right') => {
      let totalH = 0;
      children.forEach((ch, i) => {
        totalH += pos[ch.id]!.subtreeH ?? 0;
        if (i > 0) totalH += V_GAP;
      });

      let cy = yCenter - totalH / 2;
      children.forEach((ch) => {
        const chP = pos[ch.id]!;
        const chCenter = cy + (chP.subtreeH ?? 0) / 2;
        const childX = dir === 'right'
          ? x + (p.w ?? 0) + H_GAP
          : x - (chP.w ?? 0) - H_GAP;
        assignPos(ch, childX, chCenter, dir);
        cy += (chP.subtreeH ?? 0) + V_GAP;
      });
    };

    if (node.id === 'root') {
      const rightChildren = node.children.filter((ch) => ch.side !== 'left');
      const leftChildren = node.children.filter((ch) => ch.side === 'left');
      if (rightChildren.length > 0) layoutGroup(rightChildren, 'right');
      if (leftChildren.length > 0) layoutGroup(leftChildren, 'left');
    } else {
      layoutGroup(node.children, direction);
    }
  };

  assignPos(root, startX, startY);
  return pos as Record<string, LayoutEntry>;
};

/** Bezier connection path between two points. */
export const bezierPath = (
  x1: number,
  y1: number,
  x2: number,
  y2: number,
): string => {
  const mx = (x1 + x2) / 2;
  return `M ${x1},${y1} C ${mx},${y1} ${mx},${y2} ${x2},${y2}`;
};
