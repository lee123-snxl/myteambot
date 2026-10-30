use serde_json::Value;

use crate::http_client::{HttpRequest, HttpResponse};
use crate::media::{self, Loaded, Piece};
use crate::providers::{self, Provider};

pub fn generate<F>(provider: &Provider, text: &str, transport: &mut F) -> Result<Piece, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let request = build(provider, text)?;
    let response = transport(&request)?;
    let loaded = parse(&provider.kind, response.status, &response.body, media::response_bytes(&response))?;
    media::finish_fetch(provider, loaded, "模型没有返回语音", transport)
}

pub fn build(provider: &Provider, text: &str) -> Result<HttpRequest, String> {
    media::ensure_task(provider, "speech")?;
    let model = media::require_model(provider)?;
    let voice = voice_name(provider);
    let (url, body) = match provider.kind.as_str() {
        "openai" => {
            let url = format!("{}/audio/speech", media::openai_root(&provider.base_url)?);
            let body = serde_json::json!({
                "model": model,
                "input": text,
                "voice": voice,
                "response_format": "mp3",
            });
            (url, body.to_string())
        }
        "azure" => {
            let url = media::azure_leaf(provider, "audio/speech")?;
            let body = serde_json::json!({
                "input": text,
                "voice": voice,
                "response_format": "mp3",
            });
            (url, body.to_string())
        }
        "gemini" => {
            let url = media::gemini_action(provider, "generateContent")?;
            let body = serde_json::json!({
                "contents": [{ "role": "user", "parts": [{ "text": text }] }],
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": {
                        "voiceConfig": {
                            "prebuiltVoiceConfig": { "voiceName": voice }
                        }
                    }
                }
            });
            (url, body.to_string())
        }
        _ => return Err(media::unsupported("speech")),
    };
    media::authed(provider, "POST", url, Some(body))
}

pub fn parse(kind: &str, status: u16, body: &str, bytes: &[u8]) -> Result<Loaded, String> {
    if !(200..300).contains(&status) {
        return Err(providers::explain_status(status, body));
    }
    let trimmed = body.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return parse_json(kind, body);
    }
    let raw = if !bytes.is_empty() { bytes } else { body.as_bytes() };
    Ok(Loaded::Bytes(media::piece_of(raw, "mp3", "已生成语音", "模型没有返回语音")?))
}

fn parse_json(kind: &str, body: &str) -> Result<Loaded, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| "模型返回的不是 JSON".to_string())?;
    if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
        return Err(message.to_string());
    }
    if kind == "gemini" {
        if let Some(reason) = value.pointer("/promptFeedback/blockReason").and_then(|item| item.as_str()) {
            return Err(format!("请求被拦截：{reason}"));
        }
    } else if !matches!(kind, "openai" | "azure" | "ollama") {
        return Err(media::unsupported("speech"));
    }
    if let Some(encoded) = value.get("audio").and_then(|item| item.as_str()).or_else(|| value.get("data").and_then(|item| item.as_str())) {
        let bytes = media::decode_b64(encoded)?;
        return Ok(Loaded::Bytes(media::piece_of(&bytes, "mp3", "已生成语音", "模型没有返回语音")?));
    }
    for (mime, encoded) in media::inline_parts(&value) {
        let audio = mime.is_empty() || mime.to_ascii_lowercase().starts_with("audio/") || media::is_pcm(&mime) || media::ext_from_mime(&mime).is_some_and(|ext| ext == "mp3" || ext == "wav");
        if !audio {
            continue;
        }
        let decoded = media::decode_b64(&encoded)?;
        let bytes = if media::is_pcm(&mime) {
            media::wav_from_pcm(&decoded, media::pcm_rate(&mime))
        } else {
            decoded
        };
        let fallback = if media::is_pcm(&mime) { "wav" } else { media::ext_from_mime(&mime).unwrap_or("mp3") };
        return Ok(Loaded::Bytes(media::piece_of(&bytes, fallback, "已生成语音", "模型没有返回语音")?));
    }
    if let Some(url) = value.pointer("/url").and_then(|item| item.as_str()) {
        if url.starts_with("http://") || url.starts_with("https://") {
            return Ok(Loaded::Fetch {
                url: url.to_string(),
                note: "已生成语音".into(),
                ext: "mp3".into(),
            });
        }
    }
    Err("模型没有返回语音".into())
}

fn voice_name(provider: &Provider) -> String {
    let voice = provider.voice.trim();
    if !voice.is_empty() {
        return voice.to_string();
    }
    if provider.kind == "gemini" {
        "Kore".to_string()
    } else {
        "alloy".to_string()
    }
}
