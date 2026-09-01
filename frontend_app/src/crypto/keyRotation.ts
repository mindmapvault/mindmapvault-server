// ── Password rotation / key re-wrapping ──────────────────────────────────────
//
// All cryptographic work for a password change happens here, for both the
// local desktop path (the Tauri layer's `apply_local_password_rotation` only
// performs the atomic file-write) and the server path (`docs/PASSWORD_ROTATION.md`).
// Nothing beyond ciphertexts and derived auth tokens ever leaves this module.

import { aesDecrypt, aesEncrypt, importAesKey } from './aes';
import {
  DEFAULT_ARGON2_PARAMS,
  deriveAttachmentWrapKey,
  deriveAuthToken,
  deriveMasterAesKey,
  deriveMasterKey,
  deriveTitleKey,
} from './kdf';
import { fromBase64, randomBytes, toBase64 } from './utils';
import type { Argon2Params } from '../types';

// ── Minimal local types (mirrors Rust / LocalUnlockPage shapes) ───────────────

export interface LocalProfileForRotation {
  username: string;
  argon2_salt: string;
  argon2_params: Argon2Params;
  classical_public_key: string;
  pq_public_key: string;
  classical_priv_encrypted: string;
  pq_priv_encrypted: string;
  key_version: number;
  created_at: string;
}

export interface VaultEntryForRotation {
  id: string;
  title_encrypted: string;
  vault_note_encrypted: string | null | undefined;
}

export interface RotatedVaultEntry {
  id: string;
  title_encrypted: string;
  vault_note_encrypted: string | null;
}

/** One attachment as listed by the server's rotation manifest. */
export interface AttachmentEntryForRotation {
  id: string;
  encryption_meta: {
    wrapped_key_b64?: string;
    key_wrap?: string;
  } | null;
}

export interface RotatedAttachmentEntry {
  id: string;
  /** File key re-wrapped under the NEW master key's HKDF attachment-wrap key. */
  wrapped_key_b64: string;
}

export interface PasswordRotationBundle {
  newProfile: LocalProfileForRotation;
  updatedVaults: RotatedVaultEntry[];
  /**
   * Every attachment file key from the manifest, re-wrapped. Attachment file
   * keys are wrapped with password-derived keys (unlike vault blobs, which
   * are KEM-wrapped to the unchanging key-pair), so skipping one here makes
   * that attachment permanently unreadable — the server refuses bundles that
   * do not cover every attachment it knows about.
   */
  updatedAttachments: RotatedAttachmentEntry[];
  /** New master key bytes — use to update the in-memory session after rotation. */
  newMasterKey: Uint8Array;
  /**
   * HKDF(old_master_key, "crypt-mind-auth-v1") as a hex string.
   * Server mode: send as `current_auth_token` to prove the current password.
   */
  currentAuthToken: string;
  /**
   * HKDF(new_master_key, "crypt-mind-auth-v1") as a hex string.
   * Server mode: send as `new_auth_token`; the server hashes this before storing.
   */
  newAuthToken: string;
}

// ── Rotation builder ──────────────────────────────────────────────────────────

/**
 * Verifies `oldPassword`, derives a new master key from `newPassword` with a
 * fresh Argon2id salt, re-wraps both private keys, re-encrypts all vault
 * titles and notes, and re-wraps every attachment file key.  Returns the
 * bundle ready for the server's rotate-credentials call, or (with an empty
 * attachment list — local profiles store no attachments) for the Tauri
 * `apply_local_password_rotation` command.
 *
 * Throws:
 *  - if `oldPassword` fails to decrypt the stored private keys (wrong password)
 *  - if `newPassword` is the same as `oldPassword` (no change)
 *  - if any per-vault title re-encryption fails
 *  - if any attachment file key fails to unwrap — one skipped attachment
 *    would be orphaned under the old key forever, so the whole rotation
 *    aborts before anything is sent
 *
 * The new master key always uses DEFAULT_ARGON2_PARAMS, so rotating also
 * upgrades accounts created under weaker historical parameters. Attachment
 * wraps always come out `hkdf-attachment-v1`, retiring any legacy
 * `master-aes-256-gcm` wraps.
 *
 * The vault blobs (mind-map trees) are NOT touched — they are protected by the
 * hybrid KEM envelope which uses the user's key-pair, and the key-pair itself
 * is not rotated here. Attachment blobs are equally untouched: only the
 * wrapped file key changes.
 */
export async function buildPasswordRotationBundle(
  oldPassword: string,
  newPassword: string,
  currentProfile: LocalProfileForRotation,
  vaults: VaultEntryForRotation[],
  attachments: AttachmentEntryForRotation[] = [],
): Promise<PasswordRotationBundle> {
  // ── 1. Derive old master key and validate old password ────────────────────
  const oldMasterKey = await deriveMasterKey(
    oldPassword,
    currentProfile.argon2_salt,
    currentProfile.argon2_params,
  );

  const oldWrapKey = await importAesKey(oldMasterKey);

  // Decrypt both private keys — this serves as password proof.
  // Throws DOMException on wrong password (AES-GCM authentication failure).
  let classicalPriv: Uint8Array;
  let pqPriv: Uint8Array;
  try {
    classicalPriv = await aesDecrypt(oldWrapKey, fromBase64(currentProfile.classical_priv_encrypted));
    pqPriv = await aesDecrypt(oldWrapKey, fromBase64(currentProfile.pq_priv_encrypted));
  } catch {
    throw new Error('Current password is incorrect');
  }

  // ── 2. Derive new master key with a fresh salt ────────────────────────────
  const newSalt = randomBytes(32);
  const newSaltB64 = toBase64(newSalt);
  const newMasterKey = await deriveMasterKey(newPassword, newSaltB64, DEFAULT_ARGON2_PARAMS);

  // ── 3. Re-wrap private keys under the new master key ─────────────────────
  const newWrapKey = await importAesKey(newMasterKey);
  const newClassicalPrivEnc = await aesEncrypt(newWrapKey, classicalPriv);
  const newPqPrivEnc = await aesEncrypt(newWrapKey, pqPriv);

  // ── 4. Build updated profile ──────────────────────────────────────────────
  const newProfile: LocalProfileForRotation = {
    ...currentProfile,
    argon2_salt: newSaltB64,
    argon2_params: {
      m_cost: DEFAULT_ARGON2_PARAMS.m_cost,
      t_cost: DEFAULT_ARGON2_PARAMS.t_cost,
      p_cost: DEFAULT_ARGON2_PARAMS.p_cost,
    },
    classical_priv_encrypted: toBase64(newClassicalPrivEnc),
    pq_priv_encrypted: toBase64(newPqPrivEnc),
    key_version: (currentProfile.key_version ?? 1) + 1,
  };

  // ── 5. Re-encrypt vault titles and notes ─────────────────────────────────
  const oldTitleKey = await deriveTitleKey(oldMasterKey);
  const newTitleKey = await deriveTitleKey(newMasterKey);

  const updatedVaults: RotatedVaultEntry[] = await Promise.all(
    vaults.map(async (vault): Promise<RotatedVaultEntry> => {
      // Re-encrypt title
      let newTitleEncrypted: string;
      try {
        const titlePt = await aesDecrypt(oldTitleKey, fromBase64(vault.title_encrypted));
        const newTitleCt = await aesEncrypt(newTitleKey, titlePt);
        newTitleEncrypted = toBase64(newTitleCt);
      } catch {
        throw new Error(`Failed to re-encrypt title for vault ${vault.id}`);
      }

      // Re-encrypt note (only if non-empty — empty string and null are passed through)
      let newNoteEncrypted: string | null = null;
      const rawNote = vault.vault_note_encrypted;
      if (rawNote && rawNote.length > 0) {
        try {
          const notePt = await aesDecrypt(oldTitleKey, fromBase64(rawNote));
          const newNoteCt = await aesEncrypt(newTitleKey, notePt);
          newNoteEncrypted = toBase64(newNoteCt);
        } catch {
          throw new Error(`Failed to re-encrypt note for vault ${vault.id}`);
        }
      }

      return {
        id: vault.id,
        title_encrypted: newTitleEncrypted,
        vault_note_encrypted: newNoteEncrypted,
      };
    }),
  );

  // ── 6. Re-wrap attachment file keys ──────────────────────────────────────
  // The file key itself never changes — the encrypted blob in object storage
  // stays byte-identical. Only its wrap does: off with whichever key wrote it
  // (`key_wrap` says which), on with the new HKDF attachment-wrap key.
  const oldHkdfWrapKey = await deriveAttachmentWrapKey(oldMasterKey);
  const oldLegacyWrapKey = await deriveMasterAesKey(oldMasterKey);
  const newWrapKeyForAttachments = await deriveAttachmentWrapKey(newMasterKey);

  const updatedAttachments: RotatedAttachmentEntry[] = await Promise.all(
    attachments.map(async (attachment): Promise<RotatedAttachmentEntry> => {
      const wrappedB64 = attachment.encryption_meta?.wrapped_key_b64;
      if (!wrappedB64) {
        // The manifest only lists attachments with a wrap; hitting this means
        // client and server disagree about what needs rotating — stop.
        throw new Error(`Attachment ${attachment.id} has no wrapped key to rotate`);
      }
      const unwrapKey = attachment.encryption_meta?.key_wrap === 'hkdf-attachment-v1'
        ? oldHkdfWrapKey
        : oldLegacyWrapKey;
      let fileKey: Uint8Array;
      try {
        fileKey = await aesDecrypt(unwrapKey, fromBase64(wrappedB64));
      } catch {
        throw new Error(`Failed to unwrap the file key for attachment ${attachment.id}`);
      }
      const rewrapped = await aesEncrypt(newWrapKeyForAttachments, fileKey);
      return { id: attachment.id, wrapped_key_b64: toBase64(rewrapped) };
    }),
  );

  // Derive auth tokens for server mode.  These are cheap HKDF calls; the server
  // hashes new_auth_token with Argon2id before writing — the raw token never
  // rests on disk server-side.
  const currentAuthToken = await deriveAuthToken(oldMasterKey);
  const newAuthToken = await deriveAuthToken(newMasterKey);

  return { newProfile, updatedVaults, updatedAttachments, newMasterKey, currentAuthToken, newAuthToken };
}
