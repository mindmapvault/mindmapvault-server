# Server Surface Ownership

MindMapVault Server is the community self-hosted backend + web UI baseline.

## Owned Surfaces

- `backend/`
  - community server routes used by the bundled web UI
  - auth, encrypted metadata, blob orchestration, and versioning
  - single-image runtime that serves the app and admin bundles
- `frontend_app/`
  - server-capable end-user application
  - bundled into the backend image and served at `/`
- `frontend_admin/`
  - community admin surface
  - bundled into the backend image and served at `/admin/`

## Do Not Put Here

- sync or offline client feature orchestration
- collaboration protocol foundations or relay logic
- hosted billing and subscription operations
- Cloudflare-specific deployment wiring
- enterprise-only policy, SSO, audit, or compliance overlays

## Touch Guide

- touch this repo for backend + web UI server behavior and community admin behavior
- touch `mindmapvault-saas` for hosted commercial operations
- touch `mindmapvault-enterprise-server` for enterprise governance overlays
- touch `mindmapvault-foss` for offline-only desktop behavior