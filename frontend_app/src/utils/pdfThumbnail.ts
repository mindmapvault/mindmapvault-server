import type { PDFDocumentProxy } from 'pdfjs-dist';

let _initialized = false;

async function ensureWorker() {
  if (_initialized) return;
  _initialized = true;
  const pdfjsLib = await import('pdfjs-dist');
  const workerUrl = new URL('pdfjs-dist/build/pdf.worker.min.mjs', import.meta.url).href;
  pdfjsLib.GlobalWorkerOptions.workerSrc = workerUrl;
}

export interface PdfMeta {
  thumbnail: string;  // data URL of first page rendered as JPEG
  pageCount: number;
}

/**
 * Renders the first page of an uploaded PDF, in the same document that holds
 * the reader's decrypted vault. Keep pdfjs-dist at 6.2.108 or newer: earlier
 * 6.x releases ran a malicious PDF's script through pdf.js's eval-based font
 * path (GHSA-hq66-cqwq-w95j). Version 6 removed that path outright, so there
 * is no option to switch off -- the floor in package.json is the whole fix.
 */
export async function renderPdfThumbnail(
  pdfBytes: Uint8Array,
  targetWidth = 400,
): Promise<PdfMeta> {
  await ensureWorker();
  const pdfjsLib = await import('pdfjs-dist');

  const pdf: PDFDocumentProxy = await pdfjsLib.getDocument({
    data: pdfBytes,
    disableStream: true,
    disableAutoFetch: true,
  }).promise;

  const pageCount = pdf.numPages;
  const page = await pdf.getPage(1);

  // Scale so the rendered width matches targetWidth
  const unscaled = page.getViewport({ scale: 1 });
  const scale = targetWidth / unscaled.width;
  const viewport = page.getViewport({ scale });

  const canvas = document.createElement('canvas');
  canvas.width = Math.round(viewport.width);
  canvas.height = Math.round(viewport.height);

  // pdfjs-dist 6.x: `canvas` is the primary parameter; `background` fills before render
  await page.render({ canvas, viewport, background: '#ffffff' } as Parameters<typeof page.render>[0]).promise;

  const thumbnail = canvas.toDataURL('image/jpeg', 0.82);
  await pdf.cleanup();
  return { thumbnail, pageCount };
}
