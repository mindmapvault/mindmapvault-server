/**
 * Remembering a device, without handing the server a key.
 *
 * The master key is what opens everything, and it is normally derived from the
 * passphrase on every unlock. A trusted device keeps a *copy* of it, encrypted
 * under an AES key that lives in this browser's IndexedDB and is created
 * `extractable: false` — so it can encrypt and decrypt, but its bytes cannot be
 * read out by any script, exported, or sent anywhere.
 *
 * The server therefore stores ciphertext it has no way to open, which is the
 * line between this and key escrow.
 *
 * What this does *not* protect against: anyone who has the browser profile
 * itself. That is the trade being made, it is why this is opt-in per device and
 * revocable from the account settings, and it is no weaker than leaving a
 * session signed in.
 */

const DB_NAME = 'mindmapvault-device';
const STORE = 'keys';

/** One record per account, so signing in as someone else on the same browser
 *  cannot reach the first account's key. */
interface DeviceRecord {
  /** Keyed by user id. */
  userId: string;
  /** The id this device is known by on the server. */
  methodId: string;
  key: CryptoKey;
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(STORE)) {
        db.createObjectStore(STORE, { keyPath: 'userId' });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function withStore<T>(
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await openDb();
  try {
    return await new Promise<T>((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const request = run(tx.objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

/**
 * Is this browser storing a device key for this account?
 *
 * Private windows, cleared site data and browsers that refuse IndexedDB all
 * land here as "no", which is the right answer: the unlock screen simply asks
 * for the passphrase.
 */
export async function loadDeviceRecord(userId: string): Promise<DeviceRecord | null> {
  try {
    const found = await withStore<DeviceRecord | undefined>('readonly', (store) =>
      store.get(userId),
    );
    return found ?? null;
  } catch {
    return null;
  }
}

export async function forgetDevice(userId: string): Promise<void> {
  try {
    await withStore('readwrite', (store) => store.delete(userId) as IDBRequest<undefined>);
  } catch {
    // Nothing to do: the record either never existed or the store is
    // unavailable, and both mean the passphrase will be asked for.
  }
}

/**
 * Creates this device's key and encrypts the master key under it.
 *
 * Returns what the server should store. The key itself stays in IndexedDB.
 */
export async function trustThisDevice(
  userId: string,
  masterKey: Uint8Array,
): Promise<{ methodId: string; wrappedMasterKey: string; label: string }> {
  const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, [
    'encrypt',
    'decrypt',
  ]);

  const methodId = crypto.randomUUID();
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ciphertext = new Uint8Array(
    await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, masterKey as BufferSource),
  );

  // Nonce first, then ciphertext, in one blob — the server treats it as opaque
  // and only this browser ever splits it again.
  const packed = new Uint8Array(iv.length + ciphertext.length);
  packed.set(iv, 0);
  packed.set(ciphertext, iv.length);

  await withStore('readwrite', (store) =>
    store.put({ userId, methodId, key } satisfies DeviceRecord) as IDBRequest<IDBValidKey>,
  );

  return {
    methodId,
    wrappedMasterKey: btoa(String.fromCharCode(...packed)),
    label: describeThisBrowser(),
  };
}

/**
 * Decrypts a stored master key with this device's key.
 *
 * Returns `null` rather than throwing when it cannot: the usual reason is that
 * the account's passphrase was rotated, which leaves the stored copy encrypted
 * under a key that no longer exists. The caller falls back to asking.
 */
export async function unwrapWithDevice(
  record: DeviceRecord,
  wrappedMasterKey: string,
): Promise<Uint8Array | null> {
  try {
    const packed = Uint8Array.from(atob(wrappedMasterKey), (c) => c.charCodeAt(0));
    if (packed.length <= 12) return null;
    const iv = packed.slice(0, 12);
    const ciphertext = packed.slice(12);
    const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, record.key, ciphertext);
    return new Uint8Array(plain);
  } catch {
    return null;
  }
}

/** A name a person can recognise in a list of trusted devices. */
function describeThisBrowser(): string {
  const agent = navigator.userAgent;
  const browser = /Edg\//.test(agent)
    ? 'Edge'
    : /Chrome\//.test(agent)
      ? 'Chrome'
      : /Safari\//.test(agent)
        ? 'Safari'
        : /Firefox\//.test(agent)
          ? 'Firefox'
          : 'Browser';
  const platform = /Windows/.test(agent)
    ? 'Windows'
    : /Mac OS X/.test(agent)
      ? 'macOS'
      : /Android/.test(agent)
        ? 'Android'
        : /iPhone|iPad/.test(agent)
          ? 'iOS'
          : /Linux/.test(agent)
            ? 'Linux'
            : '';
  return platform ? `${browser} on ${platform}` : browser;
}

/**
 * The account id the current access token was issued for.
 *
 * Device records are keyed on it rather than on the username, because a
 * federated account is renamed at enrolment and the username is not stable.
 * Only the `sub` claim is read — the token's signature is the server's business,
 * and nothing here trusts it for anything but choosing a local storage key.
 */
export function accountIdFromToken(accessToken: string | null): string | null {
  if (!accessToken) return null;
  const payload = accessToken.split('.')[1];
  if (!payload) return null;
  try {
    const json = atob(payload.replace(/-/g, '+').replace(/_/g, '/'));
    const claims = JSON.parse(json) as { sub?: unknown };
    return typeof claims.sub === 'string' && claims.sub ? claims.sub : null;
  } catch {
    return null;
  }
}
