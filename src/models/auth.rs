use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize)]
pub struct OAuthLoginQuery {
    pub redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct NaverCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct KakaoCallbackQuery {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Deserialize)]
pub struct AppleCallbackForm {
    pub code: String,
    pub state: String,
    pub user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppleCallbackHandlerQuery {
    pub code: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub state: Option<String>,
    pub apple_user: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub user: UserResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reactivated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_new_user: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
    pub oauth_provider: String,
}
