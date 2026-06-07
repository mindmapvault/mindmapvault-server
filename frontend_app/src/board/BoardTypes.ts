export type BoardCardColor = 'default' | 'red' | 'yellow' | 'blue' | 'green' | 'purple';

export interface BoardTextCard {
  id: string;
  type: 'card';
  x: number;
  y: number;
  title: string;
  body: string;
  color: BoardCardColor;
}

export interface BoardImageCard {
  id: string;
  type: 'image';
  x: number;
  y: number;
  src: string;
  label: string;
  width: number;
  /** pixel height of the image area (label bar added on top) */
  height: number;
}

export interface BoardPdfCard {
  id: string;
  type: 'pdf';
  x: number;
  y: number;
  src: string;
  /** JPEG data-URL of the first page, stored inline so no re-download is needed on reopen. */
  thumbnailSrc?: string;
  label: string;
  width: number;
  /** pixel height of the PDF preview area (label bar added on top) */
  height: number;
  pageCount: number;
}

export type BoardNode = BoardTextCard | BoardImageCard | BoardPdfCard;

export interface BoardConnection {
  id: string;
  sourceId: string;
  targetId: string;
  color: string;
  label: string;
}

export interface BoardViewState {
  panX: number;
  panY: number;
  zoom: number;
}

// ── Background ────────────────────────────────────────────────────────────────

export type BoardBackgroundTextureId =
  | 'cork'
  | 'cork-dark'
  | 'chalkboard'
  | 'aged-paper'
  | 'blueprint';

export type BoardBackground =
  | { type: 'texture'; id: BoardBackgroundTextureId }
  | { type: 'color'; color: string };

// ── Board data ────────────────────────────────────────────────────────────────

export interface BoardData {
  version: 'board';
  cards: BoardNode[];
  connections: BoardConnection[];
  view?: BoardViewState;
  background?: BoardBackground;
}

export function emptyBoardData(title: string): BoardData {
  return {
    version: 'board',
    cards: [{ id: 'root-card', type: 'card', x: 0, y: -40, title, body: 'Start adding evidence…', color: 'default' }],
    connections: [],
  };
}
