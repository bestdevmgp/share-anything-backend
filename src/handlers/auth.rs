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
    // Store frontend redirect_uri in state parameter (we'll use it in callback)
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
    State(state): State<AppState>,
    Query(query): Query<GoogleCallbackQuery>,
) -> impl IntoResponse {
    // The 'state' parameter contains the frontend callback URL
    let frontend_callback = query.state;

    // Redirect to frontend with the authorization code
    let redirect_url = format!(
        "{}?code={}&state=google",
        frontend_callback,
        query.code
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

    // Exchange code for token
    let token = client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Fetch user info from Google
    let user_info = fetch_google_user_info(token.access_token().secret())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find or create user
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
                oauth_id: user_info.id,
                email: user_info.email,
                name: user_info.name,
                profile_image: user_info.picture,
            };

            repository::create_user(&state.db, dto)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        }
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Create JWT
    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
        .await?
        .json::<GoogleUserInfo>()
        .await?;

    Ok(response)
}

// ============================================================================
// Naver OAuth
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct NaverCallbackQuery {
    code: String,
    state: String,
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
    // Store frontend redirect_uri in state parameter
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
    State(state): State<AppState>,
    Query(query): Query<NaverCallbackQuery>,
) -> impl IntoResponse {
    // The 'state' parameter contains the frontend callback URL
    let frontend_callback = query.state;

    // Redirect to frontend with the authorization code
    let redirect_url = format!(
        "{}?code={}&state=naver",
        frontend_callback,
        query.code
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
    let client = create_naver_oauth_client(&state.config);

    // Exchange code for token
    let token = client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Fetch user info from Naver
    let user_info = fetch_naver_user_info(token.access_token().secret())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find or create user
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
                oauth_id: user_info.response.id,
                email: user_info.response.email,
                name: user_info.response.name,
                profile_image: user_info.response.profile_image,
            };

            repository::create_user(&state.db, dto)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        }
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    // Create JWT
    let jwt = create_jwt(
        &user.id,
        &user.email,
        &user.name,
        &state.config.jwt.secret,
        state.config.jwt.expiration_hours,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
        .await?
        .json::<NaverUserInfo>()
        .await?;

    Ok(response)
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
