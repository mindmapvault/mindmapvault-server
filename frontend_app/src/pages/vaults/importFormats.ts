/**
 * The mind map formats a vault can be imported from.
 *
 * These were four handlers of ~38 lines that differed in nine: which parser to
 * call, which extension to strip, whether the file is read as text or bytes,
 * and which of three state variables to set. Everything else — encrypting the
 * title, wrapping a fresh DEK, uploading the blob, caching the preview,
 * navigating to the new vault — was written out four times.
 *
 * Adding a format is now one entry in this table.
 */

import type { MindMapTreeNode } from '../../types';
import { freemindToTree } from '../../utils/freemindImport';
import { obsidianMarkdownToTree } from '../../utils/markdownImport';
import { wisemappingToTree } from '../../utils/wisemappingImport';

export type ImportFormatId = 'md' | 'mm' | 'wxml' | 'xmind';

export interface ImportFormat {
  id: ImportFormatId;
  /** The `accept` attribute of this format's hidden file input. */
  accept: string;
  /** Prefixes this format's failure message, which is shown on its own. */
  errorLabel: string;
  /** Stripped from the file name to title the new vault. */
  extensions: RegExp;
  /** Reads the file and parses it into a tree root. */
  parse: (file: File, vaultTitle: string) => Promise<MindMapTreeNode>;
}

/** The vault a file becomes is named after the file, without its extension. */
export function vaultTitleFromFileName(fileName: string, extensions: RegExp): string {
  return fileName.replace(extensions, '') || 'Imported vault';
}

export const IMPORT_FORMATS: ImportFormat[] = [
  {
    id: 'md',
    accept: '.md',
    errorLabel: 'Import failed',
    extensions: /\.md$/i,
    parse: async (file, title) => obsidianMarkdownToTree(await file.text(), title),
  },
  {
    id: 'mm',
    accept: '.mm',
    errorLabel: '.mm import failed',
    extensions: /\.mm$/i,
    parse: async (file, title) => freemindToTree(await file.text(), title),
  },
  {
    id: 'wxml',
    accept: '.wxml,.xml',
    errorLabel: 'WiseMapping import failed',
    extensions: /\.(wxml|xml)$/i,
    parse: async (file, title) => wisemappingToTree(await file.text(), title),
  },
  {
    id: 'xmind',
    accept: '.xmind',
    errorLabel: 'XMind import failed',
    extensions: /\.xmind$/i,
    // Loaded on demand: the XMind reader pulls in a zip decoder that the other
    // three formats do not need.
    parse: async (file, title) => {
      const { xmindToTree } = await import('../../utils/xmindImport');
      return xmindToTree(await file.arrayBuffer(), title);
    },
  },
];

export function importFormat(id: ImportFormatId): ImportFormat {
  const format = IMPORT_FORMATS.find((f) => f.id === id);
  if (!format) throw new Error(`Unknown import format: ${id}`);
  return format;
}

/**
 * The import menu, which is not one-to-one with the formats above: FreeMind
 * and Freeplane are listed separately because people look for their own
 * application's name, and both open the same `.mm` reader.
 */
export interface ImportMenuItem {
  label: string;
  /** Shown greyed beside the label. */
  extension: string;
  format: ImportFormatId;
}

export const IMPORT_MENU_ITEMS: ImportMenuItem[] = [
  { label: 'Markdown', extension: '.md', format: 'md' },
  { label: 'FreeMind', extension: '.mm', format: 'mm' },
  { label: 'FreePlane', extension: '.mm', format: 'mm' },
  { label: 'WiseMapping', extension: '.wxml', format: 'wxml' },
  { label: 'XMind', extension: '.xmind', format: 'xmind' },
];
