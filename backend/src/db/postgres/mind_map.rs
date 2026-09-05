//! Vaults, their attachments, their shares and share attachments.
//!
//! One trait's worth of queries. See `sql_store` for why they are split.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio_postgres::types::Json;

use crate::{
    db::sql_store::{
        MindMapStore,
        MindMapAttachmentUploadUpdate, MindMapContentUpdate, MindMapMetaUpdate,
        MindMapShareAttachmentUploadUpdate, MindMapShareUploadUpdate,
        NewMindMap,
        NewMindMapAttachment, NewMindMapShare, NewMindMapShareAttachment,
        StoredMindMap, StoredMindMapAttachment, StoredMindMapShare, StoredMindMapShareAttachment,
    },
    error::AppError,
    models::mindmap::VersionSnapshot,
};

use super::row::*;
use super::PostgresDb;

#[async_trait]
impl MindMapStore for PostgresDb {
    async fn list_mind_maps(&self, user_id: &str) -> Result<Vec<StoredMindMap>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
                    version_history, vault_color, vault_note_encrypted,
                    vault_encryption_mode, max_versions, vault_labels
                 FROM mind_maps
                 WHERE user_id = $1
                 ORDER BY updated_at DESC",
                &[&user_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_from_row).collect()
    }

    async fn create_mind_map(&self, map: NewMindMap) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_maps (
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
                    version_history, vault_color, vault_note_encrypted,
                    vault_encryption_mode, max_versions, vault_labels
                 ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, $7, $8, $9, $10,
                    $11, $12, $13,
                    $14, $15, $16
                 )",
                &[
                    &map.id,
                    &map.user_id,
                    &map.title_encrypted,
                    &map.object_key,
                    &map.eph_classical_public,
                    &map.eph_pq_ciphertext,
                    &map.wrapped_dek,
                    &map.created_at,
                    &map.updated_at,
                    &map.current_version_id,
                    &Json(&map.version_history),
                    &map.vault_color,
                    &map.vault_note_encrypted,
                    &map.vault_encryption_mode,
                    &(map.max_versions as i32),
                    &Json(&map.vault_labels),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_owned(
        &self,
        id: &str,
        user_id: &str,
    ) -> Result<Option<StoredMindMap>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT
                    id, user_id, title_encrypted, object_key, eph_classical_public,
                    eph_pq_ciphertext, wrapped_dek, created_at, updated_at, current_version_id,
                    version_history, vault_color, vault_note_encrypted,
                    vault_encryption_mode, max_versions, vault_labels
                 FROM mind_maps
                 WHERE id = $1 AND user_id = $2
                 LIMIT 1",
                &[&id, &user_id],
            )
            .await?;

        row.map(stored_mind_map_from_row).transpose()
    }

    async fn update_mind_map_content(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapContentUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET title_encrypted = $1,
                     eph_classical_public = $2,
                     eph_pq_ciphertext = $3,
                     wrapped_dek = $4,
                     updated_at = $5
                 WHERE id = $6 AND user_id = $7",
                &[
                    &update.title_encrypted,
                    &update.eph_classical_public,
                    &update.eph_pq_ciphertext,
                    &update.wrapped_dek,
                    &update.updated_at,
                    &id,
                    &user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_upload(
        &self,
        id: &str,
        user_id: &str,
        current_version_id: &str,
        version_history: Vec<VersionSnapshot>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET current_version_id = $1, version_history = $2
                 WHERE id = $3 AND user_id = $4",
                &[&current_version_id, &Json(&version_history), &id, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn delete_mind_map(&self, id: &str, user_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "DELETE FROM mind_maps WHERE id = $1 AND user_id = $2",
                &[&id, &user_id],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_meta(
        &self,
        id: &str,
        user_id: &str,
        update: MindMapMetaUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_maps
                 SET vault_color = $1,
                     vault_note_encrypted = $2,
                     vault_encryption_mode = $3,
                     max_versions = $4,
                     title_encrypted = $5,
                     updated_at = $6,
                     vault_labels = $7
                 WHERE id = $8 AND user_id = $9",
                &[
                    &update.vault_color,
                    &update.vault_note_encrypted,
                    &update.vault_encryption_mode,
                    &(update.max_versions as i32),
                    &update.title_encrypted,
                    &update.updated_at,
                    &Json(&update.vault_labels),
                    &id,
                    &user_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_attachments(
        &self,
        map_id: &str,
    ) -> Result<Vec<StoredMindMapAttachment>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                        s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_attachments
                 WHERE map_id = $1 AND status <> 'deleted'
                 ORDER BY uploaded_at DESC",
                &[&map_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_attachment_from_row).collect()
    }

    async fn create_mind_map_attachment(
        &self,
        attachment: NewMindMapAttachment,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_attachments (
                    id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                    s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                    encryption_meta, checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12,
                    $13, $14, $15
                 )",
                &[
                    &attachment.id,
                    &attachment.map_id,
                    &attachment.node_id,
                    &attachment.name,
                    &attachment.sanitized_name,
                    &attachment.content_type,
                    &attachment.size_bytes,
                    &attachment.s3_key,
                    &attachment.s3_version_id,
                    &attachment.uploaded_by,
                    &attachment.uploaded_at,
                    &attachment.encrypted,
                    &attachment.encryption_meta.map(Json),
                    &attachment.checksum_sha256,
                    &attachment.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_attachment(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<Option<StoredMindMapAttachment>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, node_id, name, sanitized_name, content_type, size_bytes,
                        s3_key, s3_version_id, uploaded_by, uploaded_at, encrypted,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_attachments
                 WHERE map_id = $1 AND id = $2 AND status <> 'deleted'
                 LIMIT 1",
                &[&map_id, &attachment_id],
            )
            .await?;

        row.map(stored_mind_map_attachment_from_row).transpose()
    }

    async fn complete_mind_map_attachment_upload(
        &self,
        map_id: &str,
        attachment_id: &str,
        update: MindMapAttachmentUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3
                 WHERE map_id = $4 AND id = $5 AND status <> 'deleted'",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &map_id,
                    &attachment_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn update_mind_map_attachment_node(
        &self,
        map_id: &str,
        attachment_id: &str,
        node_id: Option<String>,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET node_id = $1
                 WHERE map_id = $2 AND id = $3 AND status <> 'deleted'",
                &[&node_id, &map_id, &attachment_id],
            )
            .await?;

        Ok(())
    }

    async fn mark_mind_map_attachment_deleted(
        &self,
        map_id: &str,
        attachment_id: &str,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_attachments
                 SET status = 'deleted'
                 WHERE map_id = $1 AND id = $2 AND status <> 'deleted'",
                &[&map_id, &attachment_id],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_shares(&self, map_id: &str) -> Result<Vec<StoredMindMapShare>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE map_id = $1
                 ORDER BY created_at DESC",
                &[&map_id],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_share_from_row).collect()
    }

    async fn create_mind_map_share(&self, share: NewMindMapShare) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_shares (
                    id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                    created_at, updated_at, expires_at, revoked, include_attachments,
                    passphrase_hint, content_type, size_bytes, encryption_meta,
                    checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7,
                    $8, $9, $10, $11, $12,
                    $13, $14, $15, $16,
                    $17, $18
                 )",
                &[
                    &share.id,
                    &share.map_id,
                    &share.share_name,
                    &share.scope.as_str(),
                    &share.s3_key,
                    &share.s3_version_id,
                    &share.created_by,
                    &share.created_at,
                    &share.updated_at,
                    &share.expires_at,
                    &share.revoked,
                    &share.include_attachments,
                    &share.passphrase_hint,
                    &share.content_type,
                    &share.size_bytes,
                    &Json(&share.encryption_meta),
                    &share.checksum_sha256,
                    &share.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_share(
        &self,
        map_id: &str,
        share_id: &str,
    ) -> Result<Option<StoredMindMapShare>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE map_id = $1 AND id = $2
                 LIMIT 1",
                &[&map_id, &share_id],
            )
            .await?;

        row.map(stored_mind_map_share_from_row).transpose()
    }

    async fn get_public_mind_map_share(&self, share_id: &str) -> Result<Option<StoredMindMapShare>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE id = $1
                 LIMIT 1",
                &[&share_id],
            )
            .await?;

        row.map(stored_mind_map_share_from_row).transpose()
    }

    async fn complete_mind_map_share_upload(
        &self,
        map_id: &str,
        share_id: &str,
        update: MindMapShareUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3,
                     updated_at = NOW()
                 WHERE map_id = $4 AND id = $5",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &map_id,
                    &share_id,
                ],
            )
            .await?;

        Ok(())
    }

    async fn set_mind_map_share_revoked(
        &self,
        map_id: &str,
        share_id: &str,
        revoked: bool,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET revoked = $1,
                     status = CASE WHEN $1 THEN 'revoked' ELSE status END,
                     updated_at = NOW()
                 WHERE map_id = $2 AND id = $3",
                &[&revoked, &map_id, &share_id],
            )
            .await?;

        Ok(())
    }

    async fn list_purgeable_mind_map_shares(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<StoredMindMapShare>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, map_id, share_name, share_scope, s3_key, s3_version_id, created_by,
                        created_at, updated_at, expires_at, revoked, include_attachments,
                        passphrase_hint, content_type, size_bytes, encryption_meta,
                        checksum_sha256, status
                 FROM mind_map_shares
                 WHERE status = 'available'
                   AND (revoked = TRUE OR (expires_at IS NOT NULL AND expires_at < $1))
                 ORDER BY updated_at ASC
                 LIMIT $2",
                &[&now, &limit],
            )
            .await?;

        rows.into_iter().map(stored_mind_map_share_from_row).collect()
    }

    async fn mark_mind_map_share_purged(&self, share_id: &str) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_shares
                 SET status = 'revoked', revoked = TRUE, size_bytes = 0, updated_at = NOW()
                 WHERE id = $1",
                &[&share_id],
            )
            .await?;

        Ok(())
    }

    async fn list_mind_map_share_attachments(
        &self,
        share_id: &str,
    ) -> Result<Vec<StoredMindMapShareAttachment>, AppError> {
        let rows = self
            .client
            .query(
                "SELECT id, share_id, source_attachment_id, node_id, name, sanitized_name,
                        content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_share_attachments
                 WHERE share_id = $1 AND status <> 'deleted'
                 ORDER BY uploaded_at DESC",
                &[&share_id],
            )
            .await?;

        rows.into_iter()
            .map(stored_mind_map_share_attachment_from_row)
            .collect()
    }

    async fn create_mind_map_share_attachment(
        &self,
        attachment: NewMindMapShareAttachment,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "INSERT INTO mind_map_share_attachments (
                    id, share_id, source_attachment_id, node_id, name, sanitized_name,
                    content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                    encryption_meta, checksum_sha256, status
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6,
                    $7, $8, $9, $10, $11,
                    $12, $13, $14
                 )",
                &[
                    &attachment.id,
                    &attachment.share_id,
                    &attachment.source_attachment_id,
                    &attachment.node_id,
                    &attachment.name,
                    &attachment.sanitized_name,
                    &attachment.content_type,
                    &attachment.size_bytes,
                    &attachment.s3_key,
                    &attachment.s3_version_id,
                    &attachment.uploaded_at,
                    &Json(&attachment.encryption_meta),
                    &attachment.checksum_sha256,
                    &attachment.status.as_str(),
                ],
            )
            .await?;

        Ok(())
    }

    async fn get_mind_map_share_attachment(
        &self,
        share_id: &str,
        attachment_id: &str,
    ) -> Result<Option<StoredMindMapShareAttachment>, AppError> {
        let row = self
            .client
            .query_opt(
                "SELECT id, share_id, source_attachment_id, node_id, name, sanitized_name,
                        content_type, size_bytes, s3_key, s3_version_id, uploaded_at,
                        encryption_meta, checksum_sha256, status
                 FROM mind_map_share_attachments
                 WHERE share_id = $1 AND id = $2 AND status <> 'deleted'
                 LIMIT 1",
                &[&share_id, &attachment_id],
            )
            .await?;

        row.map(stored_mind_map_share_attachment_from_row).transpose()
    }

    async fn complete_mind_map_share_attachment_upload(
        &self,
        share_id: &str,
        attachment_id: &str,
        update: MindMapShareAttachmentUploadUpdate,
    ) -> Result<(), AppError> {
        self.client
            .execute(
                "UPDATE mind_map_share_attachments
                 SET s3_version_id = $1,
                     checksum_sha256 = $2,
                     status = $3
                 WHERE share_id = $4 AND id = $5 AND status <> 'deleted'",
                &[
                    &update.s3_version_id,
                    &update.checksum_sha256,
                    &update.status.as_str(),
                    &share_id,
                    &attachment_id,
                ],
            )
            .await?;

        Ok(())
    }
}
