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
        AppleCallbackForm, AppleCallbackHandlerQuery,
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

/// Origins we are willing to hand an OAuth authorization `code` to: the CORS
/// allowlist plus our own frontend and backend URLs.
fn allowed_redirect_origins(config: &Config) -> Vec<String> {
    let mut origins = config.cors.allowed_origins.clone();
    origins.push(config.server.frontend_url.clone());
    origins.push(config.server.base_url.clone());
    origins
}

/// True when `candidate` is an absolute URL whose origin (scheme+host+port)
/// matches one of `allowed`. Opaque/relative/non-http origins never match.
///
/// This is the gate that stops an attacker-supplied OAuth `redirect_uri` /
/// `state` from redirecting the authorization code (and therefore the issued
/// session) to a host they control.
fn redirect_origin_allowed(candidate: &str, allowed: &[String]) -> bool {
    use oauth2::url::Url;
    let Ok(candidate_url) = Url::parse(candidate) else {
        return false;
    };
    let candidate_origin = candidate_url.origin();
    if !candidate_origin.is_tuple() {
        return false;
    }
    allowed.iter().any(|a| {
        Url::parse(a)
            .map(|u| u.origin() == candidate_origin)
            .unwrap_or(false)
    })
}

/// Resolve the frontend callback used to start an OAuth login. The client's
/// requested `redirect_uri` is honored only when its origin is allowlisted;
/// otherwise we fall back to this server's own default callback so a crafted
/// `redirect_uri` cannot be reflected back by the provider.
fn resolve_login_redirect(config: &Config, provider: &str, requested: Option<String>) -> String {
    let default = format!("{}/auth/callback/{}", config.server.base_url, provider);
    match requested {
        Some(uri) if redirect_origin_allowed(&uri, &allowed_redirect_origins(config)) => uri,
        _ => default,
    }
}

/// Sanitize the `state` echoed back on an OAuth callback before using it as a
/// redirect target. Only allowlisted origins are used verbatim; anything else
/// falls back to the frontend URL, so the callback cannot be abused as an open
/// redirect that leaks the authorization code to an arbitrary host.
fn safe_callback_target(config: &Config, state: &str) -> String {
    if redirect_origin_allowed(state, &allowed_redirect_origins(config)) {
        state.to_string()
    } else {
        config.server.frontend_url.clone()
    }
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
    let frontend_callback =
        resolve_login_redirect(&state.config, "google", query.redirect_uri);

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
    let frontend_callback = safe_callback_target(&state.config, &query.state);

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback,
        query.code,
        query.state
    );

    Redirect::temporary(&redirect_url)
}

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
) -> Result<impl IntoResponse, AppError> {
    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);
    let info = oauth::google::fetch_user_info(&state.config, &query.code).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip, &welcome_lang).await?;
    let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;
    let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(state.auth.build_response(outcome)),
    ))
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
    let frontend_callback =
        resolve_login_redirect(&state.config, "naver", query.redirect_uri);

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
    let frontend_callback = safe_callback_target(&state.config, &query.state);

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback,
        query.code,
        query.state
    );

    Redirect::temporary(&redirect_url)
}

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
) -> Result<impl IntoResponse, AppError> {
    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);
    let info = oauth::naver::fetch_user_info(&state.config, &query.code, &query.state).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip, &welcome_lang).await?;
    let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;
    let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(state.auth.build_response(outcome)),
    ))
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
    let frontend_callback =
        resolve_login_redirect(&state.config, "kakao", query.redirect_uri);

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
    State(state): State<AppState>,
    Query(query): Query<KakaoCallbackQuery>,
) -> impl IntoResponse {
    let frontend_callback = safe_callback_target(&state.config, &query.state);

    let redirect_url = format!(
        "{}?code={}&state={}",
        frontend_callback, query.code, query.state
    );

    Redirect::temporary(&redirect_url)
}

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
) -> Result<impl IntoResponse, AppError> {
    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);
    let info = oauth::kakao::fetch_user_info(&state.config, &query.code).await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip, &welcome_lang).await?;
    let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;
    let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(state.auth.build_response(outcome)),
    ))
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
    let frontend_callback =
        resolve_login_redirect(&state.config, "apple", query.redirect_uri);

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
    State(state): State<AppState>,
    Form(form): Form<AppleCallbackForm>,
) -> impl IntoResponse {
    let frontend_callback = safe_callback_target(&state.config, &form.state);

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
) -> Result<impl IntoResponse, AppError> {
    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);
    let info = oauth::apple::fetch_user_info(
        &state.config,
        &query.code,
        query.apple_user.as_deref(),
    )
    .await?;
    let outcome = state.auth.upsert_oauth_user(info, &client_ip, &welcome_lang).await?;
    let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;
    let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(state.auth.build_response(outcome)),
    ))
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

fn detect_browser_language(headers: &HeaderMap) -> Option<&'static str> {
    let lang = headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.split(';').next().unwrap_or("").trim().to_string())?;

    match lang.as_str() {
        l if l.starts_with("en") => Some("en"),
        l if l.starts_with("ja") => Some("ja"),
        l if l.starts_with("zh-TW") || l.starts_with("zh-Hant") => Some("zh-TW"),
        l if l.starts_with("zh") => Some("zh-CN"),
        l if l.starts_with("ko") => Some("ko"),
        _ => None,
    }
}

fn extract_accept_language(headers: &HeaderMap) -> String {
    detect_browser_language(headers).unwrap_or("ko").to_string()
}

fn welcome_email_language(headers: &HeaderMap) -> String {
    detect_browser_language(headers).unwrap_or("en").to_string()
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
    existing_provider: Option<String>,
) -> EmailAuthData {
    EmailAuthData {
        user: EmailAuthUser {
            id: user.id.clone(),
            email: user.email.clone(),
            name: user.name.clone(),
            profile_image: user.profile_image.clone(),
            oauth_provider: user.oauth_provider.to_string(),
        },
        existing_provider,
    }
}

/// Wrong-code attempts allowed per email-auth session before it is refused.
const MAX_EMAIL_CODE_ATTEMPTS: u32 = 5;

type CodeFailureMap = dashmap::DashMap<String, (u32, std::time::Instant)>;

/// Per-session wrong-code counters. Process-local on purpose: a schema column
/// would not reach production (migrations only run on a fresh database), and
/// sessions live for 10 minutes, so losing counters on restart is acceptable.
fn email_code_failures() -> &'static CodeFailureMap {
    static FAILURES: std::sync::OnceLock<CodeFailureMap> = std::sync::OnceLock::new();
    FAILURES.get_or_init(dashmap::DashMap::new)
}

fn email_code_attempts_exceeded(session_id: &str) -> bool {
    email_code_failures()
        .get(session_id)
        .map(|entry| entry.0 >= MAX_EMAIL_CODE_ATTEMPTS)
        .unwrap_or(false)
}

fn record_email_code_failure(session_id: &str) {
    let failures = email_code_failures();
    if failures.len() > 10_000 {
        // Sessions expire after 10 minutes; drop stale counters so the map
        // stays bounded.
        let ttl = std::time::Duration::from_secs(15 * 60);
        failures.retain(|_, (_, first_failure)| first_failure.elapsed() < ttl);
    }
    failures
        .entry(session_id.to_string())
        .and_modify(|(count, _)| *count += 1)
        .or_insert((1, std::time::Instant::now()));
}

fn clear_email_code_failures(session_id: &str) {
    email_code_failures().remove(session_id);
}

/// Send a magic-link authentication email.
#[utoipa::path(
    post,
    path = "/auth/email/send",
    tag = "email-auth",
    request_body = EmailSendRequest,
    responses(
        (status = 200, description = "Magic link sent", body = EmailSendResponse),
        (status = 400, description = "Invalid email"),
        (status = 429, description = "Rate limited — recent session already exists")
    )
)]
pub async fn email_send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailSendRequest>,
) -> Result<Json<EmailSendResponse>, AppError> {
    let email = body.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        return Err(StatusCode::BAD_REQUEST.into());
    }

    let client_ip = crate::utils::client_ip(&headers);
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

/// Verify a magic-link token from the email.
///
/// If the request comes from the same device that initiated the flow, returns an auth token directly.
/// Otherwise marks the session as verified and returns a verification code.
#[utoipa::path(
    post,
    path = "/auth/email/verify",
    tag = "email-auth",
    request_body = EmailVerifyRequest,
    responses(
        (status = 200, description = "Token verified", body = EmailVerifyResponse),
        (status = 404, description = "Session not found"),
        (status = 410, description = "Session already used or expired")
    )
)]
pub async fn email_verify(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailVerifyRequest>,
) -> Result<axum::response::Response, AppError> {
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

    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);

    if same_device {
        // Conditional transition: a replayed or concurrent click of the same
        // magic link loses here instead of minting a second login.
        if repository::transition_email_auth_session_status(&state.db, &session.id, "pending", "completed").await? == 0 {
            return Err(StatusCode::GONE.into());
        }

        let (outcome, existing_provider) = state
            .auth
            .upsert_email_user(&session.email, &client_ip, &welcome_lang)
            .await?;
        let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;

        let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
        Ok((
            [(axum::http::header::SET_COOKIE, cookie)],
            Json(EmailVerifyResponse {
                same_device: true,
                auth: Some(build_email_auth_data(&outcome.user, existing_provider)),
                verification_code: None,
            }),
        )
            .into_response())
    } else {
        if repository::transition_email_auth_session_status(&state.db, &session.id, "pending", "verified").await? == 0 {
            return Err(StatusCode::GONE.into());
        }

        Ok(Json(EmailVerifyResponse {
            same_device: false,
            auth: None,
            verification_code: Some(session.verification_code.clone()),
        })
        .into_response())
    }
}

/// Verify the 6-digit code shown after magic-link verification on a different device.
#[utoipa::path(
    post,
    path = "/auth/email/verify-code",
    tag = "email-auth",
    request_body = EmailVerifyCodeRequest,
    responses(
        (status = 200, description = "Code verified — returns auth token", body = EmailVerifyCodeResponse),
        (status = 401, description = "Incorrect code"),
        (status = 404, description = "Session not found"),
        (status = 410, description = "Session expired")
    )
)]
pub async fn email_verify_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<EmailVerifyCodeRequest>,
) -> Result<impl IntoResponse, AppError> {
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

    // The code is only ever revealed by /auth/email/verify after the magic
    // link (proof of inbox access) was opened, which moves the session to
    // `verified`. Accepting it in any other state would let anyone holding a
    // session_id guess the 6-digit code without touching the inbox. `pending`
    // answers exactly like a wrong code so it is not an oracle.
    match session.status.as_str() {
        "verified" => {}
        "completed" => return Err(StatusCode::GONE.into()),
        _ => return Err(StatusCode::UNAUTHORIZED.into()),
    }

    if email_code_attempts_exceeded(&session.id) {
        return Err(StatusCode::GONE.into());
    }

    if session.verification_code != body.code {
        record_email_code_failure(&session.id);
        return Err(StatusCode::UNAUTHORIZED.into());
    }

    if repository::transition_email_auth_session_status(&state.db, &session.id, "verified", "completed").await? == 0 {
        return Err(StatusCode::GONE.into());
    }
    clear_email_code_failures(&session.id);

    let client_ip = crate::utils::client_ip(&headers);
    let welcome_lang = welcome_email_language(&headers);
    let (outcome, existing_provider) = state
        .auth
        .upsert_email_user(&session.email, &client_ip, &welcome_lang)
        .await?;
    let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;

    let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
    Ok((
        [(axum::http::header::SET_COOKIE, cookie)],
        Json(EmailVerifyCodeResponse {
            user: EmailAuthUser {
                id: outcome.user.id,
                email: outcome.user.email,
                name: outcome.user.name,
                profile_image: outcome.user.profile_image,
                oauth_provider: outcome.user.oauth_provider.to_string(),
            },
            existing_provider,
        }),
    ))
}

/// Poll the status of an email auth session.
///
/// Returns the current status (`pending`, `verified`, `completed`) and an auth token once completed.
#[utoipa::path(
    get,
    path = "/auth/email/status/{session_id}",
    tag = "email-auth",
    params(
        ("session_id" = String, Path, description = "Email auth session ID")
    ),
    responses(
        (status = 200, description = "Session status", body = EmailStatusResponse),
        (status = 404, description = "Session not found")
    )
)]
pub async fn email_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Result<axum::response::Response, AppError> {
    let session = repository::find_email_auth_session_by_id(&state.db, &session_id)
        .await
        .map_err(|e| {
            tracing::error!("DB error: {:?}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // A completed email-auth session must not remain a reusable credential
    // after its TTL. Without this check the `session_id` could be replayed to
    // mint fresh login sessions indefinitely (the row is otherwise never
    // consumed on this path).
    if session.expires_at < chrono::Utc::now() {
        return Ok(Json(EmailStatusResponse {
            status: "expired".to_string(),
            auth: None,
        })
        .into_response());
    }

    if session.status == "completed" {
        let (outcome, existing_provider) = state
            .auth
            .upsert_email_user(&session.email, "polling", "en")
            .await?;
        let jwt = state.auth.create_session_token(&outcome.user, outcome.is_new_user, &headers).await?;

        let cookie = crate::utils::auth_cookie::build_auth_cookie(&jwt, state.config.jwt.expiration_hours * 3600);
        Ok((
            [(axum::http::header::SET_COOKIE, cookie)],
            Json(EmailStatusResponse {
                status: "completed".to_string(),
                auth: Some(build_email_auth_data(&outcome.user, existing_provider)),
            }),
        )
            .into_response())
    } else {
        Ok(Json(EmailStatusResponse {
            status: session.status,
            auth: None,
        })
        .into_response())
    }
}

#[derive(serde::Serialize, utoipa::ToSchema)]
pub struct MeResponse {
    pub user: Option<crate::models::auth::UserResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Current user, or null with a reason when not authenticated", body = MeResponse)
    ),
    security(("cookie_auth" = []))
)]
pub async fn get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<MeResponse>, AppError> {
    if let Some(axum::Extension(c)) = claims {
        let user = repository::find_user_by_id(&state.db, &c.sub)
            .await?
            .map(|u| crate::models::auth::UserResponse {
                id: u.id,
                email: u.email,
                name: u.name,
                profile_image: u.profile_image,
                oauth_provider: u.oauth_provider.to_string(),
            });
        return Ok(Json(MeResponse { user, reason: None }));
    }
    let reason = match crate::middleware::auth::extract_jwt(&headers) {
        None => None,
        Some(token) => match crate::middleware::auth::verify_jwt(&token, &state.config.jwt.secret) {
            Ok(_) => Some("revoked".to_string()),
            Err(_) => Some("expired".to_string()),
        },
    };
    Ok(Json(MeResponse { user: None, reason }))
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "Logged out; the auth cookie is cleared")
    ),
    security(("cookie_auth" = []))
)]
pub async fn logout(
    State(state): State<AppState>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> impl IntoResponse {
    if let Some(axum::Extension(c)) = claims {
        let _ = repository::delete_session(&state.db, &c.sub, &c.jti).await;
    }
    (
        [(
            axum::http::header::SET_COOKIE,
            crate::utils::auth_cookie::clear_auth_cookie(),
        )],
        Json(serde_json::json!({ "ok": true })),
    )
}

#[cfg(test)]
mod redirect_allowlist_tests {
    use super::{redirect_origin_allowed};

    fn allowed() -> Vec<String> {
        vec![
            "https://shareany.app".to_string(),
            "https://www.shareany.app".to_string(),
            "http://localhost:3000".to_string(),
        ]
    }

    #[test]
    fn allows_exact_origin_any_path() {
        let a = allowed();
        assert!(redirect_origin_allowed(
            "https://shareany.app/auth/callback/google",
            &a
        ));
        assert!(redirect_origin_allowed("https://shareany.app/", &a));
        assert!(redirect_origin_allowed("http://localhost:3000/auth/callback/apple", &a));
    }

    #[test]
    fn rejects_foreign_host() {
        let a = allowed();
        assert!(!redirect_origin_allowed("https://evil.com/capture", &a));
        // look-alike / subdomain confusion must not pass
        assert!(!redirect_origin_allowed("https://shareany.app.evil.com/x", &a));
        assert!(!redirect_origin_allowed("https://evilshareany.app/x", &a));
    }

    #[test]
    fn rejects_scheme_and_port_mismatch() {
        let a = allowed();
        // scheme differs (https vs configured http) for localhost
        assert!(!redirect_origin_allowed("https://localhost:3000/x", &a));
        // port differs
        assert!(!redirect_origin_allowed("http://localhost:4000/x", &a));
        // http where only https is allowed
        assert!(!redirect_origin_allowed("http://shareany.app/x", &a));
    }

    #[test]
    fn rejects_non_absolute_or_opaque() {
        let a = allowed();
        assert!(!redirect_origin_allowed("/auth/callback/google", &a));
        assert!(!redirect_origin_allowed("javascript:alert(1)", &a));
        assert!(!redirect_origin_allowed("not a url", &a));
        assert!(!redirect_origin_allowed("", &a));
    }

    #[test]
    fn empty_allowlist_rejects_everything() {
        assert!(!redirect_origin_allowed("https://shareany.app/x", &[]));
    }
}
