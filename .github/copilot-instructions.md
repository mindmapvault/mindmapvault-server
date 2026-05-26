# MindMapVault Server - Copilot Instructions

These instructions define how GitHub Copilot should behave in this repository.

## Repository Purpose

MindMapVault Server is the technical sync/share platform repository.

It includes backend and server-mode application surfaces:

- `backend/` (Rust API, auth, storage, collaboration foundations)
- `frontend_app/` (authenticated app client in server/local modes)
- `frontend_admin/` (admin web surface)
- `desktop/src-tauri/` (desktop host used with server-capable app flows)
- `.github/workflows/` (CI/CD and validation automation)

Primary goals:

- sync
- sharing
- multi-device state
- future collaboration foundations

This repo is not the enterprise governance product.

## Product Boundary

This repository owns:

- backend APIs and contracts for client sync/share flows
- encrypted metadata and encrypted blob orchestration
- server-mode client integration for `frontend_app` and `frontend_admin`
- service reliability, deployment, and operability basics

This repository does not own:

- FOSS-only product positioning and release policy (`mindmapvault-foss`)
- enterprise governance overlays (policy packs, enterprise controls) (`mindmapvault-enterprise-server`)
- marketing and cross-product strategy docs (`mindmapvault-www`)

## Packaging And Surface Ownership Rules

- `mindmapvault-server` is the only product-line repo that ships a single combined image by default.
- The backend image must serve `frontend_app` at `/` and `frontend_admin` at `/admin/`.
- Do not move hosted SaaS deployment concerns into this packaging path.
- Do not add enterprise-only web or policy surfaces directly into the community server runtime.
- Keep repo-local guidance in `docs/SURFACE_OWNERSHIP.md` aligned with canonical cross-repo guidance in `mindmapvault-www/docs/internal/PRODUCT_SURFACE_OWNERSHIP.md`.

## Security and Data Rules

Copilot must preserve these non-negotiables:

- zero-knowledge-compatible architecture
- no requirement for plaintext map payload handling on server by default
- no logging of plaintext notes, decrypted payloads, keys, tokens, or secrets
- no hidden telemetry or data harvesting behavior

Additional rules:

- treat auth, crypto, upload versioning, and blob metadata paths as high-risk areas
- preserve encrypted metadata semantics across API and client surfaces
- do not introduce behavior that forces cloud dependency for core editing in server-capable clients

Changes touching auth, storage, versioning, uploads, or collaboration paths require targeted tests.

## API and Compatibility Rules

- prefer explicit, versioned API contracts
- keep backward compatibility unless a documented breaking change is intentional
- avoid ambiguous routes that could collide with id-like dynamic handlers
- keep capability negotiation explicit for optional features

## Connector Capability Key Naming (Canonical)

Copilot must use one canonical naming set for connector capability keys across all MindMapVault repos.

Feature keys for `features.hasFeature(feature)`:

- `realtime-collaboration`
- `cloud-version-history`
- `public-share-links`
- `billing-upgrade`
- `admin-controls`

Billing keys for `billing.isFeatureEnabled(feature)`:

- `large-exports`
- `advanced-attachments`
- `team-collaboration`
- `admin-controls`

Rules:

- Do not invent aliases or alternate spellings for these keys.
- Unsupported capabilities must return `false`, not throw.
- UI gating in shared surfaces must use connector capability keys, not product-name checks.
- Any key rename must be done in one coordinated change across FOSS, Server, SaaS, and docs.

When changing shared behavior, keep these paths aligned:

- backend routes and payload contracts in `backend/`
- server-mode client behavior in `frontend_app/`
- admin behavior in `frontend_admin/`
- desktop host integration in `desktop/src-tauri/`

Frontend parity contract files:

- `frontend_app/offline_capability_contract.json`
- `frontend_app/offline_scan_allowlist.txt`

Required security check command:

- `node scripts/check_no_committed_secrets.mjs`

Offline parity checks are handled in the FOSS repo and are not required for server-only changes in this repository.

## GitHub Actions Security Procedure

- keep workflow actions pinned to current stable major versions, such as `actions/checkout@v4` and `docker/build-push-action@v6`
- run the committed-secrets scan on every PR and main-branch push through `.github/workflows/security-guard.yml`
- avoid adding new workflow steps that print tokens, passwords, access keys, or private URLs into logs
- if a change touches authentication, storage, or deployment automation, review the corresponding workflow before merging

## Documentation Rules

- implementation docs and runbooks for Server behavior belong in this repo
- cross-product architecture, roadmap, and positioning docs belong in `mindmapvault-www/docs/`
- if behavior changes affect other repos, update canonical docs in `mindmapvault-www` in the same task

## License Rule

- repository license is AGPL-3.0-or-later
- keep `LICENSE` accurate and present in repo root

## Mandatory Per-Iteration Checklist

After each implementation iteration, verify:

1. Scope: changes stay inside Server responsibilities and do not drift into Enterprise-only policy logic.
2. Security: zero-knowledge boundaries are preserved.
3. Secrets: no sensitive plaintext in logs or error payloads.
4. Contract: API behavior and capability signaling stay explicit and version-aware.
5. Documentation: update repo docs and `mindmapvault-www` docs if cross-product behavior changed.
6. Validation: run relevant checks for changed surfaces (`backend`, `frontend_app`, `frontend_admin`, `desktop/src-tauri`) and fix regressions before proceeding.
7. Automation: ensure `.github/workflows` stays consistent with repository structure and validation requirements.
8. Parity: only required when a change explicitly affects offline capability in shared app code; otherwise it is not needed for server-only work in this repo.
9. Secrets: run `node scripts/check_no_committed_secrets.mjs` and remove any committed secret-like values before merge.

## Frontend Parity Rules (Server vs FOSS)

- Server frontend offline behavior must not regress below FOSS offline behavior.
- any offline capability change in `frontend_app` must update `frontend_app/offline_capability_contract.json` in both repos.
- do not add Server-only assumptions that break offline operation in local mode.
- parity changes must be documented in both repos when intentional.

## Copilot Never Rules

Copilot must never suggest:

- server-side plaintext dependency for core user content
- hidden data collection or tracking
- embedding static secrets in source
- silently changing API contracts without documentation and validation
- introducing enterprise-only governance behavior directly into base server flows
