use std::{collections::{BTreeMap, HashSet}, sync::Arc};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, FromRef, Path, Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    db::{s3::S3Store, sql_store::{DynSqlStore, MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate, MindMapShareAttachmentUploadUpdate, MindMapShareUploadUpdate, NewMindMap, NewMindMapAttachment, NewMindMapShare, NewMindMapShareAttachment, StoredMindMap, StoredMindMapAttachment, StoredMindMapShare, StoredMindMapShareAttachment}},
    error::{AppError, PlanErrorMetadata},
    middleware::auth::{AuthenticatedUser, JwtService, KeyVersionCache, VerifiedWriter},
    models::{
        attachment::{AttachmentDownloadResponse, AttachmentMetadata, AttachmentStatus, CompleteAttachmentUploadRequest, CopyAttachmentRequest, InitAttachmentRequest, InitAttachmentResponse, UpdateAttachmentRequest},
        instance_settings::InstanceSettingsHandle,
        mindmap::{
            ConfirmUploadRequest, ConfirmUploadResponse, MindMapCreatedResponse, MindMapDetail,
            MindMapListItem, PresignedUrlResponse, StorageSummary, UpdateVaultMetaRequest,
            stored_version_bytes, UpsertMindMapRequest, VaultStorageInfo, VersionDetail,
            VersionSnapshot,
        },
        share::{
            CompleteMapShareAttachmentUploadRequest, CompleteMapShareUploadRequest,
            CreateMapShareRequest, CreateMapShareResponse, InitMapShareAttachmentRequest,
            InitMapShareAttachmentResponse, MapShareAttachmentMetadata, MapShareOwnerSummary,
            ShareStatus,
        },
        user::MAX_UPLOAD_BODY_BYTES,
    },
};

#[derive(Clone)]
pub struct MindMapsSqlState {
    pub db: DynSqlStore,
    pub storage: S3Store,
    pub jwt: Arc<JwtService>,
    pub diagnostics_enabled: bool,
    pub settings: InstanceSettingsHandle,
    pub key_versions: KeyVersionCache,
}

impl FromRef<MindMapsSqlState> for Arc<JwtService> {
    fn from_ref(state: &MindMapsSqlState) -> Self {
        state.jwt.clone()
    }
}

impl FromRef<MindMapsSqlState> for DynSqlStore {
    fn from_ref(state: &MindMapsSqlState) -> Self {
        state.db.clone()
    }
}

impl FromRef<MindMapsSqlState> for KeyVersionCache {
    fn from_ref(state: &MindMapsSqlState) -> Self {
        state.key_versions.clone()
    }
}

impl FromRef<MindMapsSqlState> for InstanceSettingsHandle {
    fn from_ref(state: &MindMapsSqlState) -> Self {
        state.settings.clone()
    }
}

/// Refuses an upload that would put the account past the instance storage cap,
/// or a single file past the per-attachment cap.
///
/// Checked at `init` time against the size the client declares, so the caller
/// is told before it spends time encrypting and sending the bytes. The declared
/// size is what gets stored and billed against the cap, so a client that lies
/// about it gains nothing but a wrong number in its own storage view.
async fn enforce_storage_limits(
    state: &MindMapsSqlState,
    user_id: &str,
    incoming_bytes: i64,
) -> Result<(), AppError> {
    let settings = state.settings.get();

    if let Some(limit) = settings.attachment_limit() {
        if incoming_bytes > limit {
            return Err(AppError::PlanRestricted(
                format!("this file is larger than the {limit} byte limit set on this server"),
                PlanErrorMetadata {
                    message: "attachment too large".to_string(),
                    code: "attachment_too_large",
                    capability: "attachment_size",
                    current_tier: "community".to_string(),
                    required_tier: None,
                    current_value: Some(incoming_bytes),
                    limit_value: Some(limit),
                },
            ));
        }
    }

    let Some(limit) = settings.storage_limit() else {
        return Ok(());
    };

    let used = state.db.sum_user_stored_bytes(user_id).await?;
    if used + incoming_bytes > limit {
        return Err(AppError::PlanRestricted(
            "this upload would put the account over the storage limit set on this server"
                .to_string(),
            PlanErrorMetadata {
                message: "storage limit reached".to_string(),
                code: "storage_limit_reached",
                capability: "storage",
                current_tier: "community".to_string(),
                required_tier: None,
                current_value: Some(used + incoming_bytes),
                limit_value: Some(limit),
            },
        ));
    }

    Ok(())
}

pub fn router(state: MindMapsSqlState) -> Router {
    let mut router = Router::new()
        .route("/", get(list_mind_maps).post(create_mind_map))
        .route("/storage", get(get_storage))
        .route("/my/storage", get(get_storage))
        .route("/{id}", get(get_mind_map).put(update_mind_map).delete(delete_mind_map))
        .route("/{id}/meta", put(update_vault_meta))
        .route(
            "/{id}/upload",
            post(upload_blob).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES)),
        )
        .route("/{id}/blob", get(download_blob))
        .route("/{id}/upload-url", post(get_upload_url))
        .route("/{id}/confirm-upload", post(confirm_upload))
        .route("/{id}/download-url", get(get_download_url))
        .route("/{id}/shares", get(list_shares).post(create_share))
        .route(
            "/{id}/shares/{share_id}/upload",
            post(upload_share_blob).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES)),
        )
        .route("/{id}/shares/{share_id}/complete", post(complete_share_upload))
        .route("/{id}/shares/{share_id}/revoke", post(revoke_share))
        .route("/{id}/shares/{share_id}/attachments", post(init_share_attachment))
        .route(
            "/{id}/shares/{share_id}/attachments/{attachment_id}/upload",
            post(upload_share_attachment_blob).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES)),
        )
        .route(
            "/{id}/shares/{share_id}/attachments/{attachment_id}/complete",
            post(complete_share_attachment_upload),
        )
        .route("/{id}/attachments", get(list_attachments))
        .route("/{id}/attachments/init", post(init_attachment))
        .route("/{id}/attachments/{attachment_id}", get(get_attachment).patch(update_attachment).delete(delete_attachment))
        .route(
            "/{id}/attachments/{attachment_id}/upload",
            post(upload_attachment_blob).layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES)),
        )
        .route("/{id}/attachments/{attachment_id}/complete", post(complete_attachment_upload))
        .route("/{id}/attachments/{attachment_id}/copy", post(copy_attachment))
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
    user: VerifiedWriter,
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
            object_key: object_key.clone(),
            eph_classical_public: body.eph_classical_public,
            eph_pq_ciphertext: body.eph_pq_ciphertext,
            wrapped_dek: body.wrapped_dek,
            created_at: now,
            updated_at: now,
            current_version_id: None,
            version_history: Vec::new(),
            vault_color: None,
            vault_note_encrypted: None,
            vault_encryption_mode: "standard".to_string(),
            max_versions: 50,
            vault_labels: Vec::new(),
        })
        .await?;

    // Minted before the URL is handed out, so the upload lands directly on the
    // key this version will always live at and no id has to come back from the
    // store afterwards.
    let version_id = S3Store::new_version_id();
    let upload_url = match state
        .storage
        .presigned_put_url(&S3Store::version_key(&object_key, &version_id))
        .await
    {
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
        object_key: object_key,
        upload_url,
        version_id,
    }))
}

async fn confirm_upload(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path(id): Path<String>,
    Json(body): Json<ConfirmUploadRequest>,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.version_id.is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }

    let map = find_owned(&state.db, &id, &user.0).await?;
    let verified_vid = S3Store::validate_version_id(&body.version_id)?;
    let object_key = S3Store::version_key(&map.object_key, &verified_vid);

    // What actually proves the upload arrived. The check this replaced only
    // looked at the shape of the id and never contacted the store, so a client
    // could confirm a version that was never written.
    let size_bytes = state.storage.head_size(&object_key).await?;

    let mut version_history = normalize_version_history(&map.version_history);
    version_history.push(VersionSnapshot {
        version_id: verified_vid.clone(),
        eph_classical_public: map.eph_classical_public.clone(),
        eph_pq_ciphertext: map.eph_pq_ciphertext.clone(),
        wrapped_dek: map.wrapped_dek.clone(),
        saved_at: Utc::now(),
        object_key: Some(object_key.clone()),
        size_bytes: Some(size_bytes),
    });
    let pruned_keys = prune_history(&mut version_history, map.max_versions);

    if let Err(error) = state
        .db
        .update_mind_map_upload(&id, &user.0, &verified_vid, version_history)
        .await
    {
        if let Err(cleanup_error) = state.storage.delete_object(&object_key).await {
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

    spawn_object_cleanup(state.storage.clone(), pruned_keys);

    Ok(Json(ConfirmUploadResponse { version_id: verified_vid }))
}

async fn upload_blob(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    let map = find_owned(&state.db, &id, &user.0).await?;
    let version_id = S3Store::new_version_id();
    let object_key = S3Store::version_key(&map.object_key, &version_id);
    let size_bytes = body.len() as i64;
    state.storage.upload_blob(&object_key, body.to_vec()).await?;

    let mut version_history = normalize_version_history(&map.version_history);
    version_history.push(VersionSnapshot {
        version_id: version_id.clone(),
        eph_classical_public: map.eph_classical_public.clone(),
        eph_pq_ciphertext: map.eph_pq_ciphertext.clone(),
        wrapped_dek: map.wrapped_dek.clone(),
        saved_at: Utc::now(),
        object_key: Some(object_key.clone()),
        size_bytes: Some(size_bytes),
    });
    let pruned_keys = prune_history(&mut version_history, map.max_versions);

    if let Err(error) = state
        .db
        .update_mind_map_upload(&id, &user.0, &version_id, version_history)
        .await
    {
        if let Err(cleanup_error) = state.storage.delete_object(&object_key).await {
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

    spawn_object_cleanup(state.storage.clone(), pruned_keys);

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
    user: VerifiedWriter,
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

    let version_id = S3Store::new_version_id();
    let upload_url = state
        .storage
        .presigned_put_url(&S3Store::version_key(&map.object_key, &version_id))
        .await?;
    Ok(Json(PresignedUrlResponse {
        url: upload_url,
        expires_in_secs: state.storage.presign_expiry.as_secs(),
        version_id: Some(version_id),
    }))
}

async fn delete_mind_map(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let attachments = state.db.list_mind_map_attachments(&id).await?;
    delete_attachment_objects(&state.storage, &attachments).await?;
    state.storage.delete_all_versions(&map.object_key).await?;

    state.db.delete_mind_map(&id, &user.0).await?;

    Ok(Json(serde_json::json!({ "message": "deleted" })))
}

async fn get_upload_url(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<PresignedUrlResponse>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let version_id = S3Store::new_version_id();
    let url = state
        .storage
        .presigned_put_url(&S3Store::version_key(&map.object_key, &version_id))
        .await?;
    Ok(Json(PresignedUrlResponse {
        url,
        expires_in_secs: state.storage.presign_expiry.as_secs(),
        version_id: Some(version_id),
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
    let object_key = resolve_blob_key(&map, version_id.as_deref())?;
    let bytes = state.storage.download_blob(&object_key).await?;

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
    let object_key = resolve_blob_key(&map, normalized_version_id.as_deref())?;
    let url = state.storage.presigned_get_url(&object_key).await?;
    Ok(Json(PresignedUrlResponse {
        url,
        expires_in_secs: state.storage.presign_expiry.as_secs(),
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
    user: VerifiedWriter,
    Path(id): Path<String>,
    Json(body): Json<InitAttachmentRequest>,
) -> Result<Json<InitAttachmentResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_attachment_init(&body)?;
    enforce_storage_limits(&state, &user.0, body.size).await?;

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
    user: VerifiedWriter,
    Path((id, attachment_id)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<ConfirmUploadResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("blob is required".to_string()));
    }

    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;
    state.storage.upload_blob(&attachment.s3_key, body.to_vec()).await?;

    // Attachments already occupy a key of their own, so there is no version to
    // report. Kept in the response shape for clients that still send it back.
    Ok(Json(ConfirmUploadResponse { version_id: String::new() }))
}

async fn complete_attachment_upload(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<CompleteAttachmentUploadRequest>,
) -> Result<Json<AttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_attachment_complete(&body)?;

    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;
    // HeadObject on the attachment's own key: proof the upload landed that does
    // not depend on the store returning a version id, which R2 never does.
    state.storage.head_size(&attachment.s3_key).await?;
    let verified_vid = normalize_version_id(&body.version_id);

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
        if let Err(cleanup_error) = state.storage.delete_object(&attachment.s3_key).await {
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
    user: VerifiedWriter,
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

/// Copies an attachment within the same map, for node duplication.
///
/// One attachment belongs to exactly one node: duplicating a node has to
/// duplicate its files, or the two copies share storage and deleting either
/// one breaks the other. The bytes are copied in the object store — they are
/// ciphertext sealed under the owner's master key and stay valid unchanged, so
/// nothing is decrypted here and the client uploads nothing.
///
/// The copy counts against the storage quota like any other upload.
async fn copy_attachment(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<CopyAttachmentRequest>,
) -> Result<Json<AttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let source = find_attachment(&state.db, &id, &attachment_id).await?;

    if source.status != AttachmentStatus::Available {
        return Err(AppError::BadRequest(
            "only a completed attachment can be copied".to_string(),
        ));
    }

    enforce_storage_limits(&state, &user.0, source.size_bytes).await?;

    let new_id = Uuid::new_v4().to_string();
    let s3_key = format!(
        "maps/{id}/attachments/{new_id}/{}",
        source.sanitized_name
    );
    state.storage.copy_object(&source.s3_key, &s3_key).await?;

    let created = state
        .db
        .create_mind_map_attachment(NewMindMapAttachment {
            id: new_id.clone(),
            map_id: id.clone(),
            node_id: normalize_optional(body.node_id),
            name: source.name.clone(),
            sanitized_name: source.sanitized_name.clone(),
            content_type: source.content_type.clone(),
            size_bytes: source.size_bytes,
            s3_key: s3_key.clone(),
            s3_version_id: None,
            uploaded_by: user.0.clone(),
            uploaded_at: Utc::now(),
            encrypted: source.encrypted,
            encryption_meta: source.encryption_meta.clone(),
            checksum_sha256: source.checksum_sha256.clone(),
            status: AttachmentStatus::Available,
        })
        .await;

    // The object exists but no row points at it: remove it rather than leave a
    // byte-for-byte duplicate nothing can reach or account for.
    if let Err(error) = created {
        if let Err(cleanup_error) = state.storage.delete_object(&s3_key).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                source_attachment_id = %attachment_id,
                "failed to remove the copied object after the metadata insert failed"
            );
        }
        return Err(error);
    }

    let stored = find_attachment(&state.db, &id, &new_id).await?;
    Ok(Json(to_attachment_metadata(stored)))
}

async fn delete_attachment(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, attachment_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let attachment = find_attachment(&state.db, &id, &attachment_id).await?;

    match state.storage.delete_object(&attachment.s3_key).await {
        Ok(()) | Err(AppError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    state
        .db
        .mark_mind_map_attachment_deleted(&id, &attachment_id)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Lists a vault's saved versions.
///
/// Answered entirely from `version_history`. The object store is never asked
/// what versions exist, because that question has no portable answer: stores
/// without `ListObjectVersions` used to send this down a fallback that
/// reported every version as 0 B.
async fn list_versions(
    State(state): State<MindMapsSqlState>,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<VersionDetail>>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;
    let history = normalize_version_history(&map.version_history);
    let current_version_id = map
        .current_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());

    let mut versions: Vec<VersionDetail> = history
        .iter()
        .enumerate()
        .map(|(index, snapshot)| VersionDetail {
            version_id: snapshot.version_id.clone(),
            version_number: Some(index + 1),
            is_latest: current_version_id.as_deref() == Some(snapshot.version_id.as_str()),
            last_modified: snapshot.saved_at,
            size_bytes: snapshot.size_bytes.unwrap_or(0),
            eph_classical_public: Some(snapshot.eph_classical_public.clone()),
            eph_pq_ciphertext: Some(snapshot.eph_pq_ciphertext.clone()),
            wrapped_dek: Some(snapshot.wrapped_dek.clone()),
            saved_at: Some(snapshot.saved_at),
        })
        .collect();

    // A vault saved before version history was recorded has a current version
    // that appears nowhere in the list. Its envelope is the one on the row.
    if let Some(current) = current_version_id.as_deref() {
        if !versions.iter().any(|version| version.version_id == current) {
            versions.push(VersionDetail {
                version_id: current.to_string(),
                version_number: Some(versions.len() + 1),
                is_latest: true,
                last_modified: map.updated_at,
                size_bytes: 0,
                eph_classical_public: Some(map.eph_classical_public.clone()),
                eph_pq_ciphertext: Some(map.eph_pq_ciphertext.clone()),
                wrapped_dek: Some(map.wrapped_dek.clone()),
                saved_at: None,
            });
        }
    }

    versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    Ok(Json(versions))
}


async fn delete_vault_version(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, version_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let map = find_owned(&state.db, &id, &user.0).await?;

    let normalized_target_version_id = normalize_version_id(&version_id);
    if normalized_target_version_id.is_empty() {
        return Err(AppError::BadRequest("version_id is required".to_string()));
    }
    let normalized_current_version_id = map
        .current_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());

    if normalized_current_version_id.as_deref() == Some(normalized_target_version_id.as_str()) {
        return Err(AppError::BadRequest(
            "Cannot delete the current active version; save a new version first.".to_string(),
        ));
    }

    // A version that predates per-version keys has no object of its own; the
    // row still goes, so the list stops offering something unloadable.
    match resolve_blob_key(&map, Some(&normalized_target_version_id)) {
        Ok(object_key) => state.storage.delete_object(&object_key).await?,
        Err(AppError::NotFound(_)) => {}
        Err(error) => return Err(error),
    }

    let filtered_history: Vec<VersionSnapshot> = normalize_version_history(&map.version_history)
        .into_iter()
        .filter(|snapshot| snapshot.version_id != normalized_target_version_id)
        .collect();

    let current_version_id = normalized_current_version_id.ok_or_else(|| {
        AppError::BadRequest("Current active version is missing from vault metadata.".to_string())
    })?;

    state
        .db
        .update_mind_map_upload(&id, &user.0, &current_version_id, filtered_history)
        .await?;

    Ok(Json(serde_json::json!({ "ok": true })))
}

async fn update_vault_meta(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
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
        let version_total_bytes = stored_version_bytes(&map.version_history);
        let (attachment_count, attachment_bytes, all_attachment_bytes) = load_map_attachment_storage(&state.db, &map.id).await?;
        let total_bytes = version_total_bytes + all_attachment_bytes;
        grand_total += total_bytes;
        grand_attachment_count += attachment_count;
        grand_attachment_bytes += attachment_bytes;
        vaults.push(VaultStorageInfo {
            id: map.id,
            title_encrypted: map.title_encrypted,
            version_count: normalize_version_history(&map.version_history).len(),
            attachment_count,
            attachment_bytes,
            total_bytes,
        });
    }

    // No configured cap is reported as "effectively boundless" rather than as a
    // number, which is what the app's storage bar already understands.
    let limit_bytes = state.settings.get().storage_limit().unwrap_or(i64::MAX);

    Ok(Json(StorageSummary {
        vaults,
        total_bytes: grand_total,
        attachment_count: grand_attachment_count,
        attachment_bytes: grand_attachment_bytes,
        free_tier_bytes: limit_bytes,
        plan_tier: "community".to_string(),
        plan_limit_bytes: limit_bytes,
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
    user: VerifiedWriter,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<CreateMapShareRequest>,
) -> Result<Json<CreateMapShareResponse>, AppError> {
    let user_id = user.0.clone();
    find_owned(&state.db, &id, &user_id).await?;
    validate_share_create(&body)?;
    // A share is a second encrypted copy of the vault, so it occupies storage
    // in its own right and counts against the same cap.
    enforce_storage_limits(&state, &user_id, body.size_bytes).await?;

    let share_id = Uuid::new_v4().to_string();
    let share_name = body.name.trim().to_string();
    let sanitized_name = sanitize_attachment_name(&share_name);
    let s3_key = format!("maps/{id}/shares/{share_id}/{sanitized_name}");
    let now = Utc::now();

    state
        .db
        .create_mind_map_share(NewMindMapShare {
            id: share_id.clone(),
            map_id: id.clone(),
            share_name,
            scope: body.scope.clone(),
            s3_key: s3_key.clone(),
            s3_version_id: None,
            created_by: user_id,
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

    Ok(Json(CreateMapShareResponse {
        share_id: share_id.clone(),
        share_url: format!("{}/{}", share_base_url(&headers), share_id),
        s3_key,
        upload_url: format!("/api/mindmaps/{id}/shares/{share_id}/upload"),
        upload_headers: BTreeMap::new(),
        expires_at: presign_expires_at(&state)?,
    }))
}

async fn upload_share_blob(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
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

    state.storage.upload_blob(&share.s3_key, body.to_vec()).await?;

    Ok(Json(ConfirmUploadResponse { version_id: String::new() }))
}

async fn complete_share_upload(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    headers: HeaderMap,
    Path((id, share_id)): Path<(String, String)>,
    Json(body): Json<CompleteMapShareUploadRequest>,
) -> Result<Json<MapShareOwnerSummary>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_complete(&body.version_id, body.checksum_sha256.as_deref())?;

    let share = find_share(&state.db, &id, &share_id).await?;
    state.storage.head_size(&share.s3_key).await?;
    let verified_vid = normalize_version_id(&body.version_id);
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
        if let Err(cleanup_error) = state.storage.delete_object(&share.s3_key).await {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                share_id = %share_id,
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
    user: VerifiedWriter,
    headers: HeaderMap,
    Path((id, share_id)): Path<(String, String)>,
) -> Result<Json<MapShareOwnerSummary>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    let existing = find_share(&state.db, &id, &share_id).await?;
    state.db.set_mind_map_share_revoked(&id, &share_id, true).await?;

    // Revoking has to remove the ciphertext, not just hide the row. The share
    // blob is a second, independently-keyed copy of the map; leaving it in
    // storage means "revoked" only stops new downloads while the copy — and the
    // bytes it costs the owner — live on forever.
    delete_share_objects(&state.db, &state.storage, std::slice::from_ref(&existing)).await?;
    state.db.mark_mind_map_share_purged(&share_id).await?;

    let updated = find_share(&state.db, &id, &share_id).await?;
    Ok(Json(to_owner_share_summary(updated, &share_base_url(&headers))))
}

async fn init_share_attachment(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, share_id)): Path<(String, String)>,
    Json(body): Json<InitMapShareAttachmentRequest>,
) -> Result<Json<InitMapShareAttachmentResponse>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_attachment_init(&body)?;
    let share = find_share(&state.db, &id, &share_id).await?;

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

    enforce_storage_limits(&state, &user.0, body.size).await?;

    let attachment_id = Uuid::new_v4().to_string();
    let sanitized_name = sanitize_attachment_name(&body.name);
    let s3_key = format!("maps/{id}/shares/{share_id}/attachments/{attachment_id}/{sanitized_name}");

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
            encryption_meta: body.encryption_meta.ok_or_else(|| {
                AppError::BadRequest("encryption_meta is required for shared attachments".to_string())
            })?,
            checksum_sha256: None,
            status: AttachmentStatus::Pending,
        })
        .await?;

    Ok(Json(InitMapShareAttachmentResponse {
        attachment_id: attachment_id.clone(),
        s3_key,
        upload_url: format!(
            "/api/mindmaps/{id}/shares/{share_id}/attachments/{attachment_id}/upload"
        ),
        upload_headers: BTreeMap::new(),
        expires_at: presign_expires_at(&state)?,
    }))
}

async fn upload_share_attachment_blob(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
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
    state.storage.upload_blob(&attachment.s3_key, body.to_vec()).await?;

    Ok(Json(ConfirmUploadResponse { version_id: String::new() }))
}

async fn complete_share_attachment_upload(
    State(state): State<MindMapsSqlState>,
    user: VerifiedWriter,
    Path((id, share_id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<CompleteMapShareAttachmentUploadRequest>,
) -> Result<Json<MapShareAttachmentMetadata>, AppError> {
    find_owned(&state.db, &id, &user.0).await?;
    validate_share_complete(&body.version_id, body.checksum_sha256.as_deref())?;
    let attachment = find_share_attachment(&state.db, &share_id, &attachment_id).await?;
    state.storage.head_size(&attachment.s3_key).await?;
    let verified_vid = normalize_version_id(&body.version_id);

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
        if let Err(cleanup_error) = state
            .storage
            .delete_object(&attachment.s3_key)
            .await
        {
            tracing::error!(
                ?cleanup_error,
                map_id = %id,
                share_id = %share_id,
                attachment_id = %attachment_id,
                version_id = %verified_vid,
                "failed to roll back shared attachment upload after metadata update error"
            );
        }
        return Err(error);
    }

    let updated = find_share_attachment(&state.db, &share_id, &attachment_id).await?;
    Ok(Json(to_share_attachment_metadata(updated)))
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

pub(crate) async fn delete_share_objects(
    store: &DynSqlStore,
    storage: &S3Store,
    shares: &[StoredMindMapShare],
) -> Result<(), AppError> {
    for share in shares {
        let attachments = store.list_mind_map_share_attachments(&share.id).await?;
        for attachment in attachments {
            match storage.delete_object(&attachment.s3_key).await {
                Ok(()) | Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        match storage.delete_object(&share.s3_key).await {
            Ok(()) | Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
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

/// As with attachments, the share blob has its own key and needs no version id.
fn validate_share_complete(_version_id: &str, checksum_sha256: Option<&str>) -> Result<(), AppError> {
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

/// Where a share link points. A self-hosted server serves the web app from its
/// own origin, so the link is built from the request rather than from a
/// configured public hostname: whatever host reached this server is the host
/// the recipient can reach too. `X-Forwarded-*` wins so an instance behind a
/// reverse proxy emits its public URL, not the container's internal one.
fn share_base_url(headers: &HeaderMap) -> String {
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
        .unwrap_or("localhost:8090");

    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(if host.starts_with("localhost") || host.starts_with("127.0.0.1") {
            "http"
        } else {
            "https"
        });

    format!("{scheme}://{host}/shared")
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

fn to_share_attachment_metadata(
    attachment: StoredMindMapShareAttachment,
) -> MapShareAttachmentMetadata {
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
    storage: &S3Store,
    attachments: &[StoredMindMapAttachment],
) -> Result<(), AppError> {
    for attachment in attachments {
        match storage.delete_object(&attachment.s3_key).await {
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

/// An attachment occupies a key of its own, so there is no version to report
/// and an empty `version_id` is not an error. What proves the upload arrived is
/// the HeadObject the caller does on the key; requiring an id here only ruled
/// out stores that do not return one, R2 among them.
fn validate_attachment_complete(_body: &CompleteAttachmentUploadRequest) -> Result<(), AppError> {
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

/// Resolves a version id to the object key its ciphertext is stored under.
///
/// `None` means the current version. Versions written before per-version keys
/// carry no `object_key`: their bytes were an S3 version of the base key, which
/// only a store that really implemented versioning can still produce, so the
/// current one resolves to the base key and older ones are reported gone rather
/// than silently answered with the current data.
fn resolve_blob_key(map: &StoredMindMap, version_id: Option<&str>) -> Result<String, AppError> {
    let current = map
        .current_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());

    let wanted = match version_id {
        Some(value) => S3Store::validate_version_id(value)?,
        // Nothing saved yet, or a row from before version ids were recorded.
        None => match current.clone() {
            Some(value) => value,
            None => return Ok(map.object_key.clone()),
        },
    };

    if let Some(object_key) = normalize_version_history(&map.version_history)
        .iter()
        .find(|snapshot| snapshot.version_id == wanted)
        .and_then(|snapshot| snapshot.object_key.clone())
    {
        return Ok(object_key);
    }

    if current.as_deref() == Some(wanted.as_str()) {
        return Ok(map.object_key.clone());
    }

    Err(AppError::NotFound(format!(
        "version '{wanted}' is no longer stored"
    )))
}

/// Trims `history` to the newest `keep` entries, returning the object keys of
/// the versions dropped so the caller can delete them once the row is written.
///
/// History is the record of what exists, so retention has to trim it; deleting
/// objects alone would leave rows pointing at bytes that are gone.
fn prune_history(history: &mut Vec<VersionSnapshot>, keep: u32) -> Vec<String> {
    let keep = keep.max(1) as usize;
    if history.len() <= keep {
        return Vec::new();
    }

    let excess = history.len() - keep;
    history
        .drain(..excess)
        .filter_map(|snapshot| snapshot.object_key)
        .collect()
}

/// Deletes objects in the background, after the row that referenced them is
/// already written. A failure here leaks bytes; it must not fail the save.
fn spawn_object_cleanup(storage: S3Store, object_keys: Vec<String>) {
    if object_keys.is_empty() {
        return;
    }

    tokio::spawn(async move {
        for object_key in object_keys {
            if let Err(error) = storage.delete_object(&object_key).await {
                tracing::warn!(?error, %object_key, "could not delete a pruned version object");
            }
        }
    });
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
            object_key: snapshot.object_key.clone(),
            size_bytes: snapshot.size_bytes,
        });
    }

    normalized
}

fn presign_expires_at(state: &MindMapsSqlState) -> Result<chrono::DateTime<Utc>, AppError> {
    let delta = chrono::Duration::from_std(state.storage.presign_expiry)
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
        object_key: _,
        eph_classical_public,
        eph_pq_ciphertext,
        wrapped_dek,
        created_at,
        updated_at,
        current_version_id,
        version_history,
        vault_color,
        vault_note_encrypted,
        vault_encryption_mode,
        max_versions,
        vault_labels: _,
    } = map;

    let normalized_history = normalize_version_history(&version_history);
    let normalized_current_version_id = current_version_id
        .as_deref()
        .map(normalize_version_id)
        .filter(|value| !value.is_empty());
    let total_version_count = std::cmp::max(
        normalized_history.len(),
        usize::from(normalized_current_version_id.is_some()),
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
        current_version_id: normalized_current_version_id,
        created_at,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        normalize_version_history, normalize_version_id, prune_history, resolve_blob_key,
        sanitize_attachment_name, AppError, StoredMindMap, VersionSnapshot,
    };

    fn snapshot(version_id: &str, object_key: Option<&str>) -> VersionSnapshot {
        VersionSnapshot {
            version_id: version_id.to_string(),
            eph_classical_public: "eph".to_string(),
            eph_pq_ciphertext: "pq".to_string(),
            wrapped_dek: "dek".to_string(),
            saved_at: Utc::now(),
            object_key: object_key.map(str::to_string),
            size_bytes: Some(128),
        }
    }

    fn map_with(history: Vec<VersionSnapshot>, current: Option<&str>) -> StoredMindMap {
        let now = Utc::now();
        StoredMindMap {
            id: "map-1".to_string(),
            user_id: "user-1".to_string(),
            title_encrypted: "title".to_string(),
            object_key: "blob-key".to_string(),
            eph_classical_public: "eph".to_string(),
            eph_pq_ciphertext: "pq".to_string(),
            wrapped_dek: "dek".to_string(),
            created_at: now,
            updated_at: now,
            current_version_id: current.map(str::to_string),
            version_history: history,
            vault_color: None,
            vault_note_encrypted: None,
            vault_encryption_mode: "standard".to_string(),
            max_versions: 50,
            vault_labels: Vec::new(),
        }
    }

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
        let history = vec![
            snapshot(" \"62ea0a57-8b2a-4fd2-8046-cf8769d81489\" ", Some("blob-key/v/a")),
            snapshot("62ea0a57-8b2a-4fd2-8046-cf8769d81489", Some("blob-key/v/a")),
            snapshot(" c913788a-3366-4846-8fc5-ed3074b0a20e ", Some("blob-key/v/b")),
        ];

        let normalized = normalize_version_history(&history);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].version_id, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
        assert_eq!(normalized[1].version_id, "c913788a-3366-4846-8fc5-ed3074b0a20e");
    }

    #[test]
    fn normalizes_single_version_id_from_legacy_formatting() {
        assert_eq!(
            normalize_version_id(" \"b0000000-0000-0000-0000-000000000003\" "),
            "b0000000-0000-0000-0000-000000000003"
        );
    }

    #[test]
    fn resolves_a_version_to_its_own_object() {
        let map = map_with(
            vec![
                snapshot("v-old", Some("blob-key/v/v-old")),
                snapshot("v-new", Some("blob-key/v/v-new")),
            ],
            Some("v-new"),
        );

        assert_eq!(resolve_blob_key(&map, Some("v-old")).unwrap(), "blob-key/v/v-old");
        assert_eq!(resolve_blob_key(&map, Some("v-new")).unwrap(), "blob-key/v/v-new");
        assert_eq!(resolve_blob_key(&map, None).unwrap(), "blob-key/v/v-new");
    }

    /// The bug this design removes: an older version must never resolve to the
    /// key holding the current data, which is what a store that ignores version
    /// ids would hand back.
    #[test]
    fn refuses_a_legacy_version_instead_of_serving_the_current_blob() {
        let map = map_with(
            vec![snapshot("v-old", None), snapshot("v-current", None)],
            Some("v-current"),
        );

        let error = resolve_blob_key(&map, Some("v-old")).unwrap_err();
        assert!(matches!(error, AppError::NotFound(_)), "got {error:?}");

        // The current version is still readable from the pre-migration key.
        assert_eq!(resolve_blob_key(&map, Some("v-current")).unwrap(), "blob-key");
    }

    #[test]
    fn rejects_a_version_id_that_would_escape_the_blob_key() {
        let map = map_with(vec![snapshot("v-1", Some("blob-key/v/v-1"))], Some("v-1"));

        let error = resolve_blob_key(&map, Some("../other-vault")).unwrap_err();
        assert!(matches!(error, AppError::BadRequest(_)), "got {error:?}");
    }

    #[test]
    fn a_vault_with_nothing_saved_resolves_to_its_base_key() {
        let map = map_with(Vec::new(), None);

        assert_eq!(resolve_blob_key(&map, None).unwrap(), "blob-key");
    }

    #[test]
    fn pruning_drops_the_oldest_versions_and_reports_their_objects() {
        let mut history = vec![
            snapshot("v-1", Some("blob-key/v/v-1")),
            snapshot("v-2", Some("blob-key/v/v-2")),
            snapshot("v-3", Some("blob-key/v/v-3")),
        ];

        let dropped = prune_history(&mut history, 2);

        assert_eq!(dropped, vec!["blob-key/v/v-1".to_string()]);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].version_id, "v-2");
        assert_eq!(history[1].version_id, "v-3");
    }

    #[test]
    fn pruning_leaves_a_short_history_alone() {
        let mut history = vec![snapshot("v-1", Some("blob-key/v/v-1"))];

        assert!(prune_history(&mut history, 50).is_empty());
        assert_eq!(history.len(), 1);
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
        .storage
        .download_blob(&attachment.s3_key)
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