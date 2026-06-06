use std::{collections::{BTreeMap, HashMap, HashSet}, sync::Arc};

use axum::{
    body::Bytes,
    extract::{FromRef, Path, Query, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::{minio::MinioClient, sql_store::{DynSqlStore, MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate, NewMindMap, NewMindMapAttachment, StoredMindMap, StoredMindMapAttachment}},
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::{
        attachment::{AttachmentDownloadResponse, AttachmentMetadata, AttachmentStatus, CompleteAttachmentUploadRequest, InitAttachmentRequest, InitAttachmentResponse, UpdateAttachmentRequest},
        mindmap::{
            ConfirmUploadRequest, ConfirmUploadResponse, MindMapCreatedResponse, MindMapDetail,
            MindMapListItem, PresignedUrlResponse, StorageSummary, UpdateVaultMetaRequest,
            UpsertMindMapRequest, VaultStorageInfo, VersionDetail, VersionSnapshot,
        },
    },
};

#[derive(Clone)]
pub struct MindMapsSqlState {
    pub db: DynSqlStore,
    pub minio: MinioClient,
    pub jwt: Arc<JwtService>,
    pub diagnostics_enabled: bool,
}

impl FromRef<MindMapsSqlState> for Arc<JwtService> {
    fn from_ref(state: &MindMapsSqlState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: MindMapsSqlState) -> Router {
    let mut router = Router::new()
        .route("/", get(list_mind_maps).post(create_mind_map))
        .route("/storage", get(get_storage))
        .route("/my/storage", get(get_storage))
        .route("/{id}", get(get_mind_map).put(update_mind_map).delete(delete_mind_map))
        .route("/{id}/meta", put(update_vault_meta))
        .route("/{id}/upload", post(upload_blob))
        .route("/{id}/blob", get(download_blob))
        .route("/{id}/upload-url", post(get_upload_url))
        .route("/{id}/confirm-upload", post(confirm_upload))
        .route("/{id}/download-url", get(get_download_url))
        .route("/{id}/attachments", get(list_attachments))
        .route("/{id}/attachments/init", post(init_attachment))
        .route("/{id}/attachments/{attachment_id}", get(get_attachment).patch(update_attachment).delete(delete_attachment))
        .route("/{id}/attachments/{attachment_id}/upload", post(upload_attachment_blob))
        .route("/{id}/attachments/{attachment_id}/complete", post(complete_attachment_upload))
        .route("/{id}/attachments/{attachment_id}/download", get(get_attachment_download_url))
        .route("/{id}/attachments/{attachment_id}/blob", get(download_attachment_blob))
        .route("/{id}/versions", get(list_versions))
        .route("/{id}/versions/{version_id}", delete(delete_vault_version));

    if state.diagnostics_enabled {
        router = router.route("/maintenance/allocator-stats", get(get_allocator_stats));
    }

    router.with_state(state)
}

#[derive(Debug, Serialize)]
struct AllocatorStatsResponse {
    available: bool,
    message: String,
}

#[cfg(not(windows))]
async fn get_allocator_stats(
    State(_state): State<MindMapsSqlState>,
    _user: AuthenticatedUser,
) -> Result<Json<AllocatorStatsResponse>, AppError> {
    Ok(Json(AllocatorStatsResponse {
        available: false,
        message: "allocator stats endpoint is disabled in this build".to_string(),
    }))
}

#[cfg(windows)]
async fn get_allocator_stats(
    State(_state): State<MindMapsSqlState>,
    _user: AuthenticatedUser,
) -> Result<Json<serde_json::Value>, AppError> {
    Err(AppError::NotFound(
        "allocator stats are not available on Windows".to_string(),
    ))
}

#[derive(Debug, Deserialize)]
struct DownloadQuery {
    version_id: Option<String>,
}

async fn list_mind_maps(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<MindMapListItem>>, AppError> {
    let items = state
        .db
        .list_mind_maps(&user.0)
        .await?
        .into_iter()
        .map(|map| MindMapListItem {
            id: map.id,
            title_encrypted: map.title_encrypted,
            vault_color: map.vault_color,
            vault_note_encrypted: map.vault_note_encrypted,
            vault_encryption_mode: map.vault_encryption_mode,
            max_versions: map.max_versions,
            vault_labels: map.vault_labels,
            created_at: map.created_at,
            updated_at: map.updated_at,
        })
        .collect();

    Ok(Json(items))
}

async fn create_mind_map(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Json(body): Json<UpsertMindMapRequest>,
) -> Result<Json<MindMapCreatedResponse>, AppError> {
    validate_upsert(&body)?;

    let id = Uuid::new_v4().to_string();
    let object_key = Uuid::new_v4().to_string();
    let now = Utc::now();
    let user_id = user.0.clone();

    state
        .db
        .create_mind_map(NewMindMap {
            id: id.clone(),
            user_id: user_id.clone(),
            title_encrypted: body.title_encrypted,
            minio_object_key: object_key.clone(),
            eph_classical_public: body.eph_classical_public,
            eph_pq_ciphertext: body.eph_pq_ciphertext,
            wrapped_dek: body.wrapped_dek,
            created_at: now,
            updated_at: now,
            minio_version_id: None,
            version_history: Vec::new(),
            vault_color: None,
            vault_note_encrypted: None,
            vault_encryption_mode: "standard".to_string(),
            max_versions: 50,
            vault_labels: Vec::new(),
        })
        .await?;

    let upload_url = match state.minio.presigned_put_url(&object_key).await {
        Ok(upload_url) => upload_url,
        Err(error) => {
            if let Err(cleanup_error) = state.db.delete_mind_map(&id, &user_id).await {
                tracing::error!(
                    ?cleanup_error,
                    map_id = %id,
                    user_id = %user_id,
                    "failed to roll back mind map after upload URL creation error"
                );
            }
            return Err(error);
        }
    };

    Ok(Json(MindMapCreatedResponse {
        id,
        minio_object_key: object_key,
        upload_url,
    }))
}

async fn confirm_upload(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<ConfirmUploadRequest>,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.version_id.is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }

    let map = find_owned(&state.db, &id, &user.0).await?;
    let verified_vid = state
        .minio
        .verify_version(&map.minio_object_key, &body.version_id)
        .await?;

    let mut version_history = normalize_version_history(&map.version_history);
    version_history.push(VersionSnapshot {
        version_id: verified_vid.clone(),
        eph_classical_public: map.eph_classical_public.clone(),
        eph_pq_ciphertext: map.eph_pq_ciphertext.clone(),
        wrapped_dek: map.wrapped_dek.clone(),
        saved_at: Utc::now(),
    });

    if let Err(error) = state
        .db
        .update_mind_map_upload(&id, &user.0, &verified_vid, version_history)
        .await
    {
        if let Err(cleanup_error) = state.minio.delete_version(&map.minio_object_key, &verified_vid).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                user_id = %user.0,
                version_id = %verified_vid,
                "failed to roll back confirmed version after metadata update error"
            );
        }
        return Err(error);
    }

    let prune_key = map.minio_object_key.clone();
    let prune_limit = map.max_versions;
    let minio = state.minio.clone();
    tokio::spawn(async move {
        if let Err(e) = minio.prune_versions(&prune_key, prune_limit).await {
            tracing::warn!("Failed to prune old versions for {prune_key}: {e}");
        }
    });

    Ok(Json(ConfirmUploadResponse { version_id: verified_vid }))
}

async fn upload_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    let map = find_owned(&state.db, &id, &user.0).await?;
    let version_id = state
        .minio
        .upload_blob(&map.minio_object_key, body.to_vec())
        .await?;

    let mut version_history = normalize_version_history(&map.version_history);
    version_history.push(VersionSnapshot {
        version_id: version_id.clone(),
        eph_classical_public: map.eph_classical_public.clone(),
        eph_pq_ciphertext: map.eph_pq_ciphertext.clone(),
        wrapped_dek: map.wrapped_dek.clone(),
        saved_at: Utc::now(),
    });

    if let Err(error) = state
        .db
        .update_mind_map_upload(&id, &user.0, &version_id, version_history)
        .await
    {
        if let Err(cleanup_error) = state.minio.delete_version(&map.minio_object_key, &version_id).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                user_id = %user.0,
                version_id = %version_id,
                "failed to roll back uploaded version after metadata update error"
            );
        }
        return Err(error);
    }

    let prune_key = map.minio_object_key.clone();
    let prune_limit = map.max_versions;
    let minio = state.minio.clone();
    tokio::spawn(async move {
        if let Err(e) = minio.prune_versions(&prune_key, prune_limit).await {
            tracing::warn!("Failed to prune old versions for {prune_key}: {e}");
        }
    });

    Ok(Json(ConfirmUploadResponse { version_id }))
}

async fn get_mind_map(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<MindMapDetail>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    Ok(Json(to_detail(map)))
}

async fn update_mind_map(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<UpsertMindMapRequest>,
) -> Result<Json<PresignedUrlResponse>, AppError> {
    validate_upsert(&body)?;

    let map = find_owned(&state.db, &id, &user.0).await?;
    let now = Utc::now();

    state
        .db
        .update_mind_map_content(
            &id,
            &user.0,
            MindMapContentUpdate {
                title_encrypted: body.title_encrypted,
                eph_classical_public: body.eph_classical_public,
                eph_pq_ciphertext: body.eph_pq_ciphertext,
                wrapped_dek: body.wrapped_dek,
                updated_at: now,
            },
        )
        .await?;

    let upload_url = state.minio.presigned_put_url(&map.minio_object_key).await?;
    Ok(Json(PresignedUrlResponse {
        url: upload_url,
        expires_in_secs: state.minio.presign_expiry.as_secs(),
        version_id: None,
    }))
}

async fn delete_mind_map(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let attachments = state.db.list_mind_map_attachments(&id).await?;
    delete_attachment_objects(&state.minio, &attachments).await?;
    state.minio.delete_object(&map.minio_object_key).await?;

    state.db.delete_mind_map(&id, &user.0).await?;

    Ok(Json(serde_json::json!({ "message": "deleted" })))
}

async fn get_upload_url(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<PresignedUrlResponse>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let url = state.minio.presigned_put_url(&map.minio_object_key).await?;
    Ok(Json(PresignedUrlResponse {
        url,
        expires_in_secs: state.minio.presign_expiry.as_secs(),
        version_id: None,
    }))
}

async fn download_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Response, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let version_id = q.version_id.as_deref().map(normalize_version_id).filter(|value| !value.is_empty());
    let bytes = state
        .minio
        .download_blob(&map.minio_object_key, version_id.as_deref())
        .await?;

    Ok((
        [(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"))],
        bytes,
    )
        .into_response())
}

async fn get_download_url(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
) -> Result<Json<PresignedUrlResponse>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let normalized_version_id = q.version_id.map(|value| normalize_version_id(&value)).filter(|value| !value.is_empty());
    let vid = normalized_version_id.as_deref();
    let url = state.minio.presigned_get_url(&map.minio_object_key, vid).await?;
    Ok(Json(PresignedUrlResponse {
        url,
        expires_in_secs: state.minio.presign_expiry.as_secs(),
        version_id: normalized_version_id,
    }))
}

async fn list_attachments(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<AttachmentMetadata>>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;

    let attachments = state
        .db
        .list_mind_map_attachments(&id)
        .await?
        .into_iter()
        .map(to_attachment_metadata)
        .collect();

    Ok(Json(attachments))
}

async fn init_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<InitAttachmentRequest>,
) -> Result<Json<InitAttachmentResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_attachment_init(&body)?;

    let attachment_id = Uuid::new_v4().to_string();
    let sanitized_name = sanitize_attachment_name(&body.name);
    let s3_key = format!("maps/{id}/attachments/{attachment_id}/{sanitized_name}");
    let uploaded_at = Utc::now();

    state
        .db
        .create_mind_map_attachment(NewMindMapAttachment {
            id: attachment_id.clone(),
            map_id: id.clone(),
            node_id: normalize_optional(body.node_id),
            name: body.name.trim().to_string(),
            sanitized_name,
            content_type: body.content_type.trim().to_string(),
            size_bytes: body.size,
            s3_key: s3_key.clone(),
            s3_version_id: None,
            uploaded_by: user.0,
            uploaded_at,
            encrypted: body.encrypted,
            encryption_meta: body.encryption_meta,
            checksum_sha256: None,
            status: AttachmentStatus::Pending,
        })
        .await?;

    Ok(Json(InitAttachmentResponse {
        attachment_id: attachment_id.clone(),
        s3_key,
        upload_url: format!("/api/mindmaps/{id}/attachments/{attachment_id}/upload"),
        upload_headers: BTreeMap::new(),
        expires_at: presign_expires_at(&state)?,
    }))
}

async fn upload_attachment_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;
    let version_id = state
        .minio
        .upload_blob(&attachment.s3_key, body.to_vec())
        .await?;

    Ok(Json(ConfirmUploadResponse { version_id }))
}

async fn complete_attachment_upload(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<CompleteAttachmentUploadRequest>,
) -> Result<Json<AttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_attachment_complete(&body)?;

    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;
    let verified_vid = state
        .minio
        .verify_version(&attachment.s3_key, &body.version_id)
        .await?;

    if let Err(error) = state
        .db
        .complete_mind_map_attachment_upload(
            &id,
            &attachment_id,
            MindMapAttachmentUploadUpdate {
                s3_version_id: verified_vid.clone(),
                checksum_sha256: normalize_optional(body.checksum_sha256),
                status: AttachmentStatus::Available,
            },
        )
        .await
    {
        if let Err(cleanup_error) = state.minio.delete_version(&attachment.s3_key, &verified_vid).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                attachment_id = %attachment_id,
                user_id = %user.0,
                version_id = %verified_vid,
                "failed to roll back attachment upload after metadata update error"
            );
        }
        return Err(error);
    }

    let updated = find_attachment(&state.db, &id, &attachment_id).await?;
    Ok(Json(to_attachment_metadata(updated)))
}

async fn get_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
) -> Result<Json<AttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;

    Ok(Json(to_attachment_metadata(attachment)))
}

async fn update_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentRequest>,
) -> Result<Json<AttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    find_attachment(&state.db, &id, &attachment_id).await?;

    state
        .db
        .update_mind_map_attachment_node(&id, &attachment_id, normalize_optional(body.node_id))
        .await?;

    let updated = find_attachment(&state.db, &id, &attachment_id).await?;
    Ok(Json(to_attachment_metadata(updated)))
}

async fn get_attachment_download_url(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
) -> Result<Json<AttachmentDownloadResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;

    if attachment.status != AttachmentStatus::Available {
        return Err(AppError::BadRequest(
            "attachment upload is not complete yet".to_string(),
        ));
    }

    let download_url = format!("/api/mindmaps/{id}/attachments/{attachment_id}/blob");

    Ok(Json(AttachmentDownloadResponse {
        download_url,
        expires_at: presign_expires_at(&state)?,
        encrypted: attachment.encrypted,
        encryption_meta: attachment.encryption_meta,
        version_id: attachment.s3_version_id,
        content_type: attachment.content_type,
        name: attachment.name,
        sanitized_name: attachment.sanitized_name,
        size_bytes: attachment.size_bytes,
        checksum_sha256: attachment.checksum_sha256,
    }))
}

async fn delete_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;

    match state.minio.delete_object(&attachment.s3_key).await {
        Ok(()) | Err(AppError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    state
        .db
        .mark_mind_map_attachment_deleted(&id, &attachment_id)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn list_versions(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<VersionDetail>>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let normalized_history = normalize_version_history(&map.version_history);
    let normalized_current_version_id = map
        .minio_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());
    let total_version_count = std::cmp::max(
        normalized_history.len(),
        usize::from(normalized_current_version_id.is_some()),
    );
    let expected_version_ids = known_version_ids(&normalized_history, normalized_current_version_id.as_deref());
    let minio_versions = match state
        .minio
        .merge_known_versions(
            &map.minio_object_key,
            &expected_version_ids,
            normalized_current_version_id.as_deref(),
        )
        .await
    {
        Ok(versions) => versions,
        Err(AppError::Storage(error)) => {
            tracing::warn!(
                user_id = %user.0,
                map_id = %id,
                ?error,
                "failed to load object version details; falling back to metadata-only versions"
            );

            let current_id = normalized_current_version_id.as_deref();
            expected_version_ids
                .iter()
                .enumerate()
                .map(|(_index, version_id)| {
                    let history_snapshot = normalized_history
                        .iter()
                        .find(|snapshot| snapshot.version_id == *version_id);

                    crate::db::minio::ObjectVersionInfo {
                        version_id: version_id.clone(),
                        is_latest: current_id == Some(version_id.as_str()),
                        last_modified: history_snapshot
                            .map(|snapshot| snapshot.saved_at)
                            .unwrap_or(map.updated_at),
                        size_bytes: 0,
                    }
                })
                .collect()
        }
        Err(error) => return Err(error),
    };

    let current_version_id = normalized_current_version_id;
    let current_eph_classical = map.eph_classical_public.clone();
    let current_eph_pq = map.eph_pq_ciphertext.clone();
    let current_wrapped_dek = map.wrapped_dek.clone();

    let history: HashMap<String, (VersionSnapshot, usize)> = normalized_history
        .into_iter()
        .enumerate()
        .map(|(index, snapshot)| (snapshot.version_id.clone(), (snapshot, index + 1)))
        .collect();

    let versions = minio_versions
        .into_iter()
        .map(|version| {
            let snap = history.get(&version.version_id);
            let is_current = current_version_id.as_deref() == Some(version.version_id.as_str());
            let (version_number, eph_cl, eph_pq, wdek, saved_at) = if let Some((snapshot, version_number)) = snap {
                (
                    Some(*version_number),
                    Some(snapshot.eph_classical_public.clone()),
                    Some(snapshot.eph_pq_ciphertext.clone()),
                    Some(snapshot.wrapped_dek.clone()),
                    Some(snapshot.saved_at),
                )
            } else if is_current {
                (
                    Some(total_version_count.max(1)),
                    Some(current_eph_classical.clone()),
                    Some(current_eph_pq.clone()),
                    Some(current_wrapped_dek.clone()),
                    None,
                )
            } else {
                (None, None, None, None, None)
            };

            VersionDetail {
                version_id: version.version_id,
                version_number,
                is_latest: version.is_latest,
                last_modified: version.last_modified,
                size_bytes: version.size_bytes,
                eph_classical_public: eph_cl,
                eph_pq_ciphertext: eph_pq,
                wrapped_dek: wdek,
                saved_at,
            }
        })
        .collect();

    Ok(Json(versions))
}

fn known_version_ids(
    version_history: &[VersionSnapshot],
    current_version_id: Option<&str>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut version_ids = Vec::new();

    for snapshot in version_history.iter().rev() {
        let version_id = normalize_version_id(&snapshot.version_id);
        if version_id.is_empty() {
            continue;
        }
        if seen.insert(version_id.clone()) {
            version_ids.push(version_id);
        }
    }

    if let Some(version_id) = current_version_id {
        let version_id = normalize_version_id(version_id);
        if !version_id.is_empty() && seen.insert(version_id.clone()) {
            version_ids.insert(0, version_id);
        }
    }

    version_ids
}

async fn delete_vault_version(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, version_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;

    let normalized_target_version_id = normalize_version_id(&version_id);
    if normalized_target_version_id.is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }
    let normalized_current_version_id = map
        .minio_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());

    if normalized_current_version_id.as_deref() == Some(normalized_target_version_id.as_str()) {
        return Err(AppError::BadRequest(
            "Cannot delete the current active version; save a new version first.".to_string(),
        ));
    }

    state.minio.delete_version(&map.minio_object_key, &normalized_target_version_id).await?;

    let filtered_history: Vec<VersionSnapshot> = normalize_version_history(&map.version_history)
        .into_iter()
        .filter(|snapshot| snapshot.version_id != normalized_target_version_id)
        .collect();

    let minio_version_id = normalized_current_version_id.ok_or_else(|| {
        AppError::BadRequest("Current active version is missing from vault metadata.".to_string())
    })?;

    state
        .db
        .update_mind_map_upload(&id, &user.0, &minio_version_id, filtered_history)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn update_vault_meta(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<UpdateVaultMetaRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let title_changed = body.title_encrypted.is_some();
    let max_versions = if let Some(value) = body.max_versions {
        if value == 0 {
            return Err(AppError::BadRequest("max_versions must be >= 1".to_string()));
        }
        value
    } else {
        map.max_versions
    };

    let title_encrypted = body.title_encrypted.unwrap_or(map.title_encrypted);
    let updated_at = if title_changed {
        Utc::now()
    } else {
        map.updated_at
    };

    state
        .db
        .update_mind_map_meta(
            &id,
            &user.0,
            MindMapMetaUpdate {
                title_encrypted,
                vault_color: body.vault_color.or(map.vault_color),
                vault_note_encrypted: body.vault_note_encrypted.or(map.vault_note_encrypted),
                vault_encryption_mode: body.vault_encryption_mode.unwrap_or(map.vault_encryption_mode),
                max_versions,
                vault_labels: body.vault_labels.unwrap_or(map.vault_labels),
                updated_at,
            },
        )
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn get_storage(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
) -> Result<Json<StorageSummary>, AppError> {
    let maps = state.db.list_mind_maps(&user.0).await?;

    let mut vaults = Vec::new();
    let mut grand_total = 0_i64;
    let mut grand_attachment_count = 0_usize;
    let mut grand_attachment_bytes = 0_i64;

    for map in maps {
        let expected_version_ids = known_version_ids(&map.version_history, map.minio_version_id.as_deref());
        let versions = match state
            .minio
            .merge_known_versions(
                &map.minio_object_key,
                &expected_version_ids,
                map.minio_version_id.as_deref(),
            )
            .await
        {
            Ok(versions) => versions,
            Err(AppError::Storage(error)) => {
                tracing::warn!(
                    user_id = %user.0,
                    map_id = %map.id,
                    ?error,
                    "failed to load object version details for storage summary; falling back to attachment-only totals"
                );
                Vec::new()
            }
            Err(error) => return Err(error),
        };
        let version_total_bytes: i64 = versions.iter().map(|version| version.size_bytes).sum();
        let (attachment_count, attachment_bytes, all_attachment_bytes) = load_map_attachment_storage(&state.db, &map.id).await?;
        let total_bytes = version_total_bytes + all_attachment_bytes;
        grand_total += total_bytes;
        grand_attachment_count += attachment_count;
        grand_attachment_bytes += attachment_bytes;
        vaults.push(VaultStorageInfo {
            id: map.id,
            title_encrypted: map.title_encrypted,
            version_count: versions.len(),
            attachment_count,
            attachment_bytes,
            total_bytes,
        });
    }

    Ok(Json(StorageSummary {
        vaults,
        total_bytes: grand_total,
        attachment_count: grand_attachment_count,
        attachment_bytes: grand_attachment_bytes,
        free_tier_bytes: i64::MAX,
        plan_tier: "community".to_string(),
        plan_limit_bytes: i64::MAX,
    }))
}

async fn load_map_attachment_storage(
    db: &DynSqlStore,
    map_id: &str,
) -> Result<(usize, i64, i64), AppError> {
    // Returns (primary_count, primary_bytes, all_available_bytes).
    // Preview thumbnails (cryptmind_role == "preview") are excluded from the user-facing
    // count and bytes but still included in all_available_bytes so total storage is accurate.
    let attachments = db.list_mind_map_attachments(map_id).await?;
    let available: Vec<_> = attachments
        .iter()
        .filter(|a| a.status == AttachmentStatus::Available)
        .collect();
    let all_bytes: i64 = available.iter().map(|a| a.size_bytes).sum();
    let (primary_count, primary_bytes) = available.iter().fold((0usize, 0i64), |acc, a| {
        let is_preview = a
            .encryption_meta
            .as_ref()
            .and_then(|m| m.get("cryptmind_role"))
            .and_then(|r| r.as_str())
            == Some("preview");
        if is_preview { acc } else { (acc.0 + 1, acc.1 + a.size_bytes) }
    });
    Ok((primary_count, primary_bytes, all_bytes))
}

async fn find_owned(db: &DynSqlStore, id: &str, user_id: &str) -> Result<StoredMindMap, AppError> {
    db.get_mind_map_owned(id, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("mind map not found".to_string()))
}

async fn find_attachment(
    db: &DynSqlStore,
    map_id: &str,
    attachment_id: &str,
) -> Result<StoredMindMapAttachment, AppError> {
    db.get_mind_map_attachment(map_id, attachment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("attachment not found".to_string()))
}

async fn delete_attachment_objects(
    minio: &MinioClient,
    attachments: &[StoredMindMapAttachment],
) -> Result<(), AppError> {
    for attachment in attachments {
        match minio.delete_object(&attachment.s3_key).await {
            Ok(()) | Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(())
}

fn validate_upsert(body: &UpsertMindMapRequest) -> Result<(), AppError> {
    if body.title_encrypted.is_empty()
        || body.eph_classical_public.is_empty()
        || body.eph_pq_ciphertext.is_empty()
        || body.wrapped_dek.is_empty()
    {
        return Err(AppError::BadRequest("missing required encrypted fields".to_string()));
    }
    Ok(())
}

fn validate_attachment_init(body: &InitAttachmentRequest) -> Result<(), AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("attachment name is required".to_string()));
    }
    if body.content_type.trim().is_empty() {
        return Err(AppError::BadRequest("content_type is required".to_string()));
    }
    if body.size < 0 {
        return Err(AppError::BadRequest("size must be >= 0".to_string()));
    }
    if body.encrypted && body.encryption_meta.is_none() {
        return Err(AppError::BadRequest(
            "encryption_meta is required for encrypted attachments".to_string(),
        ));
    }
    if !body.encrypted && body.encryption_meta.is_some() {
        return Err(AppError::BadRequest(
            "encryption_meta is only allowed for encrypted attachments".to_string(),
        ));
    }
    Ok(())
}

fn validate_attachment_complete(body: &CompleteAttachmentUploadRequest) -> Result<(), AppError> {
    if body.version_id.trim().is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }
    Ok(())
}

fn sanitize_attachment_name(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len());
    for ch in name.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            sanitized.push(ch);
        } else if ch.is_whitespace() || matches!(ch, '/' | '\\' | ':') {
            sanitized.push('-');
        }
    }

    let sanitized = sanitized
        .trim_matches(|ch| matches!(ch, '-' | '.'))
        .chars()
        .take(120)
        .collect::<String>();

    if sanitized.is_empty() {
        "attachment".to_string()
    } else {
        sanitized
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.map(|item| item.trim().to_string()).filter(|item| !item.is_empty())
}

fn normalize_version_id(version_id: &str) -> String {
    version_id.trim().trim_matches('"').to_string()
}

fn normalize_version_history(version_history: &[VersionSnapshot]) -> Vec<VersionSnapshot> {
    let mut normalized = Vec::with_capacity(version_history.len());
    let mut seen = HashSet::new();

    for snapshot in version_history {
        let version_id = normalize_version_id(&snapshot.version_id);
        if version_id.is_empty() || !seen.insert(version_id.clone()) {
            continue;
        }

        normalized.push(VersionSnapshot {
            version_id,
            eph_classical_public: snapshot.eph_classical_public.clone(),
            eph_pq_ciphertext: snapshot.eph_pq_ciphertext.clone(),
            wrapped_dek: snapshot.wrapped_dek.clone(),
            saved_at: snapshot.saved_at,
        });
    }

    normalized
}

fn presign_expires_at(state: &MindMapsSqlState) -> Result<chrono::DateTime<Utc>, AppError> {
    let delta = chrono::Duration::from_std(state.minio.presign_expiry)
        .map_err(|error| AppError::Internal(format!("invalid presign expiry: {error}")))?;
    Ok(Utc::now() + delta)
}

fn to_attachment_metadata(attachment: StoredMindMapAttachment) -> AttachmentMetadata {
    AttachmentMetadata {
        id: attachment.id,
        map_id: attachment.map_id,
        node_id: attachment.node_id,
        name: attachment.name,
        sanitized_name: attachment.sanitized_name,
        content_type: attachment.content_type,
        size_bytes: attachment.size_bytes,
        uploaded_by: attachment.uploaded_by,
        uploaded_at: attachment.uploaded_at,
        encrypted: attachment.encrypted,
        encryption_meta: attachment.encryption_meta,
        checksum_sha256: attachment.checksum_sha256,
        s3_version_id: attachment.s3_version_id,
        status: attachment.status,
    }
}

fn to_detail(map: StoredMindMap) -> MindMapDetail {
    let StoredMindMap {
        id,
        user_id: _,
        title_encrypted,
        minio_object_key: _,
        eph_classical_public,
        eph_pq_ciphertext,
        wrapped_dek,
        created_at,
        updated_at,
        minio_version_id,
        version_history,
        vault_color,
        vault_note_encrypted,
        vault_encryption_mode,
        max_versions,
        vault_labels: _,
    } = map;

    let normalized_history = normalize_version_history(&version_history);
    let normalized_minio_version_id = minio_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());
    let total_version_count = std::cmp::max(
        normalized_history.len(),
        usize::from(normalized_minio_version_id.is_some()),
    );

    MindMapDetail {
        id,
        title_encrypted,
        eph_classical_public,
        eph_pq_ciphertext,
        wrapped_dek,
        vault_color,
        vault_note_encrypted,
        vault_encryption_mode,
        max_versions,
        total_version_count,
        minio_version_id: normalized_minio_version_id,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{known_version_ids, normalize_version_history, normalize_version_id, sanitize_attachment_name, VersionSnapshot};

    #[test]
    fn sanitizes_attachment_names_for_object_keys() {
        assert_eq!(sanitize_attachment_name(" Quarterly Plan 2026.pdf "), "Quarterly-Plan-2026.pdf");
        assert_eq!(sanitize_attachment_name("../../secret?.zip"), "secret.zip");
    }

    #[test]
    fn falls_back_when_name_has_no_safe_characters() {
        assert_eq!(sanitize_attachment_name("???///:::"), "attachment");
    }

    #[test]
    fn normalizes_and_deduplicates_version_history_ids() {
        let saved_at = Utc::now();
        let history = vec![
            VersionSnapshot {
                version_id: " \"62ea0a57-8b2a-4fd2-8046-cf8769d81489\" ".to_string(),
                eph_classical_public: "eph-a".to_string(),
                eph_pq_ciphertext: "pq-a".to_string(),
                wrapped_dek: "dek-a".to_string(),
                saved_at,
            },
            VersionSnapshot {
                version_id: "62ea0a57-8b2a-4fd2-8046-cf8769d81489".to_string(),
                eph_classical_public: "eph-b".to_string(),
                eph_pq_ciphertext: "pq-b".to_string(),
                wrapped_dek: "dek-b".to_string(),
                saved_at,
            },
            VersionSnapshot {
                version_id: " c913788a-3366-4846-8fc5-ed3074b0a20e ".to_string(),
                eph_classical_public: "eph-c".to_string(),
                eph_pq_ciphertext: "pq-c".to_string(),
                wrapped_dek: "dek-c".to_string(),
                saved_at,
            },
        ];

        let normalized = normalize_version_history(&history);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].version_id, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
        assert_eq!(normalized[1].version_id, "c913788a-3366-4846-8fc5-ed3074b0a20e");
    }

    #[test]
    fn prefers_normalized_current_id_and_dedupes_history() {
        let saved_at = Utc::now();
        let history = vec![
            VersionSnapshot {
                version_id: "\"a0000000-0000-0000-0000-000000000001\"".to_string(),
                eph_classical_public: "eph".to_string(),
                eph_pq_ciphertext: "pq".to_string(),
                wrapped_dek: "dek".to_string(),
                saved_at,
            },
            VersionSnapshot {
                version_id: "a0000000-0000-0000-0000-000000000002".to_string(),
                eph_classical_public: "eph".to_string(),
                eph_pq_ciphertext: "pq".to_string(),
                wrapped_dek: "dek".to_string(),
                saved_at,
            },
        ];

        let ids = known_version_ids(
            &history,
            Some(" \"a0000000-0000-0000-0000-000000000002\" "),
        );

        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "a0000000-0000-0000-0000-000000000002");
        assert_eq!(ids[1], "a0000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn normalizes_single_version_id_from_legacy_formatting() {
        assert_eq!(
            normalize_version_id(" \"b0000000-0000-0000-0000-000000000003\" "),
            "b0000000-0000-0000-0000-000000000003"
        );
    }
}

async fn download_attachment_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, attachment_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;

    if attachment.status != AttachmentStatus::Available {
        return Err(AppError::BadRequest(
            "attachment upload is not complete yet".to_string(),
        ));
    }

    let bytes = state
        .minio
        .download_blob(&attachment.s3_key, attachment.s3_version_id.as_deref())
        .await?;

    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&attachment.content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )],
        bytes,
    )
        .into_response())
}