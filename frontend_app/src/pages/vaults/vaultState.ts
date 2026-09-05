/**
 * What a vault *is*, derived from its record.
 *
 * These were computed inside `VaultCard` and again inside `VaultTableRow` —
 * the grid and table views of the same vault — from the same fields, verbatim.
 * Two copies of one derivation have to agree, and nothing was keeping them in
 * step; the mind map editor had the same shape of bug in its node measurement
 * and it put text outside the box that was measured for it.
 *
 * Nothing here touches React, storage or the DOM, so all of it is testable.
 */

import type { VaultEncryptionMode, VaultSharingMode } from '../../types';
import { BOARD_LABEL } from '../../utils/vaultLabels';

// ── Normalising what the server sent ────────────────────────────

const DEFAULT_VAULT_COLOR = '#334155';

/** Anything that is not a six-digit hex colour becomes the default. */
export function normalizeHexColor(input?: string): string {
  if (!input) return DEFAULT_VAULT_COLOR;
  return /^#[0-9a-fA-F]{6}$/.test(input) ? input : DEFAULT_VAULT_COLOR;
}

export function normalizeSharingMode(input?: string): VaultSharingMode {
  return input === 'shared' ? 'shared' : 'private';
}

export function normalizeEncryptionMode(input?: string): VaultEncryptionMode {
  return input === 're-encrypted' ? 're-encrypted' : 'standard';
}

/** Lower-cased, trimmed, de-duplicated, order preserved. */
export function normalizeVaultLabels(input?: string[]): string[] {
  if (!Array.isArray(input)) return [];
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of input) {
    if (typeof raw !== 'string') continue;
    const label = raw.trim().toLowerCase();
    if (!label || seen.has(label)) continue;
    seen.add(label);
    out.push(label);
  }
  return out;
}

/** Order-sensitive: the label list is a sequence the user can reorder. */
export function labelsEqual(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((label, idx) => label === b[idx]);
}

// ── Per-device vault colour ─────────────────────────────────────

/**
 * Local mode has no server to keep a vault's colour on, so it lives in
 * `localStorage` under this key. The key is persisted data: renaming it
 * silently resets every local vault to the default colour.
 */
export function vaultColorStorageKey(vaultId: string): string {
  return `vault-color-${vaultId}`;
}

// ── The derivation both views need ──────────────────────────────

/** Only the fields the derivation reads, so tests need not build a whole vault. */
export interface VaultRecordLike {
  id: string;
  vault_labels?: string[] | null;
  vault_sharing_mode?: string;
}

export interface VaultState {
  /** Boards are a different editor on a different route. */
  isBoard: boolean;
  /** Where opening this vault goes. */
  path: string;
  /** What the record says, before live share counts are considered. */
  persistedSharingMode: VaultSharingMode;
  /**
   * Shared *now*: either the record says so, or shares are currently active.
   * A vault can hold live shares while its record still reads `private`, so
   * the live count wins.
   */
  isShared: boolean;
}

export function deriveVaultState(
  map: VaultRecordLike,
  activeShareCount: number,
): VaultState {
  const persistedSharingMode = normalizeSharingMode(map.vault_sharing_mode);
  const isBoard = map.vault_labels?.includes(BOARD_LABEL) ?? false;
  return {
    isBoard,
    path: isBoard ? `/boards/${map.id}` : `/vaults/${map.id}`,
    persistedSharingMode,
    isShared: activeShareCount > 0 || persistedSharingMode === 'shared',
  };
}

// ── Re-render rule for the two vault views ──────────────────────

export interface RenameContext {
  map: { id: string };
  renamingId: string | null;
  renameValue: string;
  renaming: boolean;
}

/**
 * Whether a vault's rename state is unchanged for `memo` purposes.
 *
 * The asymmetry is deliberate and easy to break: the vault *being* renamed has
 * to re-render on every keystroke, so it compares the draft text too. Every
 * other vault compares only which vault is being renamed — otherwise typing a
 * name re-renders the whole list on each character.
 */
export function sameRenameContext(prev: RenameContext, next: RenameContext): boolean {
  if (prev.renamingId !== prev.map.id) return prev.renamingId === next.renamingId;
  return prev.renamingId === next.renamingId
    && prev.renameValue === next.renameValue
    && prev.renaming === next.renaming;
}
