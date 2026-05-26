# MindMapVault Server v0.3.27

This release focuses on connector capability consistency and deterministic capability behavior in the server frontend.

## Highlights

- Canonical connector capability keys are now enforced in server frontend connector typings.
- Connector capability checks now use explicit unsupported-feature defaults (`false`) instead of implicit behavior.
- Repository contributor guidance now includes canonical connector-key naming rules for cross-repo consistency.
- Version metadata is aligned to `0.3.27` across backend, frontend app, and desktop Tauri metadata.

## What Changed

- `frontend_app/src/app-core/connectors/types.ts`
  - Enforced canonical key unions for feature and billing capability checks.
- `frontend_app/src/platform/bootstrap.ts`
  - Tightened capability behavior defaults and explicit connector wiring.
- `.github/copilot-instructions.md`
  - Added canonical key naming guidance.

## Validation

- `pnpm --dir frontend_app build` -> passed.

## Notes For Operators

- This release does not alter server-side encryption boundaries.
- No enterprise-only governance behavior is introduced here.
- Existing deployment model remains unchanged.
