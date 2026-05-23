export async function openExternalUrl(url: string): Promise<void> {
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
