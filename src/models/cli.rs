use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Serialize, ToSchema)]
pub struct CliUploadResponse {
    #[schema(example = "482917")]
    pub share_code: String,
    #[schema(example = json!(["report.pdf", "photo.jpg"]))]
    pub files: Vec<String>,
    #[schema(example = "curl -OJ -H \"X-Personal-Token: sa_5UqEU7qHLkMi6aLAAmcrpNo4wS7p8pi3SYnv3dQa\" https://share-api.mingyu.dev/v1/shares/482917/download")]
    pub curl_command: String,
    #[schema(example = "2026-05-21 14:30")]
    pub expires_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliFileInfoResponse {
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    pub has_password: bool,
    pub is_one_time: bool,
    pub expires_at: String,
    pub transfer_type: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliFileDetail {
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = 5242880)]
    pub file_size: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliFileListResponse {
    #[schema(example = "482917")]
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    #[schema(example = 2)]
    pub total_count: usize,
    #[schema(example = false)]
    pub has_password: bool,
    #[schema(example = false)]
    pub is_one_time: bool,
    #[schema(example = "2026-05-21 14:30")]
    pub expires_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliP2PFileInfo {
    pub name: String,
    pub size: i64,
    #[serde(rename = "type")]
    pub content_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliP2PCreateRequest {
    pub files: Vec<CliP2PFileInfo>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliP2PCreateResponse {
    pub share_code: String,
    pub files: Vec<String>,
    pub expires_at: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliMultipartInitRequest {
    pub files: Vec<CliMultipartFileInfo>,
    #[schema(example = "Q2 report — please review by Friday")]
    pub description: Option<String>,
    #[schema(example = "hunter2")]
    pub password: Option<String>,
    #[schema(example = "1h")]
    pub expiration: Option<String>,
    #[schema(example = false)]
    pub is_one_time: Option<bool>,
    #[schema(example = 10485760)]
    pub chunk_size: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliMultipartFileInfo {
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = 104857600)]
    pub file_size: i64,
    #[schema(example = "application/pdf")]
    pub content_type: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliMultipartInitResponse {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub upload_session_id: String,
    #[schema(example = "482917")]
    pub share_code: String,
    pub files: Vec<CliMultipartFileInit>,
    #[schema(example = 10485760)]
    pub chunk_size: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliMultipartFileInit {
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = "files/550e8400-e29b-41d4-a716-446655440000.pdf")]
    pub storage_key: String,
    #[schema(example = "VXBsb2FkSWQ1MjQyODgwMA")]
    pub upload_id: String,
    #[schema(example = 10)]
    pub total_parts: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliPresignPartsRequest {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub upload_session_id: String,
    #[schema(example = "files/550e8400-e29b-41d4-a716-446655440000.pdf")]
    pub storage_key: String,
    #[schema(example = "VXBsb2FkSWQ1MjQyODgwMA")]
    pub upload_id: String,
    #[schema(example = json!([1, 2, 3]))]
    pub part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliPresignPartsResponse {
    #[schema(example = "files/550e8400-e29b-41d4-a716-446655440000.pdf")]
    pub storage_key: String,
    pub urls: Vec<CliPartUrl>,
    #[schema(example = 3600)]
    pub expires_in_secs: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliPartUrl {
    #[schema(example = 1)]
    pub part_number: i32,
    #[schema(example = "https://YOUR_ACCOUNT_ID.r2.cloudflarestorage.com/share-anything-bucket/files/550e8400-e29b-41d4-a716-446655440000.pdf?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Date=20260521T143000Z&X-Amz-Expires=3600&X-Amz-SignedHeaders=host&X-Amz-Signature=abc123...")]
    pub presigned_url: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliCompleteMultipartRequest {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub upload_session_id: String,
    #[schema(example = "482917")]
    pub share_code: String,
    pub files: Vec<CliCompleteFileInfo>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliCompleteFileInfo {
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = "files/550e8400-e29b-41d4-a716-446655440000.pdf")]
    pub storage_key: String,
    #[schema(example = "VXBsb2FkSWQ1MjQyODgwMA")]
    pub upload_id: String,
    #[schema(example = 104857600)]
    pub file_size: i64,
    #[schema(example = "application/pdf")]
    pub content_type: String,
    pub parts: Vec<CliCompletedPart>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliCompletedPart {
    #[schema(example = 1)]
    pub part_number: i32,
    #[schema(example = "\"d41d8cd98f00b204e9800998ecf8427e\"")]
    pub etag: String,
}

#[derive(Debug, Deserialize)]
pub struct CliDownloadQuery {
    pub password: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CliUploadHistoryQuery {
    #[param(example = 20)]
    pub limit: Option<i64>,
    #[param(example = 0)]
    pub offset: Option<i64>,
}
