use std::{collections::{BTreeMap, HashMap}, sync::Arc};

use axum::{
    body::Bytes,
    extract::{FromRef, Path, Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    db::{minio::MinioClient, sql_store::{DynSqlStore, MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate, MindMapShareAttachmentUploadUpdate, MindMapShareUploadUpdate, NewMindMap, NewMindMapAttachment, NewMindMapShare, NewMindMapShareAttachment, StoredMindMap, StoredMindMapAttachment, StoredMindMapShare, StoredMindMapShareAttachment}},
    error::AppError,
    middleware::auth::{AuthenticatedUser, JwtService},
    models::{
        attachment::{AttachmentDownloadResponse, AttachmentMetadata, AttachmentStatus, CompleteAttachmentUploadRequest, InitAttachmentRequest, InitAttachmentResponse, UpdateAttachmentRequest},
        mindmap::{
            ConfirmUploadRequest, ConfirmUploadResponse, MindMapCreatedResponse, MindMapDetail,
            MindMapListItem, PresignedUrlResponse, StorageSummary, UpdateVaultMetaRequest,
            UpsertMindMapRequest, VaultStorageInfo, VersionDetail, VersionSnapshot,
        },
        notifications::{NewNotificationEvent, NotificationPriority},
        share::{CompleteMapShareAttachmentUploadRequest, CompleteMapShareUploadRequest, CreateMapShareRequest, CreateMapShareResponse, InitMapShareAttachmentRequest, InitMapShareAttachmentResponse, MapShareAttachmentMetadata, MapShareOwnerSummary, ShareStatus},
        user::SubscriptionTier,
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
        .route("/{id}/shares", get(list_shares).post(create_share))
        .route("/{id}/shares/{share_id}/upload", post(upload_share_blob))
        .route("/{id}/shares/{share_id}/complete", post(complete_share_upload))
        .route("/{id}/shares/{share_id}/revoke", post(revoke_share))
        .route("/{id}/shares/{share_id}/attachments", post(init_share_attachment))
        .route("/{id}/shares/{share_id}/attachments/{attachment_id}/upload", post(upload_share_attachment_blob))
        .route("/{id}/shares/{share_id}/attachments/{attachment_id}/complete", post(complete_share_attachment_upload))
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
    allocated: usize,
    active: usize,
    resident: usize,
    mapped: usize,
    retained: usize,
}

async fn get_allocator_stats(
    State(_state): State<MindMapsSqlState>,
    _user: AuthenticatedUser,
) -> Result<Json<AllocatorStatsResponse>, AppError> {
    let epoch = jemalloc_ctl::epoch::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc epoch: {e}")))?;
    let allocated = jemalloc_ctl::stats::allocated::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc allocated stat: {e}")))?;
    let active = jemalloc_ctl::stats::active::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc active stat: {e}")))?;
    let resident = jemalloc_ctl::stats::resident::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc resident stat: {e}")))?;
    let mapped = jemalloc_ctl::stats::mapped::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc mapped stat: {e}")))?;
    let retained = jemalloc_ctl::stats::retained::mib()
        .map_err(|e| AppError::Internal(format!("failed to access jemalloc retained stat: {e}")))?;

    epoch
        .advance()
        .map_err(|e| AppError::Internal(format!("failed to refresh jemalloc stats: {e}")))?;

    Ok(Json(AllocatorStatsResponse {
        allocated: allocated
            .read()
            .map_err(|e| AppError::Internal(format!("failed to read jemalloc allocated stat: {e}")))?,
        active: active
            .read()
            .map_err(|e| AppError::Internal(format!("failed to read jemalloc active stat: {e}")))?,
        resident: resident
            .read()
            .map_err(|e| AppError::Internal(format!("failed to read jemalloc resident stat: {e}")))?,
        mapped: mapped
            .read()
            .map_err(|e| AppError::Internal(format!("failed to read jemalloc mapped stat: {e}")))?,
        retained: retained
            .read()
            .map_err(|e| AppError::Internal(format!("failed to read jemalloc retained stat: {e}")))?,
    }))
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
            vault_sharing_mode: map.vault_sharing_mode,
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
            vault_sharing_mode: "private".to_string(),
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
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;
    let total_bytes_after_upload = load_storage_usage_total_bytes(&state.db, &state.minio, &user.0).await?;
    let plan_limit_bytes = subscription_tier.storage_limit_bytes();
    if total_bytes_after_upload > plan_limit_bytes {
        state
            .minio
            .delete_version(&map.minio_object_key, &verified_vid)
            .await?;
        return Err(storage_quota_exceeded_error(
            &subscription_tier,
            total_bytes_after_upload,
            plan_limit_bytes,
        ));
    }

    let mut version_history = map.version_history.clone();
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
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;
    let current_total_bytes = load_storage_usage_total_bytes(&state.db, &state.minio, &user.0).await?;
    let projected_total_bytes = current_total_bytes + body.len() as i64;
    let plan_limit_bytes = subscription_tier.storage_limit_bytes();
    if projected_total_bytes > plan_limit_bytes {
        return Err(storage_quota_exceeded_error(
            &subscription_tier,
            projected_total_bytes,
            plan_limit_bytes,
        ));
    }

    let version_id = state
        .minio
        .upload_blob(&map.minio_object_key, body.to_vec())
        .await?;

    let mut version_history = map.version_history.clone();
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
    let shares = state.db.list_mind_map_shares(&id).await?;
    delete_attachment_objects(&state.minio, &attachments).await?;
    delete_share_objects(&state.db, &state.minio, &shares).await?;
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
    let bytes = state
        .minio
        .download_blob(&map.minio_object_key, q.version_id.as_deref())
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
    let vid = q.version_id.as_deref();
    let url = state.minio.presigned_get_url(&map.minio_object_key, vid).await?;
    Ok(Json(PresignedUrlResponse {
        url,
        expires_in_secs: state.minio.presign_expiry.as_secs(),
        version_id: q.version_id,
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

async fn list_shares(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<MapShareOwnerSummary>>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let share_base_url = share_base_url(&headers);
    let shares = state
        .db
        .list_mind_map_shares(&id)
        .await?
        .into_iter()
        .map(|share| to_owner_share_summary(share, &share_base_url))
        .collect();

    Ok(Json(shares))
}

async fn create_share(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CreateMapShareRequest>,
) -> Result<Json<CreateMapShareResponse>, AppError> {
    let user_id = user.0.clone();
    find_owned(&state.db, &id, &user_id).await?;
    validate_share_create(&body)?;
    let subscription_tier = load_effective_subscription_tier(&state.db, &user_id).await?;

    if !subscription_tier.can_create_public_shares() {
        return Err(AppError::plan_restricted(
            "your current plan cannot create encrypted share links",
            "share_creation_blocked",
            "can_create_public_shares",
            subscription_tier.as_str(),
            Some("paid"),
            None,
            None,
        ));
    }

    if body.include_attachments && !subscription_tier.can_include_attachments_in_shares() {
        return Err(AppError::plan_restricted(
            "upgrade to the paid plan to include attachments in encrypted shares",
            "share_attachments_plan_required",
            "can_include_attachments_in_shares",
            subscription_tier.as_str(),
            Some("paid"),
            None,
            None,
        ));
    }

    let active_share_count = state
        .db
        .list_mind_map_shares(&id)
        .await?
        .into_iter()
        .filter(|share| !share.revoked)
        .count() as i64;
    let max_active_shares = subscription_tier.max_active_shares();
    if active_share_count >= max_active_shares {
        return Err(AppError::plan_restricted(
            format!(
                "your current plan allows up to {max_active_shares} active encrypted shares per vault"
            ),
            "active_share_limit_reached",
            "max_active_shares",
            subscription_tier.as_str(),
            None,
            Some(active_share_count),
            Some(max_active_shares),
        ));
    }

    let share_id = Uuid::new_v4().to_string();
    let share_name = body.name.trim().to_string();
    let share_scope = body.scope.clone();
    let sanitized_name = sanitize_attachment_name(&share_name);
    let s3_key = format!("maps/{id}/shares/{share_id}/{sanitized_name}");
    let now = Utc::now();

    state
        .db
        .create_mind_map_share(NewMindMapShare {
            id: share_id.clone(),
            map_id: id.clone(),
            share_name: share_name.clone(),
            scope: share_scope.clone(),
            s3_key: s3_key.clone(),
            s3_version_id: None,
            created_by: user_id.clone(),
            created_at: now,
            updated_at: now,
            expires_at: body.expires_at,
            revoked: false,
            include_attachments: body.include_attachments,
            passphrase_hint: normalize_optional(body.passphrase_hint),
            content_type: body.content_type.trim().to_string(),
            size_bytes: body.size_bytes,
            encryption_meta: body
                .encryption_meta
                .ok_or_else(|| AppError::BadRequest("encryption_meta is required for shares".to_string()))?,
            checksum_sha256: None,
            status: ShareStatus::Pending,
        })
        .await?;

    record_notification_event(
        &state.db,
        NewNotificationEvent {
            id: Uuid::new_v4().to_string(),
            user_id: user_id.clone(),
            event_type: "share_created".to_string(),
            category: "sharing".to_string(),
            priority: NotificationPriority::Medium,
            actor_user_id: Some(user_id),
            object_type: "mind_map_share".to_string(),
            object_id: share_id.clone(),
            object_label_safe: Some(share_name),
            reason_code: "share_created".to_string(),
            payload_json: json!({
                "map_id": id,
                "include_attachments": body.include_attachments,
                "scope": share_scope.as_str(),
            }),
            created_at: now,
        },
    )
    .await;

    Ok(Json(CreateMapShareResponse {
        share_id: share_id.clone(),
        share_url: format!("{}/{}", share_base_url(&headers), share_id),
        s3_key: s3_key.clone(),
        upload_url: format!("/api/mindmaps/{id}/shares/{share_id}/upload"),
        upload_headers: BTreeMap::new(),
        expires_at: presign_expires_at(&state)?,
    }))
}

async fn upload_share_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, share_id)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    find_owned(&state.db, &id, &user.0).await?;
    let share = find_share(&state.db, &id, &share_id).await?;
    if share.revoked {
        return Err(AppError::BadRequest("share is revoked".to_string()));
    }

    let version_id = state
        .minio
        .upload_blob(&share.s3_key, body.to_vec())
        .await?;

    Ok(Json(ConfirmUploadResponse { version_id }))
}

async fn complete_share_upload(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path((id, share_id)): Path<(String, String)>,
    Json(body): Json<CompleteMapShareUploadRequest>,
) -> Result<Json<MapShareOwnerSummary>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_complete(&body.version_id, body.checksum_sha256.as_deref())?;

    let share = find_share(&state.db, &id, &share_id).await?;
    let verified_vid = state.minio.verify_version(&share.s3_key, &body.version_id).await?;
    if let Err(error) = state
        .db
        .complete_mind_map_share_upload(
            &id,
            &share_id,
            MindMapShareUploadUpdate {
                s3_version_id: verified_vid.clone(),
                checksum_sha256: normalize_optional(body.checksum_sha256),
                status: ShareStatus::Available,
            },
        )
        .await
    {
        if let Err(cleanup_error) = state.minio.delete_version(&share.s3_key, &verified_vid).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                share_id = %share_id,
                user_id = %user.0,
                version_id = %verified_vid,
                "failed to roll back share upload after metadata update error"
            );
        }
        return Err(error);
    }

    let updated = find_share(&state.db, &id, &share_id).await?;
    Ok(Json(to_owner_share_summary(updated, &share_base_url(&headers))))
}

async fn revoke_share(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    headers: HeaderMap,
    Path((id, share_id)): Path<(String, String)>,
) -> Result<Json<MapShareOwnerSummary>, AppError> {
    let user_id = user.0.clone();
    find_owned(&state.db, &id, &user_id).await?;
    let existing = find_share(&state.db, &id, &share_id).await?;
    state.db.set_mind_map_share_revoked(&id, &share_id, true).await?;

    record_notification_event(
        &state.db,
        NewNotificationEvent {
            id: Uuid::new_v4().to_string(),
            user_id,
            event_type: "share_revoked".to_string(),
            category: "sharing".to_string(),
            priority: NotificationPriority::Medium,
            actor_user_id: Some(existing.created_by.clone()),
            object_type: "mind_map_share".to_string(),
            object_id: share_id.clone(),
            object_label_safe: Some(existing.share_name.clone()),
            reason_code: "share_revoked".to_string(),
            payload_json: json!({
                "map_id": id,
                "include_attachments": existing.include_attachments,
            }),
            created_at: Utc::now(),
        },
    )
    .await;

    let updated = find_share(&state.db, &id, &share_id).await?;
    Ok(Json(to_owner_share_summary(updated, &share_base_url(&headers))))
}

async fn init_share_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, share_id)): Path<(String, String)>,
    Json(body): Json<InitMapShareAttachmentRequest>,
) -> Result<Json<InitMapShareAttachmentResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_attachment_init(&body)?;
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;
    let share = find_share(&state.db, &id, &share_id).await?;

    if !subscription_tier.can_include_attachments_in_shares() {
        return Err(AppError::plan_restricted(
            "upgrade to the paid plan to add attachments to encrypted shares",
            "share_attachments_plan_required",
            "can_include_attachments_in_shares",
            subscription_tier.as_str(),
            Some("paid"),
            None,
            None,
        ));
    }

    let max_attachment_size_bytes = subscription_tier.max_attachment_size_bytes();
    if body.size > max_attachment_size_bytes {
        return Err(AppError::plan_restricted(
            format!(
                "attachment size exceeds your plan limit of {max_attachment_size_bytes} bytes"
            ),
            "attachment_size_limit_exceeded",
            "max_attachment_size_bytes",
            subscription_tier.as_str(),
            None,
            Some(body.size),
            Some(max_attachment_size_bytes),
        ));
    }

    if !share.include_attachments {
        return Err(AppError::BadRequest(
            "share was created without attachment support".to_string(),
        ));
    }
    if share.revoked {
        return Err(AppError::BadRequest("share is revoked".to_string()));
    }

    if let Some(source_attachment_id) = body.source_attachment_id.as_deref() {
        find_attachment(&state.db, &id, source_attachment_id).await?;
    }

    let attachment_id = Uuid::new_v4().to_string();
    let sanitized_name = sanitize_attachment_name(&body.name);
    let s3_key = format!(
        "maps/{id}/shares/{share_id}/attachments/{attachment_id}/{sanitized_name}"
    );

    state
        .db
        .create_mind_map_share_attachment(NewMindMapShareAttachment {
            id: attachment_id.clone(),
            share_id: share_id.clone(),
            source_attachment_id: normalize_optional(body.source_attachment_id),
            node_id: normalize_optional(body.node_id),
            name: body.name.trim().to_string(),
            sanitized_name,
            content_type: body.content_type.trim().to_string(),
            size_bytes: body.size,
            s3_key: s3_key.clone(),
            s3_version_id: None,
            uploaded_at: Utc::now(),
            encryption_meta: body
                .encryption_meta
                .ok_or_else(|| AppError::BadRequest("encryption_meta is required for shared attachments".to_string()))?,
            checksum_sha256: None,
            status: AttachmentStatus::Pending,
        })
        .await?;

    Ok(Json(InitMapShareAttachmentResponse {
        attachment_id: attachment_id.clone(),
        s3_key: s3_key.clone(),
        upload_url: format!("/api/mindmaps/{id}/shares/{share_id}/attachments/{attachment_id}/upload"),
        upload_headers: BTreeMap::new(),
        expires_at: presign_expires_at(&state)?,
    }))
}

async fn upload_share_attachment_blob(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, share_id, attachment_id)): Path<(String, String, String)>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    find_owned(&state.db, &id, &user.0).await?;
    let share = find_share(&state.db, &id, &share_id).await?;
    if share.revoked {
        return Err(AppError::BadRequest("share is revoked".to_string()));
    }

    let attachment = find_share_attachment(&state.db, &share_id, &attachment_id).await?;
    let version_id = state
        .minio
        .upload_blob(&attachment.s3_key, body.to_vec())
        .await?;

    Ok(Json(ConfirmUploadResponse { version_id }))
}

async fn complete_share_attachment_upload(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path((id, share_id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<CompleteMapShareAttachmentUploadRequest>,
) -> Result<Json<MapShareAttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_complete(&body.version_id, body.checksum_sha256.as_deref())?;
    let attachment = find_share_attachment(&state.db, &share_id, &attachment_id).await?;
    let verified_vid = state.minio.verify_version(&attachment.s3_key, &body.version_id).await?;

    if let Err(error) = state
        .db
        .complete_mind_map_share_attachment_upload(
            &share_id,
            &attachment_id,
            MindMapShareAttachmentUploadUpdate {
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
                share_id = %share_id,
                attachment_id = %attachment_id,
                user_id = %user.0,
                version_id = %verified_vid,
                "failed to roll back shared attachment upload after metadata update error"
            );
        }
        return Err(error);
    }

    let updated = find_share_attachment(&state.db, &share_id, &attachment_id).await?;
    Ok(Json(to_share_attachment_metadata(updated)))
}

async fn init_attachment(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(body): Json<InitAttachmentRequest>,
) -> Result<Json<InitAttachmentResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_attachment_init(&body)?;
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;
    let max_attachment_size_bytes = subscription_tier.max_attachment_size_bytes();

    if body.size > max_attachment_size_bytes {
        return Err(AppError::plan_restricted(
            format!(
                "attachment size exceeds your plan limit of {max_attachment_size_bytes} bytes"
            ),
            "attachment_size_limit_exceeded",
            "max_attachment_size_bytes",
            subscription_tier.as_str(),
            None,
            Some(body.size),
            Some(max_attachment_size_bytes),
        ));
    }

    let current_total_bytes = load_storage_usage_total_bytes(&state.db, &state.minio, &user.0).await?;
    let projected_total_bytes = current_total_bytes + body.size;
    let plan_limit_bytes = subscription_tier.storage_limit_bytes();
    if projected_total_bytes > plan_limit_bytes {
        return Err(storage_quota_exceeded_error(
            &subscription_tier,
            projected_total_bytes,
            plan_limit_bytes,
        ));
    }

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
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;
    let current_total_bytes = load_storage_usage_total_bytes(&state.db, &state.minio, &user.0).await?;
    let projected_total_bytes = if attachment.status == AttachmentStatus::Available {
        current_total_bytes
    } else {
        current_total_bytes + attachment.size_bytes
    };
    let plan_limit_bytes = subscription_tier.storage_limit_bytes();
    if projected_total_bytes > plan_limit_bytes {
        state
            .minio
            .delete_version(&attachment.s3_key, &verified_vid)
            .await?;
        return Err(storage_quota_exceeded_error(
            &subscription_tier,
            projected_total_bytes,
            plan_limit_bytes,
        ));
    }

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
    let total_version_count = std::cmp::max(
        map.version_history.len(),
        usize::from(map.minio_version_id.is_some()),
    );
    let expected_version_ids = known_version_ids(&map.version_history, map.minio_version_id.as_deref());
    let minio_versions = state
        .minio
        .merge_known_versions(
            &map.minio_object_key,
            &expected_version_ids,
            map.minio_version_id.as_deref(),
        )
        .await?;

    let current_version_id = map.minio_version_id.clone();
    let current_eph_classical = map.eph_classical_public.clone();
    let current_eph_pq = map.eph_pq_ciphertext.clone();
    let current_wrapped_dek = map.wrapped_dek.clone();

    let history: HashMap<String, (VersionSnapshot, usize)> = map
        .version_history
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
    let mut seen = std::collections::HashSet::new();
    let mut version_ids = Vec::new();

    for snapshot in version_history.iter().rev() {
        if seen.insert(snapshot.version_id.clone()) {
            version_ids.push(snapshot.version_id.clone());
        }
    }

    if let Some(version_id) = current_version_id {
        if seen.insert(version_id.to_string()) {
            version_ids.insert(0, version_id.to_string());
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

    if map.minio_version_id.as_deref() == Some(version_id.as_str()) {
        return Err(AppError::BadRequest(
            "Cannot delete the current active version; save a new version first.".to_string(),
        ));
    }

    state.minio.delete_version(&map.minio_object_key, &version_id).await?;

    let filtered_history: Vec<VersionSnapshot> = map
        .version_history
        .into_iter()
        .filter(|snapshot| snapshot.version_id != version_id)
        .collect();

    let minio_version_id = map.minio_version_id.clone().ok_or_else(|| {
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
                vault_sharing_mode: body.vault_sharing_mode.unwrap_or(map.vault_sharing_mode),
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
    let subscription_tier = load_effective_subscription_tier(&state.db, &user.0).await?;

    let maps = state.db.list_mind_maps(&user.0).await?;

    let mut vaults = Vec::new();
    let mut grand_total = 0_i64;
    let mut grand_attachment_count = 0_usize;
    let mut grand_attachment_bytes = 0_i64;

    for map in maps {
        let expected_version_ids = known_version_ids(&map.version_history, map.minio_version_id.as_deref());
        let versions = state
            .minio
            .merge_known_versions(
                &map.minio_object_key,
                &expected_version_ids,
                map.minio_version_id.as_deref(),
            )
            .await?;
        let version_total_bytes: i64 = versions.iter().map(|version| version.size_bytes).sum();
        let (attachment_count, attachment_bytes) = load_map_attachment_storage(&state.db, &map.id).await?;
        let share_bytes = load_map_share_storage_bytes(&state.db, &map.id).await?;
        let total_bytes = version_total_bytes + attachment_bytes + share_bytes;
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
        free_tier_bytes: subscription_tier.storage_limit_bytes(),
        plan_tier: subscription_tier.as_str().to_string(),
        plan_limit_bytes: subscription_tier.storage_limit_bytes(),
    }))
}

async fn load_effective_subscription_tier(
    db: &DynSqlStore,
    user_id: &str,
) -> Result<SubscriptionTier, AppError> {
    Ok(db
        .load_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?
        .effective_subscription_tier(Utc::now()))
}

async fn load_storage_usage_total_bytes(
    db: &DynSqlStore,
    minio: &MinioClient,
    user_id: &str,
) -> Result<i64, AppError> {
    let maps = db.list_mind_maps(user_id).await?;
    let mut total_bytes = 0_i64;

    for map in &maps {
        let expected_version_ids = known_version_ids(&map.version_history, map.minio_version_id.as_deref());
        let versions = minio
            .merge_known_versions(
                &map.minio_object_key,
                &expected_version_ids,
                map.minio_version_id.as_deref(),
            )
            .await?;
        total_bytes += versions.iter().map(|version| version.size_bytes).sum::<i64>();
        total_bytes += load_map_attachment_storage(db, &map.id).await?.1;
        total_bytes += load_map_share_storage_bytes(db, &map.id).await?;
    }

    Ok(total_bytes)
}

async fn load_map_attachment_storage(
    db: &DynSqlStore,
    map_id: &str,
) -> Result<(usize, i64), AppError> {
    let attachments = db.list_mind_map_attachments(map_id).await?;
    let attachment_count = attachments
        .iter()
        .filter(|attachment| attachment.status == AttachmentStatus::Available)
        .count();
    let attachment_bytes = attachments
        .iter()
        .filter(|attachment| attachment.status == AttachmentStatus::Available)
        .map(|attachment| attachment.size_bytes)
        .sum();

    Ok((attachment_count, attachment_bytes))
}

async fn load_map_share_storage_bytes(
    db: &DynSqlStore,
    map_id: &str,
) -> Result<i64, AppError> {
    let shares = db.list_mind_map_shares(map_id).await?;
    let mut share_bytes = 0_i64;

    for share in shares {
        if share.status == ShareStatus::Available {
            share_bytes += share.size_bytes;
        }

        let attachments = db.list_mind_map_share_attachments(&share.id).await?;
        share_bytes += attachments
            .into_iter()
            .filter(|attachment| attachment.status == AttachmentStatus::Available)
            .map(|attachment| attachment.size_bytes)
            .sum::<i64>();
    }

    Ok(share_bytes)
}

fn storage_quota_exceeded_error(
    subscription_tier: &SubscriptionTier,
    current_total_bytes: i64,
    plan_limit_bytes: i64,
) -> AppError {
    AppError::plan_restricted(
        format!(
            "this write would exceed your cloud storage limit of {plan_limit_bytes} bytes"
        ),
        "storage_quota_exceeded",
        "storage_limit_bytes",
        subscription_tier.as_str(),
        if matches!(subscription_tier, SubscriptionTier::Free) {
            Some("paid")
        } else {
            None
        },
        Some(current_total_bytes),
        Some(plan_limit_bytes),
    )
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

async fn find_share(
    db: &DynSqlStore,
    map_id: &str,
    share_id: &str,
) -> Result<StoredMindMapShare, AppError> {
    db.get_mind_map_share(map_id, share_id)
        .await?
        .ok_or_else(|| AppError::NotFound("share not found".to_string()))
}

async fn find_share_attachment(
    db: &DynSqlStore,
    share_id: &str,
    attachment_id: &str,
) -> Result<StoredMindMapShareAttachment, AppError> {
    db.get_mind_map_share_attachment(share_id, attachment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("share attachment not found".to_string()))
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

async fn delete_share_objects(
    store: &DynSqlStore,
    minio: &MinioClient,
    shares: &[StoredMindMapShare],
) -> Result<(), AppError> {
    for share in shares {
        let attachments = store.list_mind_map_share_attachments(&share.id).await?;
        for attachment in attachments {
            match minio.delete_object(&attachment.s3_key).await {
                Ok(()) | Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        match minio.delete_object(&share.s3_key).await {
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

fn validate_share_create(body: &CreateMapShareRequest) -> Result<(), AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("share name is required".to_string()));
    }
    if body.content_type.trim().is_empty() {
        return Err(AppError::BadRequest("content_type is required".to_string()));
    }
    if body.size_bytes < 0 {
        return Err(AppError::BadRequest("size_bytes must be >= 0".to_string()));
    }
    if body.encryption_meta.is_none() {
        return Err(AppError::BadRequest("encryption_meta is required for shares".to_string()));
    }
    if let Some(expires_at) = body.expires_at {
        if expires_at <= Utc::now() {
            return Err(AppError::BadRequest("expires_at must be in the future".to_string()));
        }
    }
    if let Some(hint) = body.passphrase_hint.as_deref() {
        if hint.trim().len() > 200 {
            return Err(AppError::BadRequest("passphrase_hint is too long".to_string()));
        }
    }
    Ok(())
}

fn validate_share_attachment_init(body: &InitMapShareAttachmentRequest) -> Result<(), AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("attachment name is required".to_string()));
    }
    if body.content_type.trim().is_empty() {
        return Err(AppError::BadRequest("content_type is required".to_string()));
    }
    if body.size < 0 {
        return Err(AppError::BadRequest("size must be >= 0".to_string()));
    }
    if body.encryption_meta.is_none() {
        return Err(AppError::BadRequest(
            "encryption_meta is required for shared attachments".to_string(),
        ));
    }
    Ok(())
}

fn validate_attachment_complete(body: &CompleteAttachmentUploadRequest) -> Result<(), AppError> {
    validate_share_complete(&body.version_id, body.checksum_sha256.as_deref())
}

fn validate_share_complete(version_id: &str, checksum_sha256: Option<&str>) -> Result<(), AppError> {
    if version_id.trim().is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }

    if let Some(checksum) = checksum_sha256 {
        let normalized = checksum.trim();
        if normalized.len() != 64 || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(AppError::BadRequest(
                "checksum_sha256 must be a 64-character hex string".to_string(),
            ));
        }
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

async fn record_notification_event(db: &DynSqlStore, event: NewNotificationEvent) {
    if let Err(error) = db.create_notification_event(event).await {
        tracing::warn!(?error, "failed to persist notification event");
    }
}

fn share_base_url(headers: &HeaderMap) -> String {
    if let Some(origin) = share_app_origin(headers) {
        return format!("{origin}/shared");
    }

    let forwarded_host = headers
        .get("x-forwarded-host")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let host = forwarded_host
        .or_else(|| headers.get(header::HOST).and_then(|value| value.to_str().ok()))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("127.0.0.1:8090");
    if matches!(host, "127.0.0.1:8090" | "localhost:8090") {
        return "http://localhost:5173/shared".to_string();
    }

    "https://app.mindmapvault.com/shared".to_string()
}

fn share_app_origin(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .and_then(extract_origin)
        .or_else(|| {
            headers
                .get(header::REFERER)
                .and_then(|value| value.to_str().ok())
                .and_then(extract_origin)
        })
        .map(|origin| {
            if origin == "http://127.0.0.1:8090" || origin == "http://localhost:8090" {
                "http://localhost:5173".to_string()
            } else if origin == "https://api.mindmapvault.com" {
                "https://app.mindmapvault.com".to_string()
            } else {
                origin
            }
        })
}

fn extract_origin(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (scheme, rest) = trimmed.split_once("://")?;
    let host = rest.split('/').next()?.trim();
    if host.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host}"))
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

fn to_owner_share_summary(share: StoredMindMapShare, share_base_url: &str) -> MapShareOwnerSummary {
    MapShareOwnerSummary {
        id: share.id.clone(),
        map_id: share.map_id,
        name: share.share_name,
        scope: share.scope,
        share_url: format!("{share_base_url}/{}", share.id),
        include_attachments: share.include_attachments,
        passphrase_hint: share.passphrase_hint,
        expires_at: share.expires_at,
        revoked: share.revoked,
        created_at: share.created_at,
        updated_at: share.updated_at,
        status: share.status,
        content_type: share.content_type,
        size_bytes: share.size_bytes,
        checksum_sha256: share.checksum_sha256,
    }
}

fn to_share_attachment_metadata(attachment: StoredMindMapShareAttachment) -> MapShareAttachmentMetadata {
    MapShareAttachmentMetadata {
        id: attachment.id,
        share_id: attachment.share_id,
        node_id: attachment.node_id,
        name: attachment.name,
        sanitized_name: attachment.sanitized_name,
        content_type: attachment.content_type,
        size_bytes: attachment.size_bytes,
        uploaded_at: attachment.uploaded_at,
        encryption_meta: attachment.encryption_meta,
        checksum_sha256: attachment.checksum_sha256,
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
        vault_sharing_mode,
        vault_encryption_mode,
        max_versions,
        vault_labels: _,
    } = map;

    let total_version_count = std::cmp::max(version_history.len(), usize::from(minio_version_id.is_some()));

    MindMapDetail {
        id,
        title_encrypted,
        eph_classical_public,
        eph_pq_ciphertext,
        wrapped_dek,
        vault_color,
        vault_note_encrypted,
        vault_sharing_mode,
        vault_encryption_mode,
        max_versions,
        total_version_count,
        minio_version_id,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_attachment_name;

    #[test]
    fn sanitizes_attachment_names_for_object_keys() {
        assert_eq!(sanitize_attachment_name(" Quarterly Plan 2026.pdf "), "Quarterly-Plan-2026.pdf");
        assert_eq!(sanitize_attachment_name("../../secret?.zip"), "secret.zip");
    }

    #[test]
    fn falls_back_when_name_has_no_safe_characters() {
        assert_eq!(sanitize_attachment_name("???///:::"), "attachment");
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