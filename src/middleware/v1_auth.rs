use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::{
    db::{repository, DbPool},
    models::{unauthorized, AppError},
};

pub use crate::middleware::personal_token_auth::PersonalTokenUser;

#[derive(Clone)]
pub struct V1AuthState {
    pub db: DbPool,
}

pub async fn v1_auth(
    State(state): State<V1AuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if let Some(token_header) = request.headers().get("X-API-Key") {
        let token = token_header
            .to_str()
            .map_err(|_| unauthorized("Invalid auth header"))?;

        if !token.starts_with("sak_") {
            return Err(unauthorized(
                "Invalid token format. Only API keys (sak_ prefix) are accepted on this endpoint.",
            ));
        }

        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let api_key = repository::find_api_key_by_hash(&state.db, &token_hash)
            .await?
            .ok_or_else(|| unauthorized("Invalid API Key"))?;

        if let Some(expires_at) = api_key.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(unauthorized("API Key has expired"));
            }
        }

        if api_key.revoked_at.is_some() {
            return Err(unauthorized("API Key has been revoked"));
        }

        let scopes = repository::find_scopes_by_api_key(&state.db, &api_key.id).await?;

        let key_id = api_key.id.clone();
        let db = state.db.clone();

        request.extensions_mut().insert(PersonalTokenUser {
            user_id: api_key.user_id,
            personal_token_id: key_id.clone(),
            scopes,
        });

        tokio::spawn(async move {
            if let Err(e) =
                repository::update_api_key_last_used_with_platform(&db, &key_id, None).await
            {
                tracing::warn!(error = %e, "Failed to update API key last_used_at");
            }
        });
    }

    Ok(next.run(request).await)
}
