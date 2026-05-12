use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    models::{internal_error, AppError, OAuthProvider},
    services::auth::OAuthUserInfo,
};

#[derive(Debug, Deserialize)]
struct AppleTokenResponse {
    id_token: String,
}

#[derive(Debug, Deserialize)]
struct AppleIdTokenClaims {
    sub: String,
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppleUserFormInfo {
    name: Option<AppleNameInfo>,
}

#[derive(Debug, Deserialize)]
struct AppleNameInfo {
    #[serde(rename = "firstName")]
    first_name: Option<String>,
    #[serde(rename = "lastName")]
    last_name: Option<String>,
}

#[derive(Serialize)]
struct AppleClientSecretClaims {
    iss: String,
    iat: usize,
    exp: usize,
    aud: String,
    sub: String,
}

const APPLE_CLIENT_SECRET_TTL_SECS: usize = 86400 * 180;

fn generate_client_secret(config: &Config) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp() as usize;
    let claims = AppleClientSecretClaims {
        iss: config.oauth.apple.team_id.clone(),
        iat: now,
        exp: now + APPLE_CLIENT_SECRET_TTL_SECS,
        aud: "https://appleid.apple.com".to_string(),
        sub: config.oauth.apple.client_id.clone(),
    };

    let header = jsonwebtoken::Header {
        alg: jsonwebtoken::Algorithm::ES256,
        kid: Some(config.oauth.apple.key_id.clone()),
        ..Default::default()
    };

    let key = jsonwebtoken::EncodingKey::from_ec_pem(config.oauth.apple.private_key.as_bytes())
        .map_err(|e| {
            tracing::error!(error = ?e, "Failed to load Apple private key");
            internal_error("Apple client_secret 키 로드 실패")
        })?;

    jsonwebtoken::encode(&header, &claims, &key).map_err(|e| {
        tracing::error!(error = ?e, "Failed to sign Apple client secret");
        internal_error("Apple client_secret 서명 실패")
    })
}

fn decode_id_token(id_token: &str) -> Result<AppleIdTokenClaims, AppError> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err(internal_error("Apple id_token 형식이 잘못되었습니다"));
    }

    let payload = general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|e| {
            tracing::error!(error = ?e, "Failed to base64 decode Apple id_token payload");
            internal_error("Apple id_token 디코딩 실패")
        })?;

    serde_json::from_slice(&payload).map_err(|e| {
        tracing::error!(error = ?e, "Failed to parse Apple id_token claims");
        internal_error("Apple id_token 파싱 실패")
    })
}

fn extract_name(apple_user: Option<&str>) -> String {
    apple_user
        .and_then(|user_str| serde_json::from_str::<AppleUserFormInfo>(user_str).ok())
        .and_then(|info| info.name)
        .map(|name_info| {
            let first = name_info.first_name.unwrap_or_default();
            let last = name_info.last_name.unwrap_or_default();
            let full = format!("{} {}", last, first).trim().to_string();
            if full.is_empty() {
                "Apple User".to_string()
            } else {
                full
            }
        })
        .unwrap_or_else(|| "Apple User".to_string())
}

pub async fn fetch_user_info(
    config: &Config,
    code: &str,
    apple_user_form: Option<&str>,
) -> Result<OAuthUserInfo, AppError> {
    let client_secret = generate_client_secret(config)?;

    let http = reqwest::Client::new();
    let token_resp = http
        .post("https://appleid.apple.com/auth/token")
        .form(&[
            ("client_id", config.oauth.apple.client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", config.oauth.apple.redirect_uri.as_str()),
        ])
        .send()
        .await?;

    if !token_resp.status().is_success() {
        let status = token_resp.status();
        let body = token_resp.text().await.unwrap_or_default();
        tracing::error!("Apple token exchange failed {}: {}", status, body);
        return Err(internal_error("Apple 인증에 실패했습니다"));
    }

    let token: AppleTokenResponse = token_resp.json().await?;
    let claims = decode_id_token(&token.id_token)?;

    Ok(OAuthUserInfo {
        provider: OAuthProvider::Apple,
        oauth_id: claims.sub,
        email: claims.email.unwrap_or_default(),
        name: extract_name(apple_user_form),
        profile_image: None,
    })
}
