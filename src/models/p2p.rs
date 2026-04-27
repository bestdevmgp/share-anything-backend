use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub struct P2pStatusQuery {
    pub code: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct P2pStatusResponse {
    pub share_code: String,
    pub uploader_online: bool,
}
