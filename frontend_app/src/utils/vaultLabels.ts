/**
 * Reserved vault labels.
 *
 * These are stored like ordinary labels but carry meaning for the app, so they
 * are filtered out of every label chip the user sees. Keep new reserved labels
 * in this file rather than as string literals at the call sites.
 */

/** Marks a vault as an evidence board rather than a mind map. */
export const BOARD_LABEL = '__board__';

/**
 * Marks a vault created by importing a share someone sent you.
 *
 * A share has no recipient — it is a link plus a passphrase, and the server
 * never learns who opened it — so an imported copy is the only durable record
 * that something was shared with this account.
 */
export const IMPORTED_SHARE_LABEL = '__imported__';

export const RESERVED_VAULT_LABELS = [BOARD_LABEL, IMPORTED_SHARE_LABEL];

export function isReservedLabel(label: string): boolean {
  return RESERVED_VAULT_LABELS.includes(label);
}

/** Strips reserved labels so only what the user typed is displayed. */
export function visibleLabels(labels: string[] | undefined | null): string[] {
  return (labels ?? []).filter((label) => !isReservedLabel(label));
}
