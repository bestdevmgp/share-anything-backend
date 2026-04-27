use serde::{Deserialize, Serialize};

// --- Response types ---

#[derive(Debug, Serialize)]
pub struct CliUploadResponse {
    pub share_code: String,
    pub files: Vec<String>,
    pub curl_command: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct CliFileInfoResponse {
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    pub has_password: bool,
    pub is_one_time: bool,
    pub expires_at: String,
    pub transfer_type: String,
}

#[derive(Debug, Serialize)]
pub struct CliFileDetail {
    pub file_name: String,
    pub file_size: i64,
}

#[derive(Debug, Serialize)]
pub struct CliFileListResponse {
    pub share_code: String,
    pub files: Vec<CliFileDetail>,
    pub total_count: usize,
    pub has_password: bool,
    pub is_one_time: bool,
    pub expires_at: String,
}

// --- P2P types ---

#[derive(Debug, Deserialize)]
pub struct CliP2PFileInfo {
    pub name: String,
    pub size: i64,
    #[serde(rename = "type")]
    pub content_type: String,
}

#[derive(Debug, Deserialize)]
pub struct CliP2PCreateRequest {
    pub files: Vec<CliP2PFileInfo>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CliP2PCreateResponse {
    pub share_code: String,
    pub files: Vec<String>,
    pub expires_at: String,
}

// --- Multipart init types ---

#[derive(Debug, Deserialize)]
pub struct CliMultipartInitRequest {
    pub files: Vec<CliMultipartFileInfo>,
    pub description: Option<String>,
    pub password: Option<String>,
    pub expiration: Option<String>,
    pub is_one_time: Option<bool>,
    pub chunk_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct CliMultipartFileInfo {
    pub file_name: String,
    pub file_size: i64,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
pub struct CliMultipartInitResponse {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CliMultipartFileInit>,
    pub chunk_size: i64,
}

#[derive(Debug, Serialize)]
pub struct CliMultipartFileInit {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub total_parts: i32,
}

// --- Presign parts types ---

#[derive(Debug, Deserialize)]
pub struct CliPresignPartsRequest {
    pub upload_session_id: String,
    pub storage_key: String,
    pub upload_id: String,
    pub part_numbers: Vec<i32>,
}

#[derive(Debug, Serialize)]
pub struct CliPresignPartsResponse {
    pub storage_key: String,
    pub urls: Vec<CliPartUrl>,
    pub expires_in_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct CliPartUrl {
    pub part_number: i32,
    pub presigned_url: String,
}

// --- Complete multipart types ---

#[derive(Debug, Deserialize)]
pub struct CliCompleteMultipartRequest {
    pub upload_session_id: String,
    pub share_code: String,
    pub files: Vec<CliCompleteFileInfo>,
}

#[derive(Debug, Deserialize)]
pub struct CliCompleteFileInfo {
    pub file_name: String,
    pub storage_key: String,
    pub upload_id: String,
    pub file_size: i64,
    pub content_type: String,
    pub parts: Vec<CliCompletedPart>,
}

#[derive(Debug, Deserialize)]
pub struct CliCompletedPart {
    pub part_number: i32,
    pub etag: String,
}

// --- Query types ---

#[derive(Debug, Deserialize)]
pub struct CliDownloadQuery {
    pub password: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CliUploadHistoryQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}
