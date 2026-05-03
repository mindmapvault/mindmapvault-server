# Backend Testing

This file defines the repeatable backend regression check that should be run after backend changes.

## Rule

When a change touches backend routes, auth, storage, billing, database code, or shared API contracts used by the backend, run the regression test before considering the change done.

## Local WSL Workflow

Start the native WSL backend loop in one terminal:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/crypt-mind && bash scripts/cryptmind-wsl-native-backend.sh start'
```

Run the full regression test in another terminal after changes:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /mnt/c/Users/korne/vscode/crypt-mind && bash scripts/cryptmind-wsl-native-backend.sh test'
```

If you only need the Python test directly:

```powershell
wsl.exe -d Ubuntu bash -lc 'cd /home/kornelko/workspaces/crypt-mind && python3 scripts/backend_regression_test.py --pretty'
```

## Coverage

The regression script in [scripts/backend_regression_test.py](../scripts/backend_regression_test.py) covers the main backend path used by the encrypted app:

- `GET /health`
- `POST /api/auth/register`
- `GET /api/auth/salt`
- `POST /api/auth/login`
- `POST /api/auth/refresh`
- `GET /api/auth/profile`
- `PUT /api/auth/profile`
- `GET /api/auth/keys`
- `DELETE /api/auth/profile`
- `GET /api/billing/config`
- `POST /api/mindmaps`
- `PUT /api/mindmaps/{id}`
- `GET /api/mindmaps`
- `GET /api/mindmaps/{id}`
- `DELETE /api/mindmaps/{id}`
- `PUT /api/mindmaps/{id}/meta`
- `POST /api/mindmaps/{id}/upload`
- `POST /api/mindmaps/{id}/upload-url`
- `POST /api/mindmaps/{id}/confirm-upload`
- `GET /api/mindmaps/{id}/blob`
- `GET /api/mindmaps/{id}/download-url`
- `GET /api/mindmaps/{id}/versions`
- `DELETE /api/mindmaps/{id}/versions/{version_id}`
- `GET /api/mindmaps/storage`
- `GET /api/mindmaps/my/storage`

It validates both blob upload modes:

- direct backend upload via `/upload`
- presigned S3 upload plus `/confirm-upload`

It also verifies cleanup by deleting the disposable account at the end.

The plaintext regression script in [scripts/plaintext_regression_test.py](../scripts/plaintext_regression_test.py) covers the plaintext sharing surface:

- `GET /api/plaintext/groups`
- `POST /api/plaintext/groups`
- `GET /api/plaintext/groups/{id}`
- `PUT /api/plaintext/groups/{id}`
- `DELETE /api/plaintext/groups/{id}`
- `POST /api/plaintext/groups/{id}/members`
- `DELETE /api/plaintext/groups/{id}/members/{user_id}`
- `GET /api/plaintext/maps`
- `POST /api/plaintext/maps`
- `GET /api/plaintext/maps/{id}`
- `PUT /api/plaintext/maps/{id}`
- `DELETE /api/plaintext/maps/{id}`
- `POST /api/plaintext/maps/{id}/shares/users`
- `DELETE /api/plaintext/maps/{id}/shares/users/{user_id}`
- `POST /api/plaintext/maps/{id}/shares/groups`
- `DELETE /api/plaintext/maps/{id}/shares/groups/{group_id}`

The plaintext flow bootstraps its own disposable users, grants plaintext access directly in the local PostgreSQL database used by the WSL dev stack, and verifies owner, editor, and viewer behavior across direct shares and group shares.

The collaboration regression script in [scripts/collaboration_regression_test.py](../scripts/collaboration_regression_test.py) covers the plaintext real-time collaboration surface:

- `GET /api/collaboration/plaintext/maps/{id}/snapshot`
- `GET /api/collaboration/plaintext/maps/{id}/ws`

It bootstraps disposable plaintext users, opens two live collaboration sessions against the same shared map, verifies owner-to-editor and editor-to-owner document propagation, and checks that a stale sequence update is rejected without overwriting the persisted plaintext map.

The admin regression script in [scripts/admin_regression_test.py](../scripts/admin_regression_test.py) covers the admin endpoint family:

- `GET /api/admin/overview`
- `POST /api/admin/users/{id}/account-lock`
- `POST /api/admin/users/{id}/admin-details`
- `POST /api/admin/users/{id}/access-grants`
- `POST /api/admin/users/{id}/plan-override`
- `POST /api/admin/users/{id}/delete-account`
- `POST /api/admin/feedback/{id}/archive`
- `POST /api/admin/feedback/{id}/delete`

It bootstraps disposable target users and a disposable feedback record, verifies admin mutations through the returned overview payloads, and confirms deleted accounts can no longer log in.

## Expected Result

The test command exits with status `0` only if the encrypted, plaintext, and admin regressions all succeed, and it prints JSON summaries for each.

If a check fails, it exits non-zero and prints the failing step plus any cleanup error.

## Notes

- The scripts are intentionally dependency-light and use Python standard library only.
- It creates disposable users, vaults, groups, and plaintext maps, so it is safe for repeat local runs.
- It targets the running backend at `http://127.0.0.1:8090` by default.
- The plaintext regression assumes the local WSL dev stack is using the bundled PostgreSQL container and updates `users.access_grants_json` there to grant the required plaintext access before running route checks.
- It is a regression check, not a replacement for unit tests or route-specific tests.