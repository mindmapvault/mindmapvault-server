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
use uuid::Uuid;

use crate::{config::AppConfig, error::AppError, models::status::BucketStats};

/// Marker segment separating a blob's base key from its per-version objects.
const VERSION_SEGMENT: &str = "/v/";

#[derive(Debug, Clone)]
pub struct S3Store {
    pub client: S3Client,
    pub presign_client: S3Client,
    pub bucket: String,
    pub presign_expiry: Duration,
}

impl S3Store {
    /// The object key holding one stored version of a blob.
    ///
    /// Every save writes its own object rather than a new S3 version of a
    /// single key. Object versioning is not part of the S3 core that
    /// implementations agree on — Garage and Cloudflare R2 both answer
    /// `PutBucketVersioning` and `ListObjectVersions` with `NotImplemented` —
    /// and Garage makes the gap dangerous rather than merely inconvenient: it
    /// returns an `x-amz-version-id` on every PUT and then ignores that id on
    /// GET, answering with the current bytes and a 200. A store can therefore
    /// look like it supports versioning and silently serve the wrong data.
    ///
    /// Addressing versions by plain key needs only PutObject, GetObject,
    /// HeadObject, DeleteObject and ListObjectsV2, which every S3
    /// implementation provides.
    pub fn version_key(object_key: &str, version_id: &str) -> String {
        format!("{object_key}{VERSION_SEGMENT}{version_id}")
    }

    /// The base key a version object belongs to, or the key itself when it is
    /// not a version object.
    pub fn base_key(object_key: &str) -> &str {
        match object_key.find(VERSION_SEGMENT) {
            Some(at) => &object_key[..at],
            None => object_key,
        }
    }

    /// Mints a version id.
    ///
    /// Generated here rather than read from the store's response, so the
    /// identity of a version never depends on `x-amz-version-id` — R2 does not
    /// send that header at all, and Garage's is a value it will not honour.
    pub fn new_version_id() -> String {
        Uuid::new_v4().to_string()
    }

    fn normalize_version_id(version_id: &str) -> String {
        version_id.trim().trim_matches('"').to_string()
    }

    /// Checks that a version id is safe to concatenate into an object key.
    ///
    /// Version ids now reach `version_key` from client requests, so this is a
    /// path-traversal boundary, not just a tidiness check: a `/` or `..` would
    /// address someone else's object. Ids minted here are UUIDs; ids already
    /// stored by older builds were the object store UUIDs or Garage hex, so the character
    /// set stays wide enough to keep reading those.
    pub fn validate_version_id(version_id: &str) -> Result<String, AppError> {
        let version_id = Self::normalize_version_id(version_id);
        if version_id.is_empty() {
            return Err(AppError::BadRequest("version_id is required".to_string()));
        }
        if version_id.len() > 128 {
            return Err(AppError::BadRequest("version_id is too long".to_string()));
        }
        if !version_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        {
            return Err(AppError::BadRequest(format!(
                "invalid version id '{version_id}'"
            )));
        }
        // A character-class check alone still admits `..`, which is a legal S3
        // key segment but is collapsed by URL path normalisation in some
        // proxies and SDKs. Nothing we mint contains it, so refuse it outright
        // rather than depend on every hop treating the key as opaque.
        if version_id.contains("..") {
            return Err(AppError::BadRequest(format!(
                "invalid version id '{version_id}'"
            )));
        }
        Ok(version_id)
    }

    fn map_object_error<E>(error: &aws_sdk_s3::error::SdkError<E>, object_key: &str) -> AppError
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

        if error.raw_response().map(|response| response.status().as_u16()) == Some(404)
            || matches!(service_code, Some("NotFound" | "NoSuchKey"))
        {
            return AppError::NotFound(format!("'{object_key}' not found in storage"));
        }

        let code = service_code.unwrap_or("unknown");
        AppError::Storage(format!("storage request for '{object_key}' failed ({code}): {service_message}"))
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

        let this = Self {
            client,
            presign_client,
            bucket,
            presign_expiry: Duration::from_secs(cfg.s3_presign_expiry_secs),
        };

        this.self_test().await?;

        tracing::info!(
            "Connected to S3 endpoint at {} (bucket: {})",
            cfg.s3_endpoint,
            this.bucket
        );

        Ok(this)
    }

    /// Writes, reads back, and deletes a probe object before serving traffic.
    ///
    /// Every operation this server depends on is exercised against the real
    /// endpoint, and the bytes are compared. A store that accepts a write and
    /// then answers reads with something else is otherwise indistinguishable
    /// from a working one until a user notices their data is wrong.
    pub async fn self_test(&self) -> anyhow::Result<()> {
        let key = format!("__mindmapvault_selftest/{}", Uuid::new_v4());
        let payload = Uuid::new_v4().to_string().into_bytes();

        let result = async {
            self.upload_blob(&key, payload.clone()).await?;
            let read_back = self.download_blob(&key).await?;
            if read_back != payload {
                return Err(AppError::Storage(
                    "the object store returned different bytes than were written".to_string(),
                ));
            }
            let size = self.head_size(&key).await?;
            if size != payload.len() as i64 {
                return Err(AppError::Storage(format!(
                    "the object store reported {size} bytes for a {}-byte object",
                    payload.len()
                )));
            }
            Ok(())
        }
        .await;

        // Always try to clean up, including after a failed comparison.
        if let Err(error) = self.delete_object(&key).await {
            tracing::warn!(?error, "could not remove the storage self-test object");
        }

        result.map_err(|error| {
            anyhow::anyhow!("object storage self-test failed: {error}")
        })
    }

    /// Asks the object store whether the bucket is still there.
    ///
    /// The cheapest call that proves the endpoint is reachable, the
    /// credentials are still accepted, and the bucket exists — which is the
    /// whole of what "storage is up" means for this server.
    pub async fn health_check(&self) -> Result<(), String> {
        self.client
            .head_bucket()
            .bucket(&self.bucket)
            .send()
            .await
            .map(|_| ())
            // The SDK's error text can carry the endpoint and the bucket, so
            // the caller gets a short summary rather than the whole thing.
            .map_err(|error| {
                tracing::warn!(?error, "object storage health check failed");
                "the object store did not answer".to_string()
            })
    }

    pub fn bucket_name(&self) -> &str {
        &self.bucket
    }

    /// Walks the bucket, calling `visit` with every object key and its size.
    ///
    /// `ListObjectsV2` rather than `ListObjectVersions`: each saved version is
    /// its own object, so a plain listing already sees all of them, and it is
    /// the listing every S3 implementation supports. Returns `false` when
    /// `max_pages` cut the walk short.
    async fn walk_objects<F>(&self, max_pages: usize, mut visit: F) -> Result<bool, AppError>
    where
        F: FnMut(&str, i64),
    {
        let mut continuation: Option<String> = None;

        for _ in 0..max_pages {
            let mut request = self.client.list_objects_v2().bucket(&self.bucket);
            if let Some(token) = continuation.take() {
                request = request.continuation_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|error| AppError::Storage(error.to_string()))?;

            for object in response.contents() {
                if let Some(key) = object.key() {
                    visit(key, object.size().unwrap_or(0).max(0));
                }
            }

            if !response.is_truncated().unwrap_or(false) {
                return Ok(true);
            }

            continuation = response.next_continuation_token().map(str::to_string);

            // A truncated response with no token would loop forever.
            if continuation.is_none() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Counts what is actually in the bucket, for the status page.
    ///
    /// Bounded by `max_pages` so one status request cannot walk an enormous
    /// bucket. When the limit is hit the figures come back marked truncated, so
    /// the console can present them as a floor rather than a total.
    pub async fn bucket_stats(&self, max_pages: usize) -> Result<BucketStats, String> {
        let mut stats = BucketStats::default();

        let complete = self
            .walk_objects(max_pages, |_key, size| {
                stats.object_count += 1;
                stats.size_bytes += size as u64;
            })
            .await
            .map_err(|error| {
                tracing::warn!(?error, "bucket listing failed");
                "could not list the bucket".to_string()
            })?;

        stats.truncated = !complete;
        Ok(stats)
    }

    /// Totals the bytes stored under each of `object_keys`, counting every
    /// saved version of a blob against its base key.
    pub async fn prefix_size_totals(
        &self,
        object_keys: &HashSet<String>,
    ) -> Result<HashMap<String, i64>, AppError> {
        let mut totals = HashMap::new();
        if object_keys.is_empty() {
            return Ok(totals);
        }

        self.walk_objects(usize::MAX, |key, size| {
            let base = Self::base_key(key);
            if object_keys.contains(base) {
                *totals.entry(base.to_string()).or_insert(0) += size;
            }
        })
        .await?;

        Ok(totals)
    }

    /// Lists every object key under a prefix.
    pub async fn list_keys_under(&self, prefix: &str) -> Result<Vec<String>, AppError> {
        let mut keys = Vec::new();
        let mut continuation: Option<String> = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);
            if let Some(token) = continuation.take() {
                request = request.continuation_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|error| AppError::Storage(error.to_string()))?;

            for object in response.contents() {
                if let Some(key) = object.key() {
                    keys.push(key.to_string());
                }
            }

            if !response.is_truncated().unwrap_or(false) {
                break;
            }

            continuation = response.next_continuation_token().map(str::to_string);
            if continuation.is_none() {
                break;
            }
        }

        Ok(keys)
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

    /// Generates a presigned PUT URL for one object key.
    ///
    /// For vault blobs the caller mints the version id first and presigns the
    /// key that version will live at, so the upload lands in its final place
    /// and the client never has to report back an id the store invented.
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

    /// Uploads a blob through the backend using the internal S3 endpoint.
    pub async fn upload_blob(&self, object_key: &str, blob: Vec<u8>) -> Result<(), AppError> {
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(object_key)
            .content_type("application/octet-stream")
            .body(ByteStream::from(blob))
            .send()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Copies a stored object to a new key.
    ///
    /// Used when a node carrying an attachment is duplicated, and by the
    /// migration that moves legacy blobs onto per-version keys. The ciphertext
    /// is already sealed under the owner's master key, so the bytes stay valid
    /// unchanged — there is nothing to re-encrypt, and the backend never needs
    /// to see the plaintext.
    ///
    /// Prefers the server-side `CopyObject`, which never moves the bytes
    /// through this process. Not every S3-compatible store implements it, so a
    /// failure falls back to a download-and-upload of the same bytes rather
    /// than failing the copy; the log line says which path ran.
    pub async fn copy_object(
        &self,
        source_key: &str,
        destination_key: &str,
    ) -> Result<(), AppError> {
        // `copy_source` is a URL path, so it would need percent-encoding for
        // anything exotic. Our keys cannot contain anything exotic: every
        // segment is a UUID, a version id checked by `validate_version_id`, or
        // a name put through `sanitize_attachment_name`, which keeps only
        // `[A-Za-z0-9._-]`. The check keeps that assumption honest rather than
        // trusting it.
        if !source_key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | '/'))
        {
            return Err(AppError::Storage(
                "object key contains characters that cannot be copied".to_string(),
            ));
        }
        let copy_source = format!("{}/{}", self.bucket, source_key);

        let copied = self
            .client
            .copy_object()
            .bucket(&self.bucket)
            .key(destination_key)
            .copy_source(&copy_source)
            .send()
            .await;

        match copied {
            Ok(_) => Ok(()),
            Err(error) => {
                tracing::warn!(
                    ?error,
                    %source_key,
                    %destination_key,
                    "server-side object copy failed; falling back to download and re-upload"
                );
                let bytes = self.download_blob(source_key).await?;
                self.upload_blob(destination_key, bytes).await
            }
        }
    }

    /// Downloads a blob through the backend using the internal S3 endpoint.
    pub async fn download_blob(&self, object_key: &str) -> Result<Vec<u8>, AppError> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(|error| Self::map_object_error(&error, object_key))?;

        let collected = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(collected.into_bytes().to_vec())
    }

    /// Generates a presigned GET URL for one object key.
    pub async fn presigned_get_url(&self, object_key: &str) -> Result<String, AppError> {
        let presign_cfg = PresigningConfig::expires_in(self.presign_expiry)
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let presigned = self
            .presign_client
            .get_object()
            .bucket(&self.bucket)
            .key(object_key)
            .presigned(presign_cfg)
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Returns the stored size of an object, or `NotFound` if it is not there.
    ///
    /// This is what confirms a presigned upload actually landed. The old code
    /// took the client's word for it: `verify_version` only checked the id was
    /// a non-empty string and never contacted the store at all.
    pub async fn head_size(&self, object_key: &str) -> Result<i64, AppError> {
        let response = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(|error| Self::map_object_error(&error, object_key))?;

        Ok(response.content_length().unwrap_or(0).max(0))
    }

    /// Deletes one object.
    pub async fn delete_object(&self, object_key: &str) -> Result<(), AppError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(object_key)
            .send()
            .await
            .map_err(|e| AppError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Deletes a blob and every stored version of it.
    ///
    /// Covers the base key as well as `<key>/v/*`, so a vault deleted after the
    /// migration and one deleted before it both leave nothing behind.
    pub async fn delete_all_versions(&self, object_key: &str) -> Result<(), AppError> {
        let mut keys = self
            .list_keys_under(&format!("{object_key}{VERSION_SEGMENT}"))
            .await
            .unwrap_or_default();
        keys.push(object_key.to_string());

        for key in keys {
            match self.delete_object(&key).await {
                Ok(()) => {}
                Err(AppError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::S3Store;
    use crate::error::AppError;

    #[test]
    fn builds_a_version_key_under_the_blob_key() {
        let key = S3Store::version_key(
            "62ea0a57-8b2a-4fd2-8046-cf8769d81489",
            "4d1f0a11-0d5e-4a2c-9a1e-2f7d4c8b1a33",
        );

        assert_eq!(
            key,
            "62ea0a57-8b2a-4fd2-8046-cf8769d81489/v/4d1f0a11-0d5e-4a2c-9a1e-2f7d4c8b1a33"
        );
    }

    #[test]
    fn base_key_recovers_the_blob_key_from_a_version_key() {
        let base = S3Store::base_key(
            "62ea0a57-8b2a-4fd2-8046-cf8769d81489/v/4d1f0a11-0d5e-4a2c-9a1e-2f7d4c8b1a33",
        );

        assert_eq!(base, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
    }

    #[test]
    fn base_key_leaves_a_plain_key_alone() {
        let base = S3Store::base_key("62ea0a57-8b2a-4fd2-8046-cf8769d81489");

        assert_eq!(base, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
    }

    #[test]
    fn accepts_a_minted_uuid_version_id() {
        let version_id = "62ea0a57-8b2a-4fd2-8046-cf8769d81489";

        let validated = S3Store::validate_version_id(version_id).unwrap();

        assert_eq!(validated, version_id);
    }

    #[test]
    fn accepts_quoted_version_id_from_header() {
        let validated =
            S3Store::validate_version_id(" \"62ea0a57-8b2a-4fd2-8046-cf8769d81489\" ").unwrap();

        assert_eq!(validated, "62ea0a57-8b2a-4fd2-8046-cf8769d81489");
    }

    /// Version ids written by older builds are still readable.
    #[test]
    fn accepts_legacy_garage_hex_version_id() {
        let version_id = "4e80be07b7c7a0a45c056ad43d3cbe807c7c5704ab5579328fc1da91140b5927";

        let validated = S3Store::validate_version_id(version_id).unwrap();

        assert_eq!(validated, version_id);
    }

    #[test]
    fn rejects_empty_version_id() {
        let error = S3Store::validate_version_id("   ").unwrap_err();

        assert!(matches!(error, AppError::BadRequest(_)));
        assert_eq!(error.to_string(), "bad request: version_id is required");
    }

    /// A version id reaches `version_key`, so a separator would let a request
    /// address an object outside its own vault.
    #[test]
    fn rejects_version_id_that_would_escape_the_key() {
        for version_id in ["../../etc/passwd", "a/b", "..", "with space"] {
            let error = S3Store::validate_version_id(version_id).unwrap_err();
            assert!(
                matches!(error, AppError::BadRequest(_)),
                "expected '{version_id}' to be rejected"
            );
        }
    }

    #[test]
    fn rejects_overlong_version_id() {
        let error = S3Store::validate_version_id(&"a".repeat(129)).unwrap_err();

        assert!(matches!(error, AppError::BadRequest(_)));
    }
}
