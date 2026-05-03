# Password Rotation — Implementation Reference

This document describes how server-mode password rotation (key rotation) works in MindMapVault Server.
It covers the cryptographic design, the safety properties, the API contract, and the places to look when debugging or extending the feature.

---

## Overview

When a user changes their password, the following must happen atomically:

1. A new master key is derived from the new password.
2. Both private keys are re-wrapped under the new master key.
3. Every vault title and note is re-encrypted under a new title key derived from the new master key.
4. The new credential hash and the re-encrypted private keys are written to the `users` row.
5. All vault title/note updates land in the same database transaction as step 4.

Vault blobs stored in MinIO/S3 are **not touched**. They are KEM-encrypted to the user's keypair, which is not rotated. Every historical blob version remains decryptable after a successful rotation.

---

## Key Hierarchy

```
password + salt → Argon2id → masterKey (32 bytes, never leaves client)
                                │
                 ┌──────────────┼────────────────────┐
                 ▼              ▼                    ▼
         importAesKey     deriveTitleKey        deriveAuthToken
         (wrap privkeys)  (encrypt titles)      (authenticate to server)
```

- `masterKey` — raw 32-byte Argon2id output. Never sent to server.
- `importAesKey(masterKey)` — AES-GCM key used to wrap/unwrap the classical and PQ private keys.
- `deriveTitleKey(masterKey)` — HKDF-SHA256 with `info="crypt-mind-title-v1"`. Used to encrypt/decrypt vault titles and notes.
- `deriveAuthToken(masterKey)` — HKDF-SHA256 with `info="crypt-mind-auth-v1"`, 32 bytes, hex-encoded. Sent to the server instead of the password. The server hashes this with Argon2id before storing.

---

## Client-side rotation flow (`buildPasswordRotationBundle`)

Source: `frontend_app/src/crypto/keyRotation.ts`

```
1. Derive oldMasterKey from currentPassword + profile.argon2_salt
2. Decrypt classical_priv_encrypted and pq_priv_encrypted with AES-GCM(oldMasterKey)
   → This proves currentPassword is correct. Throws on wrong password.
3. Generate newSalt = randomBytes(16)
4. Derive newMasterKey from newPassword + newSalt
5. Re-encrypt both private keys with AES-GCM(newMasterKey)
6. Derive oldTitleKey and newTitleKey
7. For each vault: decrypt title (+ note) with oldTitleKey, re-encrypt with newTitleKey
8. Derive currentAuthToken = deriveAuthToken(oldMasterKey)
9. Derive newAuthToken     = deriveAuthToken(newMasterKey)
10. Return bundle: { newProfile, updatedVaults, newMasterKey, currentAuthToken, newAuthToken }
```

The bundle is constructed entirely in the browser/WebView. The server receives only ciphertexts and auth tokens — never the password or any plaintext key material.

---

## Server-mode `ChangePasswordPage` flow

Source: `frontend_app/src/pages/ChangePasswordPage.tsx`

```
1. GET /api/auth/keys
   → Returns classical_pub, pq_pub, classical_priv_encrypted, pq_priv_encrypted,
     argon2_salt, argon2_params, key_version.
2. GET /api/mindmaps (via ServerStorageAdapter.listVaults())
   → Returns list of { id, title_encrypted, vault_note_encrypted } for every vault.
3. buildPasswordRotationBundle(currentPassword, newPassword, profile, vaults)
   → Full client-side crypto (see above).
4. POST /api/auth/rotate-credentials (see API contract below)
5. On success: update sessionKeys.masterKey + setTokens(newAccessToken, newRefreshToken)
```

---

## API contract: `POST /api/auth/rotate-credentials`

Requires JWT authentication.

### Request

```json
{
  "current_auth_token": "<64-char hex>",
  "new_auth_token":     "<64-char hex>",
  "new_argon2_salt":    "<base64>",
  "new_argon2_params":  { "m_cost": 65536, "t_cost": 3, "p_cost": 4 },
  "new_classical_priv_encrypted": "<base64>",
  "new_pq_priv_encrypted":        "<base64>",
  "new_key_version":   3,
  "updated_vaults": [
    {
      "id":                    "<uuid>",
      "title_encrypted":       "<base64>",
      "vault_note_encrypted":  "<base64 or null>"
    }
  ]
}
```

`updated_vaults` must contain **every** vault owned by the user. The server rejects partial bundles.

`new_key_version` must equal `current_key_version + 1`. The server rejects version skew.

### Response (200)

```json
{
  "ok": true,
  "access_token":  "<new JWT>",
  "refresh_token": "<new JWT>"
}
```

Fresh tokens are returned so the client session stays valid without requiring a re-login.

### Error cases

| Status | Condition |
|--------|-----------|
| 400 | `new_auth_token` is not a 64-char hex string |
| 400 | `new_argon2_salt` is empty |
| 400 | `new_key_version` is not `current + 1` |
| 400 | One or more vaults missing from `updated_vaults` |
| 401 | `current_auth_token` does not match stored hash |
| 404 | User not found |

---

## Backend implementation: safety properties

Source: `backend/src/routes/auth_sql.rs`, `backend/src/db/postgres.rs`

### Re-verification of current password

`current_auth_token` is verified against `auth_hash` in `verify_auth_token()` even though the request already carries a valid JWT. This prevents a stolen session token from being used to rotate credentials.

### Complete vault coverage check

Before any database writes, the handler calls `db.list_mind_maps(user_id)` and computes the set difference with `updated_vaults`. Any missing vault ID causes an immediate 400 rejection. The database is never touched on a partial bundle.

### Atomic PostgreSQL transaction

The `rotate_user_credentials` method in `postgres.rs` uses an explicit `BEGIN`/`COMMIT`/`ROLLBACK` sequence because the connection is held behind `Arc<Client>` and `client.transaction()` is unavailable. All operations execute on the same connection, serialised. Either every write commits or the database is rolled back to its pre-rotation state.

The `auth_hash` for the new auth token is computed with a fresh Argon2id salt **before** the transaction opens, so the expensive hash does not hold the transaction open.

### Vault blob versions are never modified

Object versions stored in MinIO/S3 are KEM-encrypted envelopes addressed to the user's classical and PQ public keys. The keypair is unchanged by password rotation. All existing blob versions — including every entry in `version_history` — remain decryptable after a successful rotation.

---

## Debugging checklist

| Symptom | Likely cause |
|---------|-------------|
| 400 "rotation bundle is incomplete" | Client vault list and server vault list diverged (race: vault created during rotation). Retry. |
| 400 "new_key_version must be N" | Another session already rotated credentials. Re-login to refresh key_version, then retry. |
| 401 from rotate-credentials | Wrong current password entered. |
| Title decryption fails after rotation | `buildPasswordRotationBundle` succeeded but `POST /api/auth/rotate-credentials` was never called or returned an error. The DB was not written. Re-run the rotation. |
| Vault blob unreadable after rotation | Should not happen — blob key wrap uses the keypair, not the master key. File a bug. |

---

## Related files

| File | Role |
|------|------|
| `frontend_app/src/crypto/keyRotation.ts` | Client-side rotation bundle builder |
| `frontend_app/src/crypto/kdf.ts` | `deriveAuthToken`, `deriveTitleKey`, `deriveMasterKey` |
| `frontend_app/src/pages/ChangePasswordPage.tsx` | UI — local and server mode branches |
| `frontend_app/src/api/auth.ts` | `getKeyBundle()`, `rotateCredentials()` |
| `backend/src/routes/auth_sql.rs` | Route handler, coverage check, token reissue |
| `backend/src/db/postgres.rs` | `rotate_user_credentials` — transactional DB writes |
| `backend/src/db/sql_store.rs` | `RotateCredentialsUpdate`, `RotateVaultEntry` structs, trait method |
| `backend/src/models/user.rs` | `KeyBundleResponse`, `RotateCredentialsRequest`, `RotateVaultApiEntry` |
