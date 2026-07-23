//! Localized Open Graph copy for shared-file link previews.
//!
//! A link preview is fetched by the sharing platform's crawler (KakaoTalk,
//! Slack, X, iMessage, ...), not by the person who ends up seeing it, so the
//! viewer's language cannot be detected. Instead we render the preview in the
//! uploader's UI language, which is persisted on the share at upload time. When
//! that is unknown (older shares, CLI/API uploads, or an invalid link) we make a
//! best-effort guess from the request's `Accept-Language`, and finally fall back
//! to English.

/// Preview languages, mirroring the web app's i18n locales. English is the
/// fallback for anything we don't translate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OgLocale {
    Ko,
    En,
    Ja,
    ZhCn,
    ZhTw,
}

impl OgLocale {
    /// Resolve the preview language: the uploader's stored locale if known,
    /// otherwise a best-effort guess from `Accept-Language`, finally English.
    pub fn resolve(stored: Option<&str>, accept_language: Option<&str>) -> Self {
        stored
            .and_then(Self::from_tag)
            .or_else(|| accept_language.and_then(Self::from_accept_language))
            .unwrap_or(Self::En)
    }

    /// Parse a stored locale tag such as `"ko"` or `"zh-TW"`.
    fn from_tag(tag: &str) -> Option<Self> {
        match tag.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "ko" | "ko-kr" => Some(Self::Ko),
            "en" | "en-us" | "en-gb" => Some(Self::En),
            "ja" | "ja-jp" => Some(Self::Ja),
            "zh-cn" | "zh-hans" | "zh-hans-cn" | "zh-sg" | "zh" => Some(Self::ZhCn),
            "zh-tw" | "zh-hant" | "zh-hant-tw" | "zh-hk" | "zh-mo" => Some(Self::ZhTw),
            _ => None,
        }
    }

    /// Best-effort language pick from an `Accept-Language` header value.
    fn from_accept_language(header: &str) -> Option<Self> {
        for part in header.split(',') {
            let tag = part
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if tag.is_empty() {
                continue;
            }
            if let Some(loc) = Self::from_tag(&tag) {
                return Some(loc);
            }
            if tag.starts_with("ko") {
                return Some(Self::Ko);
            }
            if tag.starts_with("ja") {
                return Some(Self::Ja);
            }
            if tag.starts_with("en") {
                return Some(Self::En);
            }
            if tag.starts_with("zh") {
                if tag.contains("hant") || tag.contains("tw") || tag.contains("hk") || tag.contains("mo") {
                    return Some(Self::ZhTw);
                }
                return Some(Self::ZhCn);
            }
        }
        None
    }

    /// `og:locale` value, e.g. `"ko_KR"`.
    pub fn og_locale(self) -> &'static str {
        match self {
            Self::Ko => "ko_KR",
            Self::En => "en_US",
            Self::Ja => "ja_JP",
            Self::ZhCn => "zh_CN",
            Self::ZhTw => "zh_TW",
        }
    }

    /// `<html lang>` attribute value (BCP-47).
    pub fn html_lang(self) -> &'static str {
        match self {
            Self::Ko => "ko",
            Self::En => "en",
            Self::Ja => "ja",
            Self::ZhCn => "zh-Hans",
            Self::ZhTw => "zh-Hant",
        }
    }

    /// Shared "open the link to download" description, used by the P2P,
    /// password-protected, and multi-file previews.
    fn open_link_desc(self) -> &'static str {
        match self {
            Self::Ko => "ShareAnything - 링크를 열어 다운로드하세요.",
            Self::En => "ShareAnything - Open the link to download.",
            Self::Ja => "ShareAnything - リンクを開いてダウンロードしてください。",
            Self::ZhCn => "ShareAnything - 打开链接即可下载。",
            Self::ZhTw => "ShareAnything - 開啟連結即可下載。",
        }
    }

    /// Title + description for an invalid / expired / missing share.
    pub fn invalid(self) -> (String, String) {
        let (title, desc) = match self {
            Self::Ko => (
                "유효하지 않은 파일이에요.",
                "ShareAnything - 간편하고 안전하게 파일을 공유해보세요.",
            ),
            Self::En => (
                "This file is no longer available.",
                "ShareAnything - Share files simply and securely.",
            ),
            Self::Ja => (
                "無効なファイルです。",
                "ShareAnything - かんたん・安全にファイルを共有しましょう。",
            ),
            Self::ZhCn => (
                "文件无效或已失效。",
                "ShareAnything - 简单又安全地分享文件。",
            ),
            Self::ZhTw => (
                "檔案無效或已失效。",
                "ShareAnything - 簡單又安全地分享檔案。",
            ),
        };
        (title.to_string(), desc.to_string())
    }

    /// Title + description for a P2P share. `uploader` is the display name when
    /// known, otherwise an anonymous phrasing is used.
    pub fn p2p(self, uploader: Option<&str>) -> (String, String) {
        let title = match (self, uploader) {
            (Self::Ko, Some(name)) => format!("{}님이 파일을 공유했어요.", name),
            (Self::Ko, None) => "익명의 사용자가 파일을 공유했어요.".to_string(),
            (Self::En, Some(name)) => format!("{} shared a file with you.", name),
            (Self::En, None) => "Someone shared a file with you.".to_string(),
            (Self::Ja, Some(name)) => format!("{}さんがファイルを共有しました。", name),
            (Self::Ja, None) => "匿名のユーザーがファイルを共有しました。".to_string(),
            (Self::ZhCn, Some(name)) => format!("{} 分享了一个文件。", name),
            (Self::ZhCn, None) => "有人分享了一个文件。".to_string(),
            (Self::ZhTw, Some(name)) => format!("{} 分享了一個檔案。", name),
            (Self::ZhTw, None) => "有人分享了一個檔案。".to_string(),
        };
        (title, self.open_link_desc().to_string())
    }

    /// Title + description for a password-protected share.
    pub fn password(self) -> (String, String) {
        let title = match self {
            Self::Ko => "비밀번호가 걸린 파일이 공유되었어요.",
            Self::En => "A password-protected file was shared with you.",
            Self::Ja => "パスワード付きのファイルが共有されました。",
            Self::ZhCn => "有人分享了一个受密码保护的文件。",
            Self::ZhTw => "有人分享了一個受密碼保護的檔案。",
        };
        (title.to_string(), self.open_link_desc().to_string())
    }

    /// Title + description for a single shared file. The (language-neutral)
    /// title carries the file name; the description is localized.
    pub fn single(self, file_name: &str) -> (String, String) {
        let desc = match self {
            Self::Ko => "파일이 공유되었어요. 링크를 열어 다운로드하세요.",
            Self::En => "A file was shared with you. Open the link to download.",
            Self::Ja => "ファイルが共有されました。リンクを開いてダウンロードしてください。",
            Self::ZhCn => "有人与你分享了文件。打开链接即可下载。",
            Self::ZhTw => "有人與你分享了檔案。開啟連結即可下載。",
        };
        (format!("ShareAnything - {}", file_name), desc.to_string())
    }

    /// Title + description for a multi-file share.
    pub fn multiple(self, count: usize) -> (String, String) {
        let title = match self {
            Self::Ko => format!("{}개의 파일이 공유되었어요.", count),
            Self::En => format!("{} files were shared with you.", count),
            Self::Ja => format!("{}個のファイルが共有されました。", count),
            Self::ZhCn => format!("有人分享了 {} 个文件。", count),
            Self::ZhTw => format!("有人分享了 {} 個檔案。", count),
        };
        (title, self.open_link_desc().to_string())
    }
}
