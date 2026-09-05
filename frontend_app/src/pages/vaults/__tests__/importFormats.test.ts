import { describe, expect, it } from 'vitest';
import { IMPORT_FORMATS, importFormat, vaultTitleFromFileName } from '../importFormats';

/**
 * The title derivation was written out four times with four different regexes,
 * which is exactly where a format quietly stops stripping its own extension.
 */

describe('vaultTitleFromFileName', () => {
  const strip = (name: string, id: Parameters<typeof importFormat>[0]) =>
    vaultTitleFromFileName(name, importFormat(id).extensions);

  it('names the vault after the file, without its extension', () => {
    expect(strip('Roadmap.md', 'md')).toBe('Roadmap');
    expect(strip('Roadmap.mm', 'mm')).toBe('Roadmap');
    expect(strip('Roadmap.wxml', 'wxml')).toBe('Roadmap');
    expect(strip('Roadmap.xmind', 'xmind')).toBe('Roadmap');
  });

  it('matches the extension case-insensitively', () => {
    expect(strip('Roadmap.MD', 'md')).toBe('Roadmap');
    expect(strip('Roadmap.XMind', 'xmind')).toBe('Roadmap');
  });

  it('accepts either extension WiseMapping exports under', () => {
    expect(strip('Plan.wxml', 'wxml')).toBe('Plan');
    expect(strip('Plan.xml', 'wxml')).toBe('Plan');
  });

  it('strips only the final extension', () => {
    expect(strip('notes.md.md', 'md')).toBe('notes.md');
    expect(strip('v1.2.mm', 'mm')).toBe('v1.2');
  });

  it('leaves a name that does not carry the extension alone', () => {
    expect(strip('Roadmap.txt', 'md')).toBe('Roadmap.txt');
  });

  it('falls back when the file is nothing but its extension', () => {
    expect(strip('.md', 'md')).toBe('Imported vault');
    expect(strip('', 'md')).toBe('Imported vault');
  });
});

describe('IMPORT_FORMATS', () => {
  it('has a unique id per format', () => {
    const ids = IMPORT_FORMATS.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('gives every format its own failure wording', () => {
    const labels = IMPORT_FORMATS.map((f) => f.errorLabel);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it('refuses an id it does not know', () => {
    expect(() => importFormat('doc' as never)).toThrow(/Unknown import format/);
  });
});
