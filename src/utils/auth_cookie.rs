use axum::http::{header, HeaderMap};

pub const AUTH_COOKIE: &str = "__Host-auth_token";

pub fn build_auth_cookie(jwt: &str, max_age_secs: i64) -> String {
    format!(
        "{}={}; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age={}",
        AUTH_COOKIE, jwt, max_age_secs
    )
}

pub fn clear_auth_cookie() -> String {
    format!(
        "{}=; HttpOnly; Secure; SameSite=Lax; Path=/; Max-Age=0",
        AUTH_COOKIE
    )
}

pub fn read_auth_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    let prefix = format!("{}=", AUTH_COOKIE);
    for part in cookie_header.split(';') {
        if let Some(value) = part.trim().strip_prefix(&prefix) {
            let value = value.trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
