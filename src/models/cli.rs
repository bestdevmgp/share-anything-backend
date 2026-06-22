use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

#[derive(Debug, Serialize, ToSchema)]
pub struct CliUploadResponse {
    #[schema(example = "482917")]
    pub share_code: String,
    #[schema(example = json!(["report.pdf", "photo.jpg"]))]
    pub files: Vec<String>,
    #[schema(example = "curl -OJ -H \"X-Personal-Token: sat_5UqEU7qHLkMi6aLAAmcrpNo4wS7p8pi3SYnv3dQa\" https://share-api.mingyu.dev/cli/shares/482917/download")]
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
    #[schema(example = "01HQX3K2N0X8B7H6JZJ5JZ9YK9")]
    pub id: String,
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
    /// Either `server` (download via `GET /v1/shares/{code}/download`) or
    /// `p2p` (use the WebRTC signaling flow at `GET /v1/ws/signaling`).
    #[schema(example = "server")]
    pub transfer_type: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliP2PFileInfo {
    pub name: String,
    pub size: i64,
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(default)]
    pub relative_path: Option<String>,
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

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CliDownloadQuery {
    /// Password for password-protected shares.
    pub password: Option<String>,
    /// When the share has multiple files, the specific file id to download.
    pub file_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliDownloadUrlResponse {
    /// Short-lived presigned URL that points directly at object storage. Download the file
    /// with a plain `GET` — do not attach any auth headers. The URL also sets
    /// `Content-Disposition` so the file lands with its original name.
    #[schema(example = "https://...r2.cloudflarestorage.com/bucket/key?X-Amz-Algorithm=...")]
    pub download_url: String,
    /// ID of the file this URL is bound to. Echo it back on the completion ping so the
    /// download count is attributed to the correct file in multi-file shares.
    #[schema(example = "01HQX3K2N0X8B7H6JZJ5JZ9YK9")]
    pub file_id: String,
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = 943718400)]
    pub file_size: i64,
    #[schema(example = "application/pdf")]
    pub content_type: String,
    /// Seconds until the presigned URL expires. The client should finish the download well
    /// before this elapses.
    #[schema(example = 3600)]
    pub expires_in_secs: u64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CliDownloadCompleteRequest {
    /// `file_id` returned by `POST /cli/shares/{code}/download-url`. Required so the
    /// completion is attributed to the correct file in a multi-file share.
    #[schema(example = "01HQX3K2N0X8B7H6JZJ5JZ9YK9")]
    pub file_id: String,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CliUploadHistoryQuery {
    #[param(example = 20)]
    pub limit: Option<i64>,
    #[param(example = 0)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliMeResponse {
    #[schema(example = "Mingyu Park")]
    pub name: String,
    #[schema(example = "user@example.com")]
    pub email: String,
    /// ISO-like timestamp ("YYYY-MM-DD HH:MM UTC") of the last time the personal token was used.
    /// `null` if the token has never been used.
    #[schema(example = "2026-05-21 14:30 UTC")]
    pub last_used_at: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliUploadHistoryItem {
    #[schema(example = "482917")]
    pub share_code: String,
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = 5242880)]
    pub file_size: i64,
    #[schema(example = "2026-05-21 14:30")]
    pub expires_at: String,
    #[schema(example = "2026-05-21 13:00")]
    pub created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliUploadHistoryResponse {
    pub uploads: Vec<CliUploadHistoryItem>,
    #[schema(example = 7)]
    pub count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliDownloadHistoryItem {
    #[schema(example = "482917")]
    pub share_code: String,
    #[schema(example = "report.pdf")]
    pub file_name: String,
    #[schema(example = 5242880)]
    pub file_size: i64,
    #[schema(example = "203.0.113.4")]
    pub ip_address: String,
    #[schema(example = "2026-05-21 14:32")]
    pub downloaded_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliDownloadHistoryResponse {
    pub downloads: Vec<CliDownloadHistoryItem>,
    #[schema(example = 3)]
    pub count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliShareDownloadLog {
    #[schema(example = "report.pdf")]
    pub file_name: String,
    /// Display name of the downloader. `null` for anonymous downloads.
    #[schema(example = "Mingyu Park")]
    pub downloader_name: Option<String>,
    #[schema(example = "203.0.113.4")]
    pub ip_address: String,
    /// Best-effort device/platform string parsed from the User-Agent header.
    #[schema(example = "macOS")]
    pub device_platform: String,
    #[schema(example = "2026-05-21 14:32")]
    pub downloaded_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CliShareLogsResponse {
    #[schema(example = "482917")]
    pub share_code: String,
    pub downloads: Vec<CliShareDownloadLog>,
    #[schema(example = 4)]
    pub count: usize,
}
