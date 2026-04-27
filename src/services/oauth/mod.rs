//! Provider별 OAuth 토큰 교환 + user info 조회 로직.
//!
//! 각 provider 모듈은 인증 코드를 받아 정규화된 [`OAuthUserInfo`]를 반환한다.
//! 핸들러는 어떤 provider인지만 정하고 결과는 [`AuthService::upsert_oauth_user`]로
//! 동일하게 흐른다.
//!
//! [`OAuthUserInfo`]: crate::services::auth::OAuthUserInfo
//! [`AuthService::upsert_oauth_user`]: crate::services::auth::AuthService::upsert_oauth_user

pub mod apple;
pub mod google;
pub mod kakao;
pub mod naver;
