use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use http_body_util::BodyExt;
use std::sync::Arc;

use crate::services::discord::DiscordNotifier;

pub async fn discord_error_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let uri = request.uri().path().to_string();
    let ip = crate::utils::client_ip(request.headers());
    let discord = request.extensions().get::<Arc<DiscordNotifier>>().cloned();

    let response = next.run(request).await;

    // Health probes are polled frequently by uptime monitors; a real DB/R2 outage
    // would otherwise post one Discord alert per poll. The monitor itself is the
    // alert channel for these, so skip Discord notification for /health* paths.
    let is_health = uri == "/health" || uri.starts_with("/health/") || uri == "/v1/health";

    if !is_health && response.status().is_server_error() {
        if let Some(discord) = discord {
            let status = response.status().to_string();

            let (parts, body) = response.into_parts();
            let bytes = body
                .collect()
                .await
                .map(|collected| collected.to_bytes())
                .unwrap_or_default();

            let error_detail = if bytes.is_empty() {
                status.clone()
            } else {
                let text = String::from_utf8_lossy(&bytes).to_string();
                if text.len() > 1000 {
                    format!("{}...", &text[..1000])
                } else {
                    text
                }
            };

            discord.notify_server_error(&method, &uri, &status, &error_detail, &ip);

            return Response::from_parts(parts, Body::from(bytes));
        }
    }

    response
}
