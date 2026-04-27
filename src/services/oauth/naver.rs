use serde::Deserialize;

use crate::{
    config::Config,
    models::{internal_error, AppError, OAuthProvider},
    services::auth::OAuthUserInfo,
};

// Naver returns expires_in as a string, not a number (non-standard OAuth 2.0).
#[derive(Debug, Deserialize)]
struct NaverTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct NaverUserInfoResponse {
    response: NaverUserResponse,
}

#[derive(Debug, Deserialize)]
struct NaverUserResponse {
    id: String,
    email: String,
    name: String,
    profile_image: Option<String>,
}

pub async fn fetch_user_info(
    config: &Config,
    code: &str,
    state_param: &str,
) -> Result<OAuthUserInfo, AppError> {
    let http = reqwest::Client::new();

    let token_resp = http
        .post("https://nid.naver.com/oauth2.0/token")
        .query(&[
            ("grant_type", "authorization_code"),
            ("client_id", &config.oauth.naver.client_id),
            ("client_secret", &config.oauth.naver.client_secret),
            ("code", code),
            ("state", state_param),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        tracing::error!("Naver token exchange failed {}: {}", status, body);
        return Err(internal_error("Naver 인증에 실패했습니다"));
    }

    let token: NaverTokenResponse = token_resp.json().await?;

    let user_resp = http
        .get("https://openapi.naver.com/v1/nid/me")
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if !user_resp.status().is_success() {
        let status = user_resp.status();
        let body = user_resp.text().await.unwrap_or_default();
        tracing::error!("Naver userinfo failed {}: {}", status, body);
        return Err(internal_error("Naver 사용자 정보 조회 실패"));
    }

    let info: NaverUserInfoResponse = user_resp.json().await?;

    Ok(OAuthUserInfo {
        provider: OAuthProvider::Naver,
        oauth_id: info.response.id,
        email: info.response.email,
        name: info.response.name,
        profile_image: info.response.profile_image,
    })
}
