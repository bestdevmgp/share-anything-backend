use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::config::Config;

#[derive(Clone)]
pub struct ConvertState {
    pub config: Arc<Config>,
}

pub async fn convert_hwp_to_pdf(
    State(state): State<ConvertState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let api_key = match &state.config.cloudconvert.api_key {
        Some(key) if !key.is_empty() => key.clone(),
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "CloudConvert API key not configured" })),
            )
                .into_response();
        }
    };

    let mut file_data: Option<Vec<u8>> = None;
    let mut file_name = String::from("document.hwp");

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            if let Some(name) = field.file_name() {
                file_name = name.to_string();
            }
            match field.bytes().await {
                Ok(bytes) => file_data = Some(bytes.to_vec()),
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "Failed to read file" })),
                    )
                        .into_response();
                }
            }
        }
    }

    let data = match file_data {
        Some(d) => d,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "No file provided" })),
            )
                .into_response();
        }
    };

    let client = reqwest::Client::new();

    let job_res = client
        .post("https://api.cloudconvert.com/v2/jobs")
        .bearer_auth(&api_key)
        .json(&json!({
            "tasks": {
                "upload": {
                    "operation": "import/upload"
                },
                "convert": {
                    "operation": "convert",
                    "input": "upload",
                    "output_format": "pdf"
                },
                "export": {
                    "operation": "export/url",
                    "input": "convert"
                }
            }
        }))
        .send()
        .await;

    let job_body: serde_json::Value = match job_res {
        Ok(res) if res.status().is_success() => match res.json().await {
            Ok(v) => v,
            Err(_) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({ "error": "Failed to parse CloudConvert response" })),
                )
                    .into_response();
            }
        },
        Ok(res) => {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            tracing::error!("CloudConvert job creation failed: {} {}", status, body);
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("CloudConvert error: {}", status) })),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("CloudConvert request failed: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "CloudConvert request failed" })),
            )
                .into_response();
        }
    };

    let upload_task = job_body["data"]["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["name"] == "upload"))
        .cloned();

    let upload_url = upload_task
        .as_ref()
        .and_then(|t| t["result"]["form"]["url"].as_str())
        .map(|s| s.to_string());

    let upload_params = upload_task
        .as_ref()
        .and_then(|t| t["result"]["form"]["parameters"].as_object())
        .cloned();

    let (upload_url, upload_params) = match (upload_url, upload_params) {
        (Some(url), Some(params)) => (url, params),
        _ => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "CloudConvert upload URL not found" })),
            )
                .into_response();
        }
    };

    let mut form = reqwest::multipart::Form::new();
    for (key, value) in &upload_params {
        if let Some(v) = value.as_str() {
            form = form.text(key.clone(), v.to_string());
        }
    }
    let part = reqwest::multipart::Part::bytes(data)
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .unwrap();
    form = form.part("file", part);

    if let Err(e) = client.post(&upload_url).multipart(form).send().await {
        tracing::error!("CloudConvert upload failed: {}", e);
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "File upload to CloudConvert failed" })),
        )
            .into_response();
    }

    let job_id = job_body["data"]["id"].as_str().unwrap_or("");
    let job_url = format!("https://api.cloudconvert.com/v2/jobs/{}", job_id);

    let mut pdf_url: Option<String> = None;
    for i in 0u64..60 {
        let delay_ms = if i < 5 { 500 } else if i < 15 { 1000 } else { 2000 };
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

        let status_res = client
            .get(&job_url)
            .bearer_auth(&api_key)
            .send()
            .await;

        if let Ok(res) = status_res {
            if let Ok(body) = res.json::<serde_json::Value>().await {
                let status = body["data"]["status"].as_str().unwrap_or("");
                if status == "finished" {
                    pdf_url = body["data"]["tasks"]
                        .as_array()
                        .and_then(|tasks| tasks.iter().find(|t| t["name"] == "export"))
                        .and_then(|t| t["result"]["files"].as_array())
                        .and_then(|files| files.first())
                        .and_then(|f| f["url"].as_str())
                        .map(|s| s.to_string());
                    break;
                } else if status == "error" {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({ "error": "CloudConvert conversion failed" })),
                    )
                        .into_response();
                }
            }
        }
    }

    let pdf_url = match pdf_url {
        Some(url) => url,
        None => {
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({ "error": "Conversion timed out" })),
            )
                .into_response();
        }
    };

    match client.get(&pdf_url).send().await {
        Ok(res) if res.status().is_success() => {
            let bytes = res.bytes().await.unwrap_or_default();
            (
                StatusCode::OK,
                [
                    ("content-type", "application/pdf"),
                    ("cache-control", "private, max-age=3600"),
                ],
                bytes,
            )
                .into_response()
        }
        _ => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "Failed to download converted PDF" })),
        )
            .into_response(),
    }
}
