use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct TurnstileVerifyRequest {
    secret: String,
    response: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remoteip: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TurnstileVerifyResponse {
    success: bool,
    #[serde(default)]
    #[serde(rename = "error-codes")]
    error_codes: Vec<String>,
    #[serde(default)]
    hostname: Option<String>,
    #[serde(default)]
    action: Option<String>,
}

/// Verifies a Turnstile token: `success`, plus the solve's `hostname` (must be
/// one of ours; skipped when `allowed_hostnames` is empty) and `action` (must
/// match when non-empty — empty is allowed for deploy-order compatibility).
pub async fn verify_turnstile_token(
    secret_key: &str,
    token: &str,
    remote_ip: Option<String>,
    allowed_hostnames: &[String],
    expected_action: Option<&str>,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("HTTP 클라이언트 생성 실패: {}", e))?;

    let request_body = TurnstileVerifyRequest {
        secret: secret_key.to_string(),
        response: token.to_string(),
        remoteip: remote_ip,
    };

    let response = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Turnstile 검증 API 호출 실패: {}", e))?;

    if !response.status().is_success() {
        return Err(format!(
            "Turnstile API가 오류를 반환했습니다: {}",
            response.status()
        ));
    }

    let verify_response: TurnstileVerifyResponse = response
        .json()
        .await
        .map_err(|e| format!("Turnstile 응답 파싱 실패: {}", e))?;

    if !verify_response.success {
        let error_msg = if verify_response.error_codes.is_empty() {
            "알 수 없는 오류".to_string()
        } else {
            verify_response.error_codes.join(", ")
        };
        return Err(format!("Turnstile 검증 실패: {}", error_msg));
    }

    if !allowed_hostnames.is_empty() {
        let hostname = verify_response.hostname.unwrap_or_default();
        if !allowed_hostnames.iter().any(|h| h == &hostname) {
            return Err(format!("Turnstile hostname 불일치: {}", hostname));
        }
    }

    if let Some(expected) = expected_action {
        let action = verify_response.action.unwrap_or_default();
        if !action.is_empty() && action != expected {
            return Err(format!("Turnstile action 불일치: {}", action));
        }
    }

    Ok(())
}

/// Real client IP, preferring Cloudflare's unforgeable `CF-Connecting-IP` over
/// the client-spoofable `X-Forwarded-For` / `X-Real-IP` fallbacks.
pub fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("CF-Connecting-IP")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("X-Forwarded-For")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.split(',').next())
                .map(|s| s.trim().to_string())
        })
        .or_else(|| {
            headers
                .get("X-Real-IP")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// `https://share.example.com:443` → `share.example.com`. Derives the Turnstile
/// hostname allowlist from the CORS origins.
pub fn origin_to_host(origin: &str) -> Option<String> {
    let without_scheme = origin.split("://").nth(1).unwrap_or(origin);
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}
