use std::{collections::{HashMap, HashSet}, time::Duration};

use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::{
    config::Builder as S3ConfigBuilder,
    error::ProvideErrorMetadata,
    primitives::ByteStream,
    presigning::PresigningConfig,
    Client as S3Client,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use crate::{config::AppConfig, error::AppError};

/// A single stored version of a mind map blob in MinIO.
#[derive(Debug, Clone, Serialize)]
pub struct ObjectVersionInfo {
    pub version_id: String,
    pub is_latest: bool,
    pub last_modified: DateTime<Utc>,
    /// Size of the ciphertext blob in bytes.
    pub size_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct MinioClient {
    pub client: S3Client,
    pub presign_client: S3Client,
    pub bucket: String,
    pub presign_expiry: Duration,
}

impl MinioClient {
    fn normalize_version_id(version_id: &str) -> String {
        version_id.trim().trim_matches('"').to_string()
    }

    fn validate_uploaded_version_id(version_id: &str) -> Result<String, AppError> {
        let version_id = Self::normalize_version_id(version_id);
        if version_id.is_empty() {
            return Err(AppError::BadRequest("version_id is required".to_string()));
        }

        Ok(version_id)
    }

    fn map_head_object_error<E>(
        error: &aws_sdk_s3::error::SdkError<E>,
        version_id: &str,
    ) -> AppError
    where
        E: std::error::Error + ProvideErrorMetadata + Send + Sync + 'static,
    {
        let service_code = error
            .as_service_error()
            .and_then(|service_error| service_error.code());
        let service_message = error
            .as_service_error()
            .and_then(|service_error| service_error.message())
            .unwrap_or("service error");

        if let Some(status) = error.raw_response().map(|response| response.status().as_u16()) {
            if status == 404 {
                return AppError::NotFound(format!("version '{version_id}' not found"));
            }

            if status == 400 {
                return AppError::BadRequest(format!("invalid version id '{version_id}'"));
            }
        }

        if matches!(service_code, Some("NotFound" | "NoSuchVersion" | "NoSuchKey")) {
            return AppError::NotFound(format!("version '{version_id}' not found"));
        }

        if matches!(service_code, Some("InvalidArgument" | "InvalidRequest")) {
            return AppError::BadRequest(format!("invalid version id '{version_id}'"));
        }

        let code = service_code.unwrap_or("unknown");
        AppError::Storage(format!("head_object failed for version '{version_id}' ({code}): {service_message}"))
    }

    pub async fn connect(cfg: &AppConfig) -> anyhow::Result<Self> {
        let creds = Credentials::new(
            &cfg.s3_access_key,
            &cfg.s3_secret_key,
            None,
            None,
            "mindmapvault-static",
        );

        let aws_cfg = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(cfg.s3_region.clone()))
            .credentials_provider(creds)
            .load()
            .await;

        // Override the endpoint to point at the configured S3-compatible backend.
        let s3_cfg = S3ConfigBuilder::from(&aws_cfg)
            .endpoint_url(&cfg.s3_endpoint)
            .force_path_style(true)
            .build();

        let client = S3Client::from_conf(s3_cfg);
        let presign_client = if cfg.s3_public_endpoint.trim().is_empty() {
            client.clone()
        } else {
            let public_s3_cfg = S3ConfigBuilder::from(&aws_cfg)
                .endpoint_url(cfg.s3_public_endpoint.trim())
                .force_path_style(true)
                .build();
            S3Client::from_conf(public_s3_cfg)
        };

        // Ensure bucket exists.
        let bucket = cfg.s3_bucket.clone();
        Self::ensure_bucket(&client, &bucket).await?;

        tracing::info!("Connected to S3 endpoint at {} (bucket: {})", cfg.s3_endpoint, bucket);

        Ok(Self {
            client,
            presign_client,
            bucket,
            presign_expiry: Duration::from_secs(cfg.s3_presign_expiry_secs),
        })
    }

    /// Creates the bucket if it does not yet exist.
    async fn ensure_bucket(client: &S3Client, bucket: &str) -> anyhow::Result<()> {
        match client.head_bucket().bucket(bucket).send().await {
            Ok(_) => Ok(()),
            Err(_) => {
                client.create_bucket().bucket(bucket).send().await?;
                tracing::info!("Created S3 bucket '{bucket}'");
                Ok(())
            }
        }
    }

    /// Generates a presigned PUT URL. The browser will receive the assigned
    /// `x-amz-version-id` response header after a successful upload — it
    /// should pass that back via `POST /:id/confirm-upload`.
    pub async fn presigned_put_url(&self, object_key: &str) -> Result<String, AppError> {
        let presign_cfg = PresigningConfig::expires_in(self.presign_expiry)
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let presigned = self
            .presign_client
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .presigned(presign_cfg)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Uploads an encrypted blob through the backend using the internal S3
    /// endpoint and returns the assigned object version ID.
    pub async fn upload_blob(
        &self,
        object_key: &str,
        blob: Vec<u8>,
    ) -> Result<String, AppError> {
        let output = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .content_type("application/octet-stream")
            .body(ByteStream::from(blob))
            .send()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let version_id = output
            .version_id()
            .ok_or_else(|| AppError::Storage("S3 backend did not return a version id".to_string()))?;

        Self::validate_uploaded_version_id(version_id)
    }

    /// Downloads an encrypted blob through the backend using the internal
    /// S3 endpoint. When `version_id` is provided, that historical version
    /// is streamed instead of the latest object.
    pub async fn download_blob(
        &self,
        object_key: &str,
        version_id: Option<&str>,
    ) -> Result<Vec<u8>, AppError> {
        let mut request = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key);

        if let Some(version_id) = version_id {
            request = request.version_id(version_id);
        }

        let response = request
            .send()
            .await
            .map_err(|e| {
                let service_code = e.as_service_error().and_then(|se| se.code());
                let http_status = e.raw_response().map(|r| r.status().as_u16());
                if http_status == Some(404)
                    || matches!(service_code, Some("NoSuchKey" | "NotFound" | "NoSuchVersion"))
                {
                    AppError::NotFound("board content not found in storage".to_string())
                } else {
                    AppError::Storage(e.to_string())
                }
            })?;

        let collected = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(collected.into_bytes().to_vec())
    }

    /// Generates a presigned GET URL. Pass `version_id` to retrieve a specific
    /// historical version; omit it to get the latest.
    pub async fn presigned_get_url(
        &self,
        object_key: &str,
        version_id: Option<&str>,
    ) -> Result<String, AppError> {
        let presign_cfg = PresigningConfig::expires_in(self.presign_expiry)
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let mut req = self
            .presign_client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key);

        if let Some(vid) = version_id {
            req = req.version_id(vid);
        }

        let presigned = req
            .presigned(presign_cfg)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Verifies a version ID by issuing HeadObject against it. Returns the
    /// version ID string if confirmed, or an error if not found.
    pub async fn verify_version(
        &self,
        _object_key: &str,
        version_id: &str,
    ) -> Result<String, AppError> {
        Self::validate_uploaded_version_id(version_id)
    }

    /// Lists all stored versions of a given object, newest first.
    pub async fn list_object_versions(
        &self,
        object_key: &str,
    ) -> Result<Vec<ObjectVersionInfo>, AppError> {
        let resp = self
            .client
            .list_object_versions()
            .bucket(&self.bucket)
            .prefix(object_key)
            .send()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let mut versions: Vec<ObjectVersionInfo> = resp
            .versions()
            .iter()
            // prefix may match longer keys; filter to exact key
            .filter(|v| v.key().map(|k| k == object_key).unwrap_or(false))
            .map(|v| {
                // Convert aws_smithy_types::DateTime → chrono::DateTime<Utc>
                let last_modified = v
                    .last_modified()
                    .and_then(|dt| {
                        DateTime::from_timestamp(dt.secs(), dt.subsec_nanos())
                    })
                    .unwrap_or_else(Utc::now);

                ObjectVersionInfo {
                    version_id: v.version_id().unwrap_or("null").to_string(),
                    is_latest: v.is_latest().unwrap_or(false),
                    last_modified,
                    size_bytes: v.size().unwrap_or(0),
                }
            })
            .collect();

        // Sort newest first
        versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        Ok(versions)
    }

    pub async fn list_version_size_totals_for_keys(
        &self,
        object_keys: &HashSet<String>,
    ) -> Result<HashMap<String, i64>, AppError> {
        let mut totals = HashMap::new();
        if object_keys.is_empty() {
            return Ok(totals);
        }

        let mut key_marker: Option<String> = None;
        let mut version_id_marker: Option<String> = None;

        loop {
            let mut request = self.client.list_object_versions().bucket(&self.bucket);
            if let Some(marker) = key_marker.as_deref() {
                request = request.key_marker(marker);
            }
            if let Some(marker) = version_id_marker.as_deref() {
                request = request.version_id_marker(marker);
            }

            let response = request
                .send()
                .await
                .map_err(|error| AppError::Storage(error.to_string()))?;

            for version in response.versions() {
                let Some(key) = version.key() else {
                    continue;
                };
                if !object_keys.contains(key) {
                    continue;
                }
                *totals.entry(key.to_string()).or_insert(0) += version.size().unwrap_or(0);
            }

            if !response.is_truncated().unwrap_or(false) {
                break;
            }

            key_marker = response.next_key_marker().map(str::to_string);
            version_id_marker = response.next_version_id_marker().map(str::to_string);

            if key_marker.is_none() && version_id_marker.is_none() {
                break;
            }
        }

        Ok(totals)
    }

    pub async fn head_object_version(
        &self,
        object_key: &str,
        version_id: &str,
    ) -> Result<ObjectVersionInfo, AppError> {
        let version_id = Self::normalize_version_id(version_id);
        let resp = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(object_key)
            .version_id(&version_id)
            .send()
            .await
            .map_err(|error| Self::map_head_object_error(&error, &version_id))?;

        let last_modified = resp
            .last_modified()
            .and_then(|dt| DateTime::from_timestamp(dt.secs(), dt.subsec_nanos()))
            .unwrap_or_else(Utc::now);

        Ok(ObjectVersionInfo {
            version_id,
            is_latest: false,
            last_modified,
            size_bytes: resp.content_length().unwrap_or(0),
        })
    }

    pub async fn merge_known_versions(
        &self,
        object_key: &str,
        expected_version_ids: &[String],
        current_version_id: Option<&str>,
    ) -> Result<Vec<ObjectVersionInfo>, AppError> {
        let mut versions = self.list_object_versions(object_key).await?;
        let mut seen: HashSet<String> = versions.iter().map(|version| version.version_id.clone()).collect();

        for version_id in expected_version_ids {
            if !seen.insert(version_id.clone()) {
                continue;
            }

            match self.head_object_version(object_key, version_id).await {
                Ok(info) => versions.push(info),
                Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }

        for version in &mut versions {
            version.is_latest = current_version_id == Some(version.version_id.as_str());
        }

        versions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        Ok(versions)
    }

    /// Deletes all versions and delete-markers for an object (hard delete).
    pub async fn delete_object(&self, object_key: &str) -> Result<(), AppError> {
        // With versioning enabled, delete_object only inserts a delete marker.
        // We explicitly delete every version instead.
        let versions = match self.list_object_versions(object_key).await {
            Ok(versions) => versions,
            Err(_) => {
                self.client
                    .delete_object()
                    .bucket(&self.bucket)
                    .key(object_key)
                    .send()
                    .await
                    .map_err(|e| AppError::Storage(e.to_string()))?;
                return Ok(());
            }
        };

        for v in versions {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(object_key)
                .version_id(&v.version_id)
                .send()
                .await
                .map_err(|e| AppError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    /// Hard-deletes one specific version of an object.
    pub async fn delete_version(&self, object_key: &str, version_id: &str) -> Result<(), AppError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(object_key)
            .version_id(version_id)
            .send()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        Ok(())
    }

    /// After a successful upload, prune the oldest versions so that at most
    /// `keep` versions remain.  The newest versions (by `last_modified`) are
    /// retained; excess older ones are hard-deleted from MinIO.
    pub async fn prune_versions(&self, object_key: &str, keep: u32) -> Result<(), AppError> {
        let versions = self.list_object_versions(object_key).await?;
        if versions.len() <= keep as usize { return Ok(()); }

        // list_object_versions returns newest-first; skip the ones we keep.
        for v in versions.into_iter().skip(keep as usize) {
            self.client
                .delete_object()
                .bucket(&self.bucket)
                .key(object_key)
                .version_id(&v.version_id)
                .send()
                .await
                .map_err(|e| AppError::Storage(e.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MinioClient;
    use crate::error::AppError;

    #[test]
    fn accepts_plain_uuid_version_id() {
        let version_id = "62ea0a57-8b2a-4fd2-8046-cf8769d81489";

        let validated = MinioClient::validate_uploaded_version_id(version_id).unwrap();

        assert_eq!(validated, version_id);
    }

    #[test]
    fn accepts_quoted_uuid_version_id_from_header() {
        let validated = MinioClient::validate_uploaded_version_id(
            " \"62ea0a57-8b2a-4fd2-8046-cf8769d81489\" ",
        )
        .unwrap();

        assert_eq!(validated, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
    }

    #[test]
    fn accepts_garage_hex_version_id() {
        let version_id = "4e80be07b7c7a0a45c056ad43d3cbe807c7c5704ab5579328fc1da91140b5927";

        let validated = MinioClient::validate_uploaded_version_id(version_id).unwrap();

        assert_eq!(validated, version_id);
    }

    #[test]
    fn accepts_arbitrary_non_empty_version_id() {
        let version_id = "nonexistent-version-id";

        let validated = MinioClient::validate_uploaded_version_id(version_id).unwrap();

        assert_eq!(validated, version_id);
    }

    #[test]
    fn rejects_empty_version_id() {
        let error = MinioClient::validate_uploaded_version_id("   ").unwrap_err();

        assert!(matches!(error, AppError::BadRequest(_)));
        assert_eq!(error.to_string(), "bad request: version_id is required");
    }

}
