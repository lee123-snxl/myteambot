use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use serde_json::Value;

use crate::http_client::{HttpRequest, HttpResponse};
use crate::providers::{self, Provider};
use crate::store::{self, Store};

pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const POLL_PAUSE: Duration = Duration::from_secs(2);
pub const POLL_LIMIT: u32 = 150;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub text: String,
    pub file: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Outcome {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            file: String::new(),
            input_tokens: 0,
            output_tokens: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Piece {
    pub bytes: Vec<u8>,
    pub ext: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    Bytes(Piece),
    Fetch { url: String, note: String, ext: String },
}

pub fn bot_dir(id: &str) -> Result<String, String> {
    if !store::is_safe_id(id) {
        return Err("非法 Bot 编号".into());
    }
    Ok(format!("media/bots/{id}"))
}

pub fn group_dir(id: &str) -> Result<String, String> {
    if !store::is_safe_id(id) {
        return Err("非法群组编号".into());
    }
    Ok(format!("media/groups/{id}"))
}

pub fn generate<F>(
    store: &Store,
    provider: &Provider,
    style: &str,
    user_text: &str,
    dir: &str,
    pause: Duration,
    mut transport: F,
) -> Result<Outcome, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let task = providers::task_of(&provider.task)?;
    if !providers::supports_task(&provider.kind, task) {
        return Err(unsupported(task));
    }
    let user_text = user_text.trim();
    if user_text.is_empty() {
        return Err("没有可发送的用户消息".into());
    }
    let piece = match task {
        "image" => {
            let prompt = join_prompt(style, user_text, 4000)?;
            crate::image_gen::generate(provider, &prompt, &mut transport)?
        }
        "speech" => {
            if user_text.chars().count() > 4096 {
                return Err("要朗读的文字过长".into());
            }
            crate::speech::generate(provider, user_text, &mut transport)?
        }
        "video" => {
            let prompt = join_prompt(style, user_text, 4000)?;
            crate::video::generate(provider, &prompt, pause, &mut transport)?
        }
        _ => return Err("不支持的用途".into()),
    };
    let file = save(store, dir, &piece.bytes, &piece.ext)?;
    Ok(Outcome {
        text: piece.note,
        file,
        input_tokens: 0,
        output_tokens: 0,
    })
}

pub fn save(store: &Store, dir: &str, bytes: &[u8], ext: &str) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("模型没有返回文件".into());
    }
    if bytes.len() > MAX_BYTES {
        return Err("文件过大".into());
    }
    if !matches!(ext, "png" | "jpg" | "mp3" | "wav" | "mp4") {
        return Err("不支持的文件类型".into());
    }
    if !safe_dir(dir) {
        return Err("非法路径".into());
    }
    let rel = format!("{dir}/{}.{ext}", store::new_id());
    store.write_file(&rel, bytes)?;
    Ok(rel)
}

pub fn saved_path(store: &Store, rel: &str) -> Result<PathBuf, String> {
    if !safe_media_rel(rel) {
        return Err("非法路径".into());
    }
    let path = store.root().join(rel);
    if !path.is_file() {
        return Err("文件不存在".into());
    }
    Ok(path)
}

pub fn open_saved(store: &Store, rel: &str) -> Result<(), String> {
    let path = saved_path(store, rel)?;
    std::process::Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("无法打开：{err}"))
}

pub fn decode_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let image = image::load_from_memory(bytes).map_err(|_| "无法显示图片".to_string())?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        return Err("无法显示图片".into());
    }
    Ok((width, height, rgba.into_raw()))
}

pub(crate) fn unsupported(task: &str) -> String {
    match task {
        "image" => "这个接口不能生图".into(),
        "speech" => "这个接口不能生成语音".into(),
        "video" => "这个接口不能生成视频".into(),
        _ => "这个接口不能用于该用途".into(),
    }
}

pub(crate) fn require_model(provider: &Provider) -> Result<&str, String> {
    let model = provider.model.trim();
    if model.is_empty() {
        Err(if provider.kind == "azure" {
            "请填写 Azure 部署名".into()
        } else {
            "请填写模型名".into()
        })
    } else {
        Ok(model)
    }
}

pub(crate) fn ensure_task(provider: &Provider, task: &str) -> Result<(), String> {
    if providers::supports_task(&provider.kind, task) {
        Ok(())
    } else {
        Err(unsupported(task))
    }
}

pub(crate) fn openai_root(base: &str) -> Result<String, String> {
    let models = providers::openai_models_url(base)?;
    let root = models.trim_end_matches("/models").trim_end_matches('/');
    if root.is_empty() {
        return Err("接口地址无效".into());
    }
    Ok(root.to_string())
}

pub(crate) fn azure_leaf(provider: &Provider, leaf: &str) -> Result<String, String> {
    let chat = providers::azure_chat_url(&provider.base_url, provider.model.trim(), &provider.api_version)?;
    let (path, query) = match chat.split_once('?') {
        Some((path, query)) => (path.to_string(), format!("?{query}")),
        None => (chat, String::new()),
    };
    if !path.contains("/chat/completions") {
        return Err("接口地址无效".into());
    }
    Ok(format!("{}{query}", path.replacen("/chat/completions", &format!("/{leaf}"), 1)))
}

pub(crate) fn gemini_action(provider: &Provider, action: &str) -> Result<String, String> {
    let url = providers::gemini_generate_url(&provider.base_url, provider.model.trim(), &provider.api_key)?;
    Ok(url.replacen(":generateContent", &format!(":{action}"), 1))
}

pub(crate) fn authed(provider: &Provider, method: &str, url: String, body: Option<String>) -> Result<HttpRequest, String> {
    let mut headers = Vec::new();
    match provider.kind.as_str() {
        "openai" | "ollama" => {
            if !provider.api_key.is_empty() {
                headers.push(("authorization".into(), format!("Bearer {}", provider.api_key)));
            }
        }
        "azure" => {
            if !provider.api_key.is_empty() {
                headers.push(("api-key".into(), provider.api_key.clone()));
            }
        }
        "gemini" => {}
        "anthropic" => {
            if !provider.api_key.is_empty() {
                headers.push(("x-api-key".into(), provider.api_key.clone()));
            }
        }
        _ => return Err("不支持的接口类型".into()),
    }
    if body.is_some() {
        headers.push(("content-type".into(), "application/json".into()));
    }
    Ok(HttpRequest {
        method: method.into(),
        url,
        headers,
        body,
        timeout_secs: providers::CHAT_TIMEOUT_SECS,
        max_bytes: MAX_BYTES,
    })
}

pub(crate) fn attach_key(provider: &Provider, url: &str) -> Result<String, String> {
    if provider.kind != "gemini" {
        return Ok(url.to_string());
    }
    providers::with_query(url, "key", &provider.api_key)
}

pub(crate) fn response_bytes(response: &HttpResponse) -> &[u8] {
    if !response.bytes.is_empty() {
        &response.bytes
    } else {
        response.body.as_bytes()
    }
}

pub(crate) fn decode_b64(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text.chars().filter(|item| !item.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Err("文件数据无法解析".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(cleaned.as_bytes()))
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(cleaned.as_bytes()))
        .map_err(|_| "文件数据无法解析".to_string())
}

pub(crate) fn clip_note(text: &str, fallback: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        fallback.to_string()
    } else {
        text.chars().take(4000).collect()
    }
}

pub(crate) fn ext_from_mime(mime: &str) -> Option<&'static str> {
    let mime = mime.to_ascii_lowercase();
    if mime.contains("png") {
        Some("png")
    } else if mime.contains("jpeg") || mime.contains("jpg") {
        Some("jpg")
    } else if mime.contains("mpeg") || mime.contains("mp3") {
        Some("mp3")
    } else if mime.contains("wav") {
        Some("wav")
    } else if mime.contains("mp4") {
        Some("mp4")
    } else {
        None
    }
}

pub(crate) fn sniff(bytes: &[u8], fallback: &str) -> Result<String, String> {
    if let Some(found) = magic(bytes) {
        return Ok(found.to_string());
    }
    if matches!(fallback, "png" | "jpg" | "mp3" | "wav" | "mp4")
        && !bytes.is_empty()
        && !bytes.starts_with(b"{")
        && !bytes.starts_with(b"[")
        && !bytes.starts_with(b"<")
    {
        return Ok(fallback.to_string());
    }
    Err("模型没有返回文件".into())
}

pub(crate) fn piece_of(bytes: &[u8], fallback: &str, note: &str, missing: &str) -> Result<Piece, String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err(if bytes.is_empty() { missing.into() } else { "文件过大".into() });
    }
    let ext = sniff(bytes, fallback).map_err(|_| missing.to_string())?;
    Ok(Piece {
        bytes: bytes.to_vec(),
        ext,
        note: note.to_string(),
    })
}

pub(crate) fn finish_fetch<F>(provider: &Provider, loaded: Loaded, missing: &str, transport: &mut F) -> Result<Piece, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    match loaded {
        Loaded::Bytes(piece) => piece_of(&piece.bytes, &piece.ext, &piece.note, missing),
        Loaded::Fetch { url, note, ext } => {
            let url = attach_key(provider, url.trim())?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(missing.into());
            }
            let response = transport(&authed(provider, "GET", url, None)?)?;
            if !(200..300).contains(&response.status) {
                return Err(providers::explain_status(response.status, &response.body));
            }
            piece_of(response_bytes(&response), &ext, &note, missing)
        }
    }
}

pub(crate) fn wav_from_pcm(pcm: &[u8], rate: u32) -> Vec<u8> {
    let rate = if (8_000..=96_000).contains(&rate) { rate } else { 24_000 };
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

pub(crate) fn pcm_rate(mime: &str) -> u32 {
    mime.split("rate=")
        .nth(1)
        .and_then(|rest| rest.split([';', ' ', ',']).next())
        .and_then(|text| text.trim().parse().ok())
        .filter(|rate| (8_000..=96_000).contains(rate))
        .unwrap_or(24_000)
}

pub(crate) fn is_pcm(mime: &str) -> bool {
    let mime = mime.to_ascii_lowercase();
    mime.contains("l16") || mime.contains("pcm") || mime.contains("audio/raw")
}

pub(crate) fn inline_parts(value: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk_inline(value, &mut out);
    out
}

pub(crate) fn joined_text(value: &Value) -> String {
    let mut out = Vec::new();
    walk_text(value, &mut out);
    out.join("")
}

fn walk_inline(value: &Value, out: &mut Vec<(String, String)>) {
    match value {
        Value::Object(map) => {
            if let Some(data) = map.get("inlineData").and_then(|item| item.get("data")).and_then(|item| item.as_str()) {
                let mime = map
                    .get("inlineData")
                    .and_then(|item| item.get("mimeType"))
                    .and_then(|item| item.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push((mime, data.to_string()));
            } else if let Some(data) = map.get("bytesBase64Encoded").and_then(|item| item.as_str()) {
                let mime = map.get("mimeType").and_then(|item| item.as_str()).unwrap_or("").to_string();
                out.push((mime, data.to_string()));
            }
            for child in map.values() {
                walk_inline(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_inline(item, out);
            }
        }
        _ => {}
    }
}

fn walk_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("inlineData") || map.contains_key("bytesBase64Encoded") {
                return;
            }
            if let Some(text) = map.get("text").and_then(|item| item.as_str()) {
                if !text.trim().is_empty() {
                    out.push(text.to_string());
                }
            }
            for child in map.values() {
                walk_text(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_text(item, out);
            }
        }
        _ => {}
    }
}

fn magic(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WAVE" {
        Some("wav")
    } else if bytes.starts_with(b"ID3") || (bytes.len() > 1 && bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0) {
        Some("mp3")
    } else if bytes.len() > 11 && &bytes[4..8] == b"ftyp" {
        Some("mp4")
    } else {
        None
    }
}

fn join_prompt(style: &str, user_text: &str, max: usize) -> Result<String, String> {
    let style = style.trim();
    let text = if style.is_empty() {
        user_text.to_string()
    } else {
        format!("{style}\n\n{user_text}")
    };
    if text.chars().count() > max {
        Err("提示过长".into())
    } else {
        Ok(text)
    }
}

fn safe_dir(dir: &str) -> bool {
    !dir.is_empty()
        && !dir.contains('\\')
        && !Path::new(dir).is_absolute()
        && dir.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn safe_media_rel(rel: &str) -> bool {
    rel.starts_with("media/") && safe_dir(rel) && Path::new(rel).extension().is_some()
}
