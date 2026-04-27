use serde::Deserialize;

use crate::{
    config::Config,
    models::{internal_error, AppError, OAuthProvider},
    services::auth::OAuthUserInfo,
};

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfoResponse {
    id: String,
    email: String,
    name: String,
    picture: Option<String>,
}

pub async fn fetch_user_info(config: &Config, code: &str) -> Result<OAuthUserInfo, AppError> {
    let http = reqwest::Client::new();

    let token_resp = http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", config.oauth.google.client_id.as_str()),
            ("client_secret", config.oauth.google.client_secret.as_str()),
            ("redirect_uri", config.oauth.google.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        tracing::error!("Google token exchange failed {}: {}", status, body);
        return Err(internal_error("Google 인증에 실패했습니다"));
    }

    let token: GoogleTokenResponse = token_resp.json().await?;

    let user_resp = http
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if !user_resp.status().is_success() {
        let status = user_resp.status();
        let body = user_resp.text().await.unwrap_or_default();
        tracing::error!("Google userinfo failed {}: {}", status, body);
        return Err(internal_error("Google 사용자 정보 조회 실패"));
    }

    let info: GoogleUserInfoResponse = user_resp.json().await?;

    Ok(OAuthUserInfo {
        provider: OAuthProvider::Google,
        oauth_id: info.id,
        email: info.email,
        name: info.name,
        profile_image: info.picture,
    })
}
