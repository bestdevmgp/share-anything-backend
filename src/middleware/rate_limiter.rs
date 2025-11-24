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

/// Rate limiter state that tracks requests per IP
#[derive(Clone)]
pub struct RateLimiter {
    /// Tracks normal request counts per IP
    request_counts: Arc<DashMap<String, RequestRecord>>,
    /// Tracks failed (404) request counts per IP
    failed_counts: Arc<DashMap<String, FailedRequestRecord>>,
    /// IPs that are temporarily blocked
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

        // Spawn cleanup task
        let rate_limiter_clone = rate_limiter.clone();
        tokio::spawn(async move {
            rate_limiter_clone.cleanup_task().await;
        });

        rate_limiter
    }

    /// Check if the IP is allowed to make a request
    pub fn check_rate_limit(&self, ip: &str) -> Result<(), String> {
        // Check if IP is blocked
        if let Some(blocked_until) = self.blocked_ips.get(ip) {
            if blocked_until.value().elapsed() < Duration::from_secs(3600) {
                // Blocked for 1 hour
                return Err("Your IP has been temporarily blocked due to suspicious activity. Please try again later.".to_string());
            } else {
                // Block expired, remove it
                self.blocked_ips.remove(ip);
            }
        }

        let now = Instant::now();
        let window_duration = Duration::from_secs(60); // 1 minute window
        let max_requests_per_minute = 20; // Allow 20 requests per minute per IP

        // Check and update request count
        let mut should_allow = true;
        self.request_counts
            .entry(ip.to_string())
            .and_modify(|record| {
                // Reset window if expired
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
            return Err("Rate limit exceeded. You can make up to 20 requests per minute.".to_string());
        }

        Ok(())
    }

    /// Record a failed request (404 response)
    pub fn record_failed_request(&self, ip: &str) {
        let now = Instant::now();
        let window_duration = Duration::from_secs(60); // 1 minute window
        let max_failures_per_minute = 10; // Allow max 10 failures per minute

        let mut should_block = false;
        self.failed_counts
            .entry(ip.to_string())
            .and_modify(|record| {
                // Reset window if expired
                if now.duration_since(record.window_start) > window_duration {
                    record.count = 1;
                    record.window_start = now;
                } else {
                    record.count += 1;
                    // Block if too many failed requests
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

    /// Cleanup old entries periodically
    async fn cleanup_task(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes
        loop {
            interval.tick().await;
            self.cleanup();
        }
    }

    fn cleanup(&self) {
        let now = Instant::now();
        let max_age = Duration::from_secs(3600); // Keep records for 1 hour

        // Cleanup old request records
        self.request_counts.retain(|_, record| {
            now.duration_since(record.window_start) < max_age
        });

        // Cleanup old failed request records
        self.failed_counts.retain(|_, record| {
            now.duration_since(record.window_start) < max_age
        });

        // Cleanup expired blocks
        self.blocked_ips.retain(|_, blocked_at| {
            blocked_at.elapsed() < Duration::from_secs(3600)
        });

        tracing::debug!(
            "Rate limiter cleanup: {} IPs tracked, {} IPs blocked",
            self.request_counts.len(),
            self.blocked_ips.len()
        );
    }
}

/// Extract IP address from request headers
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

/// Middleware function for rate limiting share code lookups
pub async fn rate_limit_middleware(
    State(rate_limiter): State<RateLimiter>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let ip = extract_ip(&headers);

    // Check rate limit
    if let Err(error_message) = rate_limiter.check_rate_limit(&ip) {
        tracing::warn!("Rate limit exceeded for IP: {}", ip);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": error_message
            })),
        )
            .into_response();
    }

    // Process request
    let response = next.run(request).await;

    // Record failed requests (404s)
    if response.status() == StatusCode::NOT_FOUND {
        rate_limiter.record_failed_request(&ip);
    }

    response
}
