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
    html_lang: &'a str,
    greeting: String,
    frontend_url: &'a str,
    t: &'static WelcomeTranslations,
}

struct WelcomeTranslations {
    tagline: &'static str,
    intro: &'static str,
    feature_server_title: &'static str,
    feature_server_desc: &'static str,
    feature_p2p_title: &'static str,
    feature_p2p_desc: &'static str,
    feature_qa_title: &'static str,
    feature_qa_desc: &'static str,
    cta: &'static str,
    footer: &'static str,
}

fn get_welcome_translations(lang: &str) -> &'static WelcomeTranslations {
    match lang {
        "ko" => &WelcomeTranslations {
            tagline: "간편하고 안전한 파일 공유 서비스",
            intro: "ShareAnything에 가입해 주셔서 감사합니다.<br>지금 바로 다양한 파일 공유 기능을 이용해 보세요.",
            feature_server_title: "서버 업로드",
            feature_server_desc: "파일을 업로드하고 공유 코드로 다운로드하세요. 하루 최대 1TB까지 보낼 수 있어요.",
            feature_p2p_title: "P2P 보안 전송",
            feature_p2p_desc: "서버를 거치지 않고 상대방에게 직접 파일을 전송합니다. 용량 제한 없이 무료로 빠르게 공유해보세요. 모든 보안 전송 파일은 WebRTC DTLS로 종단간 암호화됩니다.",
            feature_qa_title: "Quick Access",
            feature_qa_desc: "여러 기기에서 같은 계정으로 로그인하여 파일을 빠르게 저장하고 어디서든 접근하세요.",
            cta: "ShareAnything 시작하기",
            footer: "본 메일은 ShareAnything 회원가입 시 자동으로 발송되는 메일입니다.",
        },
        "ja" => &WelcomeTranslations {
            tagline: "シンプルで安全なファイル共有サービス",
            intro: "ShareAnythingにご登録いただきありがとうございます。<br>今すぐ様々なファイル共有機能をお試しください。",
            feature_server_title: "サーバーアップロード",
            feature_server_desc: "ファイルをアップロードして共有コードでダウンロード。1日あたり最大1TBまで送れます。",
            feature_p2p_title: "P2Pセキュア送信",
            feature_p2p_desc: "サーバーを経由せず相手に直接ファイルを送信します。容量制限なく無料で素早く共有できます。すべてのセキュア送信ファイルはWebRTC DTLSでエンドツーエンド暗号化されます。",
            feature_qa_title: "Quick Access",
            feature_qa_desc: "同じアカウントで複数の端末からログインし、ファイルを素早く保存していつでもアクセスできます。",
            cta: "ShareAnythingを始める",
            footer: "本メールはShareAnythingご登録時に自動送信されるメールです。",
        },
        "zh-CN" => &WelcomeTranslations {
            tagline: "简单安全的文件共享服务",
            intro: "感谢您注册ShareAnything。<br>立即体验各种文件共享功能。",
            feature_server_title: "服务器上传",
            feature_server_desc: "上传文件并使用共享码下载，每日最多可发送 1TB。",
            feature_p2p_title: "P2P安全传输",
            feature_p2p_desc: "无需经过服务器，直接将文件发送给对方。免费快速共享，无大小限制。所有安全传输的文件均通过WebRTC DTLS进行端到端加密。",
            feature_qa_title: "Quick Access",
            feature_qa_desc: "在多个设备上使用同一账户登录，快速保存文件并随时随地访问。",
            cta: "开始使用ShareAnything",
            footer: "此邮件在注册ShareAnything时自动发送。",
        },
        "zh-TW" => &WelcomeTranslations {
            tagline: "簡單安全的檔案共享服務",
            intro: "感謝您註冊ShareAnything。<br>立即體驗各種檔案共享功能。",
            feature_server_title: "伺服器上傳",
            feature_server_desc: "上傳檔案並使用共享碼下載，每日最多可傳送 1TB。",
            feature_p2p_title: "P2P安全傳輸",
            feature_p2p_desc: "無需經過伺服器，直接將檔案傳送給對方。免費快速共享，無大小限制。所有安全傳輸的檔案均通過WebRTC DTLS進行端對端加密。",
            feature_qa_title: "Quick Access",
            feature_qa_desc: "在多個裝置上使用同一帳號登入，快速儲存檔案並隨時隨地存取。",
            cta: "開始使用ShareAnything",
            footer: "此郵件在註冊ShareAnything時自動發送。",
        },
        _ => &WelcomeTranslations {
            tagline: "Simple, secure file sharing",
            intro: "Thanks for joining ShareAnything.<br>Try out our file sharing features right away.",
            feature_server_title: "Server Upload",
            feature_server_desc: "Upload files and share them with a code. Send up to 1TB per day.",
            feature_p2p_title: "Secure P2P Transfer",
            feature_p2p_desc: "Send files directly to the other party without going through our servers. Share quickly and freely with no size limit. All secure transfers are end-to-end encrypted with WebRTC DTLS.",
            feature_qa_title: "Quick Access",
            feature_qa_desc: "Sign in to the same account on multiple devices to instantly store and access files anywhere.",
            cta: "Get Started",
            footer: "This email is automatically sent when you sign up for ShareAnything.",
        },
    }
}

fn welcome_subject(lang: &str, name: &str) -> String {
    match lang {
        "ko" => format!("{}님, ShareAnything에 오신 것을 환영합니다!", name),
        "ja" => format!("{}様、ShareAnythingへようこそ!", name),
        "zh-CN" => format!("{}，欢迎使用ShareAnything!", name),
        "zh-TW" => format!("{},歡迎使用ShareAnything!", name),
        _ => format!("{}, welcome to ShareAnything!", name),
    }
}

fn welcome_greeting(lang: &str, name: &str) -> String {
    match lang {
        "ko" => format!("환영합니다, {}님!", name),
        "ja" => format!("{}様、ようこそ!", name),
        "zh-CN" => format!("{},欢迎您!", name),
        "zh-TW" => format!("{},歡迎您!", name),
        _ => format!("Welcome, {}!", name),
    }
}

#[derive(Template)]
#[template(path = "api_key_approved.html")]
struct ApiKeyApprovedTemplate<'a> {
    html_lang: &'a str,
    intro: String,
    reveal_url: String,
    frontend_url: &'a str,
    t: &'static ApiKeyApprovedTranslations,
}

struct ApiKeyApprovedTranslations {
    cta: &'static str,
    expiry_notice: &'static str,
    footer: &'static str,
}

fn get_api_key_approved_translations(lang: &str) -> &'static ApiKeyApprovedTranslations {
    match lang {
        "en" => &ApiKeyApprovedTranslations {
            cta: "View API Key",
            expiry_notice: "This link expires in 7 days.",
            footer: "This email is automatically sent when your API Key application is approved.",
        },
        "ja" => &ApiKeyApprovedTranslations {
            cta: "API Key を確認",
            expiry_notice: "このリンクは 7 日後に無効になります。",
            footer: "本メールは API Key 申請が承認された際に自動送信される通知メールです。",
        },
        "zh-CN" => &ApiKeyApprovedTranslations {
            cta: "查看 API Key",
            expiry_notice: "此链接将在 7 天后失效。",
            footer: "此邮件在 API Key 申请获批时自动发送。",
        },
        "zh-TW" => &ApiKeyApprovedTranslations {
            cta: "查看 API Key",
            expiry_notice: "此連結將在 7 天後失效。",
            footer: "此郵件在 API Key 申請獲核准時自動發送。",
        },
        _ => &ApiKeyApprovedTranslations {
            cta: "API Key 확인하기",
            expiry_notice: "이 링크는 7일 후 만료됩니다.",
            footer: "본 메일은 API Key 신청 승인 시 자동으로 발송되는 알림 메일입니다.",
        },
    }
}

fn api_key_approved_intro(lang: &str, name: &str, service_name: &str) -> String {
    match lang {
        "en" => format!(
            "Hi {}, your OpenAPI Key application for <strong>\"{}\"</strong> has been approved.<br>Click the button below to view your API Key.",
            name, service_name
        ),
        "ja" => format!(
            "{}様、お申し込みいただいた「<strong>{}</strong>」の OpenAPI Key 発行申請が承認されました。<br>下のボタンから API Key をご確認ください。",
            name, service_name
        ),
        "zh-CN" => format!(
            "{},您申请的「<strong>{}</strong>」OpenAPI Key 发放申请已通过。<br>请点击下方按钮查看您的 API Key。",
            name, service_name
        ),
        "zh-TW" => format!(
            "{},您申請的「<strong>{}</strong>」OpenAPI Key 發放申請已核准。<br>請點選下方按鈕查看您的 API Key。",
            name, service_name
        ),
        _ => format!(
            "{}님, 신청하신 \"<strong>{}</strong>\"에 대한 OpenAPI Key 발급 신청이 승인되었습니다.<br>아래 버튼을 눌러 API Key를 확인하세요.",
            name, service_name
        ),
    }
}

fn api_key_approved_subject(lang: &str, service_name: &str) -> String {
    match lang {
        "en" => format!("[ShareAnything] API Key application for \"{}\" approved.", service_name),
        "ja" => format!("[ShareAnything] 「{}」の API Key 申請が承認されました。", service_name),
        "zh-CN" => format!("[ShareAnything] \"{}\" 的 API Key 申请已批准。", service_name),
        "zh-TW" => format!("[ShareAnything] \"{}\" 的 API Key 申請已核准。", service_name),
        _ => format!("[ShareAnything] \"{}\"에 대한 API Key 신청이 승인되었습니다.", service_name),
    }
}

#[derive(Template)]
#[template(path = "api_key_rejected.html")]
struct ApiKeyRejectedTemplate<'a> {
    html_lang: &'a str,
    intro: String,
    reason: &'a str,
    settings_url: String,
    frontend_url: &'a str,
    t: &'static ApiKeyRejectedTranslations,
}

struct ApiKeyRejectedTranslations {
    reason_label: &'static str,
    cta: &'static str,
    footer: &'static str,
}

fn get_api_key_rejected_translations(lang: &str) -> &'static ApiKeyRejectedTranslations {
    match lang {
        "en" => &ApiKeyRejectedTranslations {
            reason_label: "Reason",
            cta: "View Applications",
            footer: "This email is automatically sent when an API Key application is rejected.",
        },
        "ja" => &ApiKeyRejectedTranslations {
            reason_label: "却下理由",
            cta: "申請内容を確認",
            footer: "本メールは API Key 申請が却下された際に自動送信される通知メールです。",
        },
        "zh-CN" => &ApiKeyRejectedTranslations {
            reason_label: "未通过原因",
            cta: "查看申请记录",
            footer: "此邮件在 API Key 申请未通过时自动发送。",
        },
        "zh-TW" => &ApiKeyRejectedTranslations {
            reason_label: "未通過原因",
            cta: "查看申請記錄",
            footer: "此郵件在 API Key 申請未通過時自動發送。",
        },
        _ => &ApiKeyRejectedTranslations {
            reason_label: "반려 사유",
            cta: "신청 내역 확인하기",
            footer: "본 메일은 API Key 신청 반려 시 자동으로 발송되는 알림 메일입니다.",
        },
    }
}

fn api_key_rejected_intro(lang: &str, name: &str, service_name: &str) -> String {
    match lang {
        "en" => format!(
            "Hi {}, your OpenAPI Key application for <strong>\"{}\"</strong> has been rejected.<br>Please review the reason below, make adjustments, and submit a new application.",
            name, service_name
        ),
        "ja" => format!(
            "{}様、お申し込みいただいた「<strong>{}</strong>」の OpenAPI Key 発行申請が却下されました。<br>下記の理由をご確認のうえ、修正して再度お申し込みいただけます。",
            name, service_name
        ),
        "zh-CN" => format!(
            "{},您申请的「<strong>{}</strong>」OpenAPI Key 发放申请未通过。<br>请参考下方原因修改后再次申请。",
            name, service_name
        ),
        "zh-TW" => format!(
            "{},您申請的「<strong>{}</strong>」OpenAPI Key 發放申請未通過。<br>請參考下方原因修改後再次申請。",
            name, service_name
        ),
        _ => format!(
            "{}님, 신청하신 \"<strong>{}</strong>\"에 대한 OpenAPI Key 발급 신청이 반려되었습니다.<br>아래 반려 사유를 참고하여 수정 후 재신청할 수 있습니다.",
            name, service_name
        ),
    }
}

fn api_key_rejected_subject(lang: &str, service_name: &str) -> String {
    match lang {
        "en" => format!("[ShareAnything] API Key application for \"{}\" rejected.", service_name),
        "ja" => format!("[ShareAnything] 「{}」の API Key 申請が却下されました。", service_name),
        "zh-CN" => format!("[ShareAnything] \"{}\" 的 API Key 申请未通过。", service_name),
        "zh-TW" => format!("[ShareAnything] \"{}\" 的 API Key 申請未通過。", service_name),
        _ => format!("[ShareAnything] \"{}\"에 대한 API Key 신청이 반려되었습니다.", service_name),
    }
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

#[derive(Template)]
#[template(path = "device_confirm.html")]
struct DeviceConfirmTemplate<'a> {
    html_lang: &'a str,
    device_label: &'a str,
    ip_address: &'a str,
    location: Option<&'a str>,
    logged_in_at: String,
    revoke_link: String,
    t: &'static DeviceConfirmTranslations,
    frontend_url: &'a str,
}

struct DeviceConfirmTranslations {
    title: &'static str,
    device_label: &'static str,
    ip_label: &'static str,
    location_label: &'static str,
    time_label: &'static str,
    note: &'static str,
    revoke_button: &'static str,
    expiry_note: &'static str,
    footer: &'static str,
}

fn get_device_confirm_translations(lang: &str) -> &'static DeviceConfirmTranslations {
    match lang {
        "ko" => &DeviceConfirmTranslations {
            title: "[로그인 정보]",
            device_label: "기기",
            ip_label: "IP 주소",
            location_label: "위치",
            time_label: "로그인 시간",
            note: "회원님이 로그인한 것이 맞나요? 로그인 하지 않았다면 아래 버튼을 눌러주세요.",
            revoke_button: "내가 로그인 하지 않았습니다.",
            expiry_note: "이 링크는 7일 후 만료됩니다.",
            footer: "이 메일은 새 기기 로그인 감지 시 자동으로 발송되는 알림 메일입니다.",
        },
        "ja" => &DeviceConfirmTranslations {
            title: "[ログイン情報]",
            device_label: "端末",
            ip_label: "IPアドレス",
            location_label: "場所",
            time_label: "ログイン時刻",
            note: "ご本人によるログインですか？身に覚えがない場合は、下のボタンを押してください。",
            revoke_button: "私はログインしていません。",
            expiry_note: "このリンクは7日後に期限切れになります。",
            footer: "このメールは新しい端末からのログインを検知した際に自動送信される通知メールです。",
        },
        "zh-CN" => &DeviceConfirmTranslations {
            title: "[登录信息]",
            device_label: "设备",
            ip_label: "IP地址",
            location_label: "位置",
            time_label: "登录时间",
            note: "是您本人登录吗？如果不是您本人，请点击下方按钮。",
            revoke_button: "我没有登录。",
            expiry_note: "此链接将在7天后过期。",
            footer: "此邮件在检测到新设备登录时自动发送。",
        },
        "zh-TW" => &DeviceConfirmTranslations {
            title: "[登入資訊]",
            device_label: "裝置",
            ip_label: "IP位址",
            location_label: "位置",
            time_label: "登入時間",
            note: "是您本人登入嗎？如果不是您本人，請點擊下方按鈕。",
            revoke_button: "我沒有登入。",
            expiry_note: "此連結將在7天後過期。",
            footer: "此郵件在偵測到新裝置登入時自動傳送。",
        },
        _ => &DeviceConfirmTranslations {
            title: "[Sign-in details]",
            device_label: "Device",
            ip_label: "IP Address",
            location_label: "Location",
            time_label: "Sign-in Time",
            note: "Was this you? If not, click the button below.",
            revoke_button: "I didn't sign in.",
            expiry_note: "This link expires in 7 days.",
            footer: "This email is automatically sent when a sign-in from a new device is detected.",
        },
    }
}

fn device_confirm_subject(lang: &str) -> &'static str {
    match lang {
        "ko" => "[보안알림] 새로운 환경에서 로그인 되었습니다.",
        "ja" => "[セキュリティ] 新しい環境からのログインがありました",
        "zh-CN" => "[安全提醒] 您的账号有新的登录",
        "zh-TW" => "[安全提醒] 您的帳號有新的登入",
        _ => "[Security] New sign-in to your account",
    }
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
    base_url: String,
}

impl EmailService {
    pub fn new(config: &SmtpConfig, frontend_url: &str, base_url: &str) -> Self {
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
            base_url: base_url.to_string(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.transport.is_some()
    }

    pub fn send_welcome_email(self: &Arc<Self>, name: &str, email: &str, lang: &str) {
        if !self.is_enabled() {
            return;
        }

        let this = Arc::clone(self);
        let name = name.to_string();
        let email = email.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            let _ = this.do_send_welcome_email(&name, &email, &lang).await;
        });
    }

    async fn do_send_welcome_email(
        &self,
        name: &str,
        email: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = email.parse()?;

        let html_body = self.build_welcome_html(name, lang);

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(welcome_subject(lang, name))
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        Ok(())
    }

    fn build_welcome_html(&self, name: &str, lang: &str) -> String {
        WelcomeTemplate {
            html_lang: html_lang_attr(lang),
            greeting: welcome_greeting(lang, name),
            frontend_url: &self.frontend_url,
            t: get_welcome_translations(lang),
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

    pub fn send_new_device_notification(
        self: &Arc<Self>,
        email: &str,
        device_label: Option<&str>,
        ip_address: &str,
        location: Option<&str>,
        logged_in_at: DateTime<Utc>,
        revoke_token: &str,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let email = email.to_string();
        let device_label = device_label.map(|s| s.to_string());
        let ip_address = ip_address.to_string();
        let location = location.map(|s| s.to_string());
        let revoke_token = revoke_token.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            let _ = this
                .do_send_new_device_notification(
                    &email,
                    device_label.as_deref(),
                    &ip_address,
                    location.as_deref(),
                    logged_in_at,
                    &revoke_token,
                    &lang,
                )
                .await;
        });
    }

    async fn do_send_new_device_notification(
        &self,
        email: &str,
        device_label: Option<&str>,
        ip_address: &str,
        location: Option<&str>,
        logged_in_at: DateTime<Utc>,
        revoke_token: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = email.parse()?;

        let html_body = self.build_device_confirm_html(
            device_label,
            ip_address,
            location,
            logged_in_at,
            revoke_token,
            lang,
        );

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(device_confirm_subject(lang))
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        Ok(())
    }

    fn build_device_confirm_html(
        &self,
        device_label: Option<&str>,
        ip_address: &str,
        location: Option<&str>,
        logged_in_at: DateTime<Utc>,
        revoke_token: &str,
        lang: &str,
    ) -> String {
        let revoke_link =
            format!("{}/auth/device/revoke?token={}", &self.base_url, revoke_token);
        let logged_in_kst = logged_in_at + chrono::Duration::hours(9);
        let date_str = format_date_localized(&logged_in_kst, lang);
        let time_str = logged_in_kst.format("%H:%M").to_string();
        let logged_in_at_str = format!("{} {}", date_str, time_str);

        let unknown_label: &'static str = match lang {
            "ko" => "알 수 없음",
            "ja" => "不明",
            "zh-CN" => "未知",
            "zh-TW" => "未知",
            _ => "Unknown",
        };

        DeviceConfirmTemplate {
            html_lang: html_lang_attr(lang),
            device_label: device_label.unwrap_or(unknown_label),
            ip_address,
            location,
            logged_in_at: logged_in_at_str,
            revoke_link,
            t: get_device_confirm_translations(lang),
            frontend_url: &self.frontend_url,
        }
        .render()
        .unwrap_or_default()
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

    pub fn send_application_approved(
        self: &Arc<Self>,
        to_email: &str,
        applicant_name: &str,
        service_name: &str,
        reveal_token: &str,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let to_email = to_email.to_string();
        let applicant_name = applicant_name.to_string();
        let service_name = service_name.to_string();
        let reveal_token = reveal_token.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .do_send_application_approved(&to_email, &applicant_name, &service_name, &reveal_token, &lang)
                .await
            {
                tracing::warn!("Failed to send API key approval email: {}", e);
            }
        });
    }

    async fn do_send_application_approved(
        &self,
        to_email: &str,
        applicant_name: &str,
        service_name: &str,
        reveal_token: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = to_email.parse()?;

        let html_body = ApiKeyApprovedTemplate {
            html_lang: html_lang_attr(lang),
            intro: api_key_approved_intro(lang, applicant_name, service_name),
            reveal_url: format!("{}/api-keys/reveal/{}", self.frontend_url, reveal_token),
            frontend_url: &self.frontend_url,
            t: get_api_key_approved_translations(lang),
        }
        .render()
        .unwrap_or_default();

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(api_key_approved_subject(lang, service_name))
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        Ok(())
    }

    pub async fn send_api_key_expiration_warning(
        &self,
        to_email: &str,
        user_name: &str,
        service_name: &str,
        key_prefix: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
        notify_language: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !self.is_enabled() {
            return Ok(());
        }

        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = to_email.parse()?;

        let subject = match notify_language {
            "en" => "[ShareAnything] Your API Key is expiring soon",
            "ja" => "[ShareAnything] API Keyがまもなく期限切れになります",
            "zh-CN" => "[ShareAnything] 您的 API Key 即将过期",
            "zh-TW" => "[ShareAnything] 您的 API Key 即將過期",
            _ => "[ShareAnything] API Key가 곧 만료됩니다",
        };

        let now = chrono::Utc::now();
        let duration = expires_at.signed_duration_since(now);
        let days_remaining = duration.num_days().max(0);

        let display_expires = if notify_language == "ko" || notify_language == "ja" {
            expires_at + chrono::Duration::hours(9)
        } else {
            expires_at
        };
        let date_str = format_date_localized(&display_expires, notify_language);
        let time_str = display_expires.format("%H:%M").to_string();
        let timezone_label = if notify_language == "ko" || notify_language == "ja" {
            " (KST)"
        } else {
            " (UTC)"
        };
        let formatted_expires_at = format!("{} {}{}", date_str, time_str, timezone_label);

        let html_body = self.build_api_key_expiration_html(
            user_name,
            service_name,
            key_prefix,
            &formatted_expires_at,
            days_remaining,
            notify_language,
        );

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        Ok(())
    }

    fn build_api_key_expiration_html(
        &self,
        user_name: &str,
        service_name: &str,
        key_prefix: &str,
        formatted_expires_at: &str,
        days_remaining: i64,
        lang: &str,
    ) -> String {
        let html_lang = html_lang_attr(lang);
        let settings_url = format!("{}/settings?tab=api-keys", self.frontend_url);
        let frontend_url = &self.frontend_url;

        match lang {
            "en" => format!(
                r#"<!DOCTYPE html><html lang="{html_lang}"><head><meta charset="UTF-8"><title>API Key Expiring Soon</title></head><body style="font-family:sans-serif;background:#f9fafb;padding:40px 0;">
<div style="max-width:560px;margin:0 auto;background:#fff;border-radius:12px;padding:40px;border:1px solid #e5e7eb;">
<p style="margin:0 0 16px;font-size:15px;color:#111827;">Hi, {user_name}.</p>
<p style="margin:0 0 16px;font-size:15px;color:#111827;">The API Key used by <strong>{service_name}</strong> will expire in <strong>{days_remaining} day(s)</strong>.</p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">Key details:<br>
&nbsp;&nbsp;- Identifier: <code style="background:#f3f4f6;padding:2px 6px;border-radius:4px;">{key_prefix}...</code><br>
&nbsp;&nbsp;- Expiry: <strong>{formatted_expires_at}</strong></p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">Once expired, API calls will no longer work. Please issue a new API Key to continue.</p>
<p style="margin:0 0 24px;font-size:14px;color:#374151;"><a href="{settings_url}" style="color:#2563eb;text-decoration:underline;">Settings → API Keys</a> — issue a new key here.</p>
<p style="margin:0;font-size:14px;color:#374151;">Thank you,<br>The ShareAnything Team</p>
<hr style="margin:32px 0;border:none;border-top:1px solid #e5e7eb;">
<p style="margin:0;font-size:12px;color:#9ca3af;">© ShareAnything &nbsp;·&nbsp; <a href="{frontend_url}" style="color:#9ca3af;">{frontend_url}</a></p>
</div></body></html>"#,
                html_lang = html_lang,
                user_name = user_name,
                service_name = service_name,
                days_remaining = days_remaining,
                key_prefix = key_prefix,
                formatted_expires_at = formatted_expires_at,
                settings_url = settings_url,
                frontend_url = frontend_url,
            ),
            "ja" => format!(
                r#"<!DOCTYPE html><html lang="{html_lang}"><head><meta charset="UTF-8"><title>API Keyの期限切れ通知</title></head><body style="font-family:sans-serif;background:#f9fafb;padding:40px 0;">
<div style="max-width:560px;margin:0 auto;background:#fff;border-radius:12px;padding:40px;border:1px solid #e5e7eb;">
<p style="margin:0 0 16px;font-size:15px;color:#111827;">{user_name}様、こんにちは。</p>
<p style="margin:0 0 16px;font-size:15px;color:#111827;"><strong>{service_name}</strong> で使用中のAPI Keyが <strong>あと{days_remaining}日</strong> で期限切れになります。</p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">キー情報：<br>
&nbsp;&nbsp;- 識別子：<code style="background:#f3f4f6;padding:2px 6px;border-radius:4px;">{key_prefix}...</code><br>
&nbsp;&nbsp;- 期限：<strong>{formatted_expires_at}</strong></p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">期限切れになるとAPIコールが動作しなくなります。引き続きご利用の場合は、新しいAPI Keyを発行してください。</p>
<p style="margin:0 0 24px;font-size:14px;color:#374151;"><a href="{settings_url}" style="color:#2563eb;text-decoration:underline;">設定 → API Keys</a> から新しいキーを申請できます。</p>
<p style="margin:0;font-size:14px;color:#374151;">よろしくお願いいたします。<br>ShareAnythingチーム</p>
<hr style="margin:32px 0;border:none;border-top:1px solid #e5e7eb;">
<p style="margin:0;font-size:12px;color:#9ca3af;">© ShareAnything &nbsp;·&nbsp; <a href="{frontend_url}" style="color:#9ca3af;">{frontend_url}</a></p>
</div></body></html>"#,
                html_lang = html_lang,
                user_name = user_name,
                service_name = service_name,
                days_remaining = days_remaining,
                key_prefix = key_prefix,
                formatted_expires_at = formatted_expires_at,
                settings_url = settings_url,
                frontend_url = frontend_url,
            ),
            "zh-CN" => format!(
                r#"<!DOCTYPE html><html lang="{html_lang}"><head><meta charset="UTF-8"><title>API Key 即将过期</title></head><body style="font-family:sans-serif;background:#f9fafb;padding:40px 0;">
<div style="max-width:560px;margin:0 auto;background:#fff;border-radius:12px;padding:40px;border:1px solid #e5e7eb;">
<p style="margin:0 0 16px;font-size:15px;color:#111827;">您好，{user_name}。</p>
<p style="margin:0 0 16px;font-size:15px;color:#111827;"><strong>{service_name}</strong> 使用的 API Key 将在 <strong>{days_remaining} 天后</strong>过期。</p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">密钥信息：<br>
&nbsp;&nbsp;- 标识符：<code style="background:#f3f4f6;padding:2px 6px;border-radius:4px;">{key_prefix}...</code><br>
&nbsp;&nbsp;- 到期时间：<strong>{formatted_expires_at}</strong></p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">密钥过期后，API 调用将无法正常工作。如需继续使用，请申请新的 API Key。</p>
<p style="margin:0 0 24px;font-size:14px;color:#374151;"><a href="{settings_url}" style="color:#2563eb;text-decoration:underline;">设置 → API Keys</a> — 在此申请新密钥。</p>
<p style="margin:0;font-size:14px;color:#374151;">感谢您，<br>ShareAnything 团队</p>
<hr style="margin:32px 0;border:none;border-top:1px solid #e5e7eb;">
<p style="margin:0;font-size:12px;color:#9ca3af;">© ShareAnything &nbsp;·&nbsp; <a href="{frontend_url}" style="color:#9ca3af;">{frontend_url}</a></p>
</div></body></html>"#,
                html_lang = html_lang,
                user_name = user_name,
                service_name = service_name,
                days_remaining = days_remaining,
                key_prefix = key_prefix,
                formatted_expires_at = formatted_expires_at,
                settings_url = settings_url,
                frontend_url = frontend_url,
            ),
            "zh-TW" => format!(
                r#"<!DOCTYPE html><html lang="{html_lang}"><head><meta charset="UTF-8"><title>API Key 即將過期</title></head><body style="font-family:sans-serif;background:#f9fafb;padding:40px 0;">
<div style="max-width:560px;margin:0 auto;background:#fff;border-radius:12px;padding:40px;border:1px solid #e5e7eb;">
<p style="margin:0 0 16px;font-size:15px;color:#111827;">您好，{user_name}。</p>
<p style="margin:0 0 16px;font-size:15px;color:#111827;"><strong>{service_name}</strong> 使用的 API Key 將在 <strong>{days_remaining} 天後</strong>過期。</p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">金鑰資訊：<br>
&nbsp;&nbsp;- 識別碼：<code style="background:#f3f4f6;padding:2px 6px;border-radius:4px;">{key_prefix}...</code><br>
&nbsp;&nbsp;- 到期時間：<strong>{formatted_expires_at}</strong></p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">金鑰過期後，API 呼叫將無法正常運作。如需繼續使用，請申請新的 API Key。</p>
<p style="margin:0 0 24px;font-size:14px;color:#374151;"><a href="{settings_url}" style="color:#2563eb;text-decoration:underline;">設定 → API Keys</a> — 在此申請新金鑰。</p>
<p style="margin:0;font-size:14px;color:#374151;">感謝您，<br>ShareAnything 團隊</p>
<hr style="margin:32px 0;border:none;border-top:1px solid #e5e7eb;">
<p style="margin:0;font-size:12px;color:#9ca3af;">© ShareAnything &nbsp;·&nbsp; <a href="{frontend_url}" style="color:#9ca3af;">{frontend_url}</a></p>
</div></body></html>"#,
                html_lang = html_lang,
                user_name = user_name,
                service_name = service_name,
                days_remaining = days_remaining,
                key_prefix = key_prefix,
                formatted_expires_at = formatted_expires_at,
                settings_url = settings_url,
                frontend_url = frontend_url,
            ),
            _ => format!(
                r#"<!DOCTYPE html><html lang="{html_lang}"><head><meta charset="UTF-8"><title>API Key 만료 예정 안내</title></head><body style="font-family:sans-serif;background:#f9fafb;padding:40px 0;">
<div style="max-width:560px;margin:0 auto;background:#fff;border-radius:12px;padding:40px;border:1px solid #e5e7eb;">
<p style="margin:0 0 16px;font-size:15px;color:#111827;">안녕하세요, {user_name}님.</p>
<p style="margin:0 0 16px;font-size:15px;color:#111827;">'{service_name}' 서비스에서 사용 중인 API Key가 <strong>{days_remaining}일 후</strong>에 만료됩니다.</p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">키 정보:<br>
&nbsp;&nbsp;- 식별자: <code style="background:#f3f4f6;padding:2px 6px;border-radius:4px;">{key_prefix}...</code><br>
&nbsp;&nbsp;- 만료 일시: <strong>{formatted_expires_at}</strong></p>
<p style="margin:0 0 16px;font-size:14px;color:#374151;">키가 만료되면 OpenAPI 호출이 더 이상 작동하지 않습니다. 계속 사용하시려면 새 API Key를 발급받아 주세요.</p>
<p style="margin:0 0 24px;font-size:14px;color:#374151;"><a href="{settings_url}" style="color:#2563eb;text-decoration:underline;">설정 → API Keys</a>에서 새 키를 신청할 수 있습니다.</p>
<p style="margin:0;font-size:14px;color:#374151;">감사합니다.<br>ShareAnything 팀 드림.</p>
<hr style="margin:32px 0;border:none;border-top:1px solid #e5e7eb;">
<p style="margin:0;font-size:12px;color:#9ca3af;">© ShareAnything &nbsp;·&nbsp; <a href="{frontend_url}" style="color:#9ca3af;">{frontend_url}</a></p>
</div></body></html>"#,
                html_lang = html_lang,
                user_name = user_name,
                service_name = service_name,
                days_remaining = days_remaining,
                key_prefix = key_prefix,
                formatted_expires_at = formatted_expires_at,
                settings_url = settings_url,
                frontend_url = frontend_url,
            ),
        }
    }

    pub fn send_application_rejected(
        self: &Arc<Self>,
        to_email: &str,
        applicant_name: &str,
        service_name: &str,
        reason: &str,
        lang: &str,
    ) {
        if !self.is_enabled() {
            return;
        }
        let this = Arc::clone(self);
        let to_email = to_email.to_string();
        let applicant_name = applicant_name.to_string();
        let service_name = service_name.to_string();
        let reason = reason.to_string();
        let lang = lang.to_string();

        tokio::spawn(async move {
            if let Err(e) = this
                .do_send_application_rejected(&to_email, &applicant_name, &service_name, &reason, &lang)
                .await
            {
                tracing::warn!("Failed to send API key rejection email: {}", e);
            }
        });
    }

    async fn do_send_application_rejected(
        &self,
        to_email: &str,
        applicant_name: &str,
        service_name: &str,
        reason: &str,
        lang: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from: Mailbox = format!("{} <{}>", self.from_name, self.from_email).parse()?;
        let to: Mailbox = to_email.parse()?;

        let html_body = ApiKeyRejectedTemplate {
            html_lang: html_lang_attr(lang),
            intro: api_key_rejected_intro(lang, applicant_name, service_name),
            reason,
            settings_url: format!("{}/settings?tab=api-keys", self.frontend_url),
            frontend_url: &self.frontend_url,
            t: get_api_key_rejected_translations(lang),
        }
        .render()
        .unwrap_or_default();

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(api_key_rejected_subject(lang, service_name))
            .header(ContentType::TEXT_HTML)
            .body(html_body)?;

        self.transport.as_ref().unwrap().send(message).await?;
        Ok(())
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
