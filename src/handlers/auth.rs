use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
    Json,
};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::create_jwt,
    models::{CreateUserDto, OAuthProvider},
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
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
    Query(query): Query<GoogleCallbackQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
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

    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Google,
        &user_info.id,
    )
    .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Google,
                oauth_id: user_info.id.clone(),
                email: user_info.email.clone(),
                name: user_info.name.clone(),
                profile_image: user_info.picture.clone(),
            };

            repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
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
    Query(query): Query<NaverCallbackQuery>,
) -> Result<Json<AuthResponse>, StatusCode> {
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

    let user = match repository::find_user_by_oauth(
        &state.db,
        &OAuthProvider::Naver,
        &user_info.response.id,
    )
    .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            let dto = CreateUserDto {
                oauth_provider: OAuthProvider::Naver,
                oauth_id: user_info.response.id.clone(),
                email: user_info.response.email.clone(),
                name: user_info.response.name.clone(),
                profile_image: user_info.response.profile_image.clone(),
            };

            repository::create_user(&state.db, dto)
                .await
                .map_err(|e| {
                    tracing::error!("Failed to create user: {:?}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?
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

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponse,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    pub id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
}
