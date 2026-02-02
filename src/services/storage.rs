use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client as S3Client,
};
use std::time::{Duration, Instant};
use tracing::{info, warn, error};

const LARGE_FILE_THRESHOLD: usize = 100 * 1024 * 1024; // 100MB

#[derive(Clone)]
pub struct StorageService {
    client: S3Client,
    bucket_name: String,
}

impl StorageService {
    pub async fn new(
        endpoint: Option<String>,
        region: String,
        bucket_name: String,
        access_key_id: String,
        secret_access_key: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let credentials = Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "share-anything",
        );

        let mut config_builder = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(credentials)
            .region(Region::new(region));

        if let Some(endpoint_url) = endpoint {
            config_builder = config_builder.endpoint_url(endpoint_url);
        }

        let config = config_builder.build();
        let client = S3Client::from_conf(config);

        Ok(Self {
            client,
            bucket_name,
        })
    }

    pub async fn upload_file(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_size = data.len();
        let file_size_mb = file_size as f64 / 1024.0 / 1024.0;
        let is_large_file = file_size >= LARGE_FILE_THRESHOLD;

        if is_large_file {
            warn!(
                storage_key = %key,
                bucket = %self.bucket_name,
                file_size_bytes = file_size,
                file_size_mb = format!("{:.2}", file_size_mb),
                content_type = %content_type,
                "[StorageService] LARGE FILE (>=100MB) - Creating ByteStream for S3 put_object"
            );
        } else {
            info!(
                storage_key = %key,
                file_size_bytes = file_size,
                "[StorageService] Creating ByteStream for S3 upload"
            );
        }

        let bytestream_start = Instant::now();
        let body = ByteStream::from(data);
        let bytestream_elapsed = bytestream_start.elapsed();

        if is_large_file {
            info!(
                storage_key = %key,
                file_size_mb = format!("{:.2}", file_size_mb),
                bytestream_creation_ms = bytestream_elapsed.as_millis(),
                "[StorageService] LARGE FILE - ByteStream created, starting S3 put_object request"
            );
        }

        let s3_request_start = Instant::now();

        if is_large_file {
            warn!(
                storage_key = %key,
                bucket = %self.bucket_name,
                file_size_mb = format!("{:.2}", file_size_mb),
                "[StorageService] LARGE FILE - Sending put_object request to S3 (this operation may take significant time)"
            );
        }

        let result = self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await;

        let s3_request_elapsed = s3_request_start.elapsed();
        let throughput_mbps = if s3_request_elapsed.as_secs_f64() > 0.0 {
            file_size_mb / s3_request_elapsed.as_secs_f64()
        } else {
            0.0
        };

        match &result {
            Ok(output) => {
                if is_large_file {
                    warn!(
                        storage_key = %key,
                        bucket = %self.bucket_name,
                        file_size_bytes = file_size,
                        file_size_mb = format!("{:.2}", file_size_mb),
                        s3_elapsed_ms = s3_request_elapsed.as_millis(),
                        s3_elapsed_secs = format!("{:.2}", s3_request_elapsed.as_secs_f64()),
                        throughput_mbps = format!("{:.2}", throughput_mbps),
                        e_tag = ?output.e_tag(),
                        version_id = ?output.version_id(),
                        "[StorageService] LARGE FILE (>=100MB) - S3 put_object SUCCESS"
                    );
                } else {
                    info!(
                        storage_key = %key,
                        file_size_bytes = file_size,
                        s3_elapsed_ms = s3_request_elapsed.as_millis(),
                        "[StorageService] S3 put_object completed"
                    );
                }
            }
            Err(e) => {
                error!(
                    storage_key = %key,
                    bucket = %self.bucket_name,
                    file_size_bytes = file_size,
                    file_size_mb = format!("{:.2}", file_size_mb),
                    s3_elapsed_ms = s3_request_elapsed.as_millis(),
                    s3_elapsed_secs = format!("{:.2}", s3_request_elapsed.as_secs_f64()),
                    is_large_file = is_large_file,
                    error_type = ?std::any::type_name_of_val(&e),
                    error = %e,
                    "[StorageService] S3 put_object FAILED - Check S3 connection, timeouts, and credentials"
                );

                // Additional debug info for large file failures
                if is_large_file {
                    error!(
                        storage_key = %key,
                        "[StorageService] LARGE FILE UPLOAD FAILURE DIAGNOSTICS:",
                    );
                    error!(
                        "  - File size: {:.2} MB ({} bytes)",
                        file_size_mb, file_size
                    );
                    error!(
                        "  - Elapsed time: {:.2}s ({} ms)",
                        s3_request_elapsed.as_secs_f64(),
                        s3_request_elapsed.as_millis()
                    );
                    error!(
                        "  - Potential causes: S3 timeout, network interruption, insufficient bandwidth, S3 rate limiting"
                    );
                    error!(
                        "  - Suggestion: Consider implementing multipart upload for files >= 100MB"
                    );
                }
            }
        }

        result?;
        Ok(())
    }

    pub async fn download_file(
        &self,
        key: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await?;

        let data = response.body.collect().await?;
        Ok(data.into_bytes().to_vec())
    }

    pub async fn delete_file(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await?;

        Ok(())
    }

    pub async fn delete_files(&self, keys: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        for key in keys {
            let _ = self.delete_file(&key).await;
        }
        Ok(())
    }

    pub async fn generate_presigned_put_url(
        &self,
        key: &str,
        content_type: &str,
        expires_in_secs: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let presigning_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(expires_in_secs))
            .build()?;

        let presigned_request = self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .content_type(content_type)
            .presigned(presigning_config)
            .await?;

        let url = presigned_request.uri().to_string();

        info!(
            storage_key = %key,
            content_type = %content_type,
            expires_in_secs = expires_in_secs,
            "[StorageService] Generated presigned PUT URL"
        );

        Ok(url)
    }

    pub fn get_bucket_name(&self) -> &str {
        &self.bucket_name
    }

    // Multipart Upload Methods

    pub async fn create_multipart_upload(
        &self,
        key: &str,
        content_type: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let response = self.client
            .create_multipart_upload()
            .bucket(&self.bucket_name)
            .key(key)
            .content_type(content_type)
            .send()
            .await?;

        let upload_id = response.upload_id()
            .ok_or("No upload_id returned from create_multipart_upload")?
            .to_string();

        info!(
            storage_key = %key,
            upload_id = %upload_id,
            "[StorageService] Created multipart upload"
        );

        Ok(upload_id)
    }

    pub async fn generate_presigned_upload_part_url(
        &self,
        key: &str,
        upload_id: &str,
        part_number: i32,
        expires_in_secs: u64,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let presigning_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(expires_in_secs))
            .build()?;

        let presigned_request = self.client
            .upload_part()
            .bucket(&self.bucket_name)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .presigned(presigning_config)
            .await?;

        let url = presigned_request.uri().to_string();

        info!(
            storage_key = %key,
            upload_id = %upload_id,
            part_number = part_number,
            "[StorageService] Generated presigned URL for upload part"
        );

        Ok(url)
    }

    pub async fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>, // (part_number, etag)
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

        let completed_parts: Vec<CompletedPart> = parts
            .into_iter()
            .map(|(part_number, etag)| {
                CompletedPart::builder()
                    .part_number(part_number)
                    .e_tag(etag)
                    .build()
            })
            .collect();

        let completed_upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed_parts))
            .build();

        self.client
            .complete_multipart_upload()
            .bucket(&self.bucket_name)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(completed_upload)
            .send()
            .await?;

        info!(
            storage_key = %key,
            upload_id = %upload_id,
            "[StorageService] Completed multipart upload"
        );

        Ok(())
    }

    pub async fn abort_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.client
            .abort_multipart_upload()
            .bucket(&self.bucket_name)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await?;

        info!(
            storage_key = %key,
            upload_id = %upload_id,
            "[StorageService] Aborted multipart upload"
        );

        Ok(())
    }
}
