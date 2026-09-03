# Release 0.5.1 — Version History That Actually Works

**Date:** 2026-09-03

## Overview

Opening an older version of a vault did nothing. It looked like a UI bug. It was
not: the bytes never arrived, because the object store this repository's own
`docker-compose.yml` ships does not keep older versions, and does not say so.

Every save now writes its own object. Nothing in the server depends on S3 bucket
versioning any more, which also means it now runs correctly on Cloudflare R2.

Self-hosters on the shipped Compose stack were affected. **Please read
[What Changes For Existing Installs](#what-changes-for-existing-installs) before
upgrading** — version counts will drop, and the upgrade is one-way.

## What Was Wrong

Vault versions were stored as S3 object versions of one key. That is not part of
the S3 core: Garage and Cloudflare R2 both answer `PutBucketVersioning` and
`ListObjectVersions` with `NotImplemented`.

Garage makes the gap dangerous rather than merely inconvenient. It returns an
`x-amz-version-id` on every upload and then ignores that id on download — writing
two files to one key and reading both versions back gives the same bytes twice,
with HTTP 200 and no error:

```
PUT #1 -> VersionId 5a17e0ea…
PUT #2 -> VersionId 466c3fe4…

GET version #1 -> "two"
GET version #2 -> "two"
GET "deadbeef"  -> "two"    (200, not 404)
```

So the store advertised the capability at write time and declined it at read
time. The one check that would have caught a store without versioning — "did the
upload return a version id?" — passed cleanly.

## What Changed

Each version is now its own object at `<key>/v/<version-id>`, and the version id
is minted by the server rather than read from a response header. The storage
layer uses only `PutObject`, `GetObject`, `HeadObject`, `DeleteObject` and
`ListObjectsV2`, which every S3 implementation provides.

Consequences worth knowing:

- **Saves are append-only.** Previously every save overwrote the vault's single
  object, so a failure mid-upload could damage the only copy. The current version
  is now never touched by the next save.
- **Listing versions no longer contacts the object store.** Sizes are recorded
  when a version is written. This is why every row used to read 0 B.
- **An unknown version id returns 404** instead of the current data.

Also fixed: `confirm-upload` accepted uploads that never happened — it only
checked that the version id was a non-empty string, and never contacted storage.
It now confirms the object with `HeadObject`.

Added: a storage self-test at startup, which writes, reads back, compares and
deletes a probe object. A store that accepts a write and serves something else is
otherwise indistinguishable from a working one until someone loses data.

## What Changes For Existing Installs

**Version counts will drop, and the smaller number is the true one.** Only the
most recent save of each vault was ever really stored. The migration keeps every
version whose ciphertext exists and removes the rows for the ones that do not,
because leaving them would list versions that cannot be opened. Each removal is
logged.

If your bucket genuinely had versioning — MinIO or AWS S3 with it switched on —
that history is read and kept. Nothing is discarded that the store still holds.

**The upgrade is one-way.** Two columns on `mind_maps` are renamed, and the
previous image cannot read the migrated database. It fails late rather than at
boot: sign-in still works, and vault requests return 500.

**A database dump alone is not a rollback.** The migration deletes each vault's
old object after copying it, so restoring only the dump gives a server that lists
every vault and cannot open any of them. Back up PostgreSQL and object storage
together, with the stack stopped. The exact commands are in
[`docs/DEPLOYMENT.md`](DEPLOYMENT.md#one-time-vault-blob-storage-migration).

The migration runs itself on first start, is idempotent, retries per-vault
failures on the next start, and takes a PostgreSQL advisory lock so replicas
sharing a database can be restarted together.

## Storage Compatibility

| Store | Before | Now |
| --- | --- | --- |
| AWS S3 | worked with versioning enabled | works |
| MinIO | worked with versioning enabled | works |
| Garage | silently broken | works |
| Cloudflare R2 | unusable | works |

## Internal

`MinioClient` is now `S3Store` in `backend/src/db/s3.rs`, and the
`minio_object_key` / `minio_version_id` columns are `object_key` /
`current_version_id`. The server no longer assumes it is talking to MinIO. The
JSON field names are unchanged, so existing clients keep working.

## Upgrading

See [`docs/DEPLOYMENT.md`](DEPLOYMENT.md#one-time-vault-blob-storage-migration).
Self-hosting notes and links live at
[mindmapvault.com/homelab](https://www.mindmapvault.com/homelab/).
