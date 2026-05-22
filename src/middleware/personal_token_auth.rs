use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::{
    db::{repository, DbPool},
    models::{personal_token::Scope, unauthorized, AppError},
};

fn extract_cli_platform(user_agent: &str) -> Option<String> {
    if !user_agent.starts_with("share-cli/") {
        return None;
    }
    let start = user_agent.find('(')?;
    let end = user_agent.find(')')?;
    if start >= end {
        return None;
    }
    let info = user_agent[start + 1..end].trim();
    if info.is_empty() {
        None
    } else {
        Some(info.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct PersonalTokenUser {
    pub user_id: String,
    pub personal_token_id: String,
    pub scopes: Vec<Scope>,
}

#[derive(Clone)]
pub struct CliAuthState {
    pub db: DbPool,
}

pub async fn cli_auth(
    State(state): State<CliAuthState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if let Some(token_header) = request.headers().get("X-Personal-Token") {
        let token = token_header
            .to_str()
            .map_err(|_| unauthorized("Invalid auth header."))?;

        if !token.starts_with("sa_") && !token.starts_with("sk_") {
            return Err(unauthorized("Invalid token format. Expected 'sa_' or 'sk_' prefix."));
        }

        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        let token_hash = hex::encode(hasher.finalize());

        let token_record = repository::find_personal_token_by_hash(&state.db, &token_hash)
            .await?
            .ok_or_else(|| unauthorized("Invalid Personal Token."))?;

        if let Some(expires_at) = token_record.expires_at {
            if expires_at < chrono::Utc::now() {
                return Err(unauthorized("Personal Token has expired."));
            }
        }

        let token_id = token_record.id.clone();
        let db = state.db.clone();

        let platform = request
            .headers()
            .get("User-Agent")
            .and_then(|v| v.to_str().ok())
            .and_then(extract_cli_platform);

        request.extensions_mut().insert(PersonalTokenUser {
            user_id: token_record.user_id,
            personal_token_id: token_id.clone(),
            scopes: vec![],
        });

        tokio::spawn(async move {
            if let Err(e) = repository::update_personal_token_last_used_with_platform(
                &db,
                &token_id,
                platform.as_deref(),
            )
            .await
            {
                tracing::warn!(error = %e, "Failed to update personal token last_used_at");
            }
        });
    }

    Ok(next.run(request).await)
}
