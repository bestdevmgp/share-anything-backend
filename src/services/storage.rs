use aws_config::BehaviorVersion;
use aws_sdk_s3::{
    config::{Credentials, Region},
    primitives::ByteStream,
    Client as S3Client,
};

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

        // For S3-compatible services like Cloudflare R2 or MinIO
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

    /// Upload a file to object storage
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

    /// Download a file from object storage
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

    /// Delete a file from object storage
    pub async fn delete_file(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.client
            .delete_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await?;

        Ok(())
    }
    
    /// Delete multiple files
    pub async fn delete_files(&self, keys: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        for key in keys {
            // Ignore errors for individual deletions
            let _ = self.delete_file(&key).await;
        }
        Ok(())
    }
}
