use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect},
    Form, Json,
};
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope, TokenUrl,
};
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    models::{
        AppError,
        email_auth::{
            EmailAuthData, EmailAuthUser, EmailSendRequest, EmailSendResponse,
            EmailStatusResponse, EmailVerifyCodeRequest, EmailVerifyCodeResponse,
            EmailVerifyRequest, EmailVerifyResponse,
        },
        OAuthLoginQuery, GoogleCallbackQuery, NaverCallbackQuery, KakaoCallbackQuery,
        AppleCallbackForm, AppleCallbackHandlerQuery, AuthResponse,
    },
    services::auth::AuthService,
    services::email::EmailService,
    services::oauth,
};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub email: Arc<EmailService>,
    pub auth: Arc<AuthService>,
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
) -> Result<Json<AuthResponse>, AppError> {
    let client_ip = extract_client_ip(&headers);
    let info = oauth::google::fetch_user_info(&state.config, &query.code).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip).await?;
    let jwt = state.auth.issue_jwt(&outcome.user)?;
    Ok(Json(state.auth.build_response(outcome, jwt)))
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

// ============================================================================
// Naver OAuth
// ============================================================================

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
) -> Result<Json<AuthResponse>, AppError> {
    let client_ip = extract_client_ip(&headers);
    let info = oauth::naver::fetch_user_info(&state.config, &query.code, &query.state).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip).await?;
    let jwt = state.auth.issue_jwt(&outcome.user)?;
    Ok(Json(state.auth.build_response(outcome, jwt)))
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

// Kakao OAuth

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
) -> Result<Json<AuthResponse>, AppError> {
    let client_ip = extract_client_ip(&headers);
    let info = oauth::kakao::fetch_user_info(&state.config, &query.code).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip).await?;
    let jwt = state.auth.issue_jwt(&outcome.user)?;
    Ok(Json(state.auth.build_response(outcome, jwt)))
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

// Apple OAuth
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
) -> Result<Json<AuthResponse>, AppError> {
    let client_ip = extract_client_ip(&headers);
    let info = oauth::apple::fetch_user_info(
        &state.config,
        &query.code,
        query.apple_user.as_deref(),
    )
    .await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip).await?;
    let jwt = state.auth.issue_jwt(&outcome.user)?;
    Ok(Json(state.auth.build_response(outcome, jwt)))
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
) -> Result<Json<EmailSendResponse>, AppError> {
    let email = body.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let client_ip = extract_client_ip(&headers);
    let lang = extract_accept_language(&headers);

    if let Ok(Some(_)) = repository::find_recent_email_auth_session(&state.db, &email).await {
        return Err(StatusCode::TOO_MANY_REQUESTS.into());
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
) -> Result<Json<EmailVerifyResponse>, AppError> {
    let session = repository::find_email_auth_session_by_token(&state.db, &body.token)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.status != "pending" {
        return Err(StatusCode::GONE.into());
    }

    let same_device = body
        .device_id
        .as_ref()
        .map_or(false, |did| !did.is_empty() && did == &session.device_id);

    let client_ip = extract_client_ip(&headers);

    if same_device {
        repository::update_email_auth_session_status(&state.db, &session.id, "completed").await?;

        let (outcome, existing_provider) = state
            .auth
            .upsert_email_user(&session.email, &client_ip)
            .await?;
        let jwt = state.auth.issue_jwt(&outcome.user)?;

        Ok(Json(EmailVerifyResponse {
            same_device: true,
            auth: Some(build_email_auth_data(&outcome.user, jwt, existing_provider)),
            verification_code: None,
        }))
    } else {
        repository::update_email_auth_session_status(&state.db, &session.id, "verified").await?;

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
) -> Result<Json<EmailVerifyCodeResponse>, AppError> {
    let session = repository::find_email_auth_session_by_id(&state.db, &body.session_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.expires_at < chrono::Utc::now() {
        return Err(StatusCode::GONE.into());
    }

    if session.verification_code != body.code {
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    repository::update_email_auth_session_status(&state.db, &session.id, "completed").await?;

    let client_ip = extract_client_ip(&headers);
    let (outcome, existing_provider) = state
        .auth
        .upsert_email_user(&session.email, &client_ip)
        .await?;
    let jwt = state.auth.issue_jwt(&outcome.user)?;

    Ok(Json(EmailVerifyCodeResponse {
        token: jwt,
        user: EmailAuthUser {
            id: outcome.user.id,
            email: outcome.user.email,
            name: outcome.user.name,
            profile_image: outcome.user.profile_image,
        },
        existing_provider,
    }))
}

pub async fn email_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<EmailStatusResponse>, AppError> {
    let session = repository::find_email_auth_session_by_id(&state.db, &session_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    if session.status == "completed" {
        let (outcome, existing_provider) = state
            .auth
            .upsert_email_user(&session.email, "polling")
            .await?;
        let jwt = state.auth.issue_jwt(&outcome.user)?;

        Ok(Json(EmailStatusResponse {
            status: "completed".to_string(),
            auth: Some(build_email_auth_data(&outcome.user, jwt, existing_provider)),
        }))
    } else {
        Ok(Json(EmailStatusResponse {
            status: session.status,
            auth: None,
        }))
    }
}
