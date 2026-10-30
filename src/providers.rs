use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::http_client::{HttpRequest, HttpResponse};
use crate::store::{self, Store};

const FILE: &str = "providers.json";
pub const CHAT_TIMEOUT_SECS: u64 = 120;
pub const TEST_TIMEOUT_SECS: u64 = 20;
pub const KINDS: &[&str] = &["openai", "anthropic", "gemini", "ollama", "azure"];
pub const TASKS: &[&str] = &["chat", "image", "speech", "video"];

#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub group: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
    pub base_url: &'static str,
    pub model: &'static str,
    pub api_version: &'static str,
    pub task: &'static str,
    pub voice: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_version: String,
    /// 在提供方页面读取到的模型名，供 Bot 选择。
    #[serde(default)]
    pub models: Vec<String>,
    /// 空或 chat 为对话，image 生图，speech 语音，video 视频。
    #[serde(default, skip_serializing_if = "is_chat_task")]
    pub task: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub voice: String,
}

fn is_chat_task(task: &str) -> bool {
    task.is_empty() || task == "chat"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReport {
    pub ok: bool,
    pub detail: String,
    pub models: Vec<String>,
}

impl Provider {
    pub fn draft(kind: &str) -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: kind.to_string(),
            base_url: default_base(kind).to_string(),
            api_key: String::new(),
            model: String::new(),
            api_version: if kind == "azure" {
                "2024-10-21".to_string()
            } else {
                String::new()
            },
            models: Vec::new(),
            task: String::new(),
            voice: String::new(),
        }
    }

    pub fn from_preset(preset: &Preset) -> Self {
        Self {
            id: String::new(),
            name: preset.name.to_string(),
            kind: preset.kind.to_string(),
            base_url: preset.base_url.to_string(),
            api_key: String::new(),
            model: preset.model.to_string(),
            api_version: preset.api_version.to_string(),
            models: Vec::new(),
            task: preset.task.to_string(),
            voice: preset.voice.to_string(),
        }
    }
}

pub fn presets() -> &'static [Preset] {
    &[
        Preset { group: "云端 API", name: "OpenAI", kind: "openai", base_url: "https://api.openai.com/v1", model: "gpt-4o-mini", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Anthropic", kind: "anthropic", base_url: "https://api.anthropic.com", model: "claude-sonnet-4-5", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Google Gemini", kind: "gemini", base_url: "https://generativelanguage.googleapis.com/v1beta", model: "gemini-2.5-flash", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "xAI Grok", kind: "openai", base_url: "https://api.x.ai/v1", model: "grok-3", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "DeepSeek", kind: "openai", base_url: "https://api.deepseek.com/v1", model: "deepseek-chat", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Moonshot 月之暗面", kind: "openai", base_url: "https://api.moonshot.cn/v1", model: "moonshot-v1-8k", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "通义千问", kind: "openai", base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1", model: "qwen-plus", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "智谱 GLM", kind: "openai", base_url: "https://open.bigmodel.cn/api/paas/v4", model: "glm-4-flash", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "硅基流动", kind: "openai", base_url: "https://api.siliconflow.cn/v1", model: "deepseek-ai/DeepSeek-V3", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "OpenRouter", kind: "openai", base_url: "https://openrouter.ai/api/v1", model: "openai/gpt-4o-mini", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Groq", kind: "openai", base_url: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Mistral", kind: "openai", base_url: "https://api.mistral.ai/v1", model: "mistral-small-latest", api_version: "", task: "", voice: "" },
        Preset { group: "云端 API", name: "Azure OpenAI", kind: "azure", base_url: "https://example.openai.azure.com", model: "", api_version: "2024-10-21", task: "", voice: "" },
        Preset { group: "本地", name: "Ollama", kind: "ollama", base_url: "http://127.0.0.1:11434", model: "", api_version: "", task: "", voice: "" },
        Preset { group: "本地", name: "LM Studio", kind: "openai", base_url: "http://127.0.0.1:1234/v1", model: "", api_version: "", task: "", voice: "" },
        Preset { group: "本地", name: "llama.cpp", kind: "openai", base_url: "http://127.0.0.1:8080/v1", model: "", api_version: "", task: "", voice: "" },
        Preset { group: "本地", name: "vLLM", kind: "openai", base_url: "http://127.0.0.1:8000/v1", model: "", api_version: "", task: "", voice: "" },
        Preset { group: "生图、语音、视频", name: "OpenAI 生图", kind: "openai", base_url: "https://api.openai.com/v1", model: "dall-e-3", api_version: "", task: "image", voice: "" },
        Preset { group: "生图、语音、视频", name: "OpenAI 语音", kind: "openai", base_url: "https://api.openai.com/v1", model: "tts-1", api_version: "", task: "speech", voice: "alloy" },
        Preset { group: "生图、语音、视频", name: "OpenAI 视频", kind: "openai", base_url: "https://api.openai.com/v1", model: "sora-2", api_version: "", task: "video", voice: "" },
        Preset { group: "生图、语音、视频", name: "Gemini 生图", kind: "gemini", base_url: "https://generativelanguage.googleapis.com/v1beta", model: "imagen-4.0-generate-001", api_version: "", task: "image", voice: "" },
        Preset { group: "生图、语音、视频", name: "Gemini 语音", kind: "gemini", base_url: "https://generativelanguage.googleapis.com/v1beta", model: "gemini-2.5-flash-preview-tts", api_version: "", task: "speech", voice: "Kore" },
        Preset { group: "生图、语音、视频", name: "Gemini 视频", kind: "gemini", base_url: "https://generativelanguage.googleapis.com/v1beta", model: "veo-3.0-generate-001", api_version: "", task: "video", voice: "" },
    ]
}

pub fn task_of(task: &str) -> Result<&'static str, String> {
    match task.trim() {
        "" | "chat" => Ok("chat"),
        "image" => Ok("image"),
        "speech" => Ok("speech"),
        "video" => Ok("video"),
        _ => Err("不支持的用途".into()),
    }
}

pub fn task_label(task: &str) -> &'static str {
    match task_of(task).unwrap_or("") {
        "chat" => "对话",
        "image" => "生图",
        "speech" => "语音",
        "video" => "视频",
        _ => "未知",
    }
}

pub fn supports_task(kind: &str, task: &str) -> bool {
    match task_of(task) {
        Ok("chat") => true,
        Ok("image" | "speech" | "video") => matches!(kind, "openai" | "azure" | "gemini"),
        _ => false,
    }
}

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "openai" => "OpenAI 兼容",
        "anthropic" => "Anthropic",
        "gemini" => "Google Gemini",
        "ollama" => "Ollama",
        "azure" => "Azure OpenAI",
        _ => "未知",
    }
}

pub fn default_base(kind: &str) -> &'static str {
    match kind {
        "anthropic" => "https://api.anthropic.com",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta",
        "ollama" => "http://127.0.0.1:11434",
        "azure" => "https://example.openai.azure.com",
        _ => "https://api.openai.com/v1",
    }
}

pub fn change_kind(form: &mut Provider, new_kind: &str) {
    if !KINDS.contains(&new_kind) || form.kind == new_kind {
        return;
    }
    let old_default = default_base(&form.kind);
    if form.base_url.trim().is_empty() || form.base_url.trim() == old_default {
        form.base_url = default_base(new_kind).to_string();
    }
    if new_kind == "azure" && form.api_version.trim().is_empty() {
        form.api_version = "2024-10-21".to_string();
    }
    form.kind = new_kind.to_string();
    form.models.clear();
    if !supports_task(&form.kind, &form.task) {
        form.task.clear();
    }
}

pub fn apply_preset(form: &mut Provider, preset: &Preset) {
    let id = form.id.clone();
    let api_key = form.api_key.clone();
    *form = Provider::from_preset(preset);
    form.id = id;
    form.api_key = api_key;
}

pub fn list(store: &Store) -> Result<Vec<Provider>, String> {
    store.load_vec(FILE)
}

pub fn get(store: &Store, id: &str) -> Result<Provider, String> {
    list(store)?
        .into_iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| "提供方不存在".to_string())
}

pub fn upsert(store: &Store, mut provider: Provider) -> Result<Provider, String> {
    provider = validate(provider)?;
    if provider.id.is_empty() {
        provider.id = store::new_id();
    }
    let mut all = list(store)?;
    if let Some(pos) = all.iter().position(|item| item.id == provider.id) {
        all[pos] = provider.clone();
    } else {
        all.push(provider.clone());
    }
    store.save_vec(FILE, &all)?;
    Ok(provider)
}

pub fn delete(store: &Store, id: &str) -> Result<(), String> {
    if !store::is_safe_id(id) {
        return Err("非法编号".into());
    }
    let mut all = list(store)?;
    let before = all.len();
    all.retain(|provider| provider.id != id);
    if all.len() == before {
        return Err("提供方不存在".into());
    }
    store.save_vec(FILE, &all)
}

pub fn validate(mut provider: Provider) -> Result<Provider, String> {
    provider.name = provider.name.trim().to_string();
    if provider.name.is_empty() {
        return Err("请填写提供方名称".into());
    }
    if provider.name.chars().count() > 80 {
        return Err("提供方名称过长".into());
    }
    if !KINDS.contains(&provider.kind.as_str()) {
        return Err("不支持的接口类型".into());
    }
    provider.base_url = without_query(provider.base_url.trim()).trim_end_matches('/').to_string();
    if provider.base_url.is_empty() {
        return Err("请填写接口地址".into());
    }
    if provider.base_url.chars().count() > 500 {
        return Err("接口地址过长".into());
    }
    check_base(&provider.base_url)?;
    provider.model = provider.model.trim().to_string();
    if provider.model.chars().count() > 200 {
        return Err("模型名过长".into());
    }
    provider.api_key = provider.api_key.trim().to_string();
    if provider.api_key.chars().count() > 8000 {
        return Err("API 密钥过长".into());
    }
    provider.api_version = provider.api_version.trim().to_string();
    if provider.api_version.chars().count() > 40 {
        return Err("API 版本过长".into());
    }
    if provider.kind == "azure" && provider.api_version.is_empty() {
        provider.api_version = "2024-10-21".to_string();
    }
    let task = task_of(&provider.task)?;
    if !supports_task(&provider.kind, task) {
        return Err("这个接口不能用于该用途".into());
    }
    provider.task = if task == "chat" { String::new() } else { task.to_string() };
    provider.voice = provider.voice.trim().to_string();
    if provider.voice.chars().count() > 40 {
        return Err("语音角色过长".into());
    }
    if !provider.id.is_empty() && !store::is_safe_id(&provider.id) {
        return Err("非法编号".into());
    }
    provider.models = normalize_models(provider.models);
    Ok(provider)
}

pub fn normalize_models(models: Vec<String>) -> Vec<String> {
    let mut kept = Vec::new();
    for model in models {
        let model = model.trim();
        if model.is_empty() || model.chars().count() > 200 {
            continue;
        }
        if kept.iter().any(|item: &String| item == model) {
            continue;
        }
        kept.push(model.to_string());
        if kept.len() == 400 {
            break;
        }
    }
    kept
}

/// 下拉选项：已保存的模型，再加上当前 Bot 上不在列表里的自定义模型名。
pub fn model_choices(models: &[String], current: &str) -> Vec<String> {
    let mut choices = normalize_models(models.to_vec());
    let current = current.trim();
    if !current.is_empty() && current.chars().count() <= 200 && !choices.iter().any(|item| item == current) {
        choices.insert(0, current.to_string());
    }
    choices
}

pub fn save_models(store: &Store, id: &str, models: Vec<String>) -> Result<Provider, String> {
    let mut provider = get(store, id)?;
    provider.models = normalize_models(models);
    upsert(store, provider)
}

pub fn build_chat_request(provider: &Provider, system: &str, turns: &[Turn]) -> Result<HttpRequest, String> {
    check_base(&without_query(&provider.base_url))?;
    let model = provider.model.trim();
    if model.is_empty() {
        return Err(if provider.kind == "azure" {
            "请填写 Azure 部署名".into()
        } else {
            "请填写模型名".into()
        });
    }
    let usable = usable_turns(turns);
    if usable.is_empty() {
        return Err("没有可发送的用户消息".into());
    }
    let system = system.trim();
    let (url, mut headers, body) = match provider.kind.as_str() {
        "openai" | "ollama" => {
            let url = openai_chat_url(&provider.base_url)?;
            let mut headers = Vec::new();
            push_bearer(&mut headers, &provider.api_key);
            (url, headers, openai_body(model, system, &usable))
        }
        "azure" => {
            let url = azure_chat_url(&provider.base_url, model, &provider.api_version)?;
            let mut headers = Vec::new();
            if !provider.api_key.is_empty() {
                headers.push(("api-key".into(), provider.api_key.clone()));
            }
            (url, headers, openai_body(model, system, &usable))
        }
        "anthropic" => {
            let url = anthropic_messages_url(&provider.base_url)?;
            let mut headers = vec![("anthropic-version".into(), "2023-06-01".into())];
            if !provider.api_key.is_empty() {
                headers.push(("x-api-key".into(), provider.api_key.clone()));
            }
            (url, headers, anthropic_body(model, system, &usable))
        }
        "gemini" => {
            let url = gemini_generate_url(&provider.base_url, model, &provider.api_key)?;
            (url, Vec::new(), gemini_body(system, &usable))
        }
        _ => return Err("不支持的接口类型".into()),
    };
    headers.push(("content-type".into(), "application/json".into()));
    Ok(HttpRequest {
        method: "POST".into(),
        url,
        headers,
        body: Some(body),
        timeout_secs: CHAT_TIMEOUT_SECS,
        max_bytes: 0,
    })
}

pub fn complete_turns<F>(
    provider: &Provider,
    system: &str,
    mut turns: Vec<Turn>,
    root: &str,
    mut transport: F,
) -> Result<(String, crate::usage::Usage), String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let mut spent = crate::usage::Usage::default();
    for _ in 0..crate::folder::MAX_PASSES {
        let request = build_chat_request(provider, system, &turns)?;
        let response = transport(&request)?;
        let text = parse_chat_response(&provider.kind, response.status, &response.body)?;
        spent.add(crate::usage::from_chat(&provider.kind, &response.body));
        if root.trim().is_empty() {
            return Ok((text, spent));
        }
        match crate::folder::round(root, &text)? {
            crate::folder::Round::Done(answer) => return Ok((answer, spent)),
            crate::folder::Round::Continue(note) => {
                turns.push(Turn { role: "assistant".into(), content: text });
                turns.push(Turn { role: "user".into(), content: note });
            }
        }
    }
    Err("读取文件夹的次数过多".into())
}

pub fn build_models_request(provider: &Provider) -> Result<HttpRequest, String> {
    let base = without_query(&provider.base_url);
    check_base(&base)?;
    let (url, mut headers) = match provider.kind.as_str() {
        "openai" => {
            let mut headers = Vec::new();
            push_bearer(&mut headers, &provider.api_key);
            (openai_models_url(&base)?, headers)
        }
        "ollama" => (ollama_tags_url(&base)?, Vec::new()),
        "anthropic" => {
            let mut headers = vec![("anthropic-version".into(), "2023-06-01".into())];
            if !provider.api_key.is_empty() {
                headers.push(("x-api-key".into(), provider.api_key.clone()));
            }
            (anthropic_models_url(&base)?, headers)
        }
        "gemini" => (gemini_models_url(&base, &provider.api_key)?, Vec::new()),
        "azure" => {
            let mut headers = Vec::new();
            if !provider.api_key.is_empty() {
                headers.push(("api-key".into(), provider.api_key.clone()));
            }
            (azure_models_url(&base, &provider.api_version)?, headers)
        }
        _ => return Err("不支持的接口类型".into()),
    };
    if provider.kind == "ollama" && !provider.api_key.is_empty() {
        push_bearer(&mut headers, &provider.api_key);
    }
    Ok(HttpRequest {
        method: "GET".into(),
        url,
        headers,
        body: None,
        timeout_secs: TEST_TIMEOUT_SECS,
        max_bytes: 0,
    })
}

pub fn parse_chat_response(kind: &str, status: u16, body: &str) -> Result<String, String> {
    if !(200..300).contains(&status) {
        return Err(format!("请求失败（{status}）：{}", clip(&error_message(body))));
    }
    let value: Value = serde_json::from_str(body).map_err(|_| "模型返回的不是 JSON".to_string())?;
    let text = match kind {
        "anthropic" => join_text(value.pointer("/content"), "text"),
        "gemini" => {
            if let Some(reason) = value.pointer("/promptFeedback/blockReason").and_then(|item| item.as_str()) {
                return Err(format!("请求被拦截：{reason}"));
            }
            join_text(value.pointer("/candidates/0/content/parts"), "text")
        }
        "openai" | "ollama" | "azure" => openai_text(&value),
        _ => return Err("不支持的接口类型".into()),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        Err("模型没有返回文本".into())
    } else {
        Ok(text)
    }
}

pub fn parse_models_response(kind: &str, status: u16, body: &str) -> Result<Vec<String>, String> {
    if !(200..300).contains(&status) {
        return Err(format!("请求失败（{status}）：{}", clip(&error_message(body))));
    }
    let value: Value = serde_json::from_str(body).map_err(|_| "模型列表不是 JSON".to_string())?;
    let mut models = match kind {
        "ollama" => names_from(&value, &["/models"], &["name"]),
        "gemini" => names_from(&value, &["/models"], &["name"])
            .into_iter()
            .map(|name| name.trim_start_matches("models/").to_string())
            .collect(),
        "azure" => {
            let found = names_from(&value, &["/data", "/value"], &["id", "name"]);
            if found.is_empty() {
                names_from(&value, &["/data", "/value"], &["model"])
            } else {
                found
            }
        }
        _ => names_from(&value, &["/data"], &["id", "name"]),
    };
    models.retain(|model| !model.trim().is_empty());
    models.sort();
    models.dedup();
    Ok(models)
}

pub fn test_provider_with<F>(provider: &Provider, transport: F) -> Result<TestReport, String>
where
    F: FnOnce(&HttpRequest) -> Result<HttpResponse, String>,
{
    let provider = validate(provider.clone())?;
    let request = build_models_request(&provider)?;
    match transport(&request) {
        Ok(response) if matches!(response.status, 404 | 405 | 501) => Ok(TestReport {
            ok: true,
            detail: "地址可达，但这个接口没有模型列表，请手动填写模型名".into(),
            models: Vec::new(),
        }),
        Ok(response) => {
            let models = parse_models_response(&provider.kind, response.status, &response.body)?;
            let detail = if models.is_empty() {
                "已连通".into()
            } else {
                format!("已连通，读取到 {} 个模型", models.len())
            };
            Ok(TestReport { ok: true, detail, models })
        }
        Err(err) => Err(err),
    }
}

pub fn openai_chat_url(base: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    if let Some(stripped) = base.strip_suffix("/chat/completions") {
        return Ok(format!("{}/chat/completions", stripped.trim_end_matches('/')));
    }
    let path = url_path(&base)?;
    if path.is_empty() {
        Ok(format!("{base}/v1/chat/completions"))
    } else {
        Ok(format!("{base}/chat/completions"))
    }
}

pub fn openai_models_url(base: &str) -> Result<String, String> {
    let chat = openai_chat_url(base)?;
    Ok(format!("{}/models", chat.trim_end_matches("/chat/completions").trim_end_matches('/')))
}

pub fn anthropic_messages_url(base: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    if base.ends_with("/messages") {
        Ok(base)
    } else if base.ends_with("/v1") {
        Ok(format!("{base}/messages"))
    } else {
        Ok(format!("{base}/v1/messages"))
    }
}

pub fn anthropic_models_url(base: &str) -> Result<String, String> {
    let messages = anthropic_messages_url(base)?;
    Ok(format!("{}/models", messages.trim_end_matches("/messages").trim_end_matches('/')))
}

pub fn gemini_generate_url(base: &str, model: &str, api_key: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let model = model.trim().trim_start_matches("models/");
    if model.is_empty() {
        return Err("请填写模型名".into());
    }
    let endpoint = if base.contains(":generateContent") {
        base
    } else {
        let root = base.strip_suffix("/models").unwrap_or(&base);
        format!("{}/models/{}:generateContent", root.trim_end_matches('/'), path_segment(model))
    };
    with_query(&endpoint, "key", api_key)
}

pub fn gemini_models_url(base: &str, api_key: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let root = if let Some(index) = base.find("/models/") {
        base[..index].to_string()
    } else {
        base.strip_suffix("/models").unwrap_or(&base).to_string()
    };
    with_query(&format!("{}/models", root.trim_end_matches('/')), "key", api_key)
}

pub fn ollama_tags_url(base: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let url = url::Url::parse(&base).map_err(|_| "接口地址无效".to_string())?;
    let authority = url
        .host_str()
        .map(|host| match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_string(),
        })
        .ok_or("接口地址缺少主机名")?;
    Ok(format!("{}://{authority}/api/tags", url.scheme()))
}

pub fn azure_chat_url(base: &str, deployment: &str, api_version: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let deployment = deployment.trim();
    if deployment.is_empty() {
        return Err("请填写 Azure 部署名".into());
    }
    let version = if api_version.trim().is_empty() { "2024-10-21" } else { api_version.trim() };
    if base.contains("/chat/completions") {
        return with_query(&base, "api-version", version);
    }
    let endpoint = azure_endpoint(&base);
    with_query(
        &format!("{endpoint}/openai/deployments/{}/chat/completions", path_segment(deployment)),
        "api-version",
        version,
    )
}

pub fn azure_models_url(base: &str, api_version: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let version = if api_version.trim().is_empty() { "2024-10-21" } else { api_version.trim() };
    with_query(&format!("{}/openai/deployments", azure_endpoint(&base)), "api-version", version)
}

fn usable_turns(turns: &[Turn]) -> Vec<&Turn> {
    let mut out: Vec<&Turn> = turns
        .iter()
        .filter(|turn| (turn.role == "user" || turn.role == "assistant") && !turn.content.trim().is_empty())
        .collect();
    if out.len() > 40 {
        let start = out.len() - 40;
        out = out.split_off(start);
    }
    while out.first().map(|turn| turn.role == "assistant").unwrap_or(false) {
        out.remove(0);
    }
    out
}

fn openai_body(model: &str, system: &str, turns: &[&Turn]) -> String {
    let mut messages = Vec::new();
    if !system.is_empty() {
        messages.push(json!({"role": "system", "content": system}));
    }
    for turn in turns {
        messages.push(json!({"role": turn.role, "content": turn.content}));
    }
    json!({"model": model, "messages": messages}).to_string()
}

fn anthropic_body(model: &str, system: &str, turns: &[&Turn]) -> String {
    let messages: Vec<_> = turns
        .iter()
        .map(|turn| json!({"role": turn.role, "content": turn.content}))
        .collect();
    let mut body = json!({"model": model, "max_tokens": 4096, "messages": messages});
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    body.to_string()
}

fn gemini_body(system: &str, turns: &[&Turn]) -> String {
    let contents: Vec<_> = turns
        .iter()
        .map(|turn| {
            let role = if turn.role == "assistant" { "model" } else { "user" };
            json!({"role": role, "parts": [{"text": turn.content}]})
        })
        .collect();
    let mut body = json!({"contents": contents});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system}]});
    }
    body.to_string()
}

fn openai_text(value: &Value) -> String {
    match value.pointer("/choices/0/message/content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(|item| item.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn join_text(node: Option<&Value>, field: &str) -> String {
    match node {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get(field).and_then(|item| item.as_str()))
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

fn names_from(value: &Value, pointers: &[&str], fields: &[&str]) -> Vec<String> {
    for pointer in pointers {
        if let Some(Value::Array(items)) = value.pointer(pointer) {
            let mut out = Vec::new();
            for item in items {
                for field in fields {
                    if let Some(name) = item.get(*field).and_then(|value| value.as_str()) {
                        out.push(name.to_string());
                        break;
                    }
                }
            }
            return out;
        }
    }
    Vec::new()
}

pub(crate) fn explain_status(status: u16, body: &str) -> String {
    format!("请求失败（{status}）：{}", clip(&error_message(body)))
}

fn error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(body) {
        if let Some(message) = value.pointer("/error/message").and_then(|item| item.as_str()) {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(|item| item.as_str()) {
            return message.to_string();
        }
    }
    let text = body.trim();
    if text.is_empty() { "请求失败".into() } else { text.to_string() }
}

fn clip(text: &str) -> String {
    let mut out: String = text.chars().take(400).collect();
    if text.chars().count() > 400 {
        out.push('…');
    }
    out
}

fn push_bearer(headers: &mut Vec<(String, String)>, key: &str) {
    if !key.is_empty() {
        headers.push(("authorization".into(), format!("Bearer {key}")));
    }
}

fn normalize_base(base: &str) -> Result<String, String> {
    let base = without_query(base.trim()).trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("请填写接口地址".into());
    }
    check_base(&base)?;
    Ok(base)
}

fn without_query(base: &str) -> &str {
    base.split('?').next().unwrap_or(base)
}

fn check_base(base: &str) -> Result<(), String> {
    let url = url::Url::parse(base).map_err(|_| "接口地址无效".to_string())?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return Err("接口地址只支持 http 或 https".into());
    }
    if url.host_str().is_none() {
        return Err("接口地址缺少主机名".into());
    }
    Ok(())
}

fn url_path(base: &str) -> Result<String, String> {
    let url = url::Url::parse(base).map_err(|_| "接口地址无效".to_string())?;
    Ok(url.path().trim_matches('/').to_string())
}

pub(crate) fn with_query(endpoint: &str, key: &str, value: &str) -> Result<String, String> {
    let mut url = url::Url::parse(endpoint).map_err(|_| "接口地址无效".to_string())?;
    if !value.is_empty() && url.query_pairs().all(|(name, _)| name != key) {
        url.query_pairs_mut().append_pair(key, value);
    }
    Ok(url.to_string())
}

pub(crate) fn path_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn azure_endpoint(base: &str) -> String {
    let mut text = base.trim().trim_end_matches('/').to_string();
    if let Some(index) = text.find("/openai/deployments") {
        text.truncate(index);
    } else if let Some(stripped) = text.strip_suffix("/chat/completions") {
        text = stripped.trim_end_matches('/').to_string();
        if let Some(index) = text.find("/openai/deployments") {
            text.truncate(index);
        }
    }
    if let Some(stripped) = text.strip_suffix("/openai") {
        text = stripped.to_string();
    }
    text.trim_end_matches('/').to_string()
}
