use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Form, Json,
};
use base64::{engine::general_purpose, Engine as _};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::create_jwt,
    models::{
        CreateUserDto, OAuthProvider,
        user::UserStatus,
        email_auth::{
            EmailAuthData, EmailAuthUser, EmailSendRequest, EmailSendResponse,
            EmailStatusResponse, EmailVerifyCodeRequest, EmailVerifyCodeResponse,
            EmailVerifyRequest, EmailVerifyResponse,
        },
    },
    services::discord::DiscordNotifier,
    services::email::EmailService,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub discord: Arc<DiscordNotifier>,
    pub email: Arc<EmailService>,
}

const REACTIVATION_WINDOW_DAYS: i64 = 14;

enum DeletedUserAction {
    Reactivate,
    HardDeleteAndRecreate,
}

fn check_deleted_user(user: &crate::models::User) -> DeletedUserAction {
    let elapsed = Utc::now() - user.updated_at;
    if elapsed.num_days() <= REACTIVATION_WINDOW_DAYS {
        DeletedUserAction::Reactivate
    } else {
        DeletedUserAction::HardDeleteAndRecreate
    }
}

fn extract_client_ip(headers: &HeaderMap) -> String {
    if let Some(forwarded) = headers.get("x-forwarded-for") {
        if let Ok(value) = forwarded.to_str() {
            if let Some(ip) = value.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(value) = real_ip.to_str() {
            return value.trim().to_string();
        }
    }
    "unknown".to_string()
}

// ============================================================================
// Google OAuth
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct OAuthLoginQuery {
    redirect_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GoogleCallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct GoogleUserInfo {
    id: String,
    email: String,
    name: String,
    picture: Option<String>,
}

#[utoipa::path(
    get,
    path = "/auth/google",
    tag = "auth",
    params(
        ("redirect_uri" = Option<String>, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to Google OAuth login page")
    )
)]
pub async fn google_login(
    State(state): State<AppState>,
    Query(query): Query<OAuthLoginQuery>,
) -> impl IntoResponse {
    let frontend_callback = query.redirect_uri.unwrap_or_else(|| {
        format!("{}/auth/callback/google", state.config.server.base_url)
    });

    let client = create_google_oauth_client(&state.config);

    let (auth_url, _csrf_token) = client
        .authorize_url(|| CsrfToken::new(frontend_callback))
        .add_scope(Scope::new("email".to_string()))
        .add_scope(Scope::new("profile".to_string()))
        .url();

    Redirect::temporary(auth_url.as_str())
}

#[utoipa::path(
    get,
    path = "/auth/google/callback",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to frontend with code and state"),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn google_callback(
    State(_state): State<AppState>,
    Query(query): Query<GoogleCallbackQuery>,
) -> impl IntoResponse {
    let frontend_callback = &query.state;

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback,
        query.code,
        query.state
    );

    Redirect::temporary(&redirect_url)
}

/// New endpoint: Frontend calls this with the code to get the token
#[utoipa::path(
    get,
    path = "/auth/callback/google",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "CSRF token state")
    ),
    responses(
        (status = 200, description = "Successful authentication", body = AuthResponse),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn google_callback_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let client_ip = extract_client_ip(&headers);
    let client = create_google_oauth_client(&state.config);

    let token = client
        .exchange_code(AuthorizationCode::new(query.code.clone()))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| {
            tracing::error!("Google OAuth token exchange failed: {:?}", e);
            StatusCode::UNAUTHORIZED
        })?;

    let user_info = fetch_google_user_info(token.access_token().secret())
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch Google user info: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut reactivated = false;
    let mut is_new_user = false;
    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Google,
        &user_info.id,
    )
    .await
    {
        Ok(Some(mut user)) => {
            if user.status == UserStatus::Deleted {
                match check_deleted_user(&user) {
                    DeletedUserAction::Reactivate => {
                        repository::reactivate_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to reactivate user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        user.status = UserStatus::Active;
                        reactivated = true;
                        user
                    }
                    DeletedUserAction::HardDeleteAndRecreate => {
                        repository::hard_delete_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to hard delete user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        let dto = CreateUserDto {
                            oauth_provider: OAuthProvider::Google,
                            oauth_id: user_info.id.clone(),
                            email: user_info.email.clone(),
                            name: user_info.name.clone(),
                            profile_image: user_info.picture.clone(),
                        };
                        let new_user = repository::create_user(&state.db, dto)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to create user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        state.discord.notify_new_user(&new_user.name, &new_user.email, "Google", &client_ip);
                        state.email.send_welcome_email(&new_user.name, &new_user.email);
                        new_user
                    }
                }
            } else if user.status != UserStatus::Active {
                return Err(StatusCode::FORBIDDEN);
            } else {
                user
            }
        }
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Google,
                oauth_id: user_info.id.clone(),
                email: user_info.email.clone(),
                name: user_info.name.clone(),
                profile_image: user_info.picture.clone(),
            };

            let new_user = repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            state.discord.notify_new_user(&new_user.name, &new_user.email, "Google", &client_ip);
            state.email.send_welcome_email(&new_user.name, &new_user.email);
            is_new_user = true;
            new_user
        }
        Err(e) => {
            tracing::error!("Database query failed: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|e| {
        tracing::error!("JWT creation failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AuthResponse {
        token: jwt,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            profile_image: user.profile_image,
        },
        reactivated: if reactivated { Some(true) } else { None },
        is_new_user: if is_new_user { Some(true) } else { None },
    }))
}

fn create_google_oauth_client(config: &Config) -> BasicClient {
    BasicClient::new(
        ClientId::new(config.oauth.google.client_id.clone()),
        Some(ClientSecret::new(
            config.oauth.google.client_secret.clone(),
        )),
        AuthUrl::new("https://accounts.google.com/o/oauth2/v2/auth".to_string()).unwrap(),
        Some(TokenUrl::new("https://oauth2.googleapis.com/token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(config.oauth.google.redirect_uri.clone()).unwrap())
}

async fn fetch_google_user_info(
    access_token: &str,
) -> Result<GoogleUserInfo, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_else(|_| "Unable to read body".to_string());
        return Err(format!("Google API error: {} - {}", status, body).into());
    }

    let user_info = response.json::<GoogleUserInfo>().await?;
    Ok(user_info)
}

// ============================================================================
// Naver OAuth
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct NaverCallbackQuery {
    code: String,
    state: String,
}

// Naver returns expires_in as a string, not a number (non-standard OAuth 2.0)
#[derive(Debug, Deserialize)]
struct NaverTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    refresh_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    #[serde(deserialize_with = "deserialize_expires_in")]
    expires_in: u64,
}

fn deserialize_expires_in<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u64),
    }

    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => s.parse::<u64>().map_err(Error::custom),
        StringOrNumber::Number(n) => Ok(n),
    }
}

#[derive(Debug, Deserialize)]
struct NaverUserInfo {
    response: NaverUserResponse,
}

#[derive(Debug, Deserialize)]
struct NaverUserResponse {
    id: String,
    email: String,
    name: String,
    profile_image: Option<String>,
}

#[utoipa::path(
    get,
    path = "/auth/naver",
    tag = "auth",
    params(
        ("redirect_uri" = Option<String>, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to Naver OAuth login page")
    )
)]
pub async fn naver_login(
    State(state): State<AppState>,
    Query(query): Query<OAuthLoginQuery>,
) -> impl IntoResponse {
    let frontend_callback = query.redirect_uri.unwrap_or_else(|| {
        format!("{}/auth/callback/naver", state.config.server.base_url)
    });

    let client = create_naver_oauth_client(&state.config);

    let (auth_url, _csrf_token) = client
        .authorize_url(|| CsrfToken::new(frontend_callback))
        .url();

    Redirect::temporary(auth_url.as_str())
}

#[utoipa::path(
    get,
    path = "/auth/naver/callback",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to frontend with code and state"),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn naver_callback(
    State(_state): State<AppState>,
    Query(query): Query<NaverCallbackQuery>,
) -> impl IntoResponse {
    let frontend_callback = &query.state;

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback,
        query.code,
        query.state
    );

    Redirect::temporary(&redirect_url)
}

/// New endpoint: Frontend calls this with the code to get the token
#[utoipa::path(
    get,
    path = "/auth/callback/naver",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "CSRF token state")
    ),
    responses(
        (status = 200, description = "Successful authentication", body = AuthResponse),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn naver_callback_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<NaverCallbackQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let client_ip = extract_client_ip(&headers);
    // Exchange code for token (using direct HTTP request because Naver returns expires_in as string)
    let http_client = reqwest::Client::new();
    let token_response = http_client
        .post("https://nid.naver.com/oauth2.0/token")
        .query(&[
            ("grant_type", "authorization_code"),
            ("client_id", &state.config.oauth.naver.client_id),
            ("client_secret", &state.config.oauth.naver.client_secret),
            ("code", &query.code),
            ("state", &query.state),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Naver OAuth token request failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !token_response.status().is_success() {
        let status = token_response.status();
        let body = token_response.text().await.unwrap_or_else(|_| "Unable to read body".to_string());
        tracing::error!("Naver token exchange failed {}: {}", status, body);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body_text = token_response.text().await.map_err(|e| {
        tracing::error!("Failed to read Naver token response: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let naver_token: NaverTokenResponse = serde_json::from_str(&body_text).map_err(|e| {
        tracing::error!("Failed to parse Naver token response: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let user_info = fetch_naver_user_info(&naver_token.access_token)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch Naver user info: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let mut reactivated = false;
    let mut is_new_user = false;
    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Naver,
        &user_info.response.id,
    )
    .await
    {
        Ok(Some(mut user)) => {
            if user.status == UserStatus::Deleted {
                match check_deleted_user(&user) {
                    DeletedUserAction::Reactivate => {
                        repository::reactivate_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to reactivate user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        user.status = UserStatus::Active;
                        reactivated = true;
                        user
                    }
                    DeletedUserAction::HardDeleteAndRecreate => {
                        repository::hard_delete_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to hard delete user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        let dto = CreateUserDto {
                            oauth_provider: OAuthProvider::Naver,
                            oauth_id: user_info.response.id.clone(),
                            email: user_info.response.email.clone(),
                            name: user_info.response.name.clone(),
                            profile_image: user_info.response.profile_image.clone(),
                        };
                        let new_user = repository::create_user(&state.db, dto)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to create user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        state.discord.notify_new_user(&new_user.name, &new_user.email, "Naver", &client_ip);
                        state.email.send_welcome_email(&new_user.name, &new_user.email);
                        new_user
                    }
                }
            } else if user.status != UserStatus::Active {
                return Err(StatusCode::FORBIDDEN);
            } else {
                user
            }
        }
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Naver,
                oauth_id: user_info.response.id.clone(),
                email: user_info.response.email.clone(),
                name: user_info.response.name.clone(),
                profile_image: user_info.response.profile_image.clone(),
            };

            let new_user = repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            state.discord.notify_new_user(&new_user.name, &new_user.email, "Naver", &client_ip);
            state.email.send_welcome_email(&new_user.name, &new_user.email);
            is_new_user = true;
            new_user
        }
        Err(e) => {
            tracing::error!("Database query failed: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|e| {
        tracing::error!("JWT creation failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AuthResponse {
        token: jwt,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            profile_image: user.profile_image,
        },
        reactivated: if reactivated { Some(true) } else { None },
        is_new_user: if is_new_user { Some(true) } else { None },
    }))
}

fn create_naver_oauth_client(config: &Config) -> BasicClient {
    BasicClient::new(
        ClientId::new(config.oauth.naver.client_id.clone()),
        Some(ClientSecret::new(
            config.oauth.naver.client_secret.clone(),
        )),
        AuthUrl::new("https://nid.naver.com/oauth2.0/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://nid.naver.com/oauth2.0/token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(config.oauth.naver.redirect_uri.clone()).unwrap())
}

async fn fetch_naver_user_info(
    access_token: &str,
) -> Result<NaverUserInfo, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://openapi.naver.com/v1/nid/me")
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_else(|_| "Unable to read body".to_string());
        return Err(format!("Naver API error: {} - {}", status, body).into());
    }

    let user_info = response.json::<NaverUserInfo>().await?;
    Ok(user_info)
}

// Kakao OAuth
#[derive(Debug, Deserialize)]
pub struct KakaoCallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct KakaoTokenResponse {
    access_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct KakaoUserInfo {
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

#[utoipa::path(
    get,
    path = "/auth/kakao",
    tag = "auth",
    params(
        ("redirect_uri" = Option<String>, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to Kakao OAuth login page")
    )
)]
pub async fn kakao_login(
    State(state): State<AppState>,
    Query(query): Query<OAuthLoginQuery>,
) -> impl IntoResponse {
    let frontend_callback = query.redirect_uri.unwrap_or_else(|| {
        format!("{}/auth/callback/kakao", state.config.server.base_url)
    });

    let client = create_kakao_oauth_client(&state.config);

    let (auth_url, _csrf_token) = client
        .authorize_url(|| CsrfToken::new(frontend_callback))
        .add_scope(Scope::new("profile_nickname".to_string()))
        .add_scope(Scope::new("profile_image".to_string()))
        .add_scope(Scope::new("account_email".to_string()))
        .url();

    Redirect::temporary(auth_url.as_str())
}

#[utoipa::path(
    get,
    path = "/auth/kakao/callback",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to frontend with code and state"),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn kakao_callback(
    State(_state): State<AppState>,
    Query(query): Query<KakaoCallbackQuery>,
) -> impl IntoResponse {
    let frontend_callback = &query.state;

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback, query.code, query.state
    );

    Redirect::temporary(&redirect_url)
}

/// Frontend calls this with the code to get the token
#[utoipa::path(
    get,
    path = "/auth/callback/kakao",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = String, Query, description = "CSRF token state")
    ),
    responses(
        (status = 200, description = "Successful authentication", body = AuthResponse),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn kakao_callback_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<KakaoCallbackQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let client_ip = extract_client_ip(&headers);
    let http_client = reqwest::Client::new();
    let token_response = http_client
        .post("https://kauth.kakao.com/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", state.config.oauth.kakao.client_id.as_str()),
            ("client_secret", state.config.oauth.kakao.client_secret.as_str()),
            ("code", query.code.as_str()),
            ("redirect_uri", state.config.oauth.kakao.redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Kakao OAuth token request failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !token_response.status().is_success() {
        let status = token_response.status();
        let body = token_response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read body".to_string());
        tracing::error!("Kakao token exchange failed {}: {}", status, body);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body_text = token_response.text().await.map_err(|e| {
        tracing::error!("Failed to read Kakao token response: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let kakao_token: KakaoTokenResponse = serde_json::from_str(&body_text).map_err(|e| {
        tracing::error!("Failed to parse Kakao token response: {} - body: {}", e, body_text);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let user_info = fetch_kakao_user_info(&kakao_token.access_token)
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch Kakao user info: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let kakao_user_id = user_info.id.to_string();
    let account = user_info.kakao_account.unwrap_or(KakaoAccount {
        email: None,
        profile: None,
    });
    let profile = account.profile.unwrap_or(KakaoProfile {
        nickname: None,
        profile_image_url: None,
    });
    let email = account.email.unwrap_or_default();
    let name = profile.nickname.unwrap_or_else(|| "Kakao User".to_string());
    let profile_image = profile.profile_image_url;

    let mut reactivated = false;
    let mut is_new_user = false;
    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Kakao,
        &kakao_user_id,
    )
    .await
    {
        Ok(Some(mut user)) => {
            if user.status == UserStatus::Deleted {
                match check_deleted_user(&user) {
                    DeletedUserAction::Reactivate => {
                        repository::reactivate_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to reactivate user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        user.status = UserStatus::Active;
                        reactivated = true;
                        user
                    }
                    DeletedUserAction::HardDeleteAndRecreate => {
                        repository::hard_delete_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to hard delete user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        let dto = CreateUserDto {
                            oauth_provider: OAuthProvider::Kakao,
                            oauth_id: kakao_user_id,
                            email,
                            name,
                            profile_image,
                        };
                        let new_user = repository::create_user(&state.db, dto)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to create user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        state.discord.notify_new_user(&new_user.name, &new_user.email, "Kakao", &client_ip);
                        state.email.send_welcome_email(&new_user.name, &new_user.email);
                        new_user
                    }
                }
            } else if user.status != UserStatus::Active {
                return Err(StatusCode::FORBIDDEN);
            } else {
                user
            }
        }
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Kakao,
                oauth_id: kakao_user_id,
                email,
                name,
                profile_image,
            };

            let new_user = repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            state.discord.notify_new_user(&new_user.name, &new_user.email, "Kakao", &client_ip);
            state.email.send_welcome_email(&new_user.name, &new_user.email);
            is_new_user = true;
            new_user
        }
        Err(e) => {
            tracing::error!("Database query failed: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|e| {
        tracing::error!("JWT creation failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AuthResponse {
        token: jwt,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            profile_image: user.profile_image,
        },
        reactivated: if reactivated { Some(true) } else { None },
        is_new_user: if is_new_user { Some(true) } else { None },
    }))
}

fn create_kakao_oauth_client(config: &Config) -> BasicClient {
    BasicClient::new(
        ClientId::new(config.oauth.kakao.client_id.clone()),
        Some(ClientSecret::new(
            config.oauth.kakao.client_secret.clone(),
        )),
        AuthUrl::new("https://kauth.kakao.com/oauth/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://kauth.kakao.com/oauth/token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(config.oauth.kakao.redirect_uri.clone()).unwrap())
}

async fn fetch_kakao_user_info(
    access_token: &str,
) -> Result<KakaoUserInfo, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://kapi.kakao.com/v2/user/me")
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read body".to_string());
        return Err(format!("Kakao API error: {} - {}", status, body).into());
    }

    let user_info = response.json::<KakaoUserInfo>().await?;
    Ok(user_info)
}

// Apple OAuth
#[derive(Debug, Deserialize)]
pub struct AppleCallbackForm {
    code: String,
    state: String,
    user: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppleCallbackHandlerQuery {
    code: String,
    #[serde(default)]
    #[allow(dead_code)]
    state: Option<String>,
    apple_user: Option<String>,
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

#[derive(Debug, Deserialize)]
struct AppleTokenResponse {
    #[allow(dead_code)]
    access_token: String,
    id_token: String,
    #[allow(dead_code)]
    token_type: String,
    #[allow(dead_code)]
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct AppleIdTokenClaims {
    sub: String,
    email: Option<String>,
}

#[utoipa::path(
    get,
    path = "/auth/apple",
    tag = "auth",
    params(
        ("redirect_uri" = Option<String>, Query, description = "Frontend callback URL")
    ),
    responses(
        (status = 302, description = "Redirect to Apple OAuth login page")
    )
)]
pub async fn apple_login(
    State(state): State<AppState>,
    Query(query): Query<OAuthLoginQuery>,
) -> impl IntoResponse {
    let frontend_callback = query.redirect_uri.unwrap_or_else(|| {
        format!("{}/auth/callback/apple", state.config.server.base_url)
    });

    let client = create_apple_oauth_client(&state.config);

    let (auth_url, _csrf_token) = client
        .authorize_url(|| CsrfToken::new(frontend_callback))
        .add_scope(Scope::new("name".to_string()))
        .add_scope(Scope::new("email".to_string()))
        .add_extra_param("response_mode", "form_post")
        .url();

    Redirect::temporary(auth_url.as_str())
}

#[utoipa::path(
    post,
    path = "/auth/apple/callback",
    tag = "auth",
    responses(
        (status = 302, description = "Redirect to frontend with code and state"),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn apple_callback(
    State(_state): State<AppState>,
    Form(form): Form<AppleCallbackForm>,
) -> impl IntoResponse {
    let frontend_callback = &form.state;

    let mut redirect_url = format!(
        "{}?code={}&state=apple",
        frontend_callback, form.code
    );

    if let Some(user) = &form.user {
        redirect_url = format!(
            "{}&apple_user={}",
            redirect_url,
            percent_encoding::utf8_percent_encode(user, percent_encoding::NON_ALPHANUMERIC)
        );
    }

    Redirect::to(&redirect_url)
}

/// Frontend calls this with the code to get the token
#[utoipa::path(
    get,
    path = "/auth/callback/apple",
    tag = "auth",
    params(
        ("code" = String, Query, description = "OAuth authorization code"),
        ("state" = Option<String>, Query, description = "CSRF token state"),
        ("apple_user" = Option<String>, Query, description = "Apple user info JSON (first auth only)")
    ),
    responses(
        (status = 200, description = "Successful authentication", body = AuthResponse),
        (status = 401, description = "Authentication failed")
    )
)]
pub async fn apple_callback_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AppleCallbackHandlerQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
    let client_ip = extract_client_ip(&headers);
    let client_secret = generate_apple_client_secret(&state.config).map_err(|e| {
        tracing::error!("Failed to generate Apple client secret: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let http_client = reqwest::Client::new();
    let token_response = http_client
        .post("https://appleid.apple.com/auth/token")
        .form(&[
            ("client_id", state.config.oauth.apple.client_id.as_str()),
            ("client_secret", &client_secret),
            ("code", &query.code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &state.config.oauth.apple.redirect_uri),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Apple OAuth token request failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !token_response.status().is_success() {
        let status = token_response.status();
        let body = token_response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read body".to_string());
        tracing::error!("Apple token exchange failed {}: {}", status, body);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let body_text = token_response.text().await.map_err(|e| {
        tracing::error!("Failed to read Apple token response: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let apple_token: AppleTokenResponse = serde_json::from_str(&body_text).map_err(|e| {
        tracing::error!("Failed to parse Apple token response: {} - body: {}", e, body_text);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let id_token_claims = decode_apple_id_token(&apple_token.id_token).map_err(|e| {
        tracing::error!("Failed to decode Apple id_token: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let apple_user_id = id_token_claims.sub;
    let email = id_token_claims.email.unwrap_or_default();

    let name = query
        .apple_user
        .as_deref()
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
        .unwrap_or_else(|| "Apple User".to_string());

    let mut reactivated = false;
    let mut is_new_user = false;
    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Apple,
        &apple_user_id,
    )
    .await
    {
        Ok(Some(mut user)) => {
            if user.status == UserStatus::Deleted {
                match check_deleted_user(&user) {
                    DeletedUserAction::Reactivate => {
                        repository::reactivate_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to reactivate user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        user.status = UserStatus::Active;
                        reactivated = true;
                        user
                    }
                    DeletedUserAction::HardDeleteAndRecreate => {
                        repository::hard_delete_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to hard delete user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        let dto = CreateUserDto {
                            oauth_provider: OAuthProvider::Apple,
                            oauth_id: apple_user_id,
                            email,
                            name,
                            profile_image: None,
                        };
                        let new_user = repository::create_user(&state.db, dto)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to create user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        state.discord.notify_new_user(&new_user.name, &new_user.email, "Apple", &client_ip);
                        state.email.send_welcome_email(&new_user.name, &new_user.email);
                        new_user
                    }
                }
            } else if user.status != UserStatus::Active {
                return Err(StatusCode::FORBIDDEN);
            } else {
                user
            }
        }
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Apple,
                oauth_id: apple_user_id,
                email,
                name,
                profile_image: None,
            };

            let new_user = repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            state.discord.notify_new_user(&new_user.name, &new_user.email, "Apple", &client_ip);
            state.email.send_welcome_email(&new_user.name, &new_user.email);
            is_new_user = true;
            new_user
        }
        Err(e) => {
            tracing::error!("Database query failed: {:?}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|e| {
        tracing::error!("JWT creation failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(AuthResponse {
        token: jwt,
        user: UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            profile_image: user.profile_image,
        },
        reactivated: if reactivated { Some(true) } else { None },
        is_new_user: if is_new_user { Some(true) } else { None },
    }))
}

fn create_apple_oauth_client(config: &Config) -> BasicClient {
    BasicClient::new(
        ClientId::new(config.oauth.apple.client_id.clone()),
        None,
        AuthUrl::new("https://appleid.apple.com/auth/authorize".to_string()).unwrap(),
        Some(TokenUrl::new("https://appleid.apple.com/auth/token".to_string()).unwrap()),
    )
    .set_redirect_uri(RedirectUrl::new(config.oauth.apple.redirect_uri.clone()).unwrap())
}

fn generate_apple_client_secret(
    config: &Config,
) -> Result<String, Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().timestamp() as usize;
    let exp = now + (86400 * 180); // 6 months

    #[derive(Serialize)]
    struct AppleClientSecretClaims {
        iss: String,
        iat: usize,
        exp: usize,
        aud: String,
        sub: String,
    }

    let claims = AppleClientSecretClaims {
        iss: config.oauth.apple.team_id.clone(),
        iat: now,
        exp,
        aud: "https://appleid.apple.com".to_string(),
        sub: config.oauth.apple.client_id.clone(),
    };

    let header = jsonwebtoken::Header {
        alg: jsonwebtoken::Algorithm::ES256,
        kid: Some(config.oauth.apple.key_id.clone()),
        ..Default::default()
    };

    let key = jsonwebtoken::EncodingKey::from_ec_pem(config.oauth.apple.private_key.as_bytes())?;
    let token = jsonwebtoken::encode(&header, &claims, &key)?;
    Ok(token)
}

fn decode_apple_id_token(
    id_token: &str,
) -> Result<AppleIdTokenClaims, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err("Invalid id_token format".into());
    }

    let payload = general_purpose::URL_SAFE_NO_PAD.decode(parts[1])?;
    let claims: AppleIdTokenClaims = serde_json::from_slice(&payload)?;
    Ok(claims)
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
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
}

// ============================================================================
// Email Magic Link Auth
// ============================================================================

fn extract_accept_language(headers: &HeaderMap) -> String {
    headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.split(';').next().unwrap_or("ko").trim())
        .map(|lang| match lang {
            l if l.starts_with("en") => "en",
            l if l.starts_with("ja") => "ja",
            l if l.starts_with("zh-TW") || l.starts_with("zh-Hant") => "zh-TW",
            l if l.starts_with("zh") => "zh-CN",
            _ => "ko",
        })
        .unwrap_or("ko")
        .to_string()
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    parts.len() == 2
        && !parts[0].is_empty()
        && parts[1].contains('.')
        && parts[1].len() > 2
}

async fn find_or_create_email_user(
    state: &AppState,
    email: &str,
    client_ip: &str,
) -> Result<(crate::models::User, Option<String>), StatusCode> {
    match repository::find_user_by_email(&state.db, email).await {
        Ok(Some(mut user)) => {
            if user.status == UserStatus::Deleted {
                match check_deleted_user(&user) {
                    DeletedUserAction::Reactivate => {
                        repository::reactivate_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to reactivate user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        user.status = UserStatus::Active;
                    }
                    DeletedUserAction::HardDeleteAndRecreate => {
                        repository::hard_delete_user(&state.db, &user.id)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to hard delete user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        let dto = CreateUserDto {
                            oauth_provider: OAuthProvider::Email,
                            oauth_id: email.to_string(),
                            email: email.to_string(),
                            name: email.split('@').next().unwrap_or("User").to_string(),
                            profile_image: None,
                        };
                        let new_user = repository::create_user(&state.db, dto)
                            .await
                            .map_err(|e| {
                                tracing::error!("Failed to create email user: {:?}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })?;
                        state.discord.notify_new_user(&new_user.name, &new_user.email, "Email", client_ip);
                        state.email.send_welcome_email(&new_user.name, &new_user.email);
                        return Ok((new_user, None));
                    }
                }
            } else if user.status != UserStatus::Active {
                return Err(StatusCode::FORBIDDEN);
            }
            let existing = if user.oauth_provider != OAuthProvider::Email {
                Some(user.oauth_provider.to_string())
            } else {
                None
            };
            Ok((user, existing))
        }
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Email,
                oauth_id: email.to_string(),
                email: email.to_string(),
                name: email.split('@').next().unwrap_or("User").to_string(),
                profile_image: None,
            };
            let new_user = repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create email user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            state.discord.notify_new_user(&new_user.name, &new_user.email, "Email", client_ip);
            state.email.send_welcome_email(&new_user.name, &new_user.email);
            Ok((new_user, None))
        }
        Err(e) => {
            tracing::error!("DB error finding user by email: {:?}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn build_email_auth_data(
    user: &crate::models::User,
    jwt: String,
    existing_provider: Option<String>,
) -> EmailAuthData {
    EmailAuthData {
        token: jwt,
        user: EmailAuthUser {
            id: user.id.clone(),
            email: user.email.clone(),
            name: user.name.clone(),
            profile_image: user.profile_image.clone(),
        },
        existing_provider,
    }
}

pub async fn email_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailSendRequest>,
) -> Result<Json<EmailSendResponse>, StatusCode> {
    let email = body.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client_ip = extract_client_ip(&headers);
    let lang = extract_accept_language(&headers);

    if let Ok(Some(_)) = repository::find_recent_email_auth_session(&state.db, &email).await {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let token = uuid::Uuid::new_v4().to_string();
    let code = {
        let mut rng = rand::thread_rng();
        format!("{:06}", rand::Rng::gen_range(&mut rng, 0..1_000_000u32))
    };
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(10);

    repository::create_email_auth_session(
        &state.db,
        &session_id,
        &email,
        &token,
        &code,
        &client_ip,
        &body.device_id,
        expires_at,
    )
    .await
    .map_err(|e| {
        tracing::error!("Failed to create email auth session: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    state.email.send_magic_link_email(&email, &token, &lang);

    Ok(Json(EmailSendResponse {
        session_id,
    }))
}

pub async fn email_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailVerifyRequest>,
) -> Result<Json<EmailVerifyResponse>, StatusCode> {
    let session = repository::find_email_auth_session_by_token(&state.db, &body.token)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.status != "pending" {
        return Err(StatusCode::GONE);
    }

    let same_device = body
        .device_id
        .as_ref()
        .map_or(false, |did| !did.is_empty() && did == &session.device_id);

    let client_ip = extract_client_ip(&headers);

    if same_device {
        repository::update_email_auth_session_status(&state.db, &session.id, "completed")
            .await
            .map_err(|e| {
                tracing::error!("DB error: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        let (user, existing_provider) =
            find_or_create_email_user(&state, &session.email, &client_ip).await?;

        let jwt = create_jwt(
            &user.id,
            &user.email,
            &user.name,
            &state.config.jwt.secret,
            state.config.jwt.expiration_hours,
        )
        .map_err(|e| {
            tracing::error!("JWT creation failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        Ok(Json(EmailVerifyResponse {
            same_device: true,
            auth: Some(build_email_auth_data(&user, jwt, existing_provider)),
            verification_code: None,
        }))
    } else {
        repository::update_email_auth_session_status(&state.db, &session.id, "verified")
            .await
            .map_err(|e| {
                tracing::error!("DB error: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        Ok(Json(EmailVerifyResponse {
            same_device: false,
            auth: None,
            verification_code: Some(session.verification_code.clone()),
        }))
    }
}

pub async fn email_verify_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailVerifyCodeRequest>,
) -> Result<Json<EmailVerifyCodeResponse>, StatusCode> {
    let session = repository::find_email_auth_session_by_id(&state.db, &body.session_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.expires_at < chrono::Utc::now() {
        return Err(StatusCode::GONE);
    }

    if session.verification_code != body.code {
        return Err(StatusCode::UNAUTHORIZED);
    }

    repository::update_email_auth_session_status(&state.db, &session.id, "completed")
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let client_ip = extract_client_ip(&headers);
    let (user, existing_provider) =
        find_or_create_email_user(&state, &session.email, &client_ip).await?;

    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|e| {
        tracing::error!("JWT creation failed: {:?}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(EmailVerifyCodeResponse {
        token: jwt,
        user: EmailAuthUser {
            id: user.id,
            email: user.email,
            name: user.name,
            profile_image: user.profile_image,
        },
        existing_provider,
    }))
}

pub async fn email_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<EmailStatusResponse>, StatusCode> {
    let session = repository::find_email_auth_session_by_id(&state.db, &session_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.status == "completed" {
        let (user, existing_provider) =
            find_or_create_email_user(&state, &session.email, "polling").await?;

        let jwt = create_jwt(
            &user.id,
            &user.email,
            &user.name,
            &state.config.jwt.secret,
            state.config.jwt.expiration_hours,
        )
        .map_err(|e| {
            tracing::error!("JWT creation failed: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        Ok(Json(EmailStatusResponse {
            status: "completed".to_string(),
            auth: Some(build_email_auth_data(&user, jwt, existing_provider)),
        }))
    } else {
        Ok(Json(EmailStatusResponse {
            status: session.status,
            auth: None,
        }))
    }
}
