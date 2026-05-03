# Server Surface Ownership

MindMapVault Server is the community sync/share platform baseline.

## Owned Surfaces

- `backend/`
  - community API contract
  - auth, encrypted metadata, blob orchestration, sharing, collaboration foundations
  - single-image runtime that serves the app and admin bundles
- `frontend_app/`
  - server-capable end-user application
  - bundled into the backend image and served at `/`
- `frontend_admin/`
  - community admin surface
  - bundled into the backend image and served at `/admin/`

## Do Not Put Here

- hosted billing and subscription operations
- Cloudflare-specific deployment wiring
- enterprise-only policy, SSO, audit, or compliance overlays

## Touch Guide

- touch this repo for neutral sync/share features and community admin behavior
- touch `mindmapvault-saas` for hosted commercial operations
- touch `mindmapvault-enterprise-server` for enterprise governance overlays
- touch `mindmapvault-foss` for offline-only desktop behavior