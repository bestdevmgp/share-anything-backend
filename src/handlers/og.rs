use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
};

#[derive(Clone)]
pub struct OgState {
    pub config: Arc<Config>,
    pub db: DbPool,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

pub async fn get_og_page(
    State(state): State<OgState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let frontend_url = &state.config.server.frontend_url;
    let og_url = format!("{}/download/{}", frontend_url.trim_end_matches('/'), code);
    let redirect_url = format!("{}?r=1", og_url);

    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .unwrap_or_default();

    let (og_title, og_description) = if file_shares.is_empty() {
        (
            "유효하지 않은 파일입니다.".to_string(),
            "ShareAnything - 간편하고 안전하게 파일을 공유해보세요.".to_string(),
        )
    } else {
        let first = &file_shares[0];
        let has_password = first.password_hash.is_some();
        let is_p2p = first.transfer_type == "p2p";

        if is_p2p {
            let uploader_name = if let Some(ref user_id) = first.user_id {
                repository::find_user_by_id(&state.db, user_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|u| u.name)
            } else {
                None
            };
            let (name, suffix) = match uploader_name {
                Some(n) => (n, "님이"),
                None => ("익명의 사용자".to_string(), "가"),
            };
            (
                format!("{}{} 파일을 공유하였습니다.", name, suffix),
                "ShareAnything - 링크를 열어 다운로드하세요.".to_string(),
            )
        } else if has_password {
            (
                "비밀번호가 걸린 파일이 공유되었습니다.".to_string(),
                "ShareAnything - 링크를 열어 다운로드하세요.".to_string(),
            )
        } else if file_shares.len() == 1 {
            (
                first.file_name.clone(),
                "ShareAnything - 파일이 공유되었습니다. 링크를 열어 다운로드하세요.".to_string(),
            )
        } else {
            (
                format!("{}개의 파일이 공유되었습니다.", file_shares.len()),
                "ShareAnything - 링크를 열어 파일을 다운로드하세요.".to_string(),
            )
        }
    };

    let og_title_escaped = html_escape(&og_title);
    let og_description_escaped = html_escape(&og_description);
    let og_url_escaped = html_escape(&og_url);
    let redirect_url_escaped = html_escape(&redirect_url);
    let og_image = format!(
        "{}/og-image.png",
        frontend_url.trim_end_matches('/')
    );

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>{title}</title>
<meta property="og:type" content="website"/>
<meta property="og:site_name" content="ShareAnything"/>
<meta property="og:title" content="{title}"/>
<meta property="og:description" content="{description}"/>
<meta property="og:image" content="{image}"/>
<meta property="og:url" content="{og_url}"/>
<meta name="twitter:card" content="summary"/>
<meta name="twitter:title" content="{title}"/>
<meta name="twitter:description" content="{description}"/>
<meta name="twitter:image" content="{image}"/>
<meta http-equiv="refresh" content="0;url={redirect_url}"/>
</head>
<body>
<script>window.location.replace("{redirect_url}");</script>
</body>
</html>"#,
        title = og_title_escaped,
        description = og_description_escaped,
        image = html_escape(&og_image),
        og_url = og_url_escaped,
        redirect_url = redirect_url_escaped,
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    )
}
