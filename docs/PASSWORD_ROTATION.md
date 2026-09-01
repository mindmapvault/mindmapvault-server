# Password Rotation in the Web App

How changing a password must work in MindMapVault Server's web application,
why the shipped implementation is disabled, and the design that makes it safe
to turn on. Everything in this document was verified against the code it
cites; where the code and an earlier version of this document disagreed, the
code won.

**Status: implemented (2026-09-01).** The design below is built: the
settings hub's Account tab and `/change-password` (server mode) both drive
`PasswordRotationForm`, the `ROTATION_ENABLED` kill switch is gone, and the
release gate is `tests/endpoints/password-rotation.mjs` (server contract,
51 checks), `tests/ui/password-rotation.mjs` (the real client crypto in a
browser, legacy-wrap upgrade included), and
`tests/ui/rotation-multi-account.mjs` (three accounts × 20 attachments each:
non-rotating accounts byte-identical, a cross-account forgery refused,
parallel rotations, and a decrypt-everything sweep under the new passwords). Sections 2–3 are kept in the past
tense they were written in — they document why the previous implementation
was disabled and what each mechanism below exists to prevent.

---

## 1. What the password actually protects

Everything starts from one secret that never leaves the browser:

```
password + argon2_salt ──Argon2id──▶ masterKey (32 bytes)
```

From `masterKey`, four keys are derived (`frontend_app/src/crypto/kdf.ts`):

| Key | Derivation | Used for |
|---|---|---|
| auth token | HKDF, info `crypt-mind-auth-v1` | sent instead of the password; server stores `Argon2id(auth_token)` |
| private-key wrap | raw masterKey imported as AES-256-GCM | wraps `classical_priv_encrypted` and `pq_priv_encrypted` |
| title key | HKDF, info `crypt-mind-title-v1` | every vault and board title, every vault note |
| attachment wrap key | HKDF, info `crypt-mind-attachment-wrap-v1` | wraps each attachment's random file key |

Attachments written before v0.3.22 use the **raw masterKey** as the wrap key
instead (`key_wrap: 'master-aes-256-gcm'` in their `encryption_meta`);
`decryptAttachmentForOwner` in `crypto/encryptedVault.ts` still reads both.

Changing the password changes `masterKey`, so **everything in that table must
be rewritten** or it becomes undecryptable. That is the complete list — and
the equally important complement is what does *not* depend on the password:

| Data | Keyed by | Rotation impact |
|---|---|---|
| vault/board blobs in object storage | random DEK, hybrid-KEM-wrapped to the user's keypair | none — keypair is unchanged |
| every version-history entry | its own KEM envelope per version (`VersionSnapshot` in `backend/src/models/mindmap.rs`) | none |
| shares and share blobs | Argon2id of the share passphrase | none |
| share attachments | the share key | none |
| vault labels, user labels | stored in plaintext | none |

Boards need no special handling: they are ordinary `mind_maps` rows whose
blob decodes to `BoardData`, so title re-encryption covers them automatically.

## 2. The bug that got the feature disabled

The shipped rotation (`buildPasswordRotationBundle` in
`frontend_app/src/crypto/keyRotation.ts` + `POST /api/auth/rotate-credentials`)
rewrites the private keys, the auth hash, and every title and note — and
**never touches `mind_map_attachments.encryption_meta`**. After a rotation,
every attachment's file key is wrapped with a key derived from a master key
that no longer exists. The blobs sit in object storage intact and permanently
unreadable. The loss is silent: nothing fails during rotation; the failure
appears the next time someone opens an attachment.

The repair is small in bytes: the encrypted blob never moves. Only the wrapped
file key (~60 bytes, in a JSONB column) changes — unwrap with the old wrap
key, re-wrap with the new one. Rotation is also the natural moment to upgrade
every legacy `master-aes-256-gcm` wrap to `hkdf-attachment-v1`, after which
the legacy decrypt branch exists only for accounts that have never rotated.

One subtlety: `encryption_meta` is written at **init** time
(`InitAttachmentRequest` carries it), so `pending` attachments already hold
old-key wraps. The rewrite must cover every attachment with a wrapped key
whose status is not `deleted` — pending included.

## 3. Three further problems a correct implementation must solve

### 3.1 Stale sessions corrupt data after rotation

JWT claims are `{sub, typ, iat, exp}` (`backend/src/middleware/auth.rs`) —
nothing ties a token to a key generation — and `refresh` checks only that the
user exists and is not locked. So every other signed-in device keeps a valid
session for up to the refresh expiry (30 days by default) while holding the
**old** master key in memory. Such a session does not merely read garbage:

- a rename writes `title_encrypted` under the dead title key;
- a note edit does the same;
- a new attachment wraps its file key under the dead master key — recreating
  the exact data loss the rotation was fixed to avoid;
- a new vault is created with a title nobody can decrypt.

**Fix — a key-version claim.** Tokens gain `kv`, set to the user's
`key_version` at issue time (login, refresh, rotation). Enforcement:

- `refresh` rejects a token whose `kv` differs from the current
  `key_version` → the device is sent back to sign-in, where the only password
  that works is the new one, which derives the correct master key.
- Every **write** endpoint (vault create/update/meta, attachment
  init/complete/patch, share create/complete/revoke, blob confirm) rejects a
  mismatched `kv`. Read endpoints can be left alone — a stale reader only
  sees titles it cannot decrypt, which is annoying but harmless.
- The check needs the current `key_version` without a per-request DB read for
  paths that don't already load the user: keep a small in-process
  `RwLock<HashMap<user_id, key_version>>`, filled lazily and updated on
  rotation — the same pattern as `InstanceSettingsHandle`.
- Tokens issued before this change carry no `kv`. Treat missing `kv` as
  acceptable on access tokens (they die within `JWT_ACCESS_EXPIRY_SECS`,
  15 minutes by default) and reject it on `refresh`, which costs each device
  one re-login after the upgrade. A stale-password write is then possible
  only if a rotation happens in that same 15-minute window.

The rotating session itself receives fresh tokens with the new `kv` in the
rotation response, so it continues seamlessly.

### 3.2 The transaction is not isolated, because the backend shares one connection

The whole backend runs on a **single** `tokio_postgres` connection
(`Arc<Client>`, `backend/src/db/postgres.rs`). `rotate_user_credentials`
issues explicit `BEGIN`/`COMMIT` on it. On a shared connection, any other
handler's statement that executes between those two points runs **inside the
rotation transaction**:

- if rotation rolls back, that unrelated write (someone's vault save) is
  silently rolled back with it;
- if the interleaved statement errors, it aborts the rotation transaction;
- coverage was checked *before* `BEGIN`, so a vault or attachment created in
  the gap escapes rotation entirely.

**Fix.** The rotation transaction opens a **dedicated connection** for its
duration (connect on demand; rotation is rare and the cost is irrelevant).
Inside the transaction, in this order:

1. `SELECT key_version FROM users WHERE id = $1 FOR UPDATE` — take the row
   lock, re-check `new_key_version = key_version + 1` under it.
2. Re-run the complete-coverage check **inside** the transaction: the set of
   vault ids, and the set of attachment ids with a wrapped key and status ≠
   `deleted`, must exactly equal the submitted sets — missing *or unknown*
   ids abort with nothing written.
3. The `users` update, every `mind_maps` title/note update, and every
   attachment `encryption_meta` merge.
4. `COMMIT`, then update the key-version cache (3.1) so other sessions start
   failing closed immediately.

The Argon2id hash of the new auth token is still computed before the
transaction opens, so the expensive step never holds the lock.

With `kv` enforcement in place, the only writer that can race the transaction
is a request already in flight at commit time; the in-transaction coverage
check plus the row lock close that window.

### 3.3 A lost response is ambiguous — and provably resolvable

The dangerous interruption is not a browser crash during crypto (nothing has
been sent; nothing changed). It is the network dying **after the server
commits and before the client hears back**: the server now only accepts the
new password while the client believes rotation failed.

The state can be determined without guessing: `GET /auth/salt?username=…` is
unauthenticated and returns the live salt. Rotation always generates a fresh
salt, so the client compares the returned value against the old salt (from
the manifest) and the new salt (from the bundle it just built):

- old salt returned → rotation did not commit; the old password stands; safe
  to retry.
- new salt returned → rotation committed; tell the user plainly: *"Your
  password was changed. Sign in with the new password."*

The client runs this probe automatically on any timeout or network error from
the rotation POST and reports the definitive outcome. (The endpoint is
throttled per address; one probe is nothing.)

A retry against an already-committed rotation also fails cleanly on its own:
the old `current_auth_token` no longer verifies (401) and `key_version` is
stale (400). The probe exists so the *user* is told the truth instead of an
error code.

### Interruptions that need no handling

- **Crash before the POST** — pure client-side work; nothing changed anywhere.
- **Crash after a successful POST, before the client updates its session** —
  the server is fully consistent; the client holds no durable key material
  (sessionKeys are memory-only, and the localStorage tokens are either
  replaced or will fail the `kv` check). The next sign-in with the new
  password is fully correct.
- **Server crash mid-transaction** — PostgreSQL rolls back; the old password
  stands everywhere.

There is deliberately **no partial or resumable mode**. All crypto happens in
the browser before a single POST, which is the one commit point. The payload
stays far below the request-body cap (`MAX_UPLOAD_BODY_BYTES` = 51 MB): a
title/note entry is ~200 bytes and an attachment entry ~150 bytes, so even
1,000 vaults with 10,000 attachments is ~2 MB. Splitting the request into
chunks would create the partial states this design exists to rule out.

## 4. The design

### 4.1 Pre-flight: one snapshot to rotate from

`GET /api/auth/rotation-manifest` (authenticated) returns everything the
client must rewrite, in one response:

```json
{
  "key_version": 2,
  "argon2_salt": "…", "argon2_params": { "m_cost": 65536, "t_cost": 3, "p_cost": 4 },
  "classical_priv_encrypted": "…", "pq_priv_encrypted": "…",
  "vaults": [
    { "id": "…", "title_encrypted": "…", "vault_note_encrypted": "… or null" }
  ],
  "attachments": [
    { "id": "…", "map_id": "…", "encryption_meta": { "wrapped_key_b64": "…", "key_wrap": "…" } }
  ]
}
```

`attachments` lists exactly the rows the server will require coverage of:
wrapped key present, status ≠ `deleted`. Fetching this per vault instead
(N+1 `GET /{id}/attachments`) would work but widens the race window and
scatters the snapshot; the manifest is one query set and one source of truth
for "what must be covered".

### 4.2 Client (all in the browser)

```
1. oldMasterKey  = Argon2id(currentPassword, manifest.argon2_salt, manifest.argon2_params)
2. unwrap both private keys with oldMasterKey            ← proof of password; abort on failure
3. newSalt = random(32); newMasterKey = Argon2id(newPassword, newSalt, DEFAULT_ARGON2_PARAMS)
4. re-wrap both private keys under newMasterKey
5. for each vault: title (and note, when present) old-title-key → new-title-key
6. for each attachment: unwrap file key using meta.key_wrap
   ('hkdf-attachment-v1' → HKDF wrap key; 'master-aes-256-gcm' → raw master key),
   re-wrap under the NEW HKDF wrap key, emit key_wrap 'hkdf-attachment-v1'
7. currentAuthToken = HKDF(oldMasterKey); newAuthToken = HKDF(newMasterKey)
8. POST the bundle (4.3)
9. on success: replace sessionKeys.masterKey, store the new tokens, done
   on timeout / network error: salt probe (3.3), report the true outcome
```

Step 3 always uses `DEFAULT_ARGON2_PARAMS`, so a rotation silently upgrades
accounts created under weaker historical parameters. This is the general
property worth keeping: **rotation is the only moment every password-derived
ciphertext is rewritten, so it is the vehicle for any KDF or key-wrap
hygiene migration** — the legacy attachment wraps in step 6 being the first.

A step-5/6 decrypt failure aborts the whole rotation before anything is sent.
One undecryptable title must not become a rotation that skips it — that row
would be orphaned under the old key forever. If corrupt rows ever need a
bypass, it must be explicit, logged, and shown to the user, never silent.

### 4.3 API contract v2 — `POST /api/auth/rotate-credentials`

The existing request, plus attachments:

```json
{
  "current_auth_token": "<64-hex>",
  "new_auth_token": "<64-hex>",
  "new_argon2_salt": "<base64>",
  "new_argon2_params": { "m_cost": 65536, "t_cost": 3, "p_cost": 4 },
  "new_classical_priv_encrypted": "<base64>",
  "new_pq_priv_encrypted": "<base64>",
  "new_key_version": 3,
  "updated_vaults":      [ { "id": "…", "title_encrypted": "…", "vault_note_encrypted": "… or null" } ],
  "updated_attachments": [ { "id": "…", "wrapped_key_b64": "…" } ]
}
```

Server steps: validate shapes → verify `current_auth_token` against
`auth_hash` (a stolen JWT alone must not rotate credentials) → dedicated
connection, transaction as in 3.2. Attachment rows are updated with a JSONB
merge that touches only the two governed fields and preserves the rest of the
metadata:

```sql
UPDATE mind_map_attachments
SET encryption_meta = encryption_meta
    || jsonb_build_object('wrapped_key_b64', $2::text, 'key_wrap', 'hkdf-attachment-v1')
WHERE id = $1 AND map_id IN (SELECT id FROM mind_maps WHERE user_id = $3)
```

Response on success: `{ ok, access_token, refresh_token }` — both tokens
carrying the new `kv`.

| Status | Condition |
|---|---|
| 400 | malformed fields; `new_key_version` ≠ current + 1; coverage mismatch (either set, missing or unknown ids) |
| 401 | `current_auth_token` does not verify |
| 404 | user not found |

Every failure leaves the database byte-for-byte unchanged.

### 4.4 UI

The form lives in the settings hub's Account tab — directly under the text
that says nobody can reset the password, because that is where someone looks.
The desktop-local `/change-password` page keeps its Tauri branch; the
server branch moves to the shared flow above. Requirements:

- new password minimum **12** characters, matching registration (the current
  page says 8 — align it);
- staged progress ("Re-encrypting 14 titles and 37 attachment keys…"), and a
  `beforeunload` guard while the POST is in flight;
- state up front that every other signed-in device will be signed out and
  needs the new password;
- on ambiguous failure, show the salt-probe verdict, not the raw error.

## 5. Explicitly out of scope

- **Keypair rotation.** The KEM keypair is unchanged, so a compromised
  keypair is not healed by a password change. Rotating it means re-wrapping
  the DEK envelope of every blob **and every version-history entry** of every
  vault — a different, much larger design. Say so in the release notes rather
  than letting "password rotation" imply it.
- **Password reset.** Still impossible by design: the server holds nothing
  that can recreate the master key. Rotation requires the current password.
- **Re-encrypting blob content.** DEKs are unchanged; blobs and versions stay
  byte-identical in object storage. Rotation transfers no blob bytes at all.

## 6. Implementation map (all landed 2026-09-01)

1. **Transaction isolation** — `PostgresDb::dedicated_client()` opens a
   private connection for the rotation transaction; version check and
   complete coverage (vaults, notes, attachments) run inside it under
   `SELECT … FOR UPDATE` on the user row.
2. **Backend contract** — `GET /api/auth/rotation-manifest`;
   `updated_attachments` on the rotate request; the JSONB merge in
   `rotate_user_credentials`.
3. **Session invalidation** — the `kv` claim in `Claims`; the `refresh`
   check; `VerifiedWriter` (with `KeyVersionCache`) on every mutating
   `/api/mindmaps` route; rotation and login/refresh keep the cache current.
4. **Client** — `buildPasswordRotationBundle` re-wraps attachments (both wrap
   formats in, `hkdf-attachment-v1` out); `PasswordRotationForm` carries the
   manifest-driven flow and the salt-probe recovery; it renders in the
   settings hub's Account tab and on `/change-password` for server mode.
   `ROTATION_ENABLED` is deleted, not flipped — the guard comment described a
   bug that no longer exists.

## 7. Test plan

`tests/endpoints/password-rotation.mjs`, against a live backend, seeded with
the awkward account on purpose: several vaults (one board, one with a note,
one with an empty note, one with saved versions), attachments in `pending`
and `available` states, one attachment hand-written with a legacy
`master-aes-256-gcm` wrap, and one active share with a share attachment.

Must-pass checks:

1. **Full sweep after rotation** — with only the new password: every title
   and note decrypts, every blob and **every historical version** decrypts,
   every attachment (pending, available, and the legacy-wrap one) downloads
   and decrypts, and the legacy wrap now reads `hkdf-attachment-v1`.
2. **The share still opens** for a recipient with the passphrase, attachment
   included.
3. **Old password is dead** — login fails; the old auth token no longer
   verifies.
4. **Stale sessions fail closed** — a second session signed in before
   rotation: its `refresh` is rejected; its attachment init and vault rename
   are rejected; nothing it attempted left old-key ciphertext behind.
5. **Failure leaves no trace** — wrong current password (401), stale
   `key_version` (400), a bundle missing one attachment (400), a bundle with
   an unknown vault id (400): after each, the old password still works and
   every ciphertext is unchanged.
6. **The probe logic** — after a committed rotation, `GET /auth/salt` returns
   the new salt (the client-side disambiguation in 3.3 keys off exactly
   this); before one, the old.
7. **Browser pass** — extend `tests/ui/settings-hub.mjs` or add a sibling:
   change the password through the Account tab with an attachment-bearing
   vault, reload, sign in with the new password, open the attachment.

## 8. Related files

| File | Role |
|---|---|
| `frontend_app/src/crypto/kdf.ts` | every password-derived key, including the attachment wrap key |
| `frontend_app/src/crypto/keyRotation.ts` | rotation bundle builder (today: keys + titles/notes only) |
| `frontend_app/src/crypto/encryptedVault.ts` | attachment wrap/unwrap, both `key_wrap` formats |
| `frontend_app/src/components/PasswordRotationForm.tsx` | the server-mode flow: manifest → bundle → POST → salt-probe recovery |
| `frontend_app/src/pages/ChangePasswordPage.tsx` | local (Tauri) path; renders the shared form in server mode |
| `backend/src/routes/auth_sql.rs` | `rotate_credentials` handler, coverage check, token issue |
| `backend/src/db/postgres.rs` | `rotate_user_credentials`; the shared single connection (3.2) |
| `backend/src/middleware/auth.rs` | JWT claims — where `kv` goes |
| `backend/src/models/attachment.rs` | `encryption_meta` set at init; pending rows carry wraps |
| `backend/src/models/mindmap.rs` | `VersionSnapshot` — per-version KEM envelopes |
