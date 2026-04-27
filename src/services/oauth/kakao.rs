use serde::Deserialize;

use crate::{
    config::Config,
    models::{internal_error, AppError, OAuthProvider},
    services::auth::OAuthUserInfo,
};

#[derive(Debug, Deserialize)]
struct KakaoTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct KakaoUserInfoResponse {
    id: i64,
    kakao_account: Option<KakaoAccount>,
}

#[derive(Debug, Deserialize)]
struct KakaoAccount {
    email: Option<String>,
    profile: Option<KakaoProfile>,
}

#[derive(Debug, Deserialize)]
struct KakaoProfile {
    nickname: Option<String>,
    profile_image_url: Option<String>,
}

pub async fn fetch_user_info(config: &Config, code: &str) -> Result<OAuthUserInfo, AppError> {
    let http = reqwest::Client::new();

    let token_resp = http
        .post("https://kauth.kakao.com/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", config.oauth.kakao.client_id.as_str()),
            ("client_secret", config.oauth.kakao.client_secret.as_str()),
            ("code", code),
            ("redirect_uri", config.oauth.kakao.redirect_uri.as_str()),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        tracing::error!("Kakao token exchange failed {}: {}", status, body);
        return Err(internal_error("Kakao 인증에 실패했습니다"));
    }

    let token: KakaoTokenResponse = token_resp.json().await?;

    let user_resp = http
        .get("https://kapi.kakao.com/v2/user/me")
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    if !user_resp.status().is_success() {
        let status = user_resp.status();
        let body = user_resp.text().await.unwrap_or_default();
        tracing::error!("Kakao userinfo failed {}: {}", status, body);
        return Err(internal_error("Kakao 사용자 정보 조회 실패"));
    }

    let info: KakaoUserInfoResponse = user_resp.json().await?;

    let account = info.kakao_account.unwrap_or(KakaoAccount {
        email: None,
        profile: None,
    });
    let profile = account.profile.unwrap_or(KakaoProfile {
        nickname: None,
        profile_image_url: None,
    });

    Ok(OAuthUserInfo {
        provider: OAuthProvider::Kakao,
        oauth_id: info.id.to_string(),
        email: account.email.unwrap_or_default(),
        name: profile
            .nickname
            .unwrap_or_else(|| "Kakao User".to_string()),
        profile_image: profile.profile_image_url,
    })
}
