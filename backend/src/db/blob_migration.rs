//! Moves vault blobs off S3 object versioning and onto per-version object keys.
//!
//! Runs once per vault, at startup, and is idempotent: a vault whose history
//! already names an object key for every version is skipped without touching
//! the store.
//!
//! This is the one place that still asks an object store about versions, and
//! only to read data written before the change. Everything on the serving path
//! addresses versions by plain key, so a store that lacks `ListObjectVersions`
//! simply gets the fallback below instead of an error.

use aws_sdk_s3::error::ProvideErrorMetadata;

use crate::{
    db::{s3::S3Store, sql_store::DynSqlStore},
    error::AppError,
    models::mindmap::VersionSnapshot,
};

/// What the migration did, for one log line at the end.
#[derive(Debug, Default)]
pub struct MigrationReport {
    pub vaults_migrated: usize,
    pub versions_recovered: usize,
    pub versions_lost: usize,
    pub failures: usize,
    /// Another replica held the lock, so this process did nothing.
    pub skipped_locked: bool,
}

/// Identifies this migration to `pg_try_advisory_lock`. Arbitrary, but fixed:
/// changing it would let an old and a new build run at the same time.
const MIGRATION_LOCK_KEY: i64 = 0x6d6d76_5f6b6579;

/// Migrates every vault that still stores its blob at the bare object key.
pub async fn run(store: &DynSqlStore, storage: &S3Store) -> Result<MigrationReport, AppError> {
    let mut report = MigrationReport::default();

    if !store.try_lock_migration(MIGRATION_LOCK_KEY).await? {
        tracing::info!("another instance is migrating vault blobs; skipping");
        report.skipped_locked = true;
        return Ok(report);
    }

    for user in store.list_admin_users().await? {
        for map in store.list_mind_maps(&user.id).await? {
            if is_migrated(&map.version_history, map.current_version_id.as_deref()) {
                continue;
            }

            match migrate_one(store, storage, &map).await {
                Ok(Some(outcome)) => {
                    report.vaults_migrated += 1;
                    report.versions_recovered += outcome.recovered;
                    report.versions_lost += outcome.lost;
                }
                Ok(None) => {}
                Err(error) => {
                    report.failures += 1;
                    tracing::error!(
                        ?error,
                        map_id = %map.id,
                        "could not migrate this vault's blob to per-version keys; \
                         it will be retried on the next start"
                    );
                }
            }
        }
    }

    if let Err(error) = store.unlock_migration(MIGRATION_LOCK_KEY).await {
        tracing::warn!(?error, "could not release the migration lock");
    }

    Ok(report)
}

/// True when every version already names the object holding its ciphertext.
fn is_migrated(history: &[VersionSnapshot], current_version_id: Option<&str>) -> bool {
    let Some(current) = current_version_id else {
        // Nothing has ever been saved, so there is nothing to move.
        return true;
    };

    history.iter().any(|snapshot| snapshot.version_id == current)
        && history.iter().all(|snapshot| snapshot.object_key.is_some())
}

struct Outcome {
    recovered: usize,
    lost: usize,
}

async fn migrate_one(
    store: &DynSqlStore,
    storage: &S3Store,
    map: &crate::db::sql_store::StoredMindMap,
) -> Result<Option<Outcome>, AppError> {
    let Some(current_version_id) = map.current_version_id.as_deref() else {
        return Ok(None);
    };
    let current_version_id = S3Store::validate_version_id(current_version_id)?;

    // Where the store really does keep versions — a the object store or AWS bucket with
    // versioning enabled — the history is genuine and worth rescuing. Where it
    // does not, this returns None and the older entries were never stored.
    let stored_versions = list_legacy_versions(storage, &map.object_key).await;

    let mut migrated: Vec<VersionSnapshot> = Vec::with_capacity(map.version_history.len());
    let mut recovered = 0_usize;
    let mut lost = 0_usize;

    for snapshot in &map.version_history {
        if snapshot.object_key.is_some() {
            migrated.push(snapshot.clone());
            continue;
        }

        let is_current = snapshot.version_id == current_version_id;
        let Some(size_bytes) = rescue_version(
            storage,
            &map.object_key,
            &snapshot.version_id,
            is_current,
            stored_versions.as_deref(),
        )
        .await?
        else {
            // The bytes are not there. Dropping the row is what stops the
            // history offering a version that cannot be opened — under a store
            // that ignored version ids, opening it returned the current data.
            lost += 1;
            tracing::warn!(
                map_id = %map.id,
                version_id = %snapshot.version_id,
                "this version's ciphertext was never stored; removing it from the history"
            );
            continue;
        };

        let mut moved = snapshot.clone();
        moved.object_key = Some(S3Store::version_key(
            &map.object_key,
            &snapshot.version_id,
        ));
        moved.size_bytes = Some(size_bytes);
        migrated.push(moved);
        recovered += 1;
    }

    // A vault saved before history was recorded has a current version that
    // appears nowhere in it. Its envelope is the one on the row.
    if !migrated
        .iter()
        .any(|snapshot| snapshot.version_id == current_version_id)
    {
        let Some(size_bytes) =
            rescue_version(storage, &map.object_key, &current_version_id, true, None).await?
        else {
            return Err(AppError::Storage(
                "the current version's object is missing from storage".to_string(),
            ));
        };

        migrated.push(VersionSnapshot {
            version_id: current_version_id.clone(),
            eph_classical_public: map.eph_classical_public.clone(),
            eph_pq_ciphertext: map.eph_pq_ciphertext.clone(),
            wrapped_dek: map.wrapped_dek.clone(),
            saved_at: map.updated_at,
            object_key: Some(S3Store::version_key(
                &map.object_key,
                &current_version_id,
            )),
            size_bytes: Some(size_bytes),
        });
        recovered += 1;
    }

    store
        .update_mind_map_upload(&map.id, &map.user_id, &current_version_id, migrated)
        .await?;

    // Only once the rows point at the copies. The bare key held the current
    // version; leaving it would double that vault's storage for good.
    if let Err(error) = storage.delete_object(&map.object_key).await {
        tracing::warn!(
            ?error,
            map_id = %map.id,
            "migrated this vault but could not remove the old object"
        );
    }

    Ok(Some(Outcome { recovered, lost }))
}

/// Copies one legacy version onto its own key and returns the stored size.
///
/// `Ok(None)` means the bytes are not in the store, which is the normal case
/// for historical versions written against a backend that accepted version ids
/// without keeping versions.
async fn rescue_version(
    storage: &S3Store,
    object_key: &str,
    version_id: &str,
    is_current: bool,
    stored_versions: Option<&[String]>,
) -> Result<Option<i64>, AppError> {
    let destination = S3Store::version_key(object_key, version_id);

    // A previous run may have been interrupted between the copy and the row.
    if let Ok(size_bytes) = storage.head_size(&destination).await {
        return Ok(Some(size_bytes));
    }

    let source_bytes = if is_current {
        // The current version is whatever the bare key holds — no version id
        // needed, so this works on every store.
        match storage.download_blob(object_key).await {
            Ok(bytes) => bytes,
            Err(AppError::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    } else {
        // Only attempt an older version where the store proved it has them.
        // Asking a store that ignores version ids would hand back the current
        // ciphertext, and it would be stored under an older version's id.
        if !stored_versions.is_some_and(|versions| versions.iter().any(|id| id == version_id)) {
            return Ok(None);
        }
        match download_legacy_version(storage, object_key, version_id).await {
            Ok(bytes) => bytes,
            Err(AppError::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    };

    storage.upload_blob(&destination, source_bytes.clone()).await?;

    // Read it back and compare. The failure this migration exists to fix was a
    // store that accepted a write and served something else, so the copy is
    // proved before the original is deleted rather than assumed.
    let written = storage.download_blob(&destination).await?;
    if written != source_bytes {
        return Err(AppError::Storage(format!(
            "copy of version '{version_id}' read back different bytes than were written"
        )));
    }

    Ok(Some(source_bytes.len() as i64))
}

/// Lists the object versions a store still holds for a key, or `None` when it
/// does not implement versioning.
async fn list_legacy_versions(storage: &S3Store, object_key: &str) -> Option<Vec<String>> {
    let response = storage
        .client
        .list_object_versions()
        .bucket(&storage.bucket)
        .prefix(object_key)
        .send()
        .await;

    match response {
        Ok(output) => Some(
            output
                .versions()
                .iter()
                .filter(|version| version.key() == Some(object_key))
                .filter_map(|version| version.version_id().map(str::to_string))
                .collect(),
        ),
        Err(error) => {
            // NotImplemented is the expected answer from Garage and R2.
            tracing::debug!(
                code = error.code().unwrap_or("unknown"),
                %object_key,
                "object versioning is unavailable; only the current version can be migrated"
            );
            None
        }
    }
}

async fn download_legacy_version(
    storage: &S3Store,
    object_key: &str,
    version_id: &str,
) -> Result<Vec<u8>, AppError> {
    let response = storage
        .client
        .get_object()
        .bucket(&storage.bucket)
        .key(object_key)
        .version_id(version_id)
        .send()
        .await
        .map_err(|error| {
            if error.raw_response().map(|r| r.status().as_u16()) == Some(404) {
                AppError::NotFound(format!("version '{version_id}' not found"))
            } else {
                AppError::Storage(error.to_string())
            }
        })?;

    let collected = response
        .body
        .collect()
        .await
        .map_err(|error| AppError::Storage(error.to_string()))?;

    Ok(collected.into_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{is_migrated, VersionSnapshot};

    fn snapshot(version_id: &str, object_key: Option<&str>) -> VersionSnapshot {
        VersionSnapshot {
            version_id: version_id.to_string(),
            eph_classical_public: "eph".to_string(),
            eph_pq_ciphertext: "pq".to_string(),
            wrapped_dek: "dek".to_string(),
            saved_at: Utc::now(),
            object_key: object_key.map(str::to_string),
            size_bytes: Some(64),
        }
    }

    #[test]
    fn a_vault_with_keys_for_every_version_is_left_alone() {
        let history = vec![
            snapshot("v-1", Some("blob/v/v-1")),
            snapshot("v-2", Some("blob/v/v-2")),
        ];

        assert!(is_migrated(&history, Some("v-2")));
    }

    #[test]
    fn a_vault_with_any_legacy_version_still_needs_migrating() {
        let history = vec![snapshot("v-1", None), snapshot("v-2", Some("blob/v/v-2"))];

        assert!(!is_migrated(&history, Some("v-2")));
    }

    /// A vault saved before history was recorded: the current version is not in
    /// the list at all, so it still has to be moved onto a key of its own.
    #[test]
    fn a_current_version_missing_from_history_still_needs_migrating() {
        assert!(!is_migrated(&[], Some("v-1")));
    }

    #[test]
    fn a_vault_that_has_never_been_saved_needs_nothing() {
        assert!(is_migrated(&[], None));
    }
}
