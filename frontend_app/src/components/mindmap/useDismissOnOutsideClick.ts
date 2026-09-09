import { useEffect, type RefObject } from 'react';

/**
 * Close a transient popup when the pointer goes down anywhere outside it.
 *
 * The editor has four of these — the Export menu, the colour and icon pickers,
 * and the Labels dialog — and each used to rely on the canvas click handler,
 * which only fires for the canvas itself. A click on the toolbar, a tray or the
 * status bar left the popup hanging over whatever was clicked next.
 *
 * `ref` must point at an element containing **both** the popup and the control
 * that opens it. Pointing it at the popup alone makes a click on the trigger
 * count as outside: this handler closes on mousedown, then the trigger's own
 * click toggles it straight back open, so it could never be dismissed by its
 * own button.
 */
export function useDismissOnOutsideClick(
  open: boolean,
  ref: RefObject<HTMLElement | null>,
  onDismiss: () => void,
): void {
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      const el = ref.current;
      if (el && !el.contains(e.target as Node)) onDismiss();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open, ref, onDismiss]);
}
