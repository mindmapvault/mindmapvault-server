/**
 * The lobby's per-device vault metadata, decrypted for the current session.
 *
 * Local mode: hydrated from the encrypted blob once a profile is unlocked,
 * written back after every change, and dropped with the rest of the session
 * cache on logout. Server mode: the label library lives in the account
 * settings on the server and is held here in memory only; a vault's colour
 * and labels come with the vault record. Nothing is written to WebView storage
 * in either mode.
 *
 * The decrypted copy lives only here, in memory. The derived AES key is a
 * non-extractable `CryptoKey`; the master key itself is never kept.
 */

import { create } from 'zustand';
import accountApi from '../api/account';
import { importAesKey } from '../crypto/aes';
import { deriveLobbyMetaKey } from '../crypto/kdf';
import { fromBase64, randomBytes, toBase64 } from '../crypto/utils';
import { normalizeHexColor, normalizeVaultLabels } from '../pages/vaults/vaultState';
import {
  collectLegacyLobbyMeta,
  decryptLobbyMeta,
  defaultLabelColor,
  encryptLobbyMeta,
  LEGACY_USER_LABELS_KEY,
  lobbyMetaStorageKey,
  mergeLobbyMeta,
  normalizeLabelColor,
  normalizeLabelName,
  sanitizeUserLabels,
  type LobbyMeta,
  type UserLabel,
  type VaultLobbyMeta,
} from '../utils/lobbyMeta';
import { onSessionCacheClear } from '../utils/sessionCache';

export type LobbyMetaSource = 'none' | 'server' | 'encrypted';

interface LobbyMetaState {
  source: LobbyMetaSource;
  username: string | null;
  userLabels: UserLabel[];
  vaults: Record<string, VaultLobbyMeta>;
  /** See `LobbyMeta.previewKey`. */
  previewKey: string | null;

  /**
   * Local mode: load the profile's blob, folding in and removing any plaintext
   * keys an older build left. Safe to call repeatedly; one hydration runs at a
   * time.
   */
  hydrateEncrypted: (masterKey: Uint8Array, username: string) => Promise<void>;
  /**
   * Server mode: load the label library from the account settings. A failed
   * fetch leaves the store untouched, so a later call retries and nothing
   * half-loaded is ever pushed back to the server.
   */
  hydrateServer: () => Promise<void>;

  addUserLabel: (name: string, color?: string) => void;
  removeUserLabel: (name: string) => void;
  updateUserLabelColor: (name: string, color: string) => void;

  setVaultColor: (vaultId: string, color: string) => void;
  setVaultLabels: (vaultId: string, labels: string[]) => void;
  forgetVault: (vaultId: string) => void;

  /**
   * Local mode: the key the encrypted preview files are written with, minted
   * on first use. Null until a profile is unlocked, so previews are never
   * written without one.
   */
  ensurePreviewKey: () => Promise<CryptoKey | null>;

  /** Local mode: re-encrypt under a new master key, after a password change. */
  rotateKey: (oldMasterKey: Uint8Array, newMasterKey: Uint8Array, username: string) => Promise<void>;

  reset: () => void;
}

let cryptoKey: CryptoKey | null = null;
/** `previewKey` imported for use; cleared with the rest of the session. */
let previewCryptoKey: { b64: string; key: CryptoKey } | null = null;
let hydration: { kind: 'encrypted' | 'server'; username: string | null; promise: Promise<void> } | null = null;
/** Writes are serialised so a slow one cannot land after a newer one. */
let writeChain: Promise<void> = Promise.resolve();

function storage(): Storage | null {
  try {
    return typeof window === 'undefined' ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** The plaintext mirror older builds kept of the label library. */
function purgeLegacyUserLabels(): void {
  try {
    storage()?.removeItem(LEGACY_USER_LABELS_KEY);
  } catch {
    // Storage unavailable; nothing to purge.
  }
}

function snapshot(state: Pick<LobbyMetaState, 'userLabels' | 'vaults' | 'previewKey'>): LobbyMeta {
  const meta: LobbyMeta = { userLabels: state.userLabels, vaults: state.vaults };
  if (state.previewKey) meta.previewKey = state.previewKey;
  return meta;
}

function fromMeta(meta: LobbyMeta): Pick<LobbyMetaState, 'userLabels' | 'vaults' | 'previewKey'> {
  return { userLabels: meta.userLabels, vaults: meta.vaults, previewKey: meta.previewKey ?? null };
}

export const useLobbyMetaStore = create<LobbyMetaState>()((set, get) => {
  const persist = () => {
    const state = get();
    if (state.source === 'server') {
      const labels = state.userLabels;
      writeChain = writeChain
        .then(() => accountApi.updateSettings({ user_labels_json: JSON.stringify(labels) }))
        .then(() => undefined)
        .catch(() => {
          // Do not interrupt editing if settings sync fails.
        });
      return;
    }
    const store = storage();
    if (!store || state.source !== 'encrypted' || !cryptoKey || !state.username) return;
    const key = cryptoKey;
    const username = state.username;
    const meta = snapshot(state);
    writeChain = writeChain
      .then(async () => {
        const blob = await encryptLobbyMeta(meta, key);
        store.setItem(lobbyMetaStorageKey(username), blob);
      })
      .catch(() => {
        // Storage unavailable or crypto failed; the in-memory copy still serves the session.
      });
  };

  const update = (mutate: (meta: LobbyMeta) => LobbyMeta) => {
    set((state) => fromMeta(mutate(snapshot(state))));
    persist();
  };

  return {
    source: 'none',
    username: null,
    userLabels: [],
    vaults: {},
    previewKey: null,

    hydrateEncrypted: (masterKey, username) => {
      const state = get();
      if (state.source === 'encrypted' && state.username === username) return Promise.resolve();
      if (hydration && hydration.kind === 'encrypted' && hydration.username === username) return hydration.promise;

      const promise = (async () => {
        const key = await deriveLobbyMetaKey(masterKey);
        const store = storage();
        let meta: LobbyMeta = { userLabels: [], vaults: {} };
        let dirty = false;

        if (store) {
          const raw = store.getItem(lobbyMetaStorageKey(username));
          if (raw) {
            try {
              meta = await decryptLobbyMeta(raw, key);
            } catch {
              // Wrong key or damaged blob: start over rather than fail the lobby.
              dirty = true;
            }
          }
          const legacy = collectLegacyLobbyMeta(store);
          if (legacy.keys.length > 0) {
            meta = mergeLobbyMeta(meta, legacy.meta);
            legacy.keys.forEach((k) => store.removeItem(k));
            dirty = true;
          }
        }

        cryptoKey = key;
        set({ source: 'encrypted', username, ...fromMeta(meta) });
        if (dirty) persist();
      })().finally(() => {
        if (hydration?.kind === 'encrypted' && hydration.username === username) hydration = null;
      });

      hydration = { kind: 'encrypted', username, promise };
      return promise;
    },

    hydrateServer: () => {
      if (get().source === 'server') return Promise.resolve();
      if (hydration && hydration.kind === 'server') return hydration.promise;

      const promise = (async () => {
        const settings = await accountApi.getSettings();
        let labels: UserLabel[] = [];
        if (settings.user_labels_json) {
          try {
            labels = sanitizeUserLabels(JSON.parse(settings.user_labels_json));
          } catch {
            labels = [];
          }
        }
        cryptoKey = null;
        previewCryptoKey = null;
        set({ source: 'server', username: null, userLabels: labels, vaults: {}, previewKey: null });
        purgeLegacyUserLabels();
      })()
        .catch(() => {
          // Left as it was; the next mount tries again.
        })
        .finally(() => {
          if (hydration?.kind === 'server') hydration = null;
        });

      hydration = { kind: 'server', username: null, promise };
      return promise;
    },

    addUserLabel: (name, color) => {
      const trimmed = normalizeLabelName(name);
      if (!trimmed) return;
      update((meta) => {
        if (meta.userLabels.some((l) => l.name === trimmed)) return meta;
        return {
          ...meta,
          userLabels: [
            ...meta.userLabels,
            { name: trimmed, color: normalizeLabelColor(color, defaultLabelColor(meta.userLabels.length)) },
          ],
        };
      });
    },

    removeUserLabel: (name) => {
      update((meta) => ({ ...meta, userLabels: meta.userLabels.filter((l) => l.name !== name) }));
    },

    updateUserLabelColor: (name, color) => {
      const normalized = normalizeLabelColor(color, '#7c3aed');
      update((meta) => ({
        ...meta,
        userLabels: meta.userLabels.map((l) => (l.name === name ? { ...l, color: normalized } : l)),
      }));
    },

    setVaultColor: (vaultId, color) => {
      const normalized = normalizeHexColor(color);
      update((meta) => ({
        ...meta,
        vaults: { ...meta.vaults, [vaultId]: { ...meta.vaults[vaultId], color: normalized } },
      }));
    },

    setVaultLabels: (vaultId, labels) => {
      const normalized = normalizeVaultLabels(labels);
      update((meta) => ({
        ...meta,
        vaults: { ...meta.vaults, [vaultId]: { ...meta.vaults[vaultId], labels: normalized } },
      }));
    },

    forgetVault: (vaultId) => {
      update((meta) => {
        if (!(vaultId in meta.vaults)) return meta;
        const vaults = { ...meta.vaults };
        delete vaults[vaultId];
        return { ...meta, vaults };
      });
    },

    ensurePreviewKey: async () => {
      if (get().source !== 'encrypted') return null;
      let b64 = get().previewKey;
      if (!b64) {
        const minted = toBase64(randomBytes(32));
        b64 = minted;
        update((meta) => ({ ...meta, previewKey: minted }));
      }
      if (previewCryptoKey?.b64 !== b64) {
        previewCryptoKey = { b64, key: await importAesKey(fromBase64(b64)) };
      }
      return previewCryptoKey.key;
    },

    rotateKey: async (oldMasterKey, newMasterKey, username) => {
      if (get().source !== 'encrypted' || get().username !== username) {
        await get().hydrateEncrypted(oldMasterKey, username);
      }
      const state = get();
      if (state.source !== 'encrypted' || state.username !== username) return;
      cryptoKey = await deriveLobbyMetaKey(newMasterKey);
      persist();
      await writeChain;
    },

    reset: () => {
      cryptoKey = null;
      previewCryptoKey = null;
      hydration = null;
      set({ source: 'none', username: null, userLabels: [], vaults: {}, previewKey: null });
    },
  };
});

onSessionCacheClear(() => useLobbyMetaStore.getState().reset());

/** For tests: wait until every pending write has landed. */
export function flushLobbyMetaWrites(): Promise<void> {
  return writeChain;
}
