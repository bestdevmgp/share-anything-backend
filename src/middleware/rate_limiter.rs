use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use dashmap::DashMap;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RateLimiter {
    request_counts: Arc<DashMap<String, RequestRecord>>,
    failed_counts: Arc<DashMap<String, FailedRequestRecord>>,
    blocked_ips: Arc<DashMap<String, Instant>>,
}

#[derive(Debug, Clone)]
struct RequestRecord {
    count: u32,
    window_start: Instant,
}

#[derive(Debug, Clone)]
struct FailedRequestRecord {
    count: u32,
    window_start: Instant,
}

impl RateLimiter {
    pub fn new() -> Self {
        let rate_limiter = Self {
            request_counts: Arc::new(DashMap::new()),
            failed_counts: Arc::new(DashMap::new()),
            blocked_ips: Arc::new(DashMap::new()),
        };

        let rate_limiter_clone = rate_limiter.clone();
        tokio::spawn(async move {
            rate_limiter_clone.cleanup_task().await;
        });

        rate_limiter
    }

    pub fn check_rate_limit(&self, ip: &str) -> Result<(), String> {
        if let Some(blocked_until) = self.blocked_ips.get(ip) {
            if blocked_until.value().elapsed() < Duration::from_secs(600) {
                return Err("비정상적인 활동으로 인해 사용자의 IP가 일시적으로 차단되었습니다. 나중에 다시 시도해 주세요".to_string());
            } else {
                self.blocked_ips.remove(ip);
            }
        }

        let now = Instant::now();
        let window_duration = Duration::from_secs(60);
        let max_requests_per_minute = 50;

        let mut should_allow = true;
        self.request_counts
            .entry(ip.to_string())
            .and_modify(|record| {
                if now.duration_since(record.window_start) > window_duration {
                    record.count = 1;
                    record.window_start = now;
                } else {
                    record.count += 1;
                    if record.count > max_requests_per_minute {
                        should_allow = false;
                    }
                }
            })
            .or_insert_with(|| RequestRecord {
                count: 1,
                window_start: now,
            });

        if !should_allow {
            return Err("Rate limit exceeded. You can make up to 50 requests per minute".to_string());
        }

        Ok(())
    }

    pub fn record_failed_request(&self, ip: &str) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(60);
        let max_failures_per_minute = 10;

        let mut should_block = false;
        self.failed_counts
            .entry(ip.to_string())
            .and_modify(|record| {
                if now.duration_since(record.window_start) > window_duration {
                    record.count = 1;
                    record.window_start = now;
                } else {
                    record.count += 1;
                    if record.count > max_failures_per_minute {
                        should_block = true;
                    }
                }
            })
            .or_insert_with(|| FailedRequestRecord {
                count: 1,
                window_start: now,
            });

        if should_block {
            tracing::warn!("Blocking IP {} due to excessive failed requests", ip);
            self.blocked_ips.insert(ip.to_string(), now);
        }
    }

    async fn cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            self.cleanup();
        }
    }

    fn cleanup(&self) {
        let now = Instant::now();
        let max_age = Duration::from_secs(3600);

        self.request_counts.retain(|_, record| {
            now.duration_since(record.window_start) < max_age
        });

        self.failed_counts.retain(|_, record| {
            now.duration_since(record.window_start) < max_age
        });

        self.blocked_ips.retain(|_, blocked_at| {
            blocked_at.elapsed() < Duration::from_secs(600)
        });
    }
}

#[derive(Clone)]
pub struct CliRateLimiter {
    guest_upload_counts: Arc<DashMap<String, RequestRecord>>,
    apikey_upload_counts: Arc<DashMap<String, RequestRecord>>,
    guest_download_counts: Arc<DashMap<String, RequestRecord>>,
    apikey_download_counts: Arc<DashMap<String, RequestRecord>>,
    blocked_ips: Arc<DashMap<String, Instant>>,
}

impl CliRateLimiter {
    pub fn new() -> Self {
        let limiter = Self {
            guest_upload_counts: Arc::new(DashMap::new()),
            apikey_upload_counts: Arc::new(DashMap::new()),
            guest_download_counts: Arc::new(DashMap::new()),
            apikey_download_counts: Arc::new(DashMap::new()),
            blocked_ips: Arc::new(DashMap::new()),
        };

        let limiter_clone = limiter.clone();
        tokio::spawn(async move {
            limiter_clone.cli_cleanup_task().await;
        });

        limiter
    }

    pub fn check_cli_rate_limit(
        &self,
        key: &str,
        is_upload: bool,
        is_authenticated: bool,
    ) -> Result<(), String> {
        if let Some(blocked_until) = self.blocked_ips.get(key) {
            if blocked_until.value().elapsed() < Duration::from_secs(600) {
                return Err("Too many requests. Please try again later".to_string());
            } else {
                self.blocked_ips.remove(key);
            }
        }

        let now = Instant::now();
        let window = Duration::from_secs(3600); // 1 hour

        let (map, max_requests) = match (is_upload, is_authenticated) {
            (true, false) => (&self.guest_upload_counts, 10u32),
            (true, true) => (&self.apikey_upload_counts, 50u32),
            (false, false) => (&self.guest_download_counts, 30u32),
            (false, true) => (&self.apikey_download_counts, 100u32),
        };

        let mut should_allow = true;
        map.entry(key.to_string())
            .and_modify(|record| {
                if now.duration_since(record.window_start) > window {
                    record.count = 1;
                    record.window_start = now;
                } else {
                    record.count += 1;
                    if record.count > max_requests {
                        should_allow = false;
                    }
                }
            })
            .or_insert_with(|| RequestRecord {
                count: 1,
                window_start: now,
            });

        if !should_allow {
            return Err(format!(
                "Rate limit exceeded. Maximum {} requests per hour",
                max_requests
            ));
        }

        Ok(())
    }

    async fn cli_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = Instant::now();
            let max_age = Duration::from_secs(7200);

            for map in [
                &self.guest_upload_counts,
                &self.apikey_upload_counts,
                &self.guest_download_counts,
                &self.apikey_download_counts,
            ] {
                map.retain(|_, record| now.duration_since(record.window_start) < max_age);
            }

            self.blocked_ips
                .retain(|_, blocked_at| blocked_at.elapsed() < Duration::from_secs(600));
        }
    }
}

pub async fn cli_rate_limit_middleware(
    State(cli_rate_limiter): State<CliRateLimiter>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(&headers);
    let path = request.uri().path().to_string();
    let is_upload = path.contains("/upload") || path.contains("/multipart");

    let token_user = request
        .extensions()
        .get::<crate::middleware::personal_token_auth::PersonalTokenUser>();

    let (key, is_authenticated) = if let Some(user) = token_user {
        (user.user_id.clone(), true)
    } else {
        (ip, false)
    };

    if let Err(error_message) = cli_rate_limiter.check_cli_rate_limit(&key, is_upload, is_authenticated) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "RATE_LIMIT_EXCEEDED",
                "message": error_message
            })),
        )
            .into_response();
    }

    next.run(request).await
}

fn extract_ip(headers: &HeaderMap) -> String {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim())
        .or_else(|| headers.get("X-Real-IP").and_then(|v| v.to_str().ok()))
        .unwrap_or("unknown")
        .to_string()
}

pub async fn rate_limit_middleware(
    State(rate_limiter): State<RateLimiter>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(&headers);

    if let Err(error_message) = rate_limiter.check_rate_limit(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": error_message
            })),
        )
            .into_response();
    }

    let response = next.run(request).await;

    if response.status() == StatusCode::NOT_FOUND {
        rate_limiter.record_failed_request(&ip);
    }

    response
}
