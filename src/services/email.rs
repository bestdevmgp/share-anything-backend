use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::sync::Arc;

use crate::config::SmtpConfig;
use chrono::{DateTime, Datelike, Utc};

pub struct FileNotificationInfo {
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
}

struct EmailTranslations {
    share_code_label: &'static str,
    password_label: &'static str,
    description_label: &'static str,
    expires_label: &'static str,
    uploader_label: &'static str,
    downloader_label: &'static str,
    anonymous_user: &'static str,

    upload_desc: &'static str,
    upload_history_link_text: &'static str,
    upload_cta: &'static str,
    upload_footer: &'static str,

    download_desc: &'static str,
    download_cta: &'static str,
    download_footer: &'static str,

    alert_cta: &'static str,
    alert_footer: &'static str,
}

fn get_email_translations(lang: &str) -> &'static EmailTranslations {
    match lang {
        "en" => &EmailTranslations {
            share_code_label: "Share Code",
            password_label: "Password",
            description_label: "Description",
            expires_label: "Expires",
            uploader_label: "Uploader",
            downloader_label: "Downloader",
            anonymous_user: "Anonymous User",

            upload_desc: "Check the uploaded file details.",
            upload_history_link_text: "Upload History",
            upload_cta: "Download File",
            upload_footer: "This email is automatically sent when a file is uploaded.",

            download_desc: "Check the downloaded file details.",
            download_cta: "Go to ShareAnything",
            download_footer: "This email is automatically sent when a file is downloaded.",

            alert_cta: "View File",
            alert_footer: "This email is automatically sent to uploaders when their file is downloaded.",
        },
        "ja" => &EmailTranslations {
            share_code_label: "共有コード",
            password_label: "パスワード",
            description_label: "説明",
            expires_label: "有効期限",
            uploader_label: "アップローダー",
            downloader_label: "ダウンローダー",
            anonymous_user: "未ログインユーザー",

            upload_desc: "アップロードされたファイル情報をご確認ください。",
            upload_history_link_text: "アップロード履歴ページ",
            upload_cta: "ファイルをダウンロード",
            upload_footer: "このメールはファイルアップロード時に自動送信される通知メールです。",

            download_desc: "ダウンロードされたファイル情報をご確認ください。",
            download_cta: "ShareAnythingへ移動",
            download_footer: "このメールはファイルダウンロード時に自動送信される通知メールです。",

            alert_cta: "ファイルを確認",
            alert_footer: "このメールはファイルダウンロード時にアップローダーへ自動送信される通知メールです。",
        },
        "zh-CN" => &EmailTranslations {
            share_code_label: "共享码",
            password_label: "密码",
            description_label: "说明",
            expires_label: "到期",
            uploader_label: "上传者",
            downloader_label: "下载者",
            anonymous_user: "未登录用户",

            upload_desc: "请确认上传的文件信息。",
            upload_history_link_text: "上传记录页面",
            upload_cta: "下载文件",
            upload_footer: "此邮件在文件上传时自动发送。",

            download_desc: "请确认下载的文件信息。",
            download_cta: "前往ShareAnything",
            download_footer: "此邮件在文件下载时自动发送。",

            alert_cta: "查看文件",
            alert_footer: "此邮件在文件被下载时自动发送给上传者。",
        },
        "zh-TW" => &EmailTranslations {
            share_code_label: "共享碼",
            password_label: "密碼",
            description_label: "說明",
            expires_label: "到期",
            uploader_label: "上傳者",
            downloader_label: "下載者",
            anonymous_user: "未登入用戶",

            upload_desc: "請確認上傳的檔案資訊。",
            upload_history_link_text: "上傳記錄頁面",
            upload_cta: "下載檔案",
            upload_footer: "此郵件在檔案上傳時自動發送。",

            download_desc: "請確認下載的檔案資訊。",
            download_cta: "前往ShareAnything",
            download_footer: "此郵件在檔案下載時自動發送。",

            alert_cta: "查看檔案",
            alert_footer: "此郵件在檔案被下載時自動發送給上傳者。",
        },
        // "ko" and any unknown language default to Korean
        _ => &EmailTranslations {
            share_code_label: "공유 코드",
            password_label: "비밀번호",
            description_label: "설명",
            expires_label: "만료",
            uploader_label: "업로더",
            downloader_label: "다운로더",
            anonymous_user: "비로그인 사용자",

            upload_desc: "업로드된 파일 정보를 확인하세요.",
            upload_history_link_text: "업로드 기록 페이지",
            upload_cta: "파일 다운로드",
            upload_footer: "본 메일은 파일 업로드 시 자동으로 발송되는 알림 메일입니다.",

            download_desc: "다운로드된 파일 정보를 확인하세요.",
            download_cta: "ShareAnything 이동",
            download_footer: "본 메일은 파일 다운로드 시 자동으로 발송되는 알림 메일입니다.",

            alert_cta: "파일 확인하기",
            alert_footer: "본 메일은 파일 다운로드 시 업로더에게 자동으로 발송되는 알림 메일입니다.",
        },
    }
}

// --- Dynamic title/subject formatting helpers ---

fn upload_title(lang: &str, count: usize) -> String {
    match lang {
        "en" => {
            if count == 1 {
                "File upload complete.".to_string()
            } else {
                format!("{} files upload complete.", count)
            }
        }
        "ja" => {
            if count == 1 {
                "ファイルのアップロードが完了しました。".to_string()
            } else {
                format!("{}個のファイルのアップロードが完了しました。", count)
            }
        }
        "zh-CN" => {
            if count == 1 {
                "文件上传完成。".to_string()
            } else {
                format!("{}个文件上传完成。", count)
            }
        }
        "zh-TW" => {
            if count == 1 {
                "檔案上傳完成。".to_string()
            } else {
                format!("{}個檔案上傳完成。", count)
            }
        }
        _ => {
            if count == 1 {
                "파일 업로드가 완료되었습니다.".to_string()
            } else {
                format!("{}개의 파일 업로드가 완료되었습니다.", count)
            }
        }
    }
}

fn upload_subject(lang: &str, file_name: &str, count: usize) -> String {
    match lang {
        "en" => {
            if count == 1 {
                format!("\"{}\" upload complete", file_name)
            } else {
                format!("{} files upload complete", count)
            }
        }
        "ja" => {
            if count == 1 {
                format!("「{}」のアップロードが完了しました", file_name)
            } else {
                format!("{}個のファイルのアップロードが完了しました", count)
            }
        }
        "zh-CN" => {
            if count == 1 {
                format!("\"{}\" 上传完成", file_name)
            } else {
                format!("{}个文件上传完成", count)
            }
        }
        "zh-TW" => {
            if count == 1 {
                format!("\"{}\" 上傳完成", file_name)
            } else {
                format!("{}個檔案上傳完成", count)
            }
        }
        _ => {
            if count == 1 {
                format!("\"{}\" 파일 업로드가 완료되었습니다.", file_name)
            } else {
                format!("{}개의 파일 업로드가 완료되었습니다.", count)
            }
        }
    }
}

fn download_title(lang: &str, count: usize) -> String {
    match lang {
        "en" => {
            if count == 1 {
                "File download complete.".to_string()
            } else {
                format!("{} files downloaded.", count)
            }
        }
        "ja" => {
            if count == 1 {
                "ファイルのダウンロードが完了しました。".to_string()
            } else {
                format!("{}個のファイルをダウンロードしました。", count)
            }
        }
        "zh-CN" => {
            if count == 1 {
                "文件下载完成。".to_string()
            } else {
                format!("已下载{}个文件。", count)
            }
        }
        "zh-TW" => {
            if count == 1 {
                "檔案下載完成。".to_string()
            } else {
                format!("已下載{}個檔案。", count)
            }
        }
        _ => {
            if count == 1 {
                "파일 다운로드가 완료되었습니다.".to_string()
            } else {
                format!("{}개의 파일 다운로드가 완료되었습니다.", count)
            }
        }
    }
}

fn download_subject(lang: &str, file_name: &str, count: usize) -> String {
    match lang {
        "en" => {
            if count == 1 {
                format!("\"{}\" download complete", file_name)
            } else {
                format!("{} files download complete", count)
            }
        }
        "ja" => {
            if count == 1 {
                format!("「{}」のダウンロードが完了しました", file_name)
            } else {
                format!("{}個のファイルのダウンロードが完了しました", count)
            }
        }
        "zh-CN" => {
            if count == 1 {
                format!("\"{}\" 下载完成", file_name)
            } else {
                format!("{}个文件下载完成", count)
            }
        }
        "zh-TW" => {
            if count == 1 {
                format!("\"{}\" 下載完成", file_name)
            } else {
                format!("{}個檔案下載完成", count)
            }
        }
        _ => {
            if count == 1 {
                format!("\"{}\" 파일 다운로드가 완료되었습니다.", file_name)
            } else {
                format!("{}개의 파일 다운로드가 완료되었습니다.", count)
            }
        }
    }
}

fn alert_title(lang: &str, count: usize) -> String {
    match lang {
        "en" => {
            if count == 1 {
                "Your file was downloaded.".to_string()
            } else {
                format!("{} of your files were downloaded.", count)
            }
        }
        "ja" => {
            if count == 1 {
                "ファイルがダウンロードされました。".to_string()
            } else {
                format!("{}個のファイルがダウンロードされました。", count)
            }
        }
        "zh-CN" => {
            if count == 1 {
                "文件已被下载。".to_string()
            } else {
                format!("{}个文件已被下载。", count)
            }
        }
        "zh-TW" => {
            if count == 1 {
                "檔案已被下載。".to_string()
            } else {
                format!("{}個檔案已被下載。", count)
            }
        }
        _ => {
            if count == 1 {
                "파일이 다운로드되었습니다.".to_string()
            } else {
                format!("{}개의 파일이 다운로드되었습니다.", count)
            }
        }
    }
}

fn alert_subject(lang: &str, downloader_name: Option<&str>, file_name: &str, count: usize) -> String {
    match lang {
        "en" => {
            let who = downloader_name.unwrap_or("An anonymous user");
            if count == 1 {
                format!("{} downloaded \"{}\"", who, file_name)
            } else {
                format!("{} downloaded {} files", who, count)
            }
        }
        "ja" => match downloader_name {
            Some(name) => {
                if count == 1 {
                    format!("{}さんが「{}」をダウンロードしました", name, file_name)
                } else {
                    format!("{}さんが{}個のファイルをダウンロードしました", name, count)
                }
            }
            None => {
                if count == 1 {
                    format!("匿名ユーザーが「{}」をダウンロードしました", file_name)
                } else {
                    format!("匿名ユーザーが{}個のファイルをダウンロードしました", count)
                }
            }
        },
        "zh-CN" => {
            let who = downloader_name.unwrap_or("匿名用户");
            if count == 1 {
                format!("{}下载了「{}」", who, file_name)
            } else {
                format!("{}下载了{}个文件", who, count)
            }
        }
        "zh-TW" => {
            let who = downloader_name.unwrap_or("匿名使用者");
            if count == 1 {
                format!("{}下載了「{}」", who, file_name)
            } else {
                format!("{}下載了{}個檔案", who, count)
            }
        }
        _ => match downloader_name {
            Some(name) => {
                if count == 1 {
                    format!("{}님이 \"{}\" 파일을 다운로드하였습니다.", name, file_name)
                } else {
                    format!("{}님이 {}개의 파일을 다운로드하였습니다.", name, count)
                }
            }
            None => {
                if count == 1 {
                    format!("익명의 사용자가 \"{}\" 파일을 다운로드하였습니다.", file_name)
                } else {
                    format!("익명의 사용자가 {}개의 파일을 다운로드하였습니다.", count)
                }
            }
        },
    }
}

fn alert_desc(lang: &str, downloader_name: Option<&str>) -> String {
    match lang {
        "en" => match downloader_name {
            Some(name) => format!("{} downloaded your file.", name),
            None => "An anonymous user downloaded your file.".to_string(),
        },
        "ja" => match downloader_name {
            Some(name) => format!("{}さんがあなたのファイルをダウンロードしました。", name),
            None => "匿名ユーザーがあなたのファイルをダウンロードしました。".to_string(),
        },
        "zh-CN" => match downloader_name {
            Some(name) => format!("{}下载了您的文件。", name),
            None => "匿名用户下载了您的文件。".to_string(),
        },
        "zh-TW" => match downloader_name {
            Some(name) => format!("{}下載了您的檔案。", name),
            None => "匿名使用者下載了您的檔案。".to_string(),
        },
        _ => match downloader_name {
            Some(name) => format!("{}님이 회원님의 파일을 다운로드하였습니다.", name),
            None => "익명의 사용자가 회원님의 파일을 다운로드하였습니다.".to_string(),
        },
    }
}

fn upload_history_hint_html(lang: &str, frontend_url: &str, t: &EmailTranslations) -> String {
    let link = format!(
        r#"<a href="{}/history" style="color:#2563eb;text-decoration:underline;">{}</a>"#,
        frontend_url, t.upload_history_link_text
    );
    match lang {
        "en" => format!(
            r#"<p style="margin:0 0 20px;font-size:13px;line-height:1.7;color:#71717a;">For more details, visit {}.</p>"#,
            link
        ),
        "ja" => format!(
            r#"<p style="margin:0 0 20px;font-size:13px;line-height:1.7;color:#71717a;">詳細は{}をご覧ください。</p>"#,
            link
        ),
        "zh-CN" => format!(
            r#"<p style="margin:0 0 20px;font-size:13px;line-height:1.7;color:#71717a;">更多详情请访问{}。</p>"#,
            link
        ),
        "zh-TW" => format!(
            r#"<p style="margin:0 0 20px;font-size:13px;line-height:1.7;color:#71717a;">更多詳情請訪問{}。</p>"#,
            link
        ),
        // ko default
        _ => format!(
            r#"<p style="margin:0 0 20px;font-size:13px;line-height:1.7;color:#71717a;">더 자세한 내용은 {}를 참고해주세요.</p>"#,
            link
        ),
    }
}

fn html_lang_attr(lang: &str) -> &str {
    match lang {
        "en" => "en",
        "ja" => "ja",
        "zh-CN" => "zh-CN",
        "zh-TW" => "zh-TW",
        _ => "ko",
    }
}

fn format_date_localized(dt: &DateTime<Utc>, lang: &str) -> String {
    let year = dt.format("%Y").to_string();
    let month = dt.month();
    let day = dt.day();

    match lang {
        "en" => {
            let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let month_name = month_names[(month - 1) as usize];
            format!("{} {}, {}", month_name, day, year)
        }
        "ja" => format!("{}年{}月{}日", year, month, day),
        "zh-CN" => format!("{}年{}月{}日", year, month, day),
        "zh-TW" => format!("{}年{}月{}日", year, month, day),
        _ => format!("{}년 {}월 {}일", year, month, day),
    }
}

fn notification_disable_hint_html(lang: &str, frontend_url: &str) -> String {
    let settings_url = format!("{}/settings?tab=notifications", frontend_url);
    let link = |text: &str| {
        format!(
            r#"<a href="{}" style="color:#71717a;text-decoration:underline;">{}</a>"#,
            settings_url, text
        )
    };
    match lang {
        "en" => format!("You can disable notifications in {}.", link("settings")),
        "ja" => format!("{}で通知を解除できます。", link("設定")),
        "zh-CN" => format!("您可以在{}中关闭通知。", link("设置")),
        "zh-TW" => format!("您可以在{}中關閉通知。", link("設定")),
        _ => format!("{}에서 알림을 해제할 수 있습니다.", link("설정")),
    }
}

/// Common email header: logo icon + "ShareAnything" + divider line
fn email_header_html(frontend_url: &str) -> String {
    format!(
        r#"<!-- Logo -->
<tr>
<td style="padding:32px 0 20px;text-align:center;">
  <table role="presentation" cellpadding="0" cellspacing="0" style="display:inline-table;"><tr>
    <td style="vertical-align:middle;">
      <img src="{frontend_url}/logo192.png" alt="ShareAnything" width="36" height="36" style="display:block;border:0;border-radius:10px;" />
    </td>
    <td style="padding-left:12px;vertical-align:middle;">
      <span style="font-size:20px;font-weight:700;color:#18181b;letter-spacing:-0.3px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;">ShareAnything</span>
    </td>
  </tr></table>
</td>
</tr>
<tr><td style="border-bottom:1px solid #e4e4e7;"></td></tr>"#,
        frontend_url = frontend_url
    )
}

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

    // ========================================================================
    // Welcome email (stays Korean, no lang param)
    // ========================================================================

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
        let header = email_header_html(&self.frontend_url);

        format!(
            r##"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:0 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0">

{header}

<!-- Body -->
<tr>
<td style="padding:28px 0 24px;">
  <p style="margin:0 0 24px;font-size:13px;color:#a1a1aa;">간편하고 안전한 파일 공유 서비스</p>
  <h2 style="margin:0 0 8px;font-size:20px;font-weight:600;color:#18181b;">환영합니다, {name}님!</h2>
  <p style="margin:0 0 28px;font-size:14px;line-height:1.7;color:#71717a;">
    ShareAnything에 가입해 주셔서 감사합니다.<br>
    지금 바로 다양한 파일 공유 기능을 이용해 보세요.
  </p>

  <!-- Features -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:28px;">
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <strong style="color:#18181b;font-size:14px;">서버 업로드</strong>
        <p style="margin:4px 0 0;font-size:13px;color:#71717a;">파일을 업로드하고 다운로드 코드를 공유하세요. 최대 5GB까지 지원합니다.</p>
      </td>
    </tr>
    <tr><td style="height:8px;"></td></tr>
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <strong style="color:#18181b;font-size:14px;">P2P 전송</strong>
        <p style="margin:4px 0 0;font-size:13px;color:#71717a;">서버를 거치지 않고 상대방에게 직접 파일을 전송합니다. 용량 제한 없이 빠르게!</p>
      </td>
    </tr>
    <tr><td style="height:8px;"></td></tr>
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <strong style="color:#18181b;font-size:14px;">Quick Access</strong>
        <p style="margin:4px 0 0;font-size:13px;color:#71717a;">자주 사용하는 파일을 빠르게 저장하고 어디서든 접근하세요.</p>
      </td>
    </tr>
  </table>

  <!-- CTA Button -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
    <tr>
      <td align="center">
        <a href="{frontend_url}" style="display:inline-block;padding:14px 36px;background-color:#2563eb;color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;border-radius:8px;">
          ShareAnything 시작하기
        </a>
      </td>
    </tr>
  </table>
</td>
</tr>

<!-- Footer -->
<tr>
<td style="padding:24px;background-color:#f4f4f5;margin-top:20px;">
  <p style="margin:0;font-size:12px;color:#a1a1aa;text-align:center;line-height:1.8;">
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
            header = header,
            name = name,
            frontend_url = frontend_url,
        )
    }

    // ========================================================================
    // Upload notification email
    // ========================================================================

    pub fn send_upload_notification(
        self: &Arc<Self>,
        user_name: &str,
        user_email: &str,
        share_code: &str,
        files: Vec<FileNotificationInfo>,
        expires_at: DateTime<Utc>,
        password: Option<String>,
        description: Option<String>,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }

        let this = Arc::clone(self);
        let user_name = user_name.to_string();
        let user_email = user_email.to_string();
        let share_code = share_code.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .do_send_upload_notification(&user_name, &user_email, &share_code, &files, expires_at, password.as_deref(), description.as_deref(), &lang)
                .await
            {
                tracing::warn!("Upload notification email send failed: {}", e);
            }
        });
    }

    async fn do_send_upload_notification(
        &self,
        user_name: &str,
        user_email: &str,
        share_code: &str,
        files: &[FileNotificationInfo],
        expires_at: DateTime<Utc>,
        password: Option<&str>,
        description: Option<&str>,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = user_email.parse()?;

        let subject = upload_subject(lang, &files[0].file_name, files.len());

        let html_body = self.build_upload_notification_html(user_name, share_code, files, expires_at, password, description, lang);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        tracing::info!("Upload notification email sent to {}", user_email);
        Ok(())
    }

    // ========================================================================
    // Download notification email
    // ========================================================================

    pub fn send_download_notification(
        self: &Arc<Self>,
        user_name: &str,
        user_email: &str,
        share_code: &str,
        files: Vec<FileNotificationInfo>,
        uploader_name: Option<&str>,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }

        let this = Arc::clone(self);
        let user_name = user_name.to_string();
        let user_email = user_email.to_string();
        let share_code = share_code.to_string();
        let uploader_name = uploader_name.map(|s| s.to_string());
        let lang = lang.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .do_send_download_notification(&user_name, &user_email, &share_code, &files, uploader_name.as_deref(), &lang)
                .await
            {
                tracing::warn!("Download notification email send failed: {}", e);
            }
        });
    }

    async fn do_send_download_notification(
        &self,
        user_name: &str,
        user_email: &str,
        share_code: &str,
        files: &[FileNotificationInfo],
        uploader_name: Option<&str>,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = user_email.parse()?;

        let subject = download_subject(lang, &files[0].file_name, files.len());

        let html_body = self.build_download_notification_html(user_name, share_code, files, uploader_name, lang);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        tracing::info!("Download notification email sent to {}", user_email);
        Ok(())
    }

    // ========================================================================
    // Download alert notification email
    // ========================================================================

    pub fn send_download_alert_notification(
        self: &Arc<Self>,
        uploader_name: &str,
        uploader_email: &str,
        downloader_name: Option<&str>,
        share_code: &str,
        files: Vec<FileNotificationInfo>,
        client_ip: &str,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }

        let this = Arc::clone(self);
        let uploader_name = uploader_name.to_string();
        let uploader_email = uploader_email.to_string();
        let downloader_name = downloader_name.map(|s| s.to_string());
        let share_code = share_code.to_string();
        let client_ip = client_ip.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .do_send_download_alert_notification(&uploader_name, &uploader_email, downloader_name.as_deref(), &share_code, &files, &client_ip, &lang)
                .await
            {
                tracing::warn!("Download alert notification email send failed: {}", e);
            }
        });
    }

    async fn do_send_download_alert_notification(
        &self,
        uploader_name: &str,
        uploader_email: &str,
        downloader_name: Option<&str>,
        share_code: &str,
        files: &[FileNotificationInfo],
        client_ip: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = uploader_email.parse()?;

        let subject = alert_subject(lang, downloader_name, &files[0].file_name, files.len());

        let html_body = self.build_download_alert_html(uploader_name, downloader_name, share_code, files, client_ip, lang);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        tracing::info!("Download alert notification email sent to {}", uploader_email);
        Ok(())
    }

    // ========================================================================
    // HTML builders
    // ========================================================================

    fn build_upload_notification_html(
        &self,
        _name: &str,
        share_code: &str,
        files: &[FileNotificationInfo],
        expires_at: DateTime<Utc>,
        password: Option<&str>,
        description: Option<&str>,
        lang: &str,
    ) -> String {
        let t = get_email_translations(lang);
        let html_lang = html_lang_attr(lang);
        let frontend_url = &self.frontend_url;
        let download_link = format!("{}/download/{}", frontend_url, share_code);

        let file_count = files.len();
        let title = upload_title(lang, file_count);

        let file_rows = Self::build_file_list_html(files);

        let expires_kst = expires_at + chrono::Duration::hours(9);
        let expires_str = format_date_localized(&expires_kst, lang);

        let description_row = if let Some(desc) = description {
            format!(
                r#"<tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{}: <strong style="color:#18181b;">{}</strong></td></tr>"#,
                t.description_label, desc
            )
        } else {
            String::new()
        };

        let password_row = if let Some(pw) = password {
            format!(
                r#"<tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{}: <strong style="color:#18181b;">{}</strong></td></tr>"#,
                t.password_label, pw
            )
        } else {
            String::new()
        };

        let history_hint = upload_history_hint_html(lang, frontend_url, t);
        let header = email_header_html(&self.frontend_url);
        let disable_hint = notification_disable_hint_html(lang, frontend_url);

        format!(
            r##"<!DOCTYPE html>
<html lang="{html_lang}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:0 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0">

{header}

<!-- Body -->
<tr>
<td style="padding:28px 0 24px;">
  <h2 style="margin:0 0 8px;font-size:20px;font-weight:600;color:#18181b;">{title}</h2>
  <p style="margin:0 0 16px;font-size:14px;line-height:1.7;color:#71717a;">
    {upload_desc}
  </p>
  {history_hint}

  <!-- File List -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:20px;">
{file_rows}
  </table>

  <!-- Info -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:28px;">
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
          <tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{share_code_label}: <strong style="color:#18181b;">{share_code}</strong></td></tr>
          {description_row}
          {password_row}
          <tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{expires_label}: <strong style="color:#18181b;">{expires_str}</strong></td></tr>
        </table>
      </td>
    </tr>
  </table>

  <!-- CTA Button -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
    <tr>
      <td align="center">
        <a href="{download_link}" style="display:inline-block;padding:14px 36px;background-color:#2563eb;color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;border-radius:8px;">
          {upload_cta}
        </a>
      </td>
    </tr>
  </table>
</td>
</tr>

<!-- Footer -->
<tr>
<td style="padding:24px;background-color:#f4f4f5;">
  <p style="margin:0;font-size:12px;color:#a1a1aa;text-align:center;line-height:1.8;">
    {upload_footer}<br>
    {disable_hint}<br>
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
            html_lang = html_lang,
            header = header,
            title = title,
            upload_desc = t.upload_desc,
            history_hint = history_hint,
            file_rows = file_rows,
            share_code_label = t.share_code_label,
            share_code = share_code,
            description_row = description_row,
            password_row = password_row,
            expires_label = t.expires_label,
            expires_str = expires_str,
            download_link = download_link,
            upload_cta = t.upload_cta,
            upload_footer = t.upload_footer,
            disable_hint = disable_hint,
        )
    }

    fn build_download_notification_html(
        &self,
        _name: &str,
        share_code: &str,
        files: &[FileNotificationInfo],
        uploader_name: Option<&str>,
        lang: &str,
    ) -> String {
        let t = get_email_translations(lang);
        let html_lang = html_lang_attr(lang);
        let frontend_url = &self.frontend_url;

        let file_count = files.len();
        let title = download_title(lang, file_count);

        let file_rows = Self::build_file_list_html(files);

        let uploader_row = if let Some(uname) = uploader_name {
            format!(
                r#"<tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{}: <strong style="color:#18181b;">{}</strong></td></tr>"#,
                t.uploader_label, uname
            )
        } else {
            String::new()
        };

        let header = email_header_html(&self.frontend_url);
        let disable_hint = notification_disable_hint_html(lang, frontend_url);

        format!(
            r##"<!DOCTYPE html>
<html lang="{html_lang}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:0 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0">

{header}

<!-- Body -->
<tr>
<td style="padding:28px 0 24px;">
  <h2 style="margin:0 0 8px;font-size:20px;font-weight:600;color:#18181b;">{title}</h2>
  <p style="margin:0 0 16px;font-size:14px;line-height:1.7;color:#71717a;">
    {download_desc}
  </p>

  <!-- File List -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:20px;">
{file_rows}
  </table>

  <!-- Info -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:28px;">
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
          <tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{share_code_label}: <strong style="color:#18181b;">{share_code}</strong></td></tr>
          {uploader_row}
        </table>
      </td>
    </tr>
  </table>

  <!-- CTA Button -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
    <tr>
      <td align="center">
        <a href="{frontend_url}" style="display:inline-block;padding:14px 36px;background-color:#2563eb;color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;border-radius:8px;">
          {download_cta}
        </a>
      </td>
    </tr>
  </table>
</td>
</tr>

<!-- Footer -->
<tr>
<td style="padding:24px;background-color:#f4f4f5;">
  <p style="margin:0;font-size:12px;color:#a1a1aa;text-align:center;line-height:1.8;">
    {download_footer}<br>
    {disable_hint}<br>
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
            html_lang = html_lang,
            header = header,
            title = title,
            download_desc = t.download_desc,
            file_rows = file_rows,
            share_code_label = t.share_code_label,
            share_code = share_code,
            uploader_row = uploader_row,
            frontend_url = frontend_url,
            download_cta = t.download_cta,
            download_footer = t.download_footer,
            disable_hint = disable_hint,
        )
    }

    fn build_download_alert_html(
        &self,
        _uploader_name: &str,
        downloader_name: Option<&str>,
        share_code: &str,
        files: &[FileNotificationInfo],
        client_ip: &str,
        lang: &str,
    ) -> String {
        let t = get_email_translations(lang);
        let html_lang = html_lang_attr(lang);
        let frontend_url = &self.frontend_url;
        let download_link = format!("{}/download/{}", frontend_url, share_code);

        let file_count = files.len();
        let title = alert_title(lang, file_count);

        let downloader_desc = alert_desc(lang, downloader_name);

        let file_rows = Self::build_file_list_html(files);

        let downloader_display = downloader_name.unwrap_or(t.anonymous_user);
        let downloader_row = format!(
            r#"<tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{}: <strong style="color:#18181b;">{}</strong> <span style="color:#a1a1aa;">({})</span></td></tr>"#,
            t.downloader_label, downloader_display, client_ip
        );

        let header = email_header_html(&self.frontend_url);
        let disable_hint = notification_disable_hint_html(lang, frontend_url);

        format!(
            r##"<!DOCTYPE html>
<html lang="{html_lang}">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:0 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0">

{header}

<!-- Body -->
<tr>
<td style="padding:28px 0 24px;">
  <h2 style="margin:0 0 8px;font-size:20px;font-weight:600;color:#18181b;">{title}</h2>
  <p style="margin:0 0 16px;font-size:14px;line-height:1.7;color:#71717a;">
    {downloader_desc}
  </p>

  <!-- File List -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:20px;">
{file_rows}
  </table>

  <!-- Info -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-bottom:28px;">
    <tr>
      <td style="padding:14px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
          <tr><td style="padding:6px 0;font-size:13px;color:#71717a;">{share_code_label}: <strong style="color:#18181b;">{share_code}</strong></td></tr>
          {downloader_row}
        </table>
      </td>
    </tr>
  </table>

  <!-- CTA Button -->
  <table role="presentation" width="100%" cellpadding="0" cellspacing="0">
    <tr>
      <td align="center">
        <a href="{download_link}" style="display:inline-block;padding:14px 36px;background-color:#2563eb;color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;border-radius:8px;">
          {alert_cta}
        </a>
      </td>
    </tr>
  </table>
</td>
</tr>

<!-- Footer -->
<tr>
<td style="padding:24px;background-color:#f4f4f5;">
  <p style="margin:0;font-size:12px;color:#a1a1aa;text-align:center;line-height:1.8;">
    {alert_footer}<br>
    {disable_hint}<br>
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
            html_lang = html_lang,
            header = header,
            title = title,
            downloader_desc = downloader_desc,
            file_rows = file_rows,
            share_code_label = t.share_code_label,
            share_code = share_code,
            downloader_row = downloader_row,
            download_link = download_link,
            alert_cta = t.alert_cta,
            alert_footer = t.alert_footer,
            disable_hint = disable_hint,
        )
    }

    pub fn send_magic_link_email(self: &Arc<Self>, email: &str, token: &str, lang: &str) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let email = email.to_string();
        let token = token.to_string();
        let lang = lang.to_string();
        tokio::spawn(async move {
            if let Err(e) = this.do_send_magic_link_email(&email, &token, &lang).await {
                tracing::warn!("Magic link email send failed: {}", e);
            }
        });
    }

    async fn do_send_magic_link_email(
        &self,
        email: &str,
        token: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = email.parse()?;

        let subject = match lang {
            "en" => "ShareAnything Sign-in Verification",
            "ja" => "ShareAnything ログイン認証",
            "zh-CN" => "ShareAnything 登录验证",
            "zh-TW" => "ShareAnything 登入驗證",
            _ => "ShareAnything 로그인 인증",
        };

        let html_body = self.build_magic_link_html(email, token, lang);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        tracing::info!("Magic link email sent to {}", email);
        Ok(())
    }

    fn build_magic_link_html(&self, email: &str, token: &str, lang: &str) -> String {
        let frontend_url = &self.frontend_url;
        let header = email_header_html(&self.frontend_url);
        let magic_link = format!("{}/auth/email/magic-link#{}", frontend_url, token);

        let (title, desc, link_label, footer) = match lang {
            "en" => (
                "Sign-in Verification",
                "Click the button below to sign in.",
                "Sign In",
                "This link expires in 10 minutes. If you did not request this, please ignore this email.",
            ),
            "ja" => (
                "ログイン認証",
                "下のボタンをクリックしてログインしてください。",
                "ログイン",
                "このリンクは10分後に期限切れになります。リクエストしていない場合は、このメールを無視してください。",
            ),
            "zh-CN" => (
                "登录验证",
                "点击下方按钮登录。",
                "登录",
                "此链接将在10分钟后过期。如果您未请求此操作，请忽略此邮件。",
            ),
            "zh-TW" => (
                "登入驗證",
                "點擊下方按鈕登入。",
                "登入",
                "此連結將在10分鐘後過期。如果您未請求此操作，請忽略此郵件。",
            ),
            _ => (
                "로그인 인증",
                "아래 버튼을 클릭하여 로그인하세요.",
                "로그인",
                "이 링크는 10분 후 만료됩니다. 본인이 요청하지 않았다면 이 이메일을 무시해 주세요.",
            ),
        };

        format!(
            r##"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
</head>
<body style="margin:0;padding:0;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,sans-serif;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="padding:0 20px;">
<tr>
<td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0">

{header}

<tr>
<td style="padding:28px 0 24px;text-align:center;">
  <h2 style="margin:0 0 8px;font-size:20px;font-weight:600;color:#18181b;">{title}</h2>
  <p style="margin:0 0 4px;font-size:13px;color:#a1a1aa;">{email}</p>
  <p style="margin:0 0 28px;font-size:14px;line-height:1.7;color:#71717a;">{desc}</p>

  <table role="presentation" cellpadding="0" cellspacing="0" style="margin:0 auto 28px;">
  <tr><td style="background-color:#2563eb;border-radius:8px;">
    <a href="{magic_link}" style="display:inline-block;padding:12px 32px;color:#ffffff;font-size:15px;font-weight:600;text-decoration:none;">{link_label}</a>
  </td></tr>
  </table>

  <p style="margin:0;font-size:12px;color:#a1a1aa;">{footer}</p>
</td>
</tr>

<tr><td style="border-top:1px solid #e4e4e7;padding:20px 0;text-align:center;">
  <span style="font-size:12px;color:#a1a1aa;">ShareAnything</span>
</td></tr>

</table>
</td>
</tr>
</table>
</body>
</html>"##,
            header = header,
            title = title,
            email = email,
            desc = desc,
            magic_link = magic_link,
            link_label = link_label,
            footer = footer,
        )
    }

    fn build_file_list_html(files: &[FileNotificationInfo]) -> String {
        let mut rows = String::new();
        for (i, file) in files.iter().enumerate() {
            let (label, bg, fg) = file_type_label(&file.file_type);
            let size = format_file_size(file.file_size);
            if i > 0 {
                rows.push_str("    <tr><td style=\"height:6px;\"></td></tr>\n");
            }
            rows.push_str(&format!(
                r#"    <tr>
      <td style="padding:10px 16px;background-color:#f8fafc;border-radius:8px;">
        <table role="presentation" cellpadding="0" cellspacing="0"><tr>
          <td style="padding-right:10px;vertical-align:middle;">
            <span style="display:inline-block;background:{bg};color:{fg};padding:2px 7px;border-radius:4px;font-size:11px;font-weight:700;letter-spacing:0.3px;line-height:18px;">{label}</span>
          </td>
          <td>
            <span style="color:#18181b;font-size:14px;font-weight:500;">{file_name}</span>
            <span style="color:#a1a1aa;font-size:13px;margin-left:8px;">{size}</span>
          </td>
        </tr></table>
      </td>
    </tr>
"#,
                bg = bg,
                fg = fg,
                label = label,
                file_name = file.file_name,
                size = size,
            ));
        }
        rows
    }
}

fn file_type_label(file_type: &str) -> (&str, &str, &str) {
    // (label, background_color, text_color)
    if file_type.starts_with("image/") {
        ("IMG", "#dbeafe", "#2563eb")
    } else if file_type.starts_with("video/") {
        ("VID", "#ede9fe", "#7c3aed")
    } else if file_type.starts_with("audio/") {
        ("AUD", "#fce7f3", "#db2777")
    } else if file_type.contains("pdf") {
        ("PDF", "#fee2e2", "#dc2626")
    } else if file_type.contains("zip") || file_type.contains("rar") || file_type.contains("tar") || file_type.contains("gzip") || file_type.contains("7z") {
        ("ZIP", "#fef3c7", "#d97706")
    } else if file_type.contains("word") || file_type.contains("document") {
        ("DOC", "#dbeafe", "#2563eb")
    } else if file_type.contains("sheet") || file_type.contains("excel") || file_type.contains("csv") {
        ("XLS", "#dcfce7", "#16a34a")
    } else if file_type.contains("presentation") || file_type.contains("powerpoint") {
        ("PPT", "#ffedd5", "#ea580c")
    } else if file_type.starts_with("text/") {
        ("TXT", "#f1f5f9", "#64748b")
    } else {
        ("FILE", "#f1f5f9", "#64748b")
    }
}

fn format_file_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes / KB)
    } else {
        format!("{} B", bytes as i64)
    }
}
