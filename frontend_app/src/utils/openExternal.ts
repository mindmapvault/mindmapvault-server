// Schemes a link may never open: they run code, read local files or point
// back into the app itself.
const BLOCKED_SCHEMES = new Set(['javascript', 'vbscript', 'data', 'file', 'blob', 'about', 'tauri', 'asset', 'ipc']);

/**
 * The scheme of a link in lower case (`obsidian` for `obsidian://open?vault=…`),
 * or null when it has none. `example.com:8080/x` is a host and port, not a scheme.
 */
export function linkScheme(raw: string): string | null {
  const match = /^([a-z][a-z0-9+.-]*):(.*)$/is.exec(raw.trim());
  if (!match) return null;
  if (/^\d+(?:[/?#]|$)/.test(match[2])) return null;
  return match[1].toLowerCase();
}

/**
 * What a link on a node opens. A bare address gets `https://`; links to other
 * apps (`obsidian:`, `zotero:`, `mailto:` …) are kept as typed; blocked
 * schemes give null.
 */
export function linkTarget(raw: string): string | null {
  const url = raw.replace(/[\u0000-\u001F\u007F]/g, '').trim();
  if (!url) return null;
  const scheme = linkScheme(url);
  if (!scheme) return `https://${url}`;
  return BLOCKED_SCHEMES.has(scheme) ? null : url;
}

function isBlocked(url: string): boolean {
  const scheme = linkScheme(url);
  return scheme !== null && BLOCKED_SCHEMES.has(scheme);
}

export async function openExternalUrl(url: string): Promise<void> {
  if (isBlocked(url)) return;
  try {
    window.open(url, '_blank', 'noopener,noreferrer');
  } catch (e) {
    // ignore
  }
}

export function handleDelegatedLinkClick(e: MouseEvent, fallback?: (url: string) => void) {
  const target = e.target as HTMLElement | null;
  if (!target) return;
  const a = target.closest ? (target.closest('a') as HTMLAnchorElement | null) : null;
  if (a && a.href) {
    e.preventDefault();
    e.stopPropagation();
    try {
      void openExternalUrl(a.href);
    } catch (err) {
      if (fallback) fallback(a.href);
    }
  }
}
