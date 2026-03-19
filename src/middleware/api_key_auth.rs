use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::db::{repository, DbPool};

#[derive(Debug, Clone)]
pub struct ApiKeyUser {
    pub user_id: String,
    pub api_key_id: String,
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
    if let Some(api_key_header) = request.headers().get("X-API-Key") {
        let api_key = api_key_header
            .to_str()
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

        if !api_key.starts_with("sa_") {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let mut hasher = Sha256::new();
        hasher.update(api_key.as_bytes());
        let key_hash = hex::encode(hasher.finalize());

        let api_key_record = repository::find_api_key_by_hash(&state.db, &key_hash)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if let Some(expires_at) = api_key_record.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }

        let api_key_id = api_key_record.id.clone();
        let db = state.db.clone();

        request.extensions_mut().insert(ApiKeyUser {
            user_id: api_key_record.user_id,
            api_key_id: api_key_id.clone(),
        });

        tokio::spawn(async move {
            let _ = repository::update_api_key_last_used(&db, &api_key_id).await;
        });
    }

    Ok(next.run(request).await)
}
