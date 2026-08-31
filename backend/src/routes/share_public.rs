use axum::{extract::{Path, State}, routing::get, Json, Router};
use axum::{
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
};
use chrono::Utc;

use crate::{
    db::{minio::MinioClient, sql_store::{DynSqlStore, StoredMindMapShare, StoredMindMapShareAttachment}},
    error::AppError,
    models::{attachment::AttachmentStatus, share::{MapShareAttachmentDownloadResponse, MapShareAttachmentMetadata, PublicMapShareResponse, ShareStatus}},
};

#[derive(Clone)]
pub struct SharePublicState {
    pub db: DynSqlStore,
    pub minio: MinioClient,
}

pub fn router(state: SharePublicState) -> Router {
    Router::new()
        .route("/{share_id}", get(get_share))
        .route("/{share_id}/attachments/{attachment_id}/download", get(get_share_attachment_download))
    .route("/{share_id}/blob", get(get_share_blob))
    .route("/{share_id}/attachments/{attachment_id}/blob", get(get_share_attachment_blob))
        .with_state(state)
}

async fn get_share(
    State(state): State<SharePublicState>,
    Path(share_id): Path<String>,
) -> Result<Json<PublicMapShareResponse>, AppError> {
    let share = find_public_share(&state.db, &share_id).await?;
    let attachments = if share.include_attachments {
        state
            .db
            .list_mind_map_share_attachments(&share.id)
            .await?
            .into_iter()
            .filter(|attachment| attachment.status == AttachmentStatus::Available)
            .map(to_share_attachment_metadata)
            .collect()
    } else {
        Vec::new()
    };

    let download_url = format!("/share/{}/blob", share.id);

    Ok(Json(PublicMapShareResponse {
        id: share.id,
        name: share.share_name,
        scope: share.scope,
        include_attachments: share.include_attachments,
        passphrase_hint: share.passphrase_hint,
        created_at: share.created_at,
        expires_at: share.expires_at,
        content_type: share.content_type,
        size_bytes: share.size_bytes,
        encryption_meta: share.encryption_meta,
        checksum_sha256: share.checksum_sha256,
        download_url,
        download_expires_at: public_presign_expires_at(&state.minio)?,
        attachments,
    }))
}

async fn get_share_blob(
    State(state): State<SharePublicState>,
    Path(share_id): Path<String>,
) -> Result<Response, AppError> {
    let share = find_public_share(&state.db, &share_id).await?;
    let bytes = state
        .minio
        .download_blob(&share.s3_key, share.s3_version_id.as_deref())
        .await?;

    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&share.content_type)
                .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
        )],
        bytes,
    )
        .into_response())
}
async fn get_share_attachment_download(
    State(state): State<SharePublicState>,
    Path((share_id, attachment_id)): Path<(String, String)>,
) -> Result<Json<MapShareAttachmentDownloadResponse>, AppError> {
    let share = find_public_share(&state.db, &share_id).await?;
    if !share.include_attachments {
        return Err(AppError::NotFound("share attachment not found".to_string()));
    }

    let attachment = state
        .db
        .get_mind_map_share_attachment(&share_id, &attachment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("share attachment not found".to_string()))?;
    if attachment.status != AttachmentStatus::Available {
        return Err(AppError::NotFound("share attachment not found".to_string()));
    }

    Ok(Json(MapShareAttachmentDownloadResponse {
        download_url: format!("/share/{share_id}/attachments/{attachment_id}/blob"),
        expires_at: public_presign_expires_at(&state.minio)?,
        content_type: attachment.content_type,
        name: attachment.name,
        sanitized_name: attachment.sanitized_name,
        size_bytes: attachment.size_bytes,
        encryption_meta: attachment.encryption_meta,
        version_id: attachment.s3_version_id,
        checksum_sha256: attachment.checksum_sha256,
    }))
}

async fn get_share_attachment_blob(
    State(state): State<SharePublicState>,
    Path((share_id, attachment_id)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let share = find_public_share(&state.db, &share_id).await?;
    if !share.include_attachments {
        return Err(AppError::NotFound("share attachment not found".to_string()));
    }

    let attachment = state
        .db
        .get_mind_map_share_attachment(&share_id, &attachment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("share attachment not found".to_string()))?;
    if attachment.status != AttachmentStatus::Available {
        return Err(AppError::NotFound("share attachment not found".to_string()));
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
async fn find_public_share(db: &DynSqlStore, share_id: &str) -> Result<StoredMindMapShare, AppError> {
    let share = db
        .get_public_mind_map_share(share_id)
        .await?
        .ok_or_else(|| AppError::NotFound("share not found".to_string()))?;

    if share.status != ShareStatus::Available || share.revoked {
        return Err(AppError::NotFound("share not found".to_string()));
    }
    if let Some(expires_at) = share.expires_at {
        if expires_at <= Utc::now() {
            return Err(AppError::NotFound("share not found".to_string()));
        }
    }

    Ok(share)
}

fn public_presign_expires_at(minio: &MinioClient) -> Result<chrono::DateTime<Utc>, AppError> {
    let delta = chrono::Duration::from_std(minio.presign_expiry)
        .map_err(|error| AppError::Internal(format!("invalid presign expiry: {error}")))?;
    Ok(Utc::now() + delta)
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