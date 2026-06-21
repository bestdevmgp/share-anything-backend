use axum::http::HeaderMap;

/// The trusted client IP from proxy headers, or `None` if it can't be
/// determined. This is the single source of truth for client-IP extraction
/// (rate limiting, quota identity, Turnstile, audit logs).
///
/// The origin sits behind Cloudflare, which sets `CF-Connecting-IP` to the real
/// client IP and overwrites any client-supplied value — so it cannot be spoofed.
/// The *first* hop of `X-Forwarded-For`, by contrast, is attacker-controllable
/// (proxies append rather than replace it), so we never trust it. Order:
///   1. `CF-Connecting-IP` (Cloudflare-set; spoof-proof behind CF)
///   2. the *last* `X-Forwarded-For` hop (appended by the trusted proxy)
///   3. `X-Real-IP` (reverse-proxy `$remote_addr`)
///
/// NOTE: trusting `CF-Connecting-IP` assumes the origin only accepts traffic
/// from Cloudflare. Lock the origin firewall to Cloudflare IP ranges (or enable
/// Authenticated Origin Pulls) so the header cannot be forged by hitting the
/// origin directly.
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

/// Like [`client_ip_opt`] but returns the `"unknown"` sentinel when no IP can be
/// determined, for callers that store or display a non-optional string.
pub fn client_ip(headers: &HeaderMap) -> String {
    client_ip_opt(headers).unwrap_or_else(|| "unknown".to_string())
}
