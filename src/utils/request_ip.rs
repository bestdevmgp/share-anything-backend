use axum::http::HeaderMap;

/// Trusted client IP (`None` if unknown). `CF-Connecting-IP` is Cloudflare-set
/// and unspoofable; the first `X-Forwarded-For` hop is client-controllable, so
/// the fallback uses the last hop. Assumes the origin only accepts Cloudflare
/// traffic — keep its firewall locked to Cloudflare IP ranges.
pub fn client_ip_opt(headers: &HeaderMap) -> Option<String> {
    let header = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    if let Some(ip) = header("CF-Connecting-IP") {
        return Some(ip);
    }
    if let Some(ip) = headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Some(ip);
    }
    header("X-Real-IP")
}

pub fn client_ip(headers: &HeaderMap) -> String {
    client_ip_opt(headers).unwrap_or_else(|| "unknown".to_string())
}
