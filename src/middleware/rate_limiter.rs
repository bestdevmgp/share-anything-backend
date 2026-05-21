use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use dashmap::DashMap;
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Bucket classification for §6.2 per-token rate limits.
#[derive(Debug, Clone, Copy)]
pub enum Bucket {
    Read,
    Upload,
    Download,
}

impl Bucket {
    fn limit(&self) -> u32 {
        match self {
            Bucket::Read => 500,
            Bucket::Upload => 80,
            Bucket::Download => 150,
        }
    }
}

/// Rate-limit state returned in both allowed and denied cases, used to populate
/// the X-RateLimit-* response headers.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitStatus {
    pub limit: u32,
    pub remaining: u32,
    pub reset_unix: u64,
}

// TODO: rename to V1RateLimiter
#[derive(Clone)]
pub struct CliRateLimiter {
    read: Arc<DashMap<String, RequestRecord>>,
    upload: Arc<DashMap<String, RequestRecord>>,
    download: Arc<DashMap<String, RequestRecord>>,
    blocked_ips: Arc<DashMap<String, Instant>>,
}

impl CliRateLimiter {
    pub fn new() -> Self {
        let limiter = Self {
            read: Arc::new(DashMap::new()),
            upload: Arc::new(DashMap::new()),
            download: Arc::new(DashMap::new()),
            blocked_ips: Arc::new(DashMap::new()),
        };

        let limiter_clone = limiter.clone();
        tokio::spawn(async move {
            limiter_clone.cli_cleanup_task().await;
        });

        limiter
    }

    /// Check the rate limit for the given key and bucket.
    ///
    /// Returns `Ok(status)` when the request is allowed, `Err(status)` when denied.
    /// The `RateLimitStatus` contains the information needed for X-RateLimit-* headers
    /// in both cases.
    pub fn check(&self, key: &str, bucket: Bucket) -> Result<RateLimitStatus, RateLimitStatus> {
        if let Some(blocked_until) = self.blocked_ips.get(key) {
            if blocked_until.value().elapsed() < Duration::from_secs(600) {
                // Return a denied status with zeroed remaining and a reset in 10 minutes.
                let reset_unix = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    + 600;
                return Err(RateLimitStatus {
                    limit: bucket.limit(),
                    remaining: 0,
                    reset_unix,
                });
            } else {
                self.blocked_ips.remove(key);
            }
        }

        let now = Instant::now();
        let window = Duration::from_secs(3600);
        let max_requests = bucket.limit();

        let map = match bucket {
            Bucket::Read => &self.read,
            Bucket::Upload => &self.upload,
            Bucket::Download => &self.download,
        };

        let mut allowed = true;
        let mut current_count = 0u32;
        let mut window_start = now;

        map.entry(key.to_string())
            .and_modify(|record| {
                if now.duration_since(record.window_start) > window {
                    record.count = 1;
                    record.window_start = now;
                } else {
                    record.count += 1;
                    if record.count > max_requests {
                        allowed = false;
                    }
                }
                current_count = record.count;
                window_start = record.window_start;
            })
            .or_insert_with(|| {
                current_count = 1;
                window_start = now;
                RequestRecord {
                    count: 1,
                    window_start: now,
                }
            });

        // Compute seconds remaining in the current window, then derive a wall-clock reset timestamp.
        let elapsed_secs = now.duration_since(window_start).as_secs();
        let seconds_remaining = 3600u64.saturating_sub(elapsed_secs);
        let reset_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_add(seconds_remaining);

        let remaining = if allowed {
            max_requests.saturating_sub(current_count)
        } else {
            0
        };

        let status = RateLimitStatus {
            limit: max_requests,
            remaining,
            reset_unix,
        };

        if allowed {
            Ok(status)
        } else {
            Err(status)
        }
    }

    async fn cli_cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        loop {
            interval.tick().await;
            let now = Instant::now();
            let max_age = Duration::from_secs(7200);

            for map in [&self.read, &self.upload, &self.download] {
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
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // §6.2 bucket classification
    let bucket = if method == axum::http::Method::POST && path.starts_with("/v1/uploads") {
        Bucket::Upload
    } else if method == axum::http::Method::GET
        && path.starts_with("/v1/shares/")
        && path.ends_with("/download")
    {
        Bucket::Download
    } else {
        Bucket::Read
    };

    let token_user = request
        .extensions()
        .get::<crate::middleware::personal_token_auth::PersonalTokenUser>();

    // Key by authenticated user id; fall back to IP for defensive coverage
    // (no anonymous v1 paths exist today, but this avoids a panic if one is added).
    let key = match token_user {
        Some(u) => u.user_id.clone(),
        None => extract_ip(&headers),
    };

    match cli_rate_limiter.check(&key, bucket) {
        Err(status) => {
            let body = json!({
                "error": {
                    "code": "too_many_requests",
                    "message": format!(
                        "Rate limit exceeded. Maximum {} requests per hour",
                        status.limit
                    ),
                    "request_id": null
                }
            });
            let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            let h = response.headers_mut();
            h.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                HeaderValue::from(status.limit),
            );
            h.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from(status.remaining),
            );
            h.insert(
                HeaderName::from_static("x-ratelimit-reset"),
                HeaderValue::from(status.reset_unix),
            );
            response
        }
        Ok(status) => {
            let mut response = next.run(request).await;
            let h = response.headers_mut();
            h.insert(
                HeaderName::from_static("x-ratelimit-limit"),
                HeaderValue::from(status.limit),
            );
            h.insert(
                HeaderName::from_static("x-ratelimit-remaining"),
                HeaderValue::from(status.remaining),
            );
            h.insert(
                HeaderName::from_static("x-ratelimit-reset"),
                HeaderValue::from(status.reset_unix),
            );
            response
        }
    }
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
