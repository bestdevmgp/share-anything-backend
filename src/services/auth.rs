use chrono::Utc;
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    middleware::auth::create_jwt,
    models::{
        forbidden, internal_error, user::UserStatus, AppError, AuthResponse, CreateUserDto,
        OAuthProvider, User, UserResponse,
    },
    services::{discord::DiscordNotifier, email::EmailService},
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
}

impl AuthService {
    pub fn new(
        db: DbPool,
        config: Arc<Config>,
        discord: Arc<DiscordNotifier>,
        email: Arc<EmailService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            config,
            discord,
            email,
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
                    return Err(forbidden("이 계정은 비활성화 상태입니다"));
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
                    return Err(forbidden("이 계정은 비활성화 상태입니다"));
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
        };
        let new_user = repository::create_user(&self.db, dto).await?;

        self.discord
            .notify_new_user(&new_user.name, &new_user.email, provider_label, client_ip);
        self.email
            .send_welcome_email(&new_user.name, &new_user.email, welcome_lang);

        Ok(new_user)
    }

    pub fn issue_jwt(&self, user: &User) -> Result<String, AppError> {
        create_jwt(
            &user.id,
            &user.email,
            &user.name,
            &self.config.jwt.secret,
            self.config.jwt.expiration_hours,
        )
        .map_err(|e| {
            tracing::error!(error = ?e, "JWT creation failed");
            internal_error("JWT 발급 실패")
        })
    }

    pub fn build_response(&self, outcome: AuthOutcome, jwt: String) -> AuthResponse {
        AuthResponse {
            token: jwt,
            user: UserResponse {
                id: outcome.user.id,
                email: outcome.user.email,
                name: outcome.user.name,
                profile_image: outcome.user.profile_image,
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
