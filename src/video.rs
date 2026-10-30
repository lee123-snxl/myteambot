use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::http_client::{HttpRequest, HttpResponse};
use crate::media::{self, Piece};
use crate::providers::{self, Provider};

enum Step {
    Poll(HttpRequest),
    Download(HttpRequest),
    Done(Piece),
}

pub fn generate<F>(provider: &Provider, prompt: &str, pause: Duration, transport: &mut F) -> Result<Piece, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let mut request = build(provider, prompt)?;
    let mut polls = 0u32;
    loop {
        let response = transport(&request)?;
        match interpret(provider, &response)? {
            Step::Poll(next) => {
                polls += 1;
                if polls >= media::POLL_LIMIT {
                    return Err("视频生成超时".into());
                }
                thread::sleep(pause);
                request = next;
            }
            Step::Download(get) => {
                let got = transport(&get)?;
                return file_from_response(&got);
            }
            Step::Done(piece) => return Ok(piece),
        }
    }
}

pub fn build(provider: &Provider, prompt: &str) -> Result<HttpRequest, String> {
    media::ensure_task(provider, "video")?;
    let model = media::require_model(provider)?;
    let (url, body) = match provider.kind.as_str() {
        "openai" => {
            let url = format!("{}/videos", media::openai_root(&provider.base_url)?);
            let body = serde_json::json!({
                "model": model,
                "prompt": prompt,
                "size": "1280x720",
                "seconds": "4",
            });
            (url, body.to_string())
        }
        "azure" => {
            let url = media::azure_leaf(provider, "videos")?;
            let body = serde_json::json!({
                "prompt": prompt,
                "size": "1280x720",
                "seconds": "4",
            });
            (url, body.to_string())
        }
        "gemini" => {
            let url = media::gemini_action(provider, "predictLongRunning")?;
            let body = serde_json::json!({
                "instances": [{ "prompt": prompt }],
                "parameters": { "sampleCount": 1, "aspectRatio": "16:9" },
            });
            (url, body.to_string())
        }
        _ => return Err(media::unsupported("video")),
    };
    media::authed(provider, "POST", url, Some(body))
}

fn interpret(provider: &Provider, response: &HttpResponse) -> Result<Step, String> {
    if !(200..300).contains(&response.status) {
        return Err(providers::explain_status(response.status, &response.body));
    }
    let raw = media::response_bytes(response);
    let body = if response.body.is_empty() {
        String::from_utf8_lossy(raw).into_owned()
    } else {
        response.body.clone()
    };
    let trimmed = body.trim_start();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return Ok(Step::Done(media::piece_of(raw, "mp4", "已生成视频", "模型没有返回视频")?));
    }
    let value: Value = serde_json::from_str(&body).map_err(|_| "模型返回的不是 JSON".to_string())?;
    if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
        if provider.kind != "gemini" || value.get("done").and_then(|item| item.as_bool()).unwrap_or(true) {
            return Err(message.to_string());
        }
    }
    if provider.kind == "gemini" {
        interpret_gemini(provider, &value)
    } else {
        interpret_job(provider, &value)
    }
}

fn interpret_job(provider: &Provider, value: &Value) -> Result<Step, String> {
    let status = value.get("status").and_then(|item| item.as_str()).unwrap_or("").to_ascii_lowercase();
    let id = value.get("id").and_then(|item| item.as_str()).unwrap_or("").trim();
    if matches!(status.as_str(), "failed" | "error" | "cancelled" | "canceled" | "expired") {
        return Err(fail_text(value));
    }
    if let Some(encoded) = first_b64(value) {
        let bytes = media::decode_b64(encoded)?;
        return Ok(Step::Done(media::piece_of(&bytes, "mp4", "已生成视频", "模型没有返回视频")?));
    }
    let done = status.is_empty() || matches!(status.as_str(), "completed" | "succeeded" | "success" | "complete");
    if done {
        if let Some(url) = first_http_url(value) {
            return Ok(Step::Download(media::authed(provider, "GET", media::attach_key(provider, &url)?, None)?));
        }
    }
    if matches!(status.as_str(), "completed" | "succeeded" | "success" | "complete") {
        if id.is_empty() {
            return Err("模型没有返回视频".into());
        }
        return Ok(Step::Download(content_request(provider, id)?));
    }
    if !id.is_empty() {
        return Ok(Step::Poll(poll_request(provider, id)?));
    }
    Err("模型没有返回视频".into())
}

fn interpret_gemini(provider: &Provider, value: &Value) -> Result<Step, String> {
    let done = value.get("done").and_then(|item| item.as_bool()).unwrap_or(false);
    if done {
        if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
            return Err(message.to_string());
        }
        if let Some(encoded) = first_b64(value) {
            let bytes = media::decode_b64(encoded)?;
            return Ok(Step::Done(media::piece_of(&bytes, "mp4", "已生成视频", "模型没有返回视频")?));
        }
        if let Some(url) = first_http_url(value) {
            return Ok(Step::Download(media::authed(provider, "GET", media::attach_key(provider, &url)?, None)?));
        }
        return Err("模型没有返回视频".into());
    }
    let name = value.get("name").and_then(|item| item.as_str()).unwrap_or("").trim();
    if name.is_empty() {
        return Err("模型没有返回视频".into());
    }
    Ok(Step::Poll(operation_request(provider, name)?))
}

fn poll_request(provider: &Provider, id: &str) -> Result<HttpRequest, String> {
    let url = match provider.kind.as_str() {
        "azure" => media::azure_leaf(provider, &format!("videos/{}", providers::path_segment(id)))?,
        _ => format!("{}/videos/{}", media::openai_root(&provider.base_url)?, providers::path_segment(id)),
    };
    media::authed(provider, "GET", url, None)
}

fn content_request(provider: &Provider, id: &str) -> Result<HttpRequest, String> {
    let leaf = format!("videos/{}/content", providers::path_segment(id));
    let url = if provider.kind == "azure" {
        media::azure_leaf(provider, &leaf)?
    } else {
        format!("{}/{}", media::openai_root(&provider.base_url)?, leaf)
    };
    media::authed(provider, "GET", url, None)
}

fn operation_request(provider: &Provider, name: &str) -> Result<HttpRequest, String> {
    let sample = media::gemini_action(provider, "predictLongRunning")?;
    let url = if name.starts_with("http://") || name.starts_with("https://") {
        name.to_string()
    } else {
        let base = sample.split_once("/models/").map(|(base, _)| base).unwrap_or(sample.split('?').next().unwrap_or(&sample));
        format!("{}/{}", base.trim_end_matches('/'), name.trim_start_matches('/'))
    };
    media::authed(provider, "GET", media::attach_key(provider, &url)?, None)
}

fn file_from_response(response: &HttpResponse) -> Result<Piece, String> {
    if !(200..300).contains(&response.status) {
        return Err(providers::explain_status(response.status, &response.body));
    }
    let raw = media::response_bytes(response);
    let trimmed = response.body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(value) = serde_json::from_str::<Value>(&response.body) {
            if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
                return Err(message.to_string());
            }
            if let Some(encoded) = first_b64(&value) {
                let bytes = media::decode_b64(encoded)?;
                return media::piece_of(&bytes, "mp4", "已生成视频", "模型没有返回视频");
            }
        }
    }
    media::piece_of(raw, "mp4", "已生成视频", "模型没有返回视频")
}

fn fail_text(value: &Value) -> String {
    value
        .pointer("/error/message")
        .and_then(|item| item.as_str())
        .or_else(|| value.get("message").and_then(|item| item.as_str()))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or("视频生成失败")
        .to_string()
}

fn first_b64(value: &Value) -> Option<&str> {
    match value {
        Value::Object(map) => {
            for key in ["b64_json", "bytesBase64Encoded"] {
                if let Some(text) = map.get(key).and_then(|item| item.as_str()) {
                    if text.chars().any(|item| !item.is_whitespace()) {
                        return Some(text);
                    }
                }
            }
            map.values().find_map(first_b64)
        }
        Value::Array(items) => items.iter().find_map(first_b64),
        _ => None,
    }
}

fn first_http_url(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["url", "uri"] {
                if let Some(text) = map.get(key).and_then(|item| item.as_str()) {
                    if text.starts_with("http://") || text.starts_with("https://") {
                        return Some(text.to_string());
                    }
                }
            }
            map.values().find_map(first_http_url)
        }
        Value::Array(items) => items.iter().find_map(first_http_url),
        _ => None,
    }
}
