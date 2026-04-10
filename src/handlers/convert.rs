use axum::{
    extract::{Path, State},
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

pub async fn create_convert_job(
    State(state): State<ConvertState>,
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
                return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "Failed to parse response" }))).into_response();
            }
        },
        _ => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "CloudConvert job creation failed" }))).into_response();
        }
    };

    let upload_task = job_body["data"]["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["name"] == "upload"));

    let upload_url = upload_task
        .and_then(|t| t["result"]["form"]["url"].as_str());
    let upload_params = upload_task
        .and_then(|t| t["result"]["form"]["parameters"].as_object());
    let job_id = job_body["data"]["id"].as_str().unwrap_or("");

    match (upload_url, upload_params) {
        (Some(url), Some(params)) => {
            (StatusCode::OK, Json(json!({
                "jobId": job_id,
                "uploadUrl": url,
                "uploadParams": params,
            }))).into_response()
        }
        _ => {
            (StatusCode::BAD_GATEWAY, Json(json!({ "error": "Upload URL not found" }))).into_response()
        }
    }
}

pub async fn get_convert_status(
    State(state): State<ConvertState>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    let api_key = match &state.config.cloudconvert.api_key {
        Some(key) if !key.is_empty() => key.clone(),
        _ => {
            return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": "Not configured" }))).into_response();
        }
    };

    let client = reqwest::Client::new();
    let job_url = format!("https://api.cloudconvert.com/v2/jobs/{}", job_id);

    let res = match client.get(&job_url).bearer_auth(&api_key).send().await {
        Ok(r) => r,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "Request failed" }))).into_response();
        }
    };

    let body: serde_json::Value = match res.json().await {
        Ok(v) => v,
        Err(_) => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": "Parse failed" }))).into_response();
        }
    };

    let status = body["data"]["status"].as_str().unwrap_or("unknown");

    if status == "finished" {
        let pdf_url = body["data"]["tasks"]
            .as_array()
            .and_then(|tasks| tasks.iter().find(|t| t["name"] == "export"))
            .and_then(|t| t["result"]["files"].as_array())
            .and_then(|files| files.first())
            .and_then(|f| f["url"].as_str())
            .unwrap_or("");

        return (StatusCode::OK, Json(json!({ "status": "finished", "pdfUrl": pdf_url }))).into_response();
    }

    (StatusCode::OK, Json(json!({ "status": status }))).into_response()
}
