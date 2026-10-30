use serde_json::{json, Value};

use crate::http_client::{HttpRequest, HttpResponse};
use crate::media::{self, Loaded, Piece};
use crate::providers::{self, Provider};

pub fn generate<F>(provider: &Provider, prompt: &str, transport: &mut F) -> Result<Piece, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let request = build(provider, prompt)?;
    let response = transport(&request)?;
    let loaded = parse(&provider.kind, response.status, &response.body)?;
    media::finish_fetch(provider, loaded, "模型没有返回图片", transport)
}

pub fn build(provider: &Provider, prompt: &str) -> Result<HttpRequest, String> {
    media::ensure_task(provider, "image")?;
    let model = media::require_model(provider)?;
    let (url, body) = match provider.kind.as_str() {
        "openai" => {
            let url = format!("{}/images/generations", media::openai_root(&provider.base_url)?);
            let mut body = json!({
                "model": model,
                "prompt": prompt,
                "n": 1,
                "size": "1024x1024",
            });
            if !model.starts_with("gpt-image") {
                body["response_format"] = json!("b64_json");
            }
            (url, body.to_string())
        }
        "azure" => {
            let url = media::azure_leaf(provider, "images/generations")?;
            let body = json!({
                "prompt": prompt,
                "n": 1,
                "size": "1024x1024",
                "response_format": "b64_json",
            });
            (url, body.to_string())
        }
        "gemini" => {
            if model.contains("imagen") {
                let url = media::gemini_action(provider, "predict")?;
                let body = json!({
                    "instances": [{ "prompt": prompt }],
                    "parameters": { "sampleCount": 1 },
                });
                (url, body.to_string())
            } else {
                let url = media::gemini_action(provider, "generateContent")?;
                let body = json!({
                    "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
                    "generationConfig": { "responseModalities": ["TEXT", "IMAGE"] },
                });
                (url, body.to_string())
            }
        }
        _ => return Err(media::unsupported("image")),
    };
    media::authed(provider, "POST", url, Some(body))
}

pub fn parse(kind: &str, status: u16, body: &str) -> Result<Loaded, String> {
    if !(200..300).contains(&status) {
        return Err(providers::explain_status(status, body));
    }
    let value: Value = serde_json::from_str(body).map_err(|_| "模型返回的不是 JSON".to_string())?;
    if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
        return Err(message.to_string());
    }
    if kind == "gemini" {
        return parse_gemini(&value);
    }
    if !matches!(kind, "openai" | "azure" | "ollama") {
        return Err(media::unsupported("image"));
    }
    let data = value.pointer("/data/0");
    let note = media::clip_note(
        data.and_then(|item| item.get("revised_prompt")).and_then(|item| item.as_str()).unwrap_or(""),
        "已生成图片",
    );
    if let Some(encoded) = data
        .and_then(|item| item.get("b64_json").or_else(|| item.get("image")))
        .and_then(|item| item.as_str())
    {
        let bytes = media::decode_b64(encoded)?;
        let ext = media::sniff(&bytes, "png").unwrap_or_else(|_| "png".to_string());
        return Ok(Loaded::Bytes(Piece { bytes, ext, note }));
    }
    if let Some(url) = data.and_then(|item| item.get("url")).and_then(|item| item.as_str()) {
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(Loaded::Fetch { url: url.to_string(), note, ext: "png".into() });
        }
    }
    Err("模型没有返回图片".into())
}

fn parse_gemini(value: &Value) -> Result<Loaded, String> {
    if let Some(reason) = value.pointer("/promptFeedback/blockReason").and_then(|item| item.as_str()) {
        return Err(format!("请求被拦截：{reason}"));
    }
    let note = media::clip_note(&media::joined_text(value), "已生成图片");
    for (mime, encoded) in media::inline_parts(value) {
        let image = mime.is_empty() || mime.to_ascii_lowercase().starts_with("image/") || media::ext_from_mime(&mime).is_some_and(|ext| ext == "png" || ext == "jpg");
        if !image {
            continue;
        }
        let bytes = media::decode_b64(&encoded)?;
        let ext = media::ext_from_mime(&mime)
            .filter(|ext| *ext == "png" || *ext == "jpg")
            .map(|ext| ext.to_string())
            .unwrap_or_else(|| media::sniff(&bytes, "png").unwrap_or_else(|_| "png".to_string()));
        return Ok(Loaded::Bytes(Piece { bytes, ext, note }));
    }
    Err("模型没有返回图片".into())
}
