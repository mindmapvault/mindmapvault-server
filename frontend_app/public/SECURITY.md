# MindMapVault — Security & Architecture Documentation

> **Version:** MVP 1.0 · March 2026  
> This document explains how MindMapVault works, what data is stored where, and every security measure in place.

---

## Table of Contents

1. [What is MindMapVault?](#1-what-is-mindmapvault)
2. [The Zero-Knowledge Principle](#2-the-zero-knowledge-principle)
3. [Registration — How Your Keys Are Created](#3-registration--how-your-keys-are-created)
4. [Login — How Authentication Works](#4-login--how-authentication-works)
5. [Saving a Mind Map — Encryption Flow](#5-saving-a-mind-map--encryption-flow)
6. [Opening a Mind Map — Decryption Flow](#6-opening-a-mind-map--decryption-flow)
7. [Post-Quantum Protection (ML-KEM-768)](#7-post-quantum-protection-ml-kem-768)
8. [What the Server Stores](#8-what-the-server-stores)
9. [What the Server Never Sees](#9-what-the-server-never-sees)
10. [Session Security](#10-session-security)
11. [Transport Security](#11-transport-security)
12. [Technology Stack](#12-technology-stack)
13. [Threat Model & Limitations](#13-threat-model--limitations)

---

## 1. What is MindMapVault?

MindMapVault is an **end-to-end encrypted mind map vault**. You create, edit, and store mind maps that are encrypted entirely in your browser before any data leaves your device. Neither the server, the database, nor the object storage can read the content of your mind maps — only you can, using your password.

---

## 2. The Zero-Knowledge Principle

MindMapVault is designed so that the server is **blind to your data**:

- Your **password never leaves your browser** — not even a hash of it.
- Your **mind map content** is encrypted on your device before upload and decrypted on your device after download.
- Your **map titles** are individually encrypted so they can be listed without exposing content.
- The server stores your private keys only as **AES-256-GCM encrypted blobs** — it cannot unwrap them.

If the entire database and file storage were stolen, an attacker would see only opaque ciphertext blobs. Without your password, they are computationally infeasible to break.

---

## 3. Registration — How Your Keys Are Created

When you register, the following happens **entirely in your browser**:

```
password + random_salt
        │
        ▼  Argon2id (64 MiB · 3 iterations · 4 threads)
        │
   master_key (32 bytes)
        │
        ├──HKDF("crypt-mind-auth-v1")──▶  auth_token  ──▶  sent to server as login credential
        │
        ├──HKDF("crypt-mind-title-v1")──▶  title_key  (stays in memory, encrypts map titles)
        │
        └──AES-256-GCM──▶  wraps your X25519 & ML-KEM-768 private keys before upload
```

**Key generation:**
- An **X25519** keypair is generated (classical elliptic-curve Diffie-Hellman).
- An **ML-KEM-768** keypair is generated (post-quantum key encapsulation).
- Both private keys are encrypted with `master_key` using AES-256-GCM.

**What is sent to the server at registration:**
| Field | Description |
|---|---|
| `username` | Plaintext login name |
| `argon2_salt` | Random 16-byte salt (base64) — needed by client to re-derive `master_key` |
| `argon2_params` | Memory/iteration/parallelism settings |
| `auth_token` | HKDF-derived credential — NOT the password or master key |
| `classical_public_key` | X25519 public key — public by design |
| `pq_public_key` | ML-KEM-768 public key — public by design |
| `classical_priv_encrypted` | AES-256-GCM(master_key, x25519_private_key) |
| `pq_priv_encrypted` | AES-256-GCM(master_key, mlkem_private_key) |

The server hashes `auth_token` again with Argon2id before storing it — so even the credential in the database is a hash, not the original token.

---

## 4. Login — How Authentication Works

```
1.  Browser fetches salt + Argon2id params for your username (public endpoint)
2.  Browser runs Argon2id(password, salt) → master_key         [in browser, ~1-2 sec]
3.  Browser derives auth_token = HKDF(master_key, "crypt-mind-auth-v1")
4.  auth_token is sent to the server
5.  Server verifies: Argon2id(auth_token) == stored hash
6.  Server issues JWT access token (15 min) + refresh token (30 days)
7.  Server returns encrypted private key blobs
8.  Browser decrypts private keys: AES-256-GCM-decrypt(master_key, encrypted_priv)
9.  Private keys + master_key are held in RAM for the session only
```

**The master key is never stored** — it lives only in the browser's JavaScript memory for the duration of the unlocked session and is discarded when you close the tab or lock the vault.

---

## 5. Saving a Mind Map — Encryption Flow

Every save generates a fresh, unique encryption key for that specific save operation:

```
For each save:

1.  Generate  ephemeral_x25519_private  (random, used once)
2.  ECDH(ephemeral_private, your_x25519_public)  →  classical_shared (32 bytes)
3.  ML-KEM-768 encapsulate(your_pq_public)       →  (pq_ciphertext, pq_shared)
4.  combined_key = HKDF-SHA256(classical_shared ‖ pq_shared, "crypt-mind-dek-v1")
5.  DEK (Data Encryption Key) = random 32 bytes
6.  wrapped_DEK = AES-256-GCM(combined_key, DEK)
7.  blob = AES-256-GCM(DEK, JSON(mind_map_tree))
8.  title_enc = AES-256-GCM(title_key, map_title)

Stored in database:  ephemeral_x25519_public, pq_ciphertext, wrapped_DEK, title_enc
Stored in MinIO:     blob (the encrypted mind map)
```

The DEK is **never stored**. It is derived on-demand by the recipient using their private keys. Each save produces a completely independent ciphertext — there is no key reuse.

---

## 6. Opening a Mind Map — Decryption Flow

```
1.  Download: ephemeral_x25519_public, pq_ciphertext, wrapped_DEK  (from DB)
2.  Download: encrypted blob  (from MinIO via short-lived presigned URL)
3.  ECDH(your_x25519_private, ephemeral_x25519_public)  →  classical_shared
4.  ML-KEM-768 decapsulate(pq_ciphertext, your_pq_private)  →  pq_shared
5.  combined_key = HKDF-SHA256(classical_shared ‖ pq_shared, "crypt-mind-dek-v1")
6.  DEK = AES-256-GCM-decrypt(combined_key, wrapped_DEK)
7.  mind_map_tree = AES-256-GCM-decrypt(DEK, blob)
8.  Rendered in the browser — plaintext never sent anywhere
```

---

## 7. Post-Quantum Protection (ML-KEM-768)

MindMapVault uses a **hybrid classical + post-quantum KEM**:

| Layer | Algorithm | Why |
|---|---|---|
| Classical | X25519 (Diffie-Hellman) | Extremely fast, battle-tested, standard |
| Post-Quantum | ML-KEM-768 (CRYSTALS-Kyber) | Resistant to quantum computer attacks |
| Combination | HKDF-SHA256 of both shared secrets | Security holds if *either* layer is unbroken |

**Why this matters:** A sufficiently large quantum computer could break X25519 (and all classical public-key cryptography) using Shor's algorithm. ML-KEM-768 is one of the algorithms standardised by NIST in 2024 specifically to resist quantum attacks. The hybrid design means your data remains secure today (with classical algorithms) and in the quantum era (with ML-KEM-768).

The implementation uses `@noble/post-quantum` — a pure-JavaScript, audited library with no native dependencies.

---

## 8. What the Server Stores

| Location | What | Readable by server? |
|---|---|---|
| SQL `users` table | Username, Argon2id(auth_token), Argon2 salt+params, public keys, AES-wrapped private keys | ❌ Private keys encrypted; auth hash is a credential hash only |
| SQL `mindmaps` table | Map ID, `title_encrypted`, `ephemeral_x25519_pub`, `pq_ciphertext`, `wrapped_dek`, MinIO key | ❌ All content fields are ciphertext |
| MinIO (object storage) | AES-256-GCM encrypted mind map blob | ❌ Opaque binary; no key available server-side |

---

## 9. What the Server Never Sees

- Your **password**
- Your **master key**
- Your **private keys** (stored only as AES-GCM ciphertext)
- Your **DEK** (Data Encryption Key — never persisted, derived on demand client-side)
- Your **mind map content** (stored and transmitted as ciphertext only)
- Your **map titles** (stored as AES-GCM ciphertext)

---

## 10. Session Security

| Mechanism | Detail |
|---|---|
| **JWT Access Token** | HS256, 15-minute expiry, validated on every API request |
| **JWT Refresh Token** | 30-day expiry, used only to obtain new access tokens |
| **Master key in memory** | Held in Zustand store (RAM only), never written to `localStorage` or `sessionStorage` |
| **Vault lock** | Closing the browser tab discards the master key and private keys |
| **Unlock prompt** | If session keys are missing on navigation to an editor, an unlock modal re-derives them from password |
| **Token type check** | Access and refresh tokens carry a `typ` claim; the server rejects mismatched usage |

---

## 11. Transport Security

- All API communication should be served over **HTTPS/TLS** in production.
- MinIO blob downloads use **short-lived presigned URLs** (default 1 hour expiry) — the URL cannot be reused indefinitely.
- CORS is configured to allow only the declared `CORS_ALLOWED_ORIGINS` origin; all other origins are rejected.
- The backend validates all input fields and returns structured errors — no raw stack traces are exposed to clients.

---

## 12. Technology Stack

### Frontend (runs in your browser)
| Component | Technology |
|---|---|
| Framework | React 18 + TypeScript + Vite |
| Symmetric encryption | AES-256-GCM via **Web Crypto API** (browser built-in) |
| Key derivation | **Argon2id** via `hash-wasm` (WebAssembly) |
| Sub-key derivation | **HKDF-SHA256** via `@noble/hashes` |
| Classical KEM | **X25519** via `@noble/curves` |
| Post-quantum KEM | **ML-KEM-768** via `@noble/post-quantum` |
| UI | React Flow (`@xyflow/react`) + custom node components |
| State | Zustand (in-memory only for sensitive keys) |

### Backend (server)
| Component | Technology |
|---|---|
| Language | Rust (Axum 0.8, Tokio async) |
| Database | SQL backend (map metadata + encrypted key bundles) |
| File storage | MinIO S3-compatible (encrypted blobs) |
| Auth tokens | JWT (HS256) |
| Password hashing | Argon2id (auth_token hashed before storage) |
| Containerisation | Docker / docker-compose |

---

## 13. Threat Model & Limitations

### Protected against
- **Server compromise** — Database and file storage contain only ciphertext. An attacker gains no plaintext.
- **Network interception** (with TLS) — All traffic is encrypted in transit; the payload itself is also ciphertext.
- **Credential theft** — The password never leaves the browser. The auth_token sent to the server is a derived credential; stealing it alone cannot decrypt any content.
- **Future quantum computers** — ML-KEM-768 layer protects key encapsulation against quantum adversaries.
- **Replay / cross-user attacks** — Each mind map save generates a fresh ephemeral keypair and fresh DEK; ciphertexts are not reusable across users.

### Current limitations (MVP)
- **Browser memory attacks** — If an attacker has code execution in your browser (e.g. via XSS), they could read the in-memory master key. This is inherent to any browser-based E2E encryption.
- **Password strength** — Argon2id is strong, but a weak password is still a weak password. Use a long, random passphrase.
- **Offline vault locality** — Local desktop vaults are intentionally device/folder-bound. Cross-device sync is available only when using cloud mode.
- **No key rotation UI** — Key rotation (e.g. after a password change) is tracked in `key_version` on the backend but the rotation workflow is not yet exposed in the UI.
- **No audit log** — Access events are not yet recorded on the server.

---

*MindMapVault MVP — all cryptographic operations are performed client-side using well-audited, open-source libraries. No proprietary crypto is used.*
