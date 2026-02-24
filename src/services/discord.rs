use chrono_tz::Asia::Seoul;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct DiscordNotifier {
    webhook_url: Option<String>,
    client: reqwest::Client,
}

impl DiscordNotifier {
    pub fn new(webhook_url: Option<String>) -> Self {
        Self {
            webhook_url,
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
