# Refactoring plan: the SQL layer

Written 2026-09-05, after `MindMapEditor` and `VaultsPage`. Same discipline —
see `CLAUDE.md`.

## What we are dealing with

| File | Lines |
|---|---|
| `db/postgres.rs` | 1,943 |
| `db/sql_store.rs` | 672 |
| `routes/mindmaps_sql.rs` | 1,924 |

**One trait with 55 methods, and one impl implementing all 55.** Every new
query edits both files, and neither fits on a screen. The methods already fall
into domains: mind maps 23, users 12, registration invites 3, admin 3, instance
settings and the rest 14.

`postgres.rs` also carries the schema: `ensure_schema` is a single 213-line
method of `CREATE TABLE`, `ALTER TABLE` and `CREATE INDEX` inside the same file
as every query.

`routes/mindmaps_sql.rs` is a different shape of problem. Its ~45 handlers are
small and well-formed; the file simply serves four resource families — maps,
attachments, versions and shares — where `routes/` already separates `admin`
and `auth`.

## The constraint that shapes this

Rust will not let one `impl SqlStore for PostgresDb` block span files. So
splitting the impl means splitting the trait: several domain traits, with
`SqlStore` as a supertrait of all of them. `Arc<dyn SqlStore>` keeps working —
supertrait methods are callable on the trait object — so no call site changes.

That is a real change to a public interface, not a file move, and it is worth
doing only because the two files must currently be edited in lockstep.

## Order

Each step is a commit that leaves `cargo test` green (52 tests today).

| # | Step | Why here |
|---|---|---|
| 1 | Move `ensure_schema` into `db/schema.rs` | Independent of everything else, and the schema is not a query |
| 2 | Split `SqlStore` into domain traits, `SqlStore` a supertrait of them | The change that stops the lockstep editing |
| 3 | Move each domain's impl into `db/postgres/<domain>.rs` | Follows from 2 and is mechanical once it is done |
| 4 | Split `routes/mindmaps_sql.rs` by resource | Independent; do it whenever |

Step 1 is worth doing even if nothing else happens.

## What not to do

- **Do not change any SQL.** This is a move. A behaviour change hidden inside a
  1,900-line diff cannot be reviewed.
- **Do not rename the wire names.** `#[serde(rename = "minio_object_key")]` and
  `minio_version_id` are the JSON the frontend already sends; a previous rename
  pass silently rewrote strings inside `#[serde(rename)]` and would have
  changed the wire format.
- **Do not fold `ensure_schema`'s guarded `DO $$ … $$` blocks into plain
  `ALTER TABLE`.** They are idempotent on purpose — the server runs them on
  every boot, against databases at different ages.
- **Do not split by line count.** Four traits that each need the other three
  are worse than one honest trait.

## Done when

- The schema lives somewhere that is not the query file
- Adding a query touches one domain trait and one domain impl
- `cargo test` still passes 52 tests, none edited
