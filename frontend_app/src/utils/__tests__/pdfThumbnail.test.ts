import { beforeEach, describe, expect, it, vi } from 'vitest';

/**
 * pdf.js transfers the buffer it is given to its worker, which leaves the
 * caller's array detached and zero-length. BoardPage thumbnails an uploaded
 * PDF and then encrypts the very same bytes, so handing pdf.js the original
 * stored an empty attachment — the preview looked right and the file was gone.
 *
 * Only the handover is asserted here. A real transfer needs a worker, but the
 * invariant that keeps the bug away is ours: pdf.js must never be given the
 * caller's own buffer.
 */

let captured: { data: Uint8Array } | null = null;
const stop = new Error('stop after capture');

vi.mock('pdfjs-dist', () => ({
  GlobalWorkerOptions: { workerSrc: '' },
  getDocument: (opts: { data: Uint8Array }) => {
    captured = opts;
    return { promise: Promise.reject(stop) };
  },
}));

describe('renderPdfThumbnail', () => {
  beforeEach(() => { captured = null; });

  it('hands pdf.js a copy, leaving the caller\'s bytes intact', async () => {
    const { renderPdfThumbnail } = await import('../pdfThumbnail');
    const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d]); // "%PDF-"

    await expect(renderPdfThumbnail(bytes)).rejects.toBe(stop);

    expect(captured).not.toBeNull();
    expect(captured!.data).not.toBe(bytes);
    expect(captured!.data.buffer).not.toBe(bytes.buffer);
    expect(Array.from(captured!.data)).toEqual([0x25, 0x50, 0x44, 0x46, 0x2d]);
    expect(bytes.length).toBe(5);
  });
});
