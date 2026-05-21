use crate::middleware::personal_token_auth::PersonalTokenUser;
use crate::models::personal_token::Scope;
use super::error::PublicApiError;

pub fn require_token<'a>(
    token: Option<&'a axum::extract::Extension<PersonalTokenUser>>,
) -> Result<&'a PersonalTokenUser, PublicApiError> {
    token
        .map(|ext| &ext.0)
        .ok_or_else(|| PublicApiError::Unauthorized(
            "Missing or invalid Personal Token. Set the 'X-Personal-Token' header.".into(),
        ))
}

pub fn require_scope(user: &PersonalTokenUser, scope: Scope) -> Result<(), PublicApiError> {
    if user.scopes.contains(&scope) {
        Ok(())
    } else {
        Err(PublicApiError::InsufficientScope(scope.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(scopes: Vec<Scope>) -> PersonalTokenUser {
        PersonalTokenUser {
            user_id: "u1".into(),
            personal_token_id: "t1".into(),
            scopes,
        }
    }

    #[test]
    fn allows_when_scope_present() {
        let u = user(vec![Scope::Read, Scope::Upload]);
        assert!(require_scope(&u, Scope::Read).is_ok());
    }

    #[test]
    fn rejects_when_scope_missing() {
        let u = user(vec![Scope::Read]);
        let err = require_scope(&u, Scope::Delete).unwrap_err();
        assert!(matches!(err, PublicApiError::InsufficientScope("delete")));
    }
}
