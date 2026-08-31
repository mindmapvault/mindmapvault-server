/**
 * Ensures a Uint8Array is backed by a plain ArrayBuffer (required by WebCrypto
 * and Fetch). TypeScript 5.7 made Uint8Array generic; external libs may return
 * Uint8Array<ArrayBufferLike> which WebCrypto rejects at the type level.
 */
export function toBuf(u: Uint8Array): Uint8Array<ArrayBuffer> {
  if (u.buffer instanceof ArrayBuffer) return u as Uint8Array<ArrayBuffer>;
  return new Uint8Array(u);
}

/** Base64 encode/decode helpers that work in all modern browsers. */
export function toBase64(bytes: Uint8Array): string {
  let str = '';
  for (const b of bytes) str += String.fromCharCode(b);
  return btoa(str);
}

export function fromBase64(str: string): Uint8Array {
  const bin = atob(str);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('');
}

export function randomBytes(n: number): Uint8Array {
  return crypto.getRandomValues(new Uint8Array(n));
}

export function concat(...arrays: Uint8Array[]): Uint8Array {
  const total = arrays.reduce((n, a) => n + a.length, 0);
  const result = new Uint8Array(total);
  let offset = 0;
  for (const a of arrays) {
    result.set(a, offset);
    offset += a.length;
  }
  return result;
}

export const MIN_SHARE_PASSPHRASE_LENGTH = 12;

/**
 * Generates a share passphrase with roughly 100 bits of entropy.
 *
 * The alphabet drops characters that are easy to confuse when a passphrase is
 * read aloud or copied by hand (0/O, 1/l/I), and the groups exist for the same
 * reason. This is meant to be the default path: the easy choice should be the
 * safe one, and typing your own should be the deliberate exception.
 */
export function generateSharePassphrase(groups = 4, groupLength = 5): string {
  const alphabet = 'abcdefghjkmnpqrstuvwxyz23456789';
  const total = groups * groupLength;

  // Rejection sampling rather than a modulo: 256 is not a multiple of 31, so
  // `byte % 31` would make the first few characters measurably more likely.
  const limit = Math.floor(256 / alphabet.length) * alphabet.length;
  const chars: string[] = [];
  while (chars.length < total) {
    for (const byte of randomBytes(total)) {
      if (byte < limit) {
        chars.push(alphabet[byte % alphabet.length]);
        if (chars.length === total) break;
      }
    }
  }
  const parts: string[] = [];
  for (let i = 0; i < groups; i += 1) {
    parts.push(chars.slice(i * groupLength, (i + 1) * groupLength).join(''));
  }

  return parts.join('-');
}
