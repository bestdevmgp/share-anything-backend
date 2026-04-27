use askama::Template;
use lettre::{
    message::{header::ContentType, Mailbox},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::sync::Arc;

use crate::config::SmtpConfig;
use chrono::{DateTime, Datelike, Utc};

#[derive(Template)]
#[template(path = "welcome.html")]
struct WelcomeTemplate<'a> {
    name: &'a str,
    frontend_url: &'a str,
}

#[derive(Template)]
#[template(path = "magic_link.html")]
struct MagicLinkTemplate<'a> {
    email: &'a str,
    magic_link: &'a str,
    title: &'a str,
    desc: &'a str,
    link_label: &'a str,
    footer: &'a str,
    frontend_url: &'a str,
}

struct FileRow {
    file_name: String,
    label: &'static str,
    bg: &'static str,
    fg: &'static str,
    size: String,
}

impl FileRow {
    fn from_info(file: &FileNotificationInfo) -> Self {
        let (label, bg, fg) = file_type_label(&file.file_type);
        Self {
            file_name: file.file_name.clone(),
            label,
            bg,
            fg,
            size: format_file_size(file.file_size),
        }
    }

    fn list(files: &[FileNotificationInfo]) -> Vec<FileRow> {
        files.iter().map(Self::from_info).collect()
    }
}

#[derive(Template)]
#[template(path = "upload.html")]
struct UploadTemplate<'a> {
    html_lang: &'a str,
    title: String,
    t: &'static EmailTranslations,
    files: Vec<FileRow>,
    share_code: &'a str,
    description: Option<&'a str>,
    password: Option<&'a str>,
    expires_str: String,
    download_link: String,
    history_hint_html: String,
    disable_hint_html: String,
    frontend_url: &'a str,
}

#[derive(Template)]
#[template(path = "download.html")]
struct DownloadTemplate<'a> {
    html_lang: &'a str,
    title: String,
    t: &'static EmailTranslations,
    files: Vec<FileRow>,
    share_code: &'a str,
    uploader_name: Option<&'a str>,
    disable_hint_html: String,
    frontend_url: &'a str,
}

#[derive(Template)]
#[template(path = "download_alert.html")]
struct DownloadAlertTemplate<'a> {
    html_lang: &'a str,
    title: String,
    downloader_desc: String,
    t: &'static EmailTranslations,
    files: Vec<FileRow>,
    share_code: &'a str,
    downloader_display: &'a str,
    client_ip: &'a str,
    download_link: String,
    disable_hint_html: String,
    frontend_url: &'a str,
}

#[derive(Clone)]
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
            let _ = this.do_send_welcome_email(&name, &email).await;
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
        Ok(())
    }

    fn build_welcome_html(&self, name: &str) -> String {
        WelcomeTemplate {
            name,
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
    }

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
            let _ = this
                .do_send_upload_notification(&user_name, &user_email, &share_code, &files, expires_at, password.as_deref(), description.as_deref(), &lang)
                .await;
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
        Ok(())
    }

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
            let _ = this
                .do_send_download_notification(&user_name, &user_email, &share_code, &files, uploader_name.as_deref(), &lang)
                .await;
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
        Ok(())
    }

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
            let _ = this
                .do_send_download_alert_notification(&uploader_name, &uploader_email, downloader_name.as_deref(), &share_code, &files, &client_ip, &lang)
                .await;
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
        Ok(())
    }

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
        let expires_kst = expires_at + chrono::Duration::hours(9);
        let download_link = format!("{}/download/{}", &self.frontend_url, share_code);

        UploadTemplate {
            html_lang: html_lang_attr(lang),
            title: upload_title(lang, files.len()),
            t,
            files: FileRow::list(files),
            share_code,
            description,
            password,
            expires_str: format_date_localized(&expires_kst, lang),
            download_link,
            history_hint_html: upload_history_hint_html(lang, &self.frontend_url, t),
            disable_hint_html: notification_disable_hint_html(lang, &self.frontend_url),
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
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
        DownloadTemplate {
            html_lang: html_lang_attr(lang),
            title: download_title(lang, files.len()),
            t,
            files: FileRow::list(files),
            share_code,
            uploader_name,
            disable_hint_html: notification_disable_hint_html(lang, &self.frontend_url),
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
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
        let download_link = format!("{}/download/{}", &self.frontend_url, share_code);

        DownloadAlertTemplate {
            html_lang: html_lang_attr(lang),
            title: alert_title(lang, files.len()),
            downloader_desc: alert_desc(lang, downloader_name),
            t,
            files: FileRow::list(files),
            share_code,
            downloader_display: downloader_name.unwrap_or(t.anonymous_user),
            client_ip,
            download_link,
            disable_hint_html: notification_disable_hint_html(lang, &self.frontend_url),
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
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
            let _ = this.do_send_magic_link_email(&email, &token, &lang).await;
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
        Ok(())
    }

    fn build_magic_link_html(&self, email: &str, token: &str, lang: &str) -> String {
        let magic_link = format!("{}/auth/email/magic-link#{}", &self.frontend_url, token);

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

        MagicLinkTemplate {
            email,
            magic_link: &magic_link,
            title,
            desc,
            link_label,
            footer,
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
    }

}

fn file_type_label(file_type: &str) -> (&'static str, &'static str, &'static str) {
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
