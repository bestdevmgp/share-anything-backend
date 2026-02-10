use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use std::sync::Arc;
use tokio::process::Command;

use crate::{
    config::Config,
    db::{repository, DbPool},
    models::file_share::FileShare,
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

fn is_pdf_type(file_type: &str) -> bool {
    file_type == "application/pdf"
}

fn is_video_type(file_type: &str) -> bool {
    file_type.starts_with("video/")
}

fn is_previewable(file_type: &str) -> bool {
    is_image_type(file_type) || is_pdf_type(file_type) || is_video_type(file_type)
}

const MAX_THUMBNAIL_FILE_SIZE: i64 = 100 * 1024 * 1024;

fn thumbnail_s3_key(prefix: &str, file_id: &str) -> String {
    if prefix.is_empty() {
        format!("og-thumb/{}.jpg", file_id)
    } else {
        format!("{}og-thumb/{}.jpg", prefix.trim_end_matches('/'), file_id)
    }
}

async fn generate_video_thumbnail(presigned_url: &str) -> Option<Vec<u8>> {
    let output = match Command::new("ffmpeg")
        .args([
            "-i", presigned_url,
            "-vframes", "1",
            "-f", "image2pipe",
            "-vcodec", "mjpeg",
            "-q:v", "5",
            "-vf", "scale=1200:-1",
            "pipe:1",
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("ffmpeg not found or failed to execute: {}", e);
            return None;
        }
    };

    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        tracing::error!(
            "ffmpeg failed: status={}, stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        None
    }
}

async fn generate_pdf_thumbnail(storage: StorageService, storage_key: String) -> Option<Vec<u8>> {
    let temp_id = uuid::Uuid::new_v4().to_string();
    let temp_dir = std::env::temp_dir().join(format!("og-pdf-{}", temp_id));
    tokio::fs::create_dir_all(&temp_dir).await.ok()?;

    let input_file = temp_dir.join("input.pdf");
    let output_prefix = temp_dir.join("thumb");
    let output_file = temp_dir.join("thumb.jpg");

    let download_result = storage.download_file(&storage_key).await.map_err(|e| e.to_string());
    let pdf_data = match download_result {
        Ok(d) => d,
        Err(err_msg) => {
            tracing::error!("pdf thumbnail: S3 download failed: {}", err_msg);
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            return None;
        }
    };

    tracing::error!("pdf thumbnail: downloaded {} bytes from S3", pdf_data.len());

    if let Err(e) = tokio::fs::write(&input_file, &pdf_data).await {
        tracing::error!("pdf thumbnail: failed to write temp file: {}", e);
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        return None;
    }

    let result = match Command::new("pdftoppm")
        .args([
            "-jpeg", "-f", "1", "-l", "1", "-singlefile", "-scale-to", "1200",
            input_file.to_str().unwrap_or(""),
            output_prefix.to_str().unwrap_or(""),
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("pdftoppm failed to execute: {}", e);
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            return None;
        }
    };

    let data = if result.status.success() {
        match tokio::fs::read(&output_file).await {
            Ok(d) => Some(d),
            Err(e) => {
                tracing::error!("pdf thumbnail file read failed: {}", e);
                None
            }
        }
    } else {
        tracing::error!(
            "pdftoppm failed: status={}, stderr={}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        );
        None
    };

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    data
}

async fn resolve_og_image(state: &OgState, file: &FileShare) -> Option<String> {
    tracing::info!("resolve_og_image: file_type={}, file_id={}", file.file_type, file.id);

    if is_image_type(&file.file_type) {
        return state
            .storage
            .generate_presigned_get_url(&file.storage_key, 3600, None)
            .await
            .ok();
    }

    let thumb_key = thumbnail_s3_key(&state.config.s3.prefix, &file.id);

    if state.storage.key_exists(&thumb_key).await {
        tracing::info!("resolve_og_image: cached thumbnail found at {}", thumb_key);
        return state
            .storage
            .generate_presigned_get_url(&thumb_key, 3600, None)
            .await
            .ok();
    }

    let thumbnail_data = if is_video_type(&file.file_type) {
        let original_url = match state
            .storage
            .generate_presigned_get_url(&file.storage_key, 600, None)
            .await
        {
            Ok(url) => url,
            Err(e) => {
                tracing::error!("resolve_og_image: failed to get presigned url: {}", e);
                return None;
            }
        };
        generate_video_thumbnail(&original_url).await
    } else if is_pdf_type(&file.file_type) {
        generate_pdf_thumbnail(state.storage.clone(), file.storage_key.clone()).await
    } else {
        None
    };

    let thumbnail_data = match thumbnail_data {
        Some(d) => {
            tracing::info!("resolve_og_image: thumbnail generated, size={} bytes", d.len());
            d
        }
        None => {
            tracing::error!("resolve_og_image: thumbnail generation failed for {}", file.file_type);
            return None;
        }
    };

    if let Err(e) = state
        .storage
        .upload_file(&thumb_key, thumbnail_data, "image/jpeg")
        .await
    {
        tracing::error!("resolve_og_image: failed to upload thumbnail: {}", e);
        return None;
    }

    state
        .storage
        .generate_presigned_get_url(&thumb_key, 3600, None)
        .await
        .ok()
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
            let previewable = is_previewable(&first.file_type)
                && first.file_size <= MAX_THUMBNAIL_FILE_SIZE;
            (
                format!("ShareAnything - {}", first.file_name),
                "파일이 공유되었습니다. 링크를 열어 다운로드하세요.".to_string(),
                previewable,
            )
        } else {
            (
                format!("{}개의 파일이 공유되었습니다.", file_shares.len()),
                "ShareAnything - 링크를 열어 파일을 다운로드하세요.".to_string(),
                false,
            )
        }
    };

    let default_image = format!(
        "{}/og-image.png",
        frontend_url.trim_end_matches('/')
    );

    let og_image = if use_file_image {
        resolve_og_image(&state, &file_shares[0]).await.unwrap_or(default_image.clone())
    } else {
        default_image.clone()
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
        || !is_previewable(&file_shares[0].file_type)
        || file_shares[0].file_size > MAX_THUMBNAIL_FILE_SIZE
    {
        return Redirect::temporary(&default_image).into_response();
    }

    let file = &file_shares[0];

    if is_image_type(&file.file_type) {
        return match state
            .storage
            .generate_presigned_get_url(&file.storage_key, 3600, None)
            .await
        {
            Ok(url) => Redirect::temporary(&url).into_response(),
            Err(_) => Redirect::temporary(&default_image).into_response(),
        };
    }

    let thumb_key = thumbnail_s3_key(&state.config.s3.prefix, &file.id);

    if state.storage.key_exists(&thumb_key).await {
        if let Ok(url) = state
            .storage
            .generate_presigned_get_url(&thumb_key, 3600, None)
            .await
        {
            return Redirect::temporary(&url).into_response();
        }
    }

    let thumbnail_data = if is_video_type(&file.file_type) {
        let original_url = match state
            .storage
            .generate_presigned_get_url(&file.storage_key, 600, None)
            .await
        {
            Ok(url) => url,
            Err(_) => return Redirect::temporary(&default_image).into_response(),
        };
        generate_video_thumbnail(&original_url).await
    } else if is_pdf_type(&file.file_type) {
        generate_pdf_thumbnail(state.storage.clone(), file.storage_key.clone()).await
    } else {
        None
    };

    if let Some(data) = thumbnail_data {
        if state
            .storage
            .upload_file(&thumb_key, data, "image/jpeg")
            .await
            .is_ok()
        {
            if let Ok(url) = state
                .storage
                .generate_presigned_get_url(&thumb_key, 3600, None)
                .await
            {
                return Redirect::temporary(&url).into_response();
            }
        }
    }

    Redirect::temporary(&default_image).into_response()
}
