use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::sync::Arc;

use crate::config::SmtpConfig;

#[derive(Clone)]
pub struct EmailService {
    transport: Option<AsyncSmtpTransport<Tokio1Executor>>,
    from_email: String,
    from_name: String,
    frontend_url: String,
}

impl EmailService {
    pub fn new(config: &SmtpConfig, frontend_url: &str) -> Self {
        let mut from_email = None;
        let mut from_name = None;

        let transport = config.host.as_ref().and_then(|host| {
            let username = config.username.as_ref()?;
            let password = config.password.as_ref()?;
            let email = config.from_email.as_ref()?;
            let name = config.from_name.as_ref()?;

            from_email = Some(email.clone());
            from_name = Some(name.clone());

            let creds = Credentials::new(username.clone(), password.clone());

            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .ok()?
                .port(config.port)
                .credentials(creds)
                .build()
                .into()
        });

        Self {
            transport,
            from_email: from_email.unwrap_or_default(),
            from_name: from_name.unwrap_or_default(),
            frontend_url: frontend_url.to_string(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.transport.is_some()
    }

    pub fn send_welcome_email(self: &Arc<Self>, name: &str, email: &str) {
        if !self.is_enabled() {
            return;
        }

        let this = Arc::clone(self);
        let name = name.to_string();
        let email = email.to_string();

        tokio::spawn(async move {
            if let Err(e) = this.do_send_welcome_email(&name, &email).await {
                tracing::warn!("Welcome email send failed: {}", e);
            }
        });
    }

    async fn do_send_welcome_email(
        &self,
        name: &str,
        email: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = email.parse()?;

        let html_body = self.build_welcome_html(name);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(format!("{}님, ShareAnything에 오신 것을 환영합니다!", name))
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        tracing::info!("Welcome email sent to {}", email);
        Ok(())
    }

    fn build_welcome_html(&self, name: &str) -> String {
        let frontend_url = &self.frontend_url;

        format!(
            r##"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;background-color:#f4f4f5;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background-color:#f4f4f5;padding:40px 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0" style="background-color:#ffffff;border-radius:12px;overflow:hidden;box-shadow:0 1px 3px rgba(0,0,0,0.1);">

<!-- Header -->
<tr>
<td style="background:linear-gradient(135deg,#2563eb,#7c3aed);padding:40px 40px 32px;text-align:center;">
  <h1 style="margin:0;font-size:28px;font-weight:700;color:#ffffff;letter-spacing:-0.5px;">ShareAnything</h1>
  <p style="margin:8px 0 0;font-size:14px;color:rgba(255,255,255,0.85);">파일 공유, 더 쉽고 빠르게</p>
</td>
</tr>

<!-- Body -->
<tr>
<td style="padding:36px 40px 20px;">
  <h2 style="margin:0 0 8px;font-size:22px;font-weight:600;color:#18181b;">환영합니다, {name}님!</h2>
  <p style="margin:0 0 28px;font-size:15px;line-height:1.7;color:#52525b;">
    ShareAnything에 가입해 주셔서 감사합니다.<br>
    지금 바로 다양한 파일 공유 기능을 이용해 보세요.
  </p>

  <!-- Features -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:28px;">
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;margin-bottom:8px;">
        <table role="presentation" cellpadding="0" cellspacing="0"><tr>
          <td style="font-size:20px;padding-right:12px;vertical-align:top;">&#x1F4E4;</td>
          <td>
            <strong style="color:#18181b;font-size:14px;">서버 업로드</strong>
            <p style="margin:2px 0 0;font-size:13px;color:#71717a;">파일을 업로드하고 다운로드 코드를 공유하세요. 최대 5GB까지 지원합니다.</p>
          </td>
        </tr></table>
      </td>
    </tr>
    <tr><td style="height:8px;"></td></tr>
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" cellpadding="0" cellspacing="0"><tr>
          <td style="font-size:20px;padding-right:12px;vertical-align:top;">&#x1F91D;</td>
          <td>
            <strong style="color:#18181b;font-size:14px;">P2P 전송</strong>
            <p style="margin:2px 0 0;font-size:13px;color:#71717a;">서버를 거치지 않고 상대방에게 직접 파일을 전송합니다. 용량 제한 없이 빠르게!</p>
          </td>
        </tr></table>
      </td>
    </tr>
    <tr><td style="height:8px;"></td></tr>
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" cellpadding="0" cellspacing="0"><tr>
          <td style="font-size:20px;padding-right:12px;vertical-align:top;">&#x26A1;</td>
          <td>
            <strong style="color:#18181b;font-size:14px;">Quick Access</strong>
            <p style="margin:2px 0 0;font-size:13px;color:#71717a;">자주 사용하는 파일을 빠르게 저장하고 어디서든 접근하세요.</p>
          </td>
        </tr></table>
      </td>
    </tr>
  </table>

  <!-- CTA Button -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
    <tr>
      <td align="center" style="padding-bottom:8px;">
        <a href="{frontend_url}" style="display:inline-block;padding:14px 36px;background:linear-gradient(135deg,#2563eb,#7c3aed);color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;border-radius:8px;">
          ShareAnything 시작하기
        </a>
      </td>
    </tr>
  </table>
</td>
</tr>

<!-- Footer -->
<tr>
<td style="padding:20px 40px 32px;border-top:1px solid #f0f0f0;">
  <p style="margin:0;font-size:12px;color:#a1a1aa;text-align:center;line-height:1.6;">
    본 메일은 ShareAnything 회원가입 시 자동으로 발송되는 메일입니다.<br>
    &copy; ShareAnything
  </p>
</td>
</tr>

</table>
</td>
</tr>
</table>
</body>
</html>"##,
            name = name,
            frontend_url = frontend_url,
        )
    }
}
