use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::db::{repository, DbPool};

#[derive(Debug, Clone)]
pub struct PersonalTokenUser {
    pub user_id: String,
    pub personal_token_id: String,
}

#[derive(Clone)]
pub struct CliAuthState {
    pub db: DbPool,
}

pub async fn cli_auth(
    State(state): State<CliAuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(token_header) = request.headers().get("X-Personal-Token") {
        let token = token_header
            .to_str()
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        if !token.starts_with("sa_") {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let token_record = repository::find_personal_token_by_hash(&state.db, &token_hash)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if let Some(expires_at) = token_record.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }

        let token_id = token_record.id.clone();
        let db = state.db.clone();

        request.extensions_mut().insert(PersonalTokenUser {
            user_id: token_record.user_id,
            personal_token_id: token_id.clone(),
        });

        tokio::spawn(async move {
            let _ = repository::update_personal_token_last_used(&db, &token_id).await;
        });
    }

    Ok(next.run(request).await)
}
