use dashmap::DashMap;
use serde::Deserialize;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Deserialize)]
struct IpInfoResponse {
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

struct CachedLocation {
    location: Option<String>,
    expires_at: Instant,
}

const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24);

pub struct GeolocationService {
    client: reqwest::Client,
    cache: Arc<DashMap<String, CachedLocation>>,
    api_token: Option<String>,
}

impl GeolocationService {
    pub fn new(api_token: Option<String>) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_default();
        Arc::new(Self {
            client,
            cache: Arc::new(DashMap::new()),
            api_token,
        })
    }

    pub async fn lookup(&self, ip: &str) -> Option<String> {
        if !is_public_ip(ip) {
            return None;
        }

        if let Some(cached) = self.cache.get(ip) {
            if cached.expires_at > Instant::now() {
                return cached.location.clone();
            }
        }

        let location = self.fetch(ip).await;

        self.cache.insert(
            ip.to_string(),
            CachedLocation {
                location: location.clone(),
                expires_at: Instant::now() + CACHE_TTL,
            },
        );

        location
    }

    async fn fetch(&self, ip: &str) -> Option<String> {
        let url = format!("https://ipinfo.io/{}/json", ip);
        let mut req = self.client.get(&url);
        if let Some(token) = &self.api_token {
            req = req.bearer_auth(token);
        }

        let resp = req.send().await.ok()?;
        if !resp.status().is_success() {
            tracing::debug!(ip = ip, status = %resp.status(), "ipinfo lookup non-success");
            return None;
        }

        let info: IpInfoResponse = resp.json().await.ok()?;
        format_location(&info)
    }
}

fn format_location(info: &IpInfoResponse) -> Option<String> {
    let parts: Vec<&str> = [
        info.city.as_deref(),
        info.region.as_deref(),
        info.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|v| !v.is_empty())
    .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn is_public_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return false;
    };
    match addr {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified())
        }
        IpAddr::V6(v6) => !(v6.is_loopback() || v6.is_unspecified()),
    }
}
