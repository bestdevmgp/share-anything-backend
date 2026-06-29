use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    operation::get_object::GetObjectOutput,
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client as S3Client,
};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::time::Duration;

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
        let body = ByteStream::from(data);

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await?;

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

    pub async fn download_file_stream(
        &self,
        key: &str,
    ) -> Result<GetObjectOutput, crate::models::error::AppError> {
        self.client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .map_err(|e| crate::models::error::internal_error(format!("Failed to fetch from R2: {}", e)))
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

        Ok(presigned_request.uri().to_string())
    }

    pub async fn generate_presigned_get_url(
        &self,
        key: &str,
        expires_in_secs: u64,
        file_name: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let presigning_config = PresigningConfig::builder()
            .expires_in(Duration::from_secs(expires_in_secs))
            .build()?;

        let mut request = self.client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key);

        if let Some(name) = file_name {
            let content_disposition = if name.is_ascii() {
                format!("attachment; filename=\"{}\"", name)
            } else {
                let encoded = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
                format!("attachment; filename*=UTF-8''{}", encoded)
            };
            request = request.response_content_disposition(content_disposition);
        }

        let presigned_request = request.presigned(presigning_config).await?;

        Ok(presigned_request.uri().to_string())
    }

    pub async fn key_exists(&self, key: &str) -> bool {
        self.client
            .head_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
            .is_ok()
    }

    /// Cheap R2/S3 connectivity probe for uptime monitoring. Issues a `HeadObject`
    /// on a reserved, non-existent key: a `404 NotFound` response proves R2 is
    /// reachable AND the credentials are valid (the key is simply absent), while an
    /// auth (403) or network error means unhealthy. No object enumeration, no data
    /// transfer. `HeadObject` is used on purpose (not `HeadBucket`): it is already
    /// exercised by `key_exists`, so it is known to be within the app's object-scoped
    /// R2 token permissions, whereas a bucket-level op could be forbidden. The
    /// underlying error is logged server-side and never surfaced in the public body.
    pub async fn health_check(&self) -> bool {
        match self
            .client
            .head_object()
            .bucket(&self.bucket_name)
            .key(".health-probe")
            .send()
            .await
        {
            Ok(_) => true,
            Err(err) => {
                if err.as_service_error().map(|e| e.is_not_found()).unwrap_or(false) {
                    true
                } else {
                    tracing::warn!("R2 health check (head_object) failed: {}", err);
                    false
                }
            }
        }
    }

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

        Ok(presigned_request.uri().to_string())
    }

    pub async fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>,
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

        Ok(())
    }

}
