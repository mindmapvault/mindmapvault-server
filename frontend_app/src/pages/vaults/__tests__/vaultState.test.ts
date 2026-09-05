import { describe, expect, it } from 'vitest';
import {
  deriveVaultState,
  labelsEqual,
  normalizeEncryptionMode,
  normalizeHexColor,
  normalizeSharingMode,
  normalizeVaultLabels,
  sameRenameContext,
  vaultColorStorageKey,
} from '../vaultState';
import { BOARD_LABEL } from '../../../utils/vaultLabels';

/**
 * Characterisation tests: these assert what the page does today, so collapsing
 * the two copies of this derivation can be shown to change nothing.
 */

describe('normalizeHexColor', () => {
  it('keeps a six-digit hex colour, in either case', () => {
    expect(normalizeHexColor('#AABBCC')).toBe('#AABBCC');
    expect(normalizeHexColor('#aabbcc')).toBe('#aabbcc');
  });

  it('falls back for anything else', () => {
    for (const bad of [undefined, '', 'red', '#abc', '#gggggg', '#aabbccdd', 'aabbcc']) {
      expect(normalizeHexColor(bad)).toBe('#334155');
    }
  });
});

describe('normalizeSharingMode / normalizeEncryptionMode', () => {
  it('admits exactly one non-default value each', () => {
    expect(normalizeSharingMode('shared')).toBe('shared');
    expect(normalizeEncryptionMode('re-encrypted')).toBe('re-encrypted');
  });

  it('treats everything else as the safe default', () => {
    for (const bad of [undefined, '', 'Shared', 'public', 'private']) {
      expect(normalizeSharingMode(bad)).toBe('private');
    }
    for (const bad of [undefined, '', 'standard', 'reencrypted']) {
      expect(normalizeEncryptionMode(bad)).toBe('standard');
    }
  });
});

describe('normalizeVaultLabels', () => {
  it('trims, lower-cases and de-duplicates, keeping first-seen order', () => {
    expect(normalizeVaultLabels(['  Work ', 'work', 'HOME', 'work'])).toEqual(['work', 'home']);
  });

  it('drops empties and non-strings, and defends against a non-array', () => {
    expect(normalizeVaultLabels(['a', '', '   ', 7 as never, null as never])).toEqual(['a']);
    expect(normalizeVaultLabels(undefined)).toEqual([]);
    expect(normalizeVaultLabels('work' as never)).toEqual([]);
  });

  it('leaves reserved labels alone — they are stored like any other', () => {
    expect(normalizeVaultLabels([BOARD_LABEL, 'notes'])).toEqual([BOARD_LABEL, 'notes']);
  });
});

describe('labelsEqual', () => {
  it('is order-sensitive, because the list is a sequence the user arranges', () => {
    expect(labelsEqual(['a', 'b'], ['a', 'b'])).toBe(true);
    expect(labelsEqual(['a', 'b'], ['b', 'a'])).toBe(false);
    expect(labelsEqual(['a'], ['a', 'b'])).toBe(false);
    expect(labelsEqual([], [])).toBe(true);
  });
});

describe('vaultColorStorageKey', () => {
  /** Persisted in localStorage: changing the shape resets everyone's colours. */
  it('is the key existing installs already hold', () => {
    expect(vaultColorStorageKey('abc')).toBe('vault-color-abc');
  });
});

describe('deriveVaultState', () => {
  const vault = (over: Partial<Parameters<typeof deriveVaultState>[0]> = {}) =>
    ({ id: 'v1', ...over });

  it('routes an ordinary vault to the mind map editor', () => {
    const state = deriveVaultState(vault(), 0);
    expect(state.isBoard).toBe(false);
    expect(state.path).toBe('/vaults/v1');
  });

  it('routes a board to the board editor', () => {
    const state = deriveVaultState(vault({ vault_labels: [BOARD_LABEL] }), 0);
    expect(state.isBoard).toBe(true);
    expect(state.path).toBe('/boards/v1');
  });

  it('reads shared from the record', () => {
    expect(deriveVaultState(vault({ vault_sharing_mode: 'shared' }), 0).isShared).toBe(true);
  });

  /**
   * The case the two copies existed to get right: a vault whose record still
   * says private but which has live shares is shared.
   */
  it('lets a live share outrank a private record', () => {
    const state = deriveVaultState(vault({ vault_sharing_mode: 'private' }), 2);
    expect(state.persistedSharingMode).toBe('private');
    expect(state.isShared).toBe(true);
  });

  it('is private with no shares and nothing on the record', () => {
    expect(deriveVaultState(vault(), 0).isShared).toBe(false);
  });
});

describe('sameRenameContext', () => {
  const ctx = (over: Partial<Parameters<typeof sameRenameContext>[0]> = {}) => ({
    map: { id: 'v1' },
    renamingId: null as string | null,
    renameValue: '',
    renaming: false,
    ...over,
  });

  it('ignores the draft text for a vault that is not being renamed', () => {
    const prev = ctx({ renamingId: 'other', renameValue: 'a' });
    const next = ctx({ renamingId: 'other', renameValue: 'ab' });
    // Typing in another vault's name must not re-render this one.
    expect(sameRenameContext(prev, next)).toBe(true);
  });

  it('follows the draft text for the vault being renamed', () => {
    const prev = ctx({ renamingId: 'v1', renameValue: 'a' });
    const next = ctx({ renamingId: 'v1', renameValue: 'ab' });
    expect(sameRenameContext(prev, next)).toBe(false);
  });

  it('notices a rename starting or finishing on this vault', () => {
    expect(sameRenameContext(ctx({ renamingId: null }), ctx({ renamingId: 'v1' }))).toBe(false);
    expect(sameRenameContext(ctx({ renamingId: 'v1' }), ctx({ renamingId: null }))).toBe(false);
  });

  it('follows the in-flight flag while this vault is being renamed', () => {
    const prev = ctx({ renamingId: 'v1', renaming: false });
    const next = ctx({ renamingId: 'v1', renaming: true });
    expect(sameRenameContext(prev, next)).toBe(false);
  });
});
