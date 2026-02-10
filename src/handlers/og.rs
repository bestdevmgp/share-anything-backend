use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use std::sync::Arc;

use crate::{
    config::Config,
    db::{repository, DbPool},
    services::StorageService,
};

#[derive(Clone)]
pub struct OgState {
    pub config: Arc<Config>,
    pub db: DbPool,
    pub storage: StorageService,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn is_image_type(file_type: &str) -> bool {
    file_type.starts_with("image/") && file_type != "image/svg+xml"
}

pub async fn get_og_page(
    State(state): State<OgState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let frontend_url = &state.config.server.frontend_url;
    let backend_url = &state.config.server.base_url;
    let og_url = format!("{}/download/{}", frontend_url.trim_end_matches('/'), code);
    let redirect_url = format!("{}?r=1", og_url);

    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .unwrap_or_default();

    let (og_title, og_description, use_file_image) = if file_shares.is_empty() {
        (
            "유효하지 않은 파일입니다.".to_string(),
            "ShareAnything - 간편하고 안전하게 파일을 공유해보세요.".to_string(),
            false,
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
                false,
            )
        } else if has_password {
            (
                "비밀번호가 걸린 파일이 공유되었습니다.".to_string(),
                "ShareAnything - 링크를 열어 다운로드하세요.".to_string(),
                false,
            )
        } else if file_shares.len() == 1 {
            let is_image = is_image_type(&first.file_type);
            (
                format!("ShareAnything - {}", first.file_name),
                "파일이 공유되었습니다. 링크를 열어 다운로드하세요.".to_string(),
                is_image,
            )
        } else {
            (
                format!("{}개의 파일이 공유되었습니다.", file_shares.len()),
                "ShareAnything - 링크를 열어 파일을 다운로드하세요.".to_string(),
                false,
            )
        }
    };

    let og_image = if use_file_image {
        format!(
            "{}/og/{}/image",
            backend_url.trim_end_matches('/'),
            code
        )
    } else {
        format!(
            "{}/og-image.png",
            frontend_url.trim_end_matches('/')
        )
    };

    let og_title_escaped = html_escape(&og_title);
    let og_description_escaped = html_escape(&og_description);
    let og_url_escaped = html_escape(&og_url);
    let redirect_url_escaped = html_escape(&redirect_url);

    let twitter_card = if use_file_image {
        "summary_large_image"
    } else {
        "summary"
    };

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
<meta name="twitter:card" content="{twitter_card}"/>
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
        twitter_card = twitter_card,
    );

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        Html(html),
    )
}

pub async fn get_og_image(
    State(state): State<OgState>,
    Path(code): Path<String>,
) -> impl IntoResponse {
    let frontend_url = &state.config.server.frontend_url;
    let default_image = format!(
        "{}/og-image.png",
        frontend_url.trim_end_matches('/')
    );

    let file_shares = repository::find_file_shares_by_code(&state.db, &code)
        .await
        .unwrap_or_default();

    if file_shares.len() != 1
        || file_shares[0].password_hash.is_some()
        || !is_image_type(&file_shares[0].file_type)
    {
        return Redirect::temporary(&default_image).into_response();
    }

    let file = &file_shares[0];

    match state
        .storage
        .generate_presigned_get_url(&file.storage_key, 3600, None)
        .await
    {
        Ok(presigned_url) => Redirect::temporary(&presigned_url).into_response(),
        Err(_) => Redirect::temporary(&default_image).into_response(),
    }
}
