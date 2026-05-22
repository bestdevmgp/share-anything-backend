use chrono_tz::Asia::Seoul;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct DiscordNotifier {
    webhook_url: Option<String>,
    application_webhook_url: Option<String>,
    client: reqwest::Client,
}

impl DiscordNotifier {
    pub fn new(webhook_url: Option<String>) -> Self {
        let application_webhook_url = std::env::var("DISCORD_APPLICATION_WEBHOOK_URL").ok()
            .or_else(|| webhook_url.clone());
        Self {
            webhook_url,
            application_webhook_url,
            client: reqwest::Client::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.webhook_url.is_some()
    }

    fn kst_now() -> String {
        chrono::Utc::now()
            .with_timezone(&Seoul)
            .format("%Y-%m-%d %H:%M:%S KST")
            .to_string()
    }

    pub fn notify_new_user(
        self: &Arc<Self>,
        name: &str,
        email: &str,
        provider: &str,
        ip: &str,
    ) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let name = name.to_string();
        let email = email.to_string();
        let provider = provider.to_string();
        let ip = ip.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .send_new_user_embed(&name, &email, &provider, &ip)
                .await
            {
                tracing::warn!("Discord new user notification failed: {}", e);
            }
        });
    }

    pub fn notify_server_error(
        self: &Arc<Self>,
        method: &str,
        uri: &str,
        status: &str,
        error_detail: &str,
        ip: &str,
    ) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let method = method.to_string();
        let uri = uri.to_string();
        let status = status.to_string();
        let error_detail = error_detail.to_string();
        let ip = ip.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .send_server_error_embed(&method, &uri, &status, &error_detail, &ip)
                .await
            {
                tracing::warn!("Discord error notification failed: {}", e);
            }
        });
    }

    pub fn notify_api_key_application(
        self: &Arc<Self>,
        application: &crate::models::ApiKeyApplication,
        applicant_name: &str,
        applicant_email: &str,
    ) {
        if self.application_webhook_url.is_none() {
            return;
        }
        let this = Arc::clone(self);
        let id = application.id;
        let user_id = application.user_id.clone();
        let service_name = application.service_name.clone();
        let service_url = application.service_url.clone();
        let purpose = application.purpose.clone();
        let scopes = application.scopes.clone();
        let ip = application.applicant_ip.clone().unwrap_or_else(|| "N/A".to_string());
        let platform = application.applicant_platform.clone().unwrap_or_else(|| "N/A".to_string());
        let created_at = application.created_at.to_rfc3339();
        let name = applicant_name.to_string();
        let email = applicant_email.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .send_application_embed(id, &user_id, &name, &email, &service_name, &service_url, &purpose, &scopes, &ip, &platform, &created_at)
                .await
            {
                tracing::warn!("Discord API key application notification failed: {}", e);
            }
        });
    }

    async fn send_application_embed(
        &self,
        id: i64,
        user_id: &str,
        name: &str,
        email: &str,
        service_name: &str,
        service_url: &str,
        purpose: &str,
        scopes: &str,
        ip: &str,
        platform: &str,
        created_at: &str,
    ) -> Result<(), reqwest::Error> {
        let url = self.application_webhook_url.as_ref().unwrap();
        let payload = json!({
            "embeds": [{
                "title": format!("🔑 New API Key Application #{}", id),
                "color": 5814783,
                "fields": [
                    { "name": "신청 ID", "value": id.to_string(), "inline": true },
                    { "name": "신청자", "value": format!("{} ({})", name, email), "inline": true },
                    { "name": "유저 ID", "value": user_id, "inline": false },
                    { "name": "서비스", "value": service_name, "inline": true },
                    { "name": "URL", "value": service_url, "inline": true },
                    { "name": "사용 목적", "value": purpose, "inline": false },
                    { "name": "요청 스코프", "value": scopes, "inline": true },
                    { "name": "신청 IP", "value": ip, "inline": true },
                    { "name": "Platform", "value": platform, "inline": true },
                    { "name": "신청 시간", "value": created_at, "inline": true }
                ]
            }]
        });
        self.client.post(url).json(&payload).send().await?;
        Ok(())
    }

    async fn send_new_user_embed(
        &self,
        name: &str,
        email: &str,
        provider: &str,
        ip: &str,
    ) -> Result<(), reqwest::Error> {
    let url = self.webhook_url.as_ref().unwrap();
        let description = format!(
            "**이름**\n{}\n\n**이메일**\n{}\n\n**Provider**\n{}\n\n**Client**\n{}\n\n**시간**\n{}",
            name, email, provider, ip, Self::kst_now()
        );
        let payload = json!({
            "embeds": [{
                "title": "🟢 신규 유저 가입",
                "color": 3066993,
                "description": description
            }]
        });

        self.client.post(url).json(&payload).send().await?;
        Ok(())
    }

    async fn send_server_error_embed(
        &self,
        method: &str,
        uri: &str,
        status: &str,
        error_detail: &str,
        ip: &str,
    ) -> Result<(), reqwest::Error> {
        let url = self.webhook_url.as_ref().unwrap();
        let description = format!(
            "**Status**\n{}\n\n**Endpoint**\n{} {}\n\n**Client**\n{}\n\n**상세 내용**\n```\n{}\n```\n\n**시간**\n{}",
            status, method, uri, ip, error_detail, Self::kst_now()
        );
        let payload = json!({
            "embeds": [{
                "title": "🔴 에러 발생",
                "color": 15158332,
                "description": description
            }]
        });

        self.client.post(url).json(&payload).send().await?;
        Ok(())
    }
}
