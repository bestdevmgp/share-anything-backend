use axum::http::HeaderMap;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    config::Config,
    db::{repository, DbPool},
    handlers::device_confirm::issue_device_revoke_token,
    middleware::auth::create_jwt,
    models::{
        forbidden, internal_error, session::CreateSessionDto, user::UserStatus, AppError,
        AuthResponse, CreateUserDto, OAuthProvider, User, UserResponse,
    },
    services::{
        discord::DiscordNotifier, email::EmailService, geolocation::GeolocationService,
    },
    utils::client_ip,
};

const REACTIVATION_WINDOW_DAYS: i64 = 14;

pub struct OAuthUserInfo {
    pub provider: OAuthProvider,
    pub oauth_id: String,
    pub email: String,
    pub name: String,
    pub profile_image: Option<String>,
}

pub struct AuthOutcome {
    pub user: User,
    pub reactivated: bool,
    pub is_new_user: bool,
}

pub struct AuthService {
    db: DbPool,
    config: Arc<Config>,
    discord: Arc<DiscordNotifier>,
    email: Arc<EmailService>,
    geolocation: Arc<GeolocationService>,
}

impl AuthService {
    pub fn new(
        db: DbPool,
        config: Arc<Config>,
        discord: Arc<DiscordNotifier>,
        email: Arc<EmailService>,
        geolocation: Arc<GeolocationService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            discord,
            email,
            geolocation,
        })
    }

    pub async fn upsert_oauth_user(
        &self,
        info: OAuthUserInfo,
        client_ip: &str,
        welcome_lang: &str,
    ) -> Result<AuthOutcome, AppError> {
        let mut reactivated = false;
        let mut is_new_user = false;

        let user = match repository::find_user_by_oauth(&self.db, &info.provider, &info.oauth_id)
            .await?
        {
            Some(mut existing) => {
                if existing.status == UserStatus::Deleted {
                    match check_deleted_user(&existing) {
                        DeletedUserAction::Reactivate => {
                            repository::reactivate_user(&self.db, &existing.id).await?;
                            existing.status = UserStatus::Active;
                            reactivated = true;
                            existing
                        }
                        DeletedUserAction::HardDeleteAndRecreate => {
                            repository::hard_delete_user(&self.db, &existing.id).await?;
                            self.create_and_announce_user(info, client_ip, welcome_lang)
                                .await?
                        }
                    }
                } else if existing.status != UserStatus::Active {
                    return Err(forbidden("This account is inactive"));
                } else {
                    existing
                }
            }
            None => {
                is_new_user = true;
                self.create_and_announce_user(info, client_ip, welcome_lang)
                    .await?
            }
        };

        Ok(AuthOutcome {
            user,
            reactivated,
            is_new_user,
        })
    }

    pub async fn upsert_email_user(
        &self,
        email: &str,
        client_ip: &str,
        welcome_lang: &str,
    ) -> Result<(AuthOutcome, Option<String>), AppError> {
        let mut reactivated = false;
        let mut is_new_user = false;
        let mut existing_provider: Option<String> = None;

        let user = match repository::find_user_by_email(&self.db, email).await? {
            Some(mut existing) => {
                if existing.status == UserStatus::Deleted {
                    match check_deleted_user(&existing) {
                        DeletedUserAction::Reactivate => {
                            repository::reactivate_user(&self.db, &existing.id).await?;
                            existing.status = UserStatus::Active;
                            reactivated = true;
                            existing
                        }
                        DeletedUserAction::HardDeleteAndRecreate => {
                            repository::hard_delete_user(&self.db, &existing.id).await?;
                            self.create_and_announce_user(
                                email_user_info(email),
                                client_ip,
                                welcome_lang,
                            )
                            .await?
                        }
                    }
                } else if existing.status != UserStatus::Active {
                    return Err(forbidden("This account is inactive"));
                } else {
                    if existing.oauth_provider != OAuthProvider::Email {
                        existing_provider = Some(existing.oauth_provider.to_string());
                    }
                    existing
                }
            }
            None => {
                is_new_user = true;
                self.create_and_announce_user(email_user_info(email), client_ip, welcome_lang)
                    .await?
            }
        };

        Ok((
            AuthOutcome {
                user,
                reactivated,
                is_new_user,
            },
            existing_provider,
        ))
    }

    async fn create_and_announce_user(
        &self,
        info: OAuthUserInfo,
        client_ip: &str,
        welcome_lang: &str,
    ) -> Result<User, AppError> {
        let provider_label = display_name(&info.provider);
        let dto = CreateUserDto {
            oauth_provider: info.provider,
            oauth_id: info.oauth_id,
            email: info.email,
            name: info.name,
            profile_image: info.profile_image,
            notify_language: welcome_lang.to_string(),
        };
        let new_user = repository::create_user(&self.db, dto).await?;

        self.discord
            .notify_new_user(&new_user.name, &new_user.email, provider_label, client_ip);
        self.email
            .send_welcome_email(&new_user.name, &new_user.email, welcome_lang);

        Ok(new_user)
    }

    pub async fn create_session_token(
        &self,
        user: &User,
        is_new_user: bool,
        headers: &HeaderMap,
    ) -> Result<String, AppError> {
        let ip = client_ip(headers);
        let user_agent = extract_user_agent(headers);
        let user_agent_hash = hash_ua(&user_agent);
        let device_id = resolve_device_id(headers, &user_agent_hash);

        let was_trusted =
            repository::is_device_trusted(&self.db, &user.id, &device_id).await?;

        repository::delete_sessions_by_device(&self.db, &user.id, &device_id).await?;

        let jti = Uuid::new_v4().to_string();
        let device_label = parse_device_label(&user_agent);
        let location = self.geolocation.lookup(&ip).await;
        let now = Utc::now();
        let expires_at =
            now + chrono::Duration::hours(self.config.jwt.expiration_hours);

        repository::create_session(
            &self.db,
            CreateSessionDto {
                jti: jti.clone(),
                user_id: user.id.clone(),
                device_id: device_id.clone(),
                device_label: device_label.clone(),
                user_agent: user_agent.clone(),
                user_agent_hash: user_agent_hash.clone(),
                ip_address: ip.clone(),
                location: location.clone(),
                expires_at,
            },
        )
        .await?;

        repository::upsert_trusted_device(
            &self.db,
            &user.id,
            &device_id,
            &user_agent_hash,
            Some(&user_agent),
            &ip,
            device_label.as_deref(),
            location.as_deref(),
        )
        .await?;

        if !was_trusted && !is_new_user && user.notify_security {
            self.notify_new_device(
                user,
                &jti,
                &device_id,
                &ip,
                location.as_deref(),
                device_label.as_deref(),
                now,
            );
        }

        create_jwt(
            &user.id,
            &user.email,
            &user.name,
            &jti,
            &self.config.jwt.secret,
            self.config.jwt.expiration_hours,
        )
        .map_err(|e| {
            tracing::error!(error = ?e, "JWT creation failed");
            internal_error("Failed to issue JWT")
        })
    }

    fn notify_new_device(
        &self,
        user: &User,
        jti: &str,
        device_id: &str,
        ip: &str,
        location: Option<&str>,
        device_label: Option<&str>,
        logged_in_at: chrono::DateTime<Utc>,
    ) {
        let revoke_token = match issue_device_revoke_token(
            &self.config.jwt.secret,
            &user.id,
            jti,
            device_id,
        ) {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, "Failed to issue device revoke token");
                return;
            }
        };

        self.email.send_new_device_notification(
            &user.email,
            device_label,
            ip,
            location,
            logged_in_at,
            &revoke_token,
            &user.notify_language,
        );
    }

    pub fn build_response(&self, outcome: AuthOutcome) -> AuthResponse {
        AuthResponse {
            user: UserResponse {
                id: outcome.user.id,
                email: outcome.user.email,
                name: outcome.user.name,
                profile_image: outcome.user.profile_image,
                oauth_provider: outcome.user.oauth_provider.to_string(),
            },
            reactivated: if outcome.reactivated { Some(true) } else { None },
            is_new_user: if outcome.is_new_user { Some(true) } else { None },
        }
    }
}

enum DeletedUserAction {
    Reactivate,
    HardDeleteAndRecreate,
}

fn check_deleted_user(user: &User) -> DeletedUserAction {
    let elapsed = Utc::now() - user.updated_at;
    if elapsed.num_days() <= REACTIVATION_WINDOW_DAYS {
        DeletedUserAction::Reactivate
    } else {
        DeletedUserAction::HardDeleteAndRecreate
    }
}

fn email_user_info(email: &str) -> OAuthUserInfo {
    OAuthUserInfo {
        provider: OAuthProvider::Email,
        oauth_id: email.to_string(),
        email: email.to_string(),
        name: email.split('@').next().unwrap_or("User").to_string(),
        profile_image: None,
    }
}

fn display_name(provider: &OAuthProvider) -> &'static str {
    match provider {
        OAuthProvider::Google => "Google",
        OAuthProvider::Naver => "Naver",
        OAuthProvider::Kakao => "Kakao",
        OAuthProvider::Apple => "Apple",
        OAuthProvider::Email => "Email",
    }
}

fn resolve_device_id(headers: &HeaderMap, user_agent_hash: &str) -> String {
    headers
        .get("x-device-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 64)
        .unwrap_or_else(|| user_agent_hash.to_string())
}

fn extract_user_agent(headers: &HeaderMap) -> String {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

fn hash_ua(ua: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ua.as_bytes());
    hex::encode(hasher.finalize())
}

fn parse_device_label(ua: &str) -> Option<String> {
    if ua == "unknown" || ua.is_empty() {
        return None;
    }
    match woothee::parser::Parser::new().parse(ua) {
        Some(result) => {
            let os = if result.os.is_empty() { "Unknown" } else { result.os };
            let browser = if result.name.is_empty() { "Unknown" } else { result.name };
            Some(format!("{} on {}", browser, os))
        }
        None => Some(ua.chars().take(120).collect()),
    }
}
