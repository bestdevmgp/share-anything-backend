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

/// OAuth 또는 매직 링크 등 외부 출처로부터 받아온 사용자 정보의 정규화된 형태.
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

/// 인증 흐름의 비즈니스 로직 — 사용자 조회/생성/재활성화, JWT 발급, 응답 빌드를 한 곳에 모은다.
/// OAuth 4종 콜백이 모두 같은 흐름을 타도록 통합한다.
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

    /// 외부 인증 결과(OAuth user info)로 사용자를 찾거나 생성한다.
    /// - 활성 사용자: 그대로 반환
    /// - 14일 이내 삭제 사용자: 재활성화
    /// - 14일 초과 삭제 사용자: 하드 삭제 후 재생성
    /// - 비활성(deactivated) 사용자: 403 Forbidden
    /// - 신규 사용자: 생성 + 환영 메일 + Discord 알림
    pub async fn upsert_oauth_user(
        &self,
        info: OAuthUserInfo,
        client_ip: &str,
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
                            self.create_and_announce_user(info, client_ip).await?
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
                self.create_and_announce_user(info, client_ip).await?
            }
        };

        Ok(AuthOutcome {
            user,
            reactivated,
            is_new_user,
        })
    }

    /// 매직 링크 인증 — 이메일로 기존 사용자(어떤 provider든) 찾기. 있으면 그대로 사용,
    /// 없으면 `OAuthProvider::Email`로 신규 생성. 기존 사용자가 다른 provider 가입자라면
    /// 그 provider 이름을 함께 반환해서 frontend가 "이미 Google로 가입된 계정입니다" 같은
    /// 안내를 띄울 수 있도록 한다.
    pub async fn upsert_email_user(
        &self,
        email: &str,
        client_ip: &str,
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
                            self.create_and_announce_user(email_user_info(email), client_ip)
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
                self.create_and_announce_user(email_user_info(email), client_ip)
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
            .send_welcome_email(&new_user.name, &new_user.email);

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
