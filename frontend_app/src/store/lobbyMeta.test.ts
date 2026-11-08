// @vitest-environment jsdom
/**
 * The lobby metadata store against a stubbed `localStorage` and a mocked
 * account API: what lands on disk is encrypted, the plaintext keys of older
 * builds are migrated away, logout drops the decrypted copy, a password change
 * re-encrypts, and server mode never touches storage at all.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { deriveLobbyMetaKey } from '../crypto/kdf';
import { randomBytes } from '../crypto/utils';
import { decryptLobbyMeta, lobbyMetaStorageKey } from '../utils/lobbyMeta';
import { clearSessionCache } from '../utils/sessionCache';
import accountApi from '../api/account';
import { flushLobbyMetaWrites, useLobbyMetaStore } from './lobbyMeta';

vi.mock('../api/account', () => ({
  default: {
    getSettings: vi.fn(),
    updateSettings: vi.fn(),
  },
}));

const mockedAccount = vi.mocked(accountApi);

function createStorage(entries: Record<string, string> = {}): Storage {
  const data = new Map<string, string>(Object.entries(entries));
  return {
    get length() {
      return data.size;
    },
    key: (index: number) => Array.from(data.keys())[index] ?? null,
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, String(value));
    },
    removeItem: (key: string) => {
      data.delete(key);
    },
    clear: () => {
      data.clear();
    },
  } as Storage;
}

const masterKey = randomBytes(32);
const store = () => useLobbyMetaStore.getState();

/** Everything currently in storage, as one string, to search for leaks. */
function storageDump(): string {
  const parts: string[] = [];
  for (let i = 0; i < window.localStorage.length; i += 1) {
    const key = window.localStorage.key(i)!;
    parts.push(`${key}=${window.localStorage.getItem(key)}`);
  }
  return parts.join('\n');
}

beforeEach(() => {
  vi.stubGlobal('localStorage', createStorage());
  mockedAccount.getSettings.mockReset();
  mockedAccount.updateSettings.mockReset();
  mockedAccount.updateSettings.mockResolvedValue({} as never);
  store().reset();
});

afterEach(async () => {
  await flushLobbyMetaWrites();
});

describe('hydrateEncrypted', () => {
  it('migrates the plaintext keys into an encrypted blob and removes them', async () => {
    window.localStorage.setItem('user-labels', JSON.stringify([{ name: 'divorce', color: '#ef4444' }]));
    window.localStorage.setItem('vault-color-v1', '#abcdef');
    window.localStorage.setItem('vault-labels-v1', JSON.stringify(['divorce']));
    window.localStorage.setItem('mmv-lobby-view', 'table');

    await store().hydrateEncrypted(masterKey, 'alice');
    await flushLobbyMetaWrites();

    expect(store().source).toBe('encrypted');
    expect(store().userLabels).toEqual([{ name: 'divorce', color: '#ef4444' }]);
    expect(store().vaults).toEqual({ v1: { color: '#abcdef', labels: ['divorce'] } });

    expect(window.localStorage.getItem('user-labels')).toBeNull();
    expect(window.localStorage.getItem('vault-color-v1')).toBeNull();
    expect(window.localStorage.getItem('vault-labels-v1')).toBeNull();
    expect(window.localStorage.getItem('mmv-lobby-view')).toBe('table');

    const blob = window.localStorage.getItem(lobbyMetaStorageKey('alice'));
    expect(blob).toBeTruthy();
    expect(storageDump()).not.toContain('divorce');
    expect(storageDump()).not.toContain('abcdef');
    expect(await decryptLobbyMeta(blob!, await deriveLobbyMetaKey(masterKey))).toEqual({
      userLabels: [{ name: 'divorce', color: '#ef4444' }],
      vaults: { v1: { color: '#abcdef', labels: ['divorce'] } },
    });
  });

  it('writes nothing when there is nothing to migrate', async () => {
    await store().hydrateEncrypted(masterKey, 'alice');
    await flushLobbyMetaWrites();
    expect(window.localStorage.length).toBe(0);
  });

  it('starts over when the blob cannot be decrypted', async () => {
    window.localStorage.setItem(lobbyMetaStorageKey('alice'), 'bm90IGEgcmVhbCBibG9i');
    await store().hydrateEncrypted(masterKey, 'alice');
    expect(store().source).toBe('encrypted');
    expect(store().userLabels).toEqual([]);
    expect(store().vaults).toEqual({});
  });

  it('runs one hydration for concurrent callers', async () => {
    const first = store().hydrateEncrypted(masterKey, 'alice');
    const second = store().hydrateEncrypted(masterKey, 'alice');
    await Promise.all([first, second]);
    expect(store().source).toBe('encrypted');
  });
});

describe('changes in local mode', () => {
  it('persist encrypted and read back after the session is cleared', async () => {
    await store().hydrateEncrypted(masterKey, 'alice');
    store().addUserLabel(' Taxes ', '#F59E0B');
    store().setVaultColor('v1', '#abcdef');
    store().setVaultLabels('v1', ['Taxes', 'taxes']);
    store().setVaultLabels('v2', ['home']);
    store().forgetVault('v2');
    await flushLobbyMetaWrites();

    expect(storageDump()).not.toContain('taxes');
    expect(storageDump()).not.toContain('abcdef');
    expect(mockedAccount.updateSettings).not.toHaveBeenCalled();

    clearSessionCache();
    expect(store().source).toBe('none');
    expect(store().userLabels).toEqual([]);
    expect(store().vaults).toEqual({});

    await store().hydrateEncrypted(masterKey, 'alice');
    expect(store().userLabels).toEqual([{ name: 'taxes', color: '#f59e0b' }]);
    expect(store().vaults).toEqual({ v1: { color: '#abcdef', labels: ['taxes'] } });
  });

  it('keep profiles apart', async () => {
    await store().hydrateEncrypted(masterKey, 'alice');
    store().addUserLabel('alice-only');
    await flushLobbyMetaWrites();
    clearSessionCache();

    await store().hydrateEncrypted(randomBytes(32), 'bob');
    expect(store().userLabels).toEqual([]);
    expect(window.localStorage.getItem(lobbyMetaStorageKey('alice'))).toBeTruthy();
  });
});

describe('ensurePreviewKey', () => {
  it('is minted once, stored in the blob, and comes back after a re-hydrate', async () => {
    expect(await store().ensurePreviewKey()).toBeNull();

    await store().hydrateEncrypted(masterKey, 'alice');
    const first = await store().ensurePreviewKey();
    const minted = store().previewKey;
    expect(first).toBeInstanceOf(CryptoKey);
    expect(minted).toBeTruthy();
    expect(await store().ensurePreviewKey()).toBe(first);
    await flushLobbyMetaWrites();
    expect(storageDump()).not.toContain(minted!);

    clearSessionCache();
    await store().hydrateEncrypted(masterKey, 'alice');
    expect(store().previewKey).toBe(minted);
    expect(await store().ensurePreviewKey()).toBeInstanceOf(CryptoKey);
  });

  it('is not available in server mode', async () => {
    mockedAccount.getSettings.mockResolvedValue({ user_labels_json: null } as never);
    await store().hydrateServer();
    expect(await store().ensurePreviewKey()).toBeNull();
  });
});

describe('rotateKey', () => {
  it('re-encrypts the blob so the new master key reads it and the old one does not', async () => {
    await store().hydrateEncrypted(masterKey, 'alice');
    store().addUserLabel('work');
    await flushLobbyMetaWrites();

    const newMasterKey = randomBytes(32);
    await store().rotateKey(masterKey, newMasterKey, 'alice');

    const blob = window.localStorage.getItem(lobbyMetaStorageKey('alice'))!;
    await expect(decryptLobbyMeta(blob, await deriveLobbyMetaKey(masterKey))).rejects.toThrow();
    expect(await decryptLobbyMeta(blob, await deriveLobbyMetaKey(newMasterKey))).toEqual({
      userLabels: [{ name: 'work', color: '#7c3aed' }],
      vaults: {},
    });

    clearSessionCache();
    await store().hydrateEncrypted(newMasterKey, 'alice');
    expect(store().userLabels).toEqual([{ name: 'work', color: '#7c3aed' }]);
  });

  /** The password form can run before the lobby was opened this session. */
  it('hydrates with the old key first when nothing is loaded', async () => {
    await store().hydrateEncrypted(masterKey, 'alice');
    store().setVaultColor('v1', '#abcdef');
    await flushLobbyMetaWrites();
    clearSessionCache();

    const newMasterKey = randomBytes(32);
    await store().rotateKey(masterKey, newMasterKey, 'alice');

    clearSessionCache();
    await store().hydrateEncrypted(newMasterKey, 'alice');
    expect(store().vaults).toEqual({ v1: { color: '#abcdef' } });
  });
});

describe('server mode', () => {
  it('loads the library from the account settings and drops the plaintext mirror', async () => {
    window.localStorage.setItem('user-labels', JSON.stringify([{ name: 'stale', color: '#000000' }]));
    mockedAccount.getSettings.mockResolvedValue({
      user_labels_json: JSON.stringify([{ name: 'Work', color: '#3B82F6' }]),
    } as never);

    await store().hydrateServer();

    expect(store().source).toBe('server');
    expect(store().userLabels).toEqual([{ name: 'work', color: '#3b82f6' }]);
    expect(window.localStorage.getItem('user-labels')).toBeNull();
  });

  it('pushes changes to the server and writes nothing to storage', async () => {
    mockedAccount.getSettings.mockResolvedValue({ user_labels_json: null } as never);
    await store().hydrateServer();

    store().addUserLabel('todo', '#10b981');
    await flushLobbyMetaWrites();

    expect(mockedAccount.updateSettings).toHaveBeenCalledWith({
      user_labels_json: JSON.stringify([{ name: 'todo', color: '#10b981' }]),
    });
    expect(window.localStorage.length).toBe(0);
  });

  it('leaves the store alone when the fetch fails, so a retry can load it', async () => {
    mockedAccount.getSettings.mockRejectedValueOnce(new Error('offline'));
    await store().hydrateServer();
    expect(store().source).toBe('none');

    mockedAccount.getSettings.mockResolvedValue({
      user_labels_json: JSON.stringify([{ name: 'work', color: '#3b82f6' }]),
    } as never);
    await store().hydrateServer();
    expect(store().source).toBe('server');
    expect(store().userLabels).toEqual([{ name: 'work', color: '#3b82f6' }]);
  });

  it('is dropped on logout like the local copy', async () => {
    mockedAccount.getSettings.mockResolvedValue({
      user_labels_json: JSON.stringify([{ name: 'work', color: '#3b82f6' }]),
    } as never);
    await store().hydrateServer();
    clearSessionCache();
    expect(store().source).toBe('none');
    expect(store().userLabels).toEqual([]);
  });
});
