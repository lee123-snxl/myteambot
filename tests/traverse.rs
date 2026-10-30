use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use base64::Engine;

use local_bot::bots::{self, Bot};
use local_bot::chat;
use local_bot::folder;
use local_bot::group::{self, GroupMessage, Preview};
use local_bot::http_client::{self, HttpRequest, HttpResponse};
use local_bot::image_gen;
use local_bot::media::{self, Loaded};
use local_bot::pick_folder;
use local_bot::providers::{self, Provider, Turn};
use local_bot::proxy;
use local_bot::speech;
use local_bot::store::{self, Store};
use local_bot::usage;
use local_bot::video;

struct Tmp {
    path: std::path::PathBuf,
}

impl Tmp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("localbot-{}", store::new_id()));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn store(&self) -> Store {
        Store::open(&self.path).unwrap()
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn traverse_ids_and_data_dir() {
    let id = store::new_id();
    assert!(store::is_safe_id(&id));
    for bad in ["", "abc", "../secret", "a/b", "has space", "名字"] {
        assert!(!store::is_safe_id(bad), "{bad}");
    }
    assert!(!store::default_data_dir().as_os_str().is_empty());
}

#[test]
fn traverse_store_roundtrip_and_corrupt_file() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let bot = Bot {
        id: store::new_id(),
        name: "助手".into(),
        prompt: "用中文回答".into(),
        provider_id: String::new(),
        model: String::new(),
        folder: String::new(),
    };
    store.save_vec("bots.json", &[bot.clone()]).unwrap();
    store.save_vec("bots.json", &[bot.clone()]).unwrap();
    let loaded: Vec<Bot> = store.load_vec("bots.json").unwrap();
    assert_eq!(loaded, vec![bot]);
    assert!(store.load_vec::<Bot>("missing.json").unwrap().is_empty());
    std::fs::write(tmp.path.join("bots.json"), "{").unwrap();
    assert!(store.load_vec::<Bot>("bots.json").unwrap_err().contains("损坏"));
    assert!(store.remove("nope.json").is_ok());
    assert!(store.load_vec::<Bot>("../bots.json").is_err());
}

#[test]
fn traverse_presets_and_kind_changes() {
    let mut cloud = 0;
    let mut local = 0;
    for preset in providers::presets() {
        assert!(providers::KINDS.contains(&preset.kind), "{}", preset.name);
        assert!(preset.base_url.starts_with("http"), "{}", preset.name);
        let mut form = Provider::draft("openai");
        form.api_key = "keep".into();
        form.id = "id-keep-1".into();
        providers::apply_preset(&mut form, preset);
        assert_eq!(form.kind, preset.kind);
        assert_eq!(form.base_url, preset.base_url);
        assert_eq!(form.api_key, "keep");
        assert_eq!(form.id, "id-keep-1");
        if preset.group == "本地" {
            local += 1;
            assert!(preset.base_url.contains("127.0.0.1"), "{}", preset.name);
        } else {
            cloud += 1;
            assert!(preset.base_url.starts_with("https://"), "{}", preset.name);
        }
        assert!(!providers::kind_label(preset.kind).is_empty());
    }
    assert!(cloud >= 12);
    assert!(local >= 4);

    let mut form = Provider::draft("openai");
    providers::change_kind(&mut form, "ollama");
    assert_eq!(form.base_url, providers::default_base("ollama"));
    form.base_url = "https://example.com/v1".into();
    providers::change_kind(&mut form, "anthropic");
    assert_eq!(form.base_url, "https://example.com/v1");
    providers::change_kind(&mut form, "azure");
    assert_eq!(form.api_version, "2024-10-21");
    providers::change_kind(&mut form, "nope");
    assert_eq!(form.kind, "azure");
}

#[test]
fn traverse_saved_models_can_be_chosen_for_a_bot() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let saved = providers::upsert(&store, Provider {
        name: "远程".into(),
        ..Provider::draft("openai")
    })
    .unwrap();
    let updated = providers::save_models(
        &store,
        &saved.id,
        vec![" gpt-4o ".into(), "gpt-4o".into(), String::new(), "a".repeat(201), "grok-3".into()],
    )
    .unwrap();
    assert_eq!(updated.models, vec!["gpt-4o".to_string(), "grok-3".to_string()]);
    assert_eq!(providers::get(&store, &saved.id).unwrap().models, updated.models);
    assert_eq!(providers::model_choices(&updated.models, ""), updated.models);
    let choices = providers::model_choices(&updated.models, "custom");
    assert_eq!(choices[0], "custom");
    assert!(choices.contains(&"gpt-4o".to_string()));

    let old: Provider = serde_json::from_str(
        r#"{"id":"old-provider","name":"旧","kind":"openai","baseUrl":"https://api.openai.com/v1"}"#,
    )
    .unwrap();
    assert!(old.models.is_empty());

    let mut form = updated.clone();
    providers::change_kind(&mut form, "ollama");
    assert!(form.models.is_empty());
    providers::apply_preset(&mut form, &providers::presets()[0]);
    assert!(form.models.is_empty());
}

#[test]
fn traverse_provider_validation() {
    let cases = [
        (Provider::draft("openai"), "请填写提供方名称"),
        (named("   ", "openai", "https://api.openai.com/v1"), "请填写提供方名称"),
        (named("A", "nope", "https://api.openai.com/v1"), "不支持的接口类型"),
        (named("A", "openai", "ftp://example.com"), "只支持 http"),
        (named("A", "openai", "not a url"), "接口地址无效"),
    ];
    for (provider, needle) in cases {
        let err = providers::validate(provider).unwrap_err();
        assert!(err.contains(needle), "{err}");
    }
    let mut ok = named(" 我的 OpenAI ", "openai", "https://api.openai.com/v1/");
    ok.api_key = " sk ".into();
    let saved = providers::validate(ok).unwrap();
    assert_eq!(saved.name, "我的 OpenAI");
    assert_eq!(saved.base_url, "https://api.openai.com/v1");
    assert_eq!(saved.api_key, "sk");
    let mut azure = named("Azure", "azure", "https://example.openai.azure.com");
    azure.api_version.clear();
    assert_eq!(providers::validate(azure).unwrap().api_version, "2024-10-21");
}

#[test]
fn traverse_urls_for_every_kind() {
    let rows = [
        ("https://api.openai.com", "https://api.openai.com/v1/chat/completions"),
        ("https://api.openai.com/v1", "https://api.openai.com/v1/chat/completions"),
        ("https://api.openai.com/v1/chat/completions", "https://api.openai.com/v1/chat/completions"),
        ("https://open.bigmodel.cn/api/paas/v4", "https://open.bigmodel.cn/api/paas/v4/chat/completions"),
        ("http://127.0.0.1:11434", "http://127.0.0.1:11434/v1/chat/completions"),
        ("http://127.0.0.1:1234/v1/", "http://127.0.0.1:1234/v1/chat/completions"),
    ];
    for (base, expect) in rows {
        assert_eq!(providers::openai_chat_url(base).unwrap(), expect, "{base}");
    }
    assert_eq!(
        providers::openai_models_url("https://api.openai.com").unwrap(),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        providers::anthropic_messages_url("https://api.anthropic.com").unwrap(),
        "https://api.anthropic.com/v1/messages"
    );
    assert_eq!(
        providers::anthropic_models_url("https://api.anthropic.com/v1").unwrap(),
        "https://api.anthropic.com/v1/models"
    );
    let gemini = providers::gemini_generate_url(
        "https://generativelanguage.googleapis.com/v1beta",
        "models/gemini-2.5-flash",
        "secret",
    )
    .unwrap();
    assert!(gemini.contains("/models/gemini-2.5-flash:generateContent"));
    assert!(gemini.contains("key=secret"));
    assert_eq!(
        providers::ollama_tags_url("http://127.0.0.1:11434/v1").unwrap(),
        "http://127.0.0.1:11434/api/tags"
    );
    let azure = providers::azure_chat_url("https://example.openai.azure.com/", "my deploy", "").unwrap();
    assert!(azure.contains("/openai/deployments/my%20deploy/chat/completions"));
    assert!(azure.contains("api-version=2024-10-21"));
    assert_eq!(
        providers::azure_models_url("https://example.openai.azure.com/openai", "2024-10-21").unwrap(),
        "https://example.openai.azure.com/openai/deployments?api-version=2024-10-21"
    );
}

#[test]
fn traverse_chat_request_and_response_for_every_kind() {
    let turns = vec![
        Turn { role: "assistant".into(), content: "先忽略".into() },
        Turn { role: "user".into(), content: "你好".into() },
        Turn { role: "error".into(), content: "不要发给模型".into() },
    ];
    for kind in providers::KINDS {
        let mut provider = named("测试", kind, providers::default_base(kind));
        provider.model = "unit-model".into();
        provider.api_key = "secret".into();
        let request = providers::build_chat_request(&provider, "系统提示", &turns).unwrap();
        assert_eq!(request.method, "POST");
        assert!(request.url.starts_with("http"));
        let body: serde_json::Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
        match *kind {
            "anthropic" => {
                assert!(request.url.ends_with("/messages"), "{}", request.url);
                assert_eq!(header(&request, "x-api-key"), Some("secret"));
                assert_eq!(body["system"], "系统提示");
                assert_eq!(body["messages"][0]["content"], "你好");
                assert!(body["max_tokens"].as_u64().unwrap() > 0);
            }
            "gemini" => {
                assert!(request.url.contains("key=secret"), "{}", request.url);
                assert!(!request.body.as_deref().unwrap().contains("secret"));
                assert_eq!(body["systemInstruction"]["parts"][0]["text"], "系统提示");
                assert_eq!(body["contents"][0]["role"], "user");
            }
            "azure" => {
                assert!(request.url.contains("api-version="), "{}", request.url);
                assert_eq!(header(&request, "api-key"), Some("secret"));
                assert!(header(&request, "authorization").is_none());
                assert_eq!(body["messages"][0]["role"], "system");
                assert_eq!(body["model"], "unit-model");
            }
            _ => {
                assert!(request.url.ends_with("/chat/completions"), "{}", request.url);
                assert_eq!(header(&request, "authorization"), Some("Bearer secret"));
                assert_eq!(body["messages"][0]["content"], "系统提示");
                assert_eq!(body["messages"][1]["content"], "你好");
            }
        }
        assert!(!request.body.as_deref().unwrap().contains("不要发给模型"));
        assert!(!request.body.as_deref().unwrap().contains("先忽略"));
    }

    let mut many = Vec::new();
    for index in 0..45 {
        many.push(Turn { role: "user".into(), content: format!("m-{index}") });
    }
    let provider = named_model("openai", "m");
    let request = providers::build_chat_request(&provider, "", &many).unwrap();
    let body: serde_json::Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 40);
    assert_eq!(messages[0]["content"], "m-5");
    assert_eq!(messages.last().unwrap()["content"], "m-44");

    let success = [
        ("openai", r#"{"choices":[{"message":{"content":"你好"}}]}"#),
        ("ollama", r#"{"choices":[{"message":{"content":[{"type":"text","text":"本地"}]}}]}"#),
        ("azure", r#"{"choices":[{"message":{"content":"azure"}}]}"#),
        ("anthropic", r#"{"content":[{"type":"text","text":"claude"}]}"#),
        ("gemini", r#"{"candidates":[{"content":{"parts":[{"text":"gemini"}]}}]}"#),
    ];
    for (kind, body) in success {
        assert!(!providers::parse_chat_response(kind, 200, body).unwrap().is_empty());
    }
    let errors = [
        ("openai", 401, r#"{"error":{"message":"bad key"}}"#, "bad key"),
        ("anthropic", 400, r#"{"error":{"message":"nope"}}"#, "nope"),
        ("gemini", 400, r#"{"error":{"message":"blocked"}}"#, "blocked"),
        ("ollama", 404, r#"{"error":{"message":"model not found"}}"#, "model not found"),
        ("azure", 401, r#"{"error":{"message":"denied"}}"#, "denied"),
    ];
    for (kind, status, body, needle) in errors {
        let err = providers::parse_chat_response(kind, status, body).unwrap_err();
        assert!(err.contains(needle), "{err}");
    }
    assert!(providers::parse_chat_response("openai", 200, "not-json").is_err());
    assert!(providers::parse_chat_response("openai", 200, r#"{"choices":[]}"#).is_err());
    assert!(providers::parse_chat_response("gemini", 200, r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#)
        .unwrap_err()
        .contains("SAFETY"));
    assert!(providers::build_chat_request(&named("A", "openai", "https://api.openai.com/v1"), "", &[]).is_err());
}

#[test]
fn traverse_model_list_parsing_and_probe() {
    let ollama = providers::parse_models_response("ollama", 200, r#"{"models":[{"name":"llama3.2:latest"}]}"#).unwrap();
    assert_eq!(ollama, vec!["llama3.2:latest".to_string()]);
    let gemini = providers::parse_models_response(
        "gemini",
        200,
        r#"{"models":[{"name":"models/gemini-2.5-flash"}]}"#,
    )
    .unwrap();
    assert_eq!(gemini, vec!["gemini-2.5-flash".to_string()]);
    let openai = providers::parse_models_response("openai", 200, r#"{"data":[{"id":"gpt-4o-mini"},{"id":"gpt-4o-mini"}]}"#).unwrap();
    assert_eq!(openai, vec!["gpt-4o-mini".to_string()]);
    let azure = providers::parse_models_response("azure", 200, r#"{"value":[{"id":"deploy-a"}]}"#).unwrap();
    assert_eq!(azure, vec!["deploy-a".to_string()]);
    assert!(providers::parse_models_response("openai", 401, r#"{"error":{"message":"bad key"}}"#)
        .unwrap_err()
        .contains("bad key"));

    let provider = named_model("ollama", "");
    let report = providers::test_provider_with(&provider, |_| {
        Ok(HttpResponse::text(200, r#"{"models":[{"name":"qwen"}]}"#))
    })
    .unwrap();
    assert!(report.ok);
    assert_eq!(report.models, vec!["qwen".to_string()]);
    let missing = providers::test_provider_with(&provider, |_| {
        Ok(HttpResponse::text(404, "missing"))
    })
    .unwrap();
    assert!(missing.detail.contains("没有模型列表"));
    let failed = providers::test_provider_with(&provider, |_| Err("网络错误：拒绝连接".into())).unwrap_err();
    assert!(failed.contains("拒绝连接"));

    for kind in providers::KINDS {
        let mut provider = named("列表", kind, providers::default_base(kind));
        if kind == &"azure" {
            provider.model = "deploy".into();
        }
        let request = providers::build_models_request(&provider).unwrap();
        assert_eq!(request.method, "GET");
        assert!(request.body.is_none());
        assert!(request.url.starts_with("http"), "{}", request.url);
    }
}

#[test]
fn traverse_bot_crud() {
    let tmp = Tmp::new();
    let store = tmp.store();
    assert!(bots::validate(Bot::draft()).unwrap_err().contains("名字"));
    let mut long = Bot::draft();
    long.name = "名".repeat(81);
    assert!(bots::validate(long).unwrap_err().contains("过长"));
    let created = bots::upsert(&store, Bot {
        name: " 日程助手 ".into(),
        prompt: "只谈日程".into(),
        ..Bot::draft()
    })
    .unwrap();
    assert!(store::is_safe_id(&created.id));
    assert_eq!(created.name, "日程助手");
    let updated = bots::upsert(&store, Bot {
        id: created.id.clone(),
        name: "日程助手".into(),
        prompt: "改过的提示词".into(),
        provider_id: String::new(),
        model: "local".into(),
        folder: String::new(),
    })
    .unwrap();
    assert_eq!(bots::list(&store).unwrap().len(), 1);
    assert_eq!(bots::get(&store, &created.id).unwrap().prompt, "改过的提示词");
    assert_eq!(updated.model, "local");
    assert!(bots::get(&store, "missing-id-value").is_err());
    bots::delete(&store, &created.id).unwrap();
    assert!(bots::list(&store).unwrap().is_empty());
    assert!(bots::delete(&store, &created.id).unwrap_err().contains("不存在"));
}

#[test]
fn traverse_chat_dispatch_across_kinds() {
    let tmp = Tmp::new();
    let store = tmp.store();
    for kind in providers::KINDS {
        let provider = providers::upsert(&store, Provider {
            name: format!("{kind} 提供方"),
            kind: (*kind).into(),
            base_url: providers::default_base(kind).into(),
            api_key: "secret".into(),
            model: "provider-model".into(),
            ..Provider::draft(kind)
        })
        .unwrap();
        let bot = bots::upsert(&store, Bot {
            name: format!("{kind} Bot"),
            prompt: "系统提示".into(),
            provider_id: provider.id.clone(),
            model: "override-model".into(),
            ..Bot::draft()
        })
        .unwrap();
        let messages = chat::dispatch(&store, &bot.id, " 你好 ", |request| {
            let body: serde_json::Value = serde_json::from_str(request.body.as_deref().unwrap()).unwrap();
            if *kind == "gemini" {
                assert_eq!(body["systemInstruction"]["parts"][0]["text"], "系统提示");
            } else if *kind == "anthropic" {
                assert_eq!(body["system"], "系统提示");
                assert_eq!(body["model"], "override-model");
            } else {
                assert_eq!(body["model"], "override-model");
                assert_eq!(body["messages"][0]["content"], "系统提示");
            }
            assert!(request.url.contains("override") || body.to_string().contains("override-model") || *kind == "gemini");
            Ok(reply_for(kind, "收到"))
        })
        .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "你好");
        assert_eq!(messages[1].content, "收到");
        chat::delete_bot(&store, &bot.id).unwrap();
        providers::delete(&store, &provider.id).unwrap();
    }
}

#[test]
fn traverse_chat_history_errors_and_model_fallback() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let provider = providers::upsert(&store, Provider {
        name: "本地".into(),
        model: "provider-model".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "Ollama").unwrap())
    })
    .unwrap();
    let bot = bots::upsert(&store, Bot {
        name: "助手".into(),
        prompt: "简短".into(),
        provider_id: provider.id,
        ..Bot::draft()
    })
    .unwrap();
    assert!(chat::dispatch(&store, &bot.id, "   ", |_| unreachable!()).is_err());
    assert!(chat::load(&store, &bot.id).unwrap().is_empty());

    chat::dispatch(&store, &bot.id, "第一句", |_| Ok(reply_for("ollama", "第一答"))).unwrap();
    let failed = chat::dispatch(&store, &bot.id, "第二句", |_| {
        Ok(HttpResponse::text(401, r#"{"error":{"message":"bad key"}}"#))
    })
    .unwrap();
    assert_eq!(failed.last().unwrap().role, "error");
    assert!(failed.last().unwrap().content.contains("bad key"));

    chat::dispatch(&store, &bot.id, "第三句", |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("第一答"));
        assert!(body.contains("第三句"));
        assert!(!body.contains("bad key"));
        let value: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(value["model"], "provider-model");
        Ok(reply_for("ollama", "第三答"))
    })
    .unwrap();

    let bare = bots::upsert(&store, Bot { name: "未配置".into(), ..Bot::draft() }).unwrap();
    assert!(chat::dispatch(&store, &bare.id, "hi", |_| unreachable!()).unwrap_err().contains("提供方"));
}

#[test]
fn traverse_http_client_against_local_server() {
    let url = serve_once(200, "{\"ok\":true}");
    let response = http_client::send(&HttpRequest {
        method: "POST".into(),
        url,
        headers: vec![("content-type".into(), "application/json".into())],
        body: Some("{\"ping\":1}".into()),
        timeout_secs: 5,
        max_bytes: 0,
    })
    .unwrap();
    assert_eq!(response.status, 200);
    assert!(response.body.contains("ok"));

    let url = serve_once(401, "{\"error\":{\"message\":\"bad key\"}}");
    let response = http_client::send(&HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
        timeout_secs: 5,
        max_bytes: 0,
    })
    .unwrap();
    assert_eq!(response.status, 401);
    assert!(response.body.contains("bad key"));

    let url = serve_bytes(200, &[0xFF, 0xD8, 0xFF, 0x00]);
    let response = http_client::send(&HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
        timeout_secs: 5,
        max_bytes: 0,
    })
    .unwrap();
    assert_eq!(response.status, 200);
    assert!(response.body.is_empty());
    assert_eq!(response.bytes, vec![0xFF, 0xD8, 0xFF, 0x00]);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let err = http_client::send(&HttpRequest {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{port}/"),
        headers: vec![],
        body: None,
        timeout_secs: 3,
        max_bytes: 0,
    })
    .unwrap_err();
    assert!(err.contains("网络错误"), "{err}");
}

#[test]
fn traverse_https_uses_system_tls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let mut buf = [0u8; 64];
        let _ = stream.read(&mut buf);
    });
    let err = http_client::send(&HttpRequest {
        method: "GET".into(),
        url: format!("https://127.0.0.1:{port}/v1/models"),
        headers: vec![],
        body: None,
        timeout_secs: 5,
        max_bytes: 0,
    })
    .unwrap_err();
    assert!(
        !err.contains("no TLS backend"),
        "HTTPS 没有接上 TLS：{err}"
    );
    assert!(err.contains("网络错误"), "{err}");
}

#[test]
fn traverse_proxy_rules() {
    assert_eq!(
        proxy::choose_proxy("https://api.deepseek.com/v1", true, "127.0.0.1:7890", "<local>"),
        Some("http://127.0.0.1:7890".into())
    );
    assert_eq!(proxy::choose_proxy("http://127.0.0.1:11434", true, "127.0.0.1:7890", ""), None);
}

#[test]
fn traverse_group_members_and_turns() {
    let tmp = Tmp::new();
    let store = tmp.store();
    assert_eq!(group::unused_name(&store).unwrap(), "新群组");
    assert!(group::create(&store, "   ").unwrap_err().contains("名字"));
    assert!(group::create(&store, &"名".repeat(81)).unwrap_err().contains("过长"));
    let named = group::create(&store, "  新群组  ").unwrap();
    assert_eq!(named.name, "新群组");
    assert_eq!(group::unused_name(&store).unwrap(), "新群组 2");
    assert!(group::rename(&store, &named.id, "  ").unwrap_err().contains("名字"));
    assert_eq!(group::get(&store, &named.id).unwrap().name, "新群组");
    let renamed = group::rename(&store, &named.id, " 改过的名字 ").unwrap();
    assert_eq!(renamed.name, "改过的名字");
    assert!(group::get(&store, "nope").is_err());
    assert!(group::delete(&store, "../secret").is_err());
    assert!(store.root().join("groups.json").exists());

    let provider = providers::upsert(&store, Provider {
        name: "本地".into(),
        model: "test-model".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "Ollama").unwrap())
    })
    .unwrap();
    let jia = make_bot(&store, "甲", &provider.id);
    let yi = make_bot(&store, "乙", "");
    let bing = make_bot(&store, "丙", &provider.id);
    let group = group::create(&store, "  讨论  ").unwrap();
    assert_eq!(group.name, "讨论");
    assert!(group::add_bot(&store, &group.id, "not-a-real-bot").unwrap_err().contains("不存在"));
    let group = group::add_bot(&store, &group.id, &jia.id).unwrap();
    assert!(group::add_bot(&store, &group.id, &jia.id).unwrap_err().contains("已经在群组"));
    let group = group::add_bot(&store, &group.id, &yi.id).unwrap();
    let group = group::add_bot(&store, &group.id, &bing.id).unwrap();
    assert_eq!(group.bot_ids, vec![jia.id.clone(), yi.id.clone(), bing.id.clone()]);
    assert!(group::remove_bot(&store, &group.id, "missing-bot").unwrap_err().contains("不在群组"));

    let packed = group::create(&store, "满员").unwrap();
    for index in 0..12 {
        let bot = make_bot(&store, &format!("成员{index}"), &provider.id);
        group::add_bot(&store, &packed.id, &bot.id).unwrap();
    }
    let extra = make_bot(&store, "多出来", &provider.id);
    assert!(group::add_bot(&store, &packed.id, &extra.id).unwrap_err().contains("12"));

    let mut calls = Vec::new();
    let messages = drain_group(&store, &group.id, "你好", |bot_id, request| {
        calls.push((bot_id.to_string(), request.body.clone().unwrap()));
        if bot_id == jia.id {
            Ok(reply_for("ollama", "甲的回复"))
        } else if bot_id == bing.id {
            Ok(HttpResponse::text(500, r#"{"error":{"message":"UNIQUE_UPSTREAM"}}"#))
        } else {
            panic!("不应该请求 {bot_id}");
        }
    })
    .unwrap();
    assert_eq!(calls.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), vec![jia.id.as_str(), bing.id.as_str()]);
    assert!(calls[0].1.contains("讨论"));
    assert!(calls[0].1.contains("其他 Bot：乙、丙"));
    assert!(calls[0].1.contains("用户：你好"));
    assert!(calls[0].1.contains("甲的提示"));
    assert!(!calls[0].1.contains("甲的回复"));
    assert_eq!(role_text(&calls[1].1, "user"), "用户：你好\n甲：甲的回复");
    assert!(!calls[1].1.contains("UNIQUE_UPSTREAM"));
    assert!(!calls[1].1.contains("选择模型提供方"));
    assert!(calls[1].1.contains("\"model\":\"test-model\""));
    assert_eq!(messages.len(), 4);
    assert_eq!(
        messages[0],
        GroupMessage {
            role: "user".into(),
            content: "你好".into(),
            speaker_id: String::new(),
            file: String::new(),
            input_tokens: 0,
            output_tokens: 0,
        }
    );
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].speaker_id, jia.id);
    assert_eq!(messages[1].content, "甲的回复");
    assert_eq!(messages[2].role, "error");
    assert_eq!(messages[2].speaker_id, yi.id);
    assert!(messages[2].content.contains("提供方"), "{}", messages[2].content);
    assert_eq!(messages[3].role, "error");
    assert_eq!(messages[3].speaker_id, bing.id);
    assert!(messages[3].content.contains("UNIQUE_UPSTREAM"), "{}", messages[3].content);

    calls.clear();
    drain_group(&store, &group.id, "第二句", |bot_id, request| {
        calls.push((bot_id.to_string(), request.body.clone().unwrap()));
        if bot_id == jia.id {
            Ok(reply_for("ollama", "甲的第二句"))
        } else if bot_id == bing.id {
            Ok(reply_for("ollama", "丙的回复"))
        } else {
            panic!("不应该请求 {bot_id}");
        }
    })
    .unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(role_text(&calls[0].1, "assistant"), "甲的回复");
    assert!(role_text(&calls[0].1, "user").contains("用户：第二句"));
    assert!(!calls[0].1.contains("UNIQUE_UPSTREAM"));
    let bing_user = role_text(&calls[1].1, "user");
    assert!(bing_user.contains("用户：你好"));
    assert!(bing_user.contains("甲：甲的回复"));
    assert!(bing_user.contains("用户：第二句"));
    assert!(bing_user.contains("甲：甲的第二句"));
    assert!(!calls[1].1.contains("UNIQUE_UPSTREAM"));

    let quiet = group::create(&store, "空群").unwrap();
    let mut called = false;
    let only_user = drain_group(&store, &quiet.id, "只有我", |_, _| {
        called = true;
        unreachable!();
    })
    .unwrap();
    assert!(!called);
    assert_eq!(only_user.len(), 1);
    assert_eq!(only_user[0].role, "user");
    assert!(group::post_user(&store, &quiet.id, "   ").is_err());
    assert_eq!(group::load(&store, &group.id).unwrap().len(), 8);
    assert_eq!(group::load(&store, &quiet.id).unwrap().len(), 1);

    let kicked = group::remove_bot(&store, &group.id, &bing.id).unwrap();
    assert!(!kicked.bot_ids.contains(&bing.id));
    let mut called = Vec::new();
    drain_group(&store, &group.id, "丙已离开", |bot_id, _| {
        called.push(bot_id.to_string());
        Ok(reply_for("ollama", "还在"))
    })
    .unwrap();
    assert_eq!(called, vec![jia.id.clone()]);

    let elsewhere = group::add_bot(&store, &quiet.id, &jia.id).unwrap();
    assert_eq!(elsewhere.bot_ids, vec![jia.id.clone()]);
    chat::delete_bot(&store, &jia.id).unwrap();
    assert!(bots::get(&store, &jia.id).is_err());
    assert!(!group::get(&store, &group.id).unwrap().bot_ids.contains(&jia.id));
    assert!(!group::get(&store, &quiet.id).unwrap().bot_ids.contains(&jia.id));
    let mut after_delete = Vec::new();
    drain_group(&store, &group.id, "甲已删除", |bot_id, _| {
        after_delete.push(bot_id.to_string());
        Ok(reply_for("ollama", "不应出现"))
    })
    .unwrap();
    assert!(after_delete.is_empty());

    let mut stale = group::get(&store, &quiet.id).unwrap();
    stale.bot_ids.push("missing-bot-1".into());
    let mut all = group::list(&store).unwrap();
    let pos = all.iter().position(|item| item.id == stale.id).unwrap();
    all[pos] = stale;
    store.save_vec("groups.json", &all).unwrap();
    let queue = group::post_user(&store, &quiet.id, "跳过无效成员").unwrap();
    assert!(!queue.iter().any(|id| id == "missing-bot-1"));
    assert!(!group::remove_bot(&store, &quiet.id, "missing-bot-1").unwrap().bot_ids.iter().any(|id| id == "missing-bot-1"));

    let blank = group::commit_reply(&store, &group.id, &yi.id, "   ").unwrap();
    assert_eq!(blank.last().unwrap().role, "error");
    assert_eq!(blank.last().unwrap().speaker_id, yi.id);
    assert!(blank.last().unwrap().content.contains("模型没有返回文本"));

    let group_id = group.id.clone();
    group::delete(&store, &group_id).unwrap();
    assert!(group::get(&store, &group_id).is_err());
    assert!(!tmp.path.join(format!("group-chats/{group_id}.json")).exists());
    assert!(group::commit_reply(&store, &group_id, &yi.id, "晚到").unwrap().is_empty());
    assert!(group::take_next(&store, &group_id, vec![yi.id.clone()]).unwrap().is_none());
    assert!(!tmp.path.join(format!("group-chats/{group_id}.json")).exists());
}

#[test]
fn traverse_group_files() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let group = group::create(&store, "资料").unwrap();
    group::post_user(&store, &group.id, "先记一句").unwrap();
    let note = tmp.path.join("笔记.txt");
    std::fs::write(&note, "群组笔记\n第二行").unwrap();
    let saved = group::import_path(&store, &group.id, &note).unwrap();
    assert_eq!(saved.name, "笔记.txt");
    assert_eq!(group::list_files(&store, &group.id).unwrap(), vec![saved.clone()]);
    assert_eq!(
        group::preview(&store, &group.id, &saved.id).unwrap(),
        Preview::Text { body: "群组笔记\n第二行".into(), truncated: false }
    );

    let empty = tmp.path.join("empty.txt");
    std::fs::write(&empty, "").unwrap();
    let empty_file = group::import_path(&store, &group.id, &empty).unwrap();
    assert_eq!(group::preview(&store, &group.id, &empty_file.id).unwrap(), Preview::Text { body: String::new(), truncated: false });

    let binary = tmp.path.join("pic.bin");
    std::fs::write(&binary, [0u8, 65, 66]).unwrap();
    let binary_file = group::import_path(&store, &group.id, &binary).unwrap();
    assert_eq!(group::preview(&store, &group.id, &binary_file.id).unwrap(), Preview::Binary);
    let odd = tmp.path.join("odd.bin");
    std::fs::write(&odd, [0xff_u8, 0xfe]).unwrap();
    let odd_file = group::import_path(&store, &group.id, &odd).unwrap();
    assert_eq!(group::preview(&store, &group.id, &odd_file.id).unwrap(), Preview::Binary);

    let long_name = format!("{}.txt", "名".repeat(90));
    let long_path = tmp.path.join(&long_name);
    std::fs::write(&long_path, "短").unwrap();
    let long_file = group::import_path(&store, &group.id, &long_path).unwrap();
    assert_eq!(long_file.name.chars().count(), 80);

    let huge = "字".repeat(100_001);
    let huge_path = tmp.path.join("long.txt");
    std::fs::write(&huge_path, &huge).unwrap();
    let huge_file = group::import_path(&store, &group.id, &huge_path).unwrap();
    match group::preview(&store, &group.id, &huge_file.id).unwrap() {
        Preview::Text { body, truncated } => {
            assert!(truncated);
            assert_eq!(body.chars().count(), 100_000);
            assert!(huge.starts_with(&body));
        }
        Preview::Binary => panic!("长文本不应被当成二进制"),
    }

    let big = tmp.path.join("big.bin");
    std::fs::File::create(&big).unwrap().set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(group::import_path(&store, &group.id, &big).unwrap_err().contains("8MB"));
    let before_big = group::list_files(&store, &group.id).unwrap().len();
    assert!(group::list_files(&store, &group.id).unwrap().iter().all(|file| file.name != "big.bin"));
    assert_eq!(group::list_files(&store, &group.id).unwrap().len(), before_big);

    let folder = tmp.path.join("not-a-file");
    std::fs::create_dir(&folder).unwrap();
    assert!(group::import_path(&store, &group.id, &folder).unwrap_err().contains("请选择一个文件"));
    assert!(group::import_path(&store, &group.id, &tmp.path.join("missing.txt")).unwrap_err().contains("请选择一个文件"));
    assert!(group::preview(&store, &group.id, "missing-file").is_err());
    assert!(group::remove_file(&store, &group.id, "missing-file").is_err());
    assert!(group::remove_file(&store, &group.id, "../secret").is_err());

    let mut count = group::list_files(&store, &group.id).unwrap().len();
    while count < 100 {
        let path = tmp.path.join(format!("pad-{count}.txt"));
        std::fs::write(&path, "x").unwrap();
        group::import_path(&store, &group.id, &path).unwrap();
        count += 1;
    }
    let overflow = tmp.path.join("overflow.txt");
    std::fs::write(&overflow, "nope").unwrap();
    assert!(group::import_path(&store, &group.id, &overflow).unwrap_err().contains("100"));
    group::remove_file(&store, &group.id, &saved.id).unwrap();
    assert!(group::preview(&store, &group.id, &saved.id).is_err());
    assert!(!tmp.path.join(format!("group-files/{}/{}", group.id, saved.id)).exists());
    let again = group::import_path(&store, &group.id, &note).unwrap();
    assert_eq!(again.name, "笔记.txt");
    assert_eq!(group::load(&store, &group.id).unwrap()[0].content, "先记一句");

    let id = group.id.clone();
    group::delete(&store, &id).unwrap();
    assert!(!tmp.path.join(format!("group-chats/{id}.json")).exists());
    assert!(!tmp.path.join(format!("group-files/{id}")).exists());
    assert!(group::list(&store).unwrap().iter().all(|item| item.id != id));
    assert!(group::import_path(&store, &id, &note).is_err());
}

#[test]
fn traverse_delete_chat_and_group_leaves_nothing() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let provider = providers::upsert(&store, Provider {
        name: "本地".into(),
        model: "m".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "Ollama").unwrap())
    })
    .unwrap();
    let keep = bots::upsert(&store, Bot {
        name: "留下".into(),
        provider_id: provider.id.clone(),
        ..Bot::draft()
    })
    .unwrap();
    let gone = bots::upsert(&store, Bot {
        name: "清掉".into(),
        provider_id: provider.id.clone(),
        ..Bot::draft()
    })
    .unwrap();
    let gone_file = tmp.path.join(format!("chats/{}.json", gone.id));
    let keep_file = tmp.path.join(format!("chats/{}.json", keep.id));
    chat::delete_chat(&store, &gone.id).unwrap();
    assert!(!gone_file.exists());

    chat::dispatch(&store, &keep.id, "KEEP_LINE", |_| Ok(reply_for("ollama", "KEEP_REPLY"))).unwrap();
    chat::dispatch(&store, &gone.id, "GONE_USER", |_| Ok(reply_for("ollama", "GONE_REPLY"))).unwrap();
    let failed = chat::dispatch(&store, &gone.id, "GONE_NEXT", |_| {
        Ok(HttpResponse::text(500, r#"{"error":{"message":"GONE_ERR"}}"#))
    })
    .unwrap();
    assert_eq!(failed.last().unwrap().role, "error");
    assert!(failed.last().unwrap().content.contains("GONE_ERR"));
    let raw = std::fs::read_to_string(&gone_file).unwrap();
    assert!(raw.contains("GONE_USER"));
    assert!(raw.contains("GONE_REPLY"));
    assert!(raw.contains("GONE_ERR"));

    chat::delete_chat(&store, &gone.id).unwrap();
    assert!(!gone_file.exists());
    assert!(chat::load(&store, &gone.id).unwrap().is_empty());
    assert_eq!(bots::get(&store, &gone.id).unwrap().name, "清掉");
    let kept = std::fs::read_to_string(&keep_file).unwrap();
    assert!(kept.contains("KEEP_LINE"));
    assert!(kept.contains("KEEP_REPLY"));

    let fresh = chat::dispatch(&store, &gone.id, "FRESH_LINE", |request| {
        let body = request.body.clone().unwrap();
        assert!(!body.contains("GONE_USER"));
        assert!(!body.contains("GONE_REPLY"));
        assert!(!body.contains("GONE_ERR"));
        assert!(body.contains("FRESH_LINE"));
        Ok(reply_for("ollama", "FRESH_REPLY"))
    })
    .unwrap();
    assert_eq!(fresh.len(), 2);
    assert_eq!(fresh[0].content, "FRESH_LINE");
    assert_eq!(fresh[1].content, "FRESH_REPLY");
    chat::delete_chat(&store, &gone.id).unwrap();
    chat::delete_chat(&store, &gone.id).unwrap();
    assert!(!gone_file.exists());
    assert!(chat::load(&store, &gone.id).unwrap().is_empty());

    for bad in ["", "abc", "../secret", "a/b", "has space"] {
        assert!(chat::delete_chat(&store, bad).is_err(), "{bad}");
    }
    let orphan = store::new_id();
    let orphan_file = tmp.path.join(format!("chats/{orphan}.json"));
    std::fs::write(&orphan_file, r#"[{"role":"user","content":"ORPHAN"}]"#).unwrap();
    chat::delete_chat(&store, &orphan).unwrap();
    assert!(!orphan_file.exists());

    let stay = group::create(&store, "还在的群").unwrap();
    group::post_user(&store, &stay.id, "STAY_GROUP").unwrap();
    let drop = group::create(&store, "要删的群").unwrap();
    group::add_bot(&store, &drop.id, &keep.id).unwrap();
    group::post_user(&store, &drop.id, "DROP_GROUP").unwrap();
    let note = tmp.path.join("群笔记.txt");
    std::fs::write(&note, "GROUP_FILE_BODY").unwrap();
    let saved = group::import_path(&store, &drop.id, &note).unwrap();
    assert_eq!(group::preview(&store, &drop.id, &saved.id).unwrap(), Preview::Text { body: "GROUP_FILE_BODY".into(), truncated: false });

    chat::delete_chat(&store, &keep.id).unwrap();
    assert!(!keep_file.exists());
    assert_eq!(group::load(&store, &drop.id).unwrap()[0].content, "DROP_GROUP");
    assert!(bots::get(&store, &keep.id).is_ok());

    let empty = group::create(&store, "空壳").unwrap();
    group::delete(&store, &empty.id).unwrap();
    assert!(group::get(&store, &empty.id).is_err());
    assert!(!tmp.path.join(format!("group-chats/{}.json", empty.id)).exists());
    assert!(!tmp.path.join(format!("group-files/{}", empty.id)).exists());

    let drop_id = drop.id.clone();
    group::delete(&store, &drop_id).unwrap();
    assert!(group::get(&store, &drop_id).is_err());
    assert!(group::delete(&store, &drop_id).is_err());
    let roster = std::fs::read_to_string(tmp.path.join("groups.json")).unwrap();
    assert!(!roster.contains(&drop_id));
    assert!(!roster.contains(&empty.id));
    assert!(!roster.contains("要删的群"));
    assert!(!roster.contains("空壳"));
    assert!(roster.contains(&stay.id));
    assert!(roster.contains("还在的群"));
    assert!(!tmp.path.join(format!("group-chats/{drop_id}.json")).exists());
    assert!(!tmp.path.join(format!("group-files/{drop_id}")).exists());
    assert!(tmp.path.join(format!("group-chats/{}.json", stay.id)).exists());
    assert_eq!(group::load(&store, &stay.id).unwrap()[0].content, "STAY_GROUP");
    assert!(group::list_files(&store, &drop_id).is_err());
}

#[test]
fn traverse_folder_read_and_write() {
    let tmp = Tmp::new();
    let store = tmp.store();
    let sandbox = tmp.path.join("sandbox");
    std::fs::create_dir(&sandbox).unwrap();
    let shown = folder::clean_root(&sandbox.display().to_string()).unwrap();
    assert_eq!(folder::clean_root(&format!("  {shown}{}  ", std::path::MAIN_SEPARATOR)).unwrap(), shown);
    assert_eq!(folder::clean_root("   ").unwrap(), "");
    assert!(folder::clean_root("work").unwrap_err().contains("绝对路径"));
    let missing = tmp.path.join("no-such-dir");
    assert!(folder::clean_root(&missing.display().to_string()).unwrap_err().contains("已有"));
    let file = tmp.path.join("file.txt");
    std::fs::write(&file, "x").unwrap();
    assert!(folder::clean_root(&file.display().to_string()).unwrap_err().contains("已有"));
    let long = format!("C:\\{}", "a".repeat(1000));
    assert!(long.chars().count() > 1000);
    assert!(folder::clean_root(&long).unwrap_err().contains("过长"));

    let empty = folder::list(&shown, "").unwrap();
    assert!(empty.entries.is_empty());
    assert!(!empty.truncated);
    assert_eq!(folder::write(&shown, "", "a.txt", b"hello").unwrap(), "a.txt");
    assert_eq!(std::fs::read_to_string(sandbox.join("a.txt")).unwrap(), "hello");
    assert_eq!(folder::read(&shown, "a.txt").unwrap(), folder::Preview::Text { body: "hello".into(), truncated: false });
    assert_eq!(folder::read(&shown, "./a.txt").unwrap(), folder::Preview::Text { body: "hello".into(), truncated: false });
    assert_eq!(folder::read(&shown, "a.txt").unwrap(), folder::Preview::Text { body: "hello".into(), truncated: false });
    folder::write(&shown, "", "a.txt", b"next").unwrap();
    assert_eq!(std::fs::read(sandbox.join("a.txt")).unwrap(), b"next");
    assert_eq!(folder::write(&shown, "", "empty.txt", b"").unwrap(), "empty.txt");
    assert_eq!(folder::read(&shown, "empty.txt").unwrap(), folder::Preview::Text { body: String::new(), truncated: false });
    std::fs::create_dir(sandbox.join("zzz")).unwrap();
    std::fs::create_dir_all(sandbox.join("sub").join("dir")).unwrap();
    assert_eq!(folder::write(&shown, "sub/dir", "c.txt", "nested\n".as_bytes()).unwrap(), "sub/dir/c.txt");
    assert_eq!(std::fs::read_to_string(sandbox.join("sub").join("dir").join("c.txt")).unwrap(), "nested\n");
    assert_eq!(
        folder::read(&shown, "sub\\dir\\c.txt").unwrap(),
        folder::Preview::Text { body: "nested\n".into(), truncated: false }
    );
    let listed = folder::list(&shown, "").unwrap();
    assert!(listed.entries.iter().any(|entry| entry.dir && entry.name == "sub"));
    assert!(listed.entries.iter().any(|entry| entry.dir && entry.name == "zzz"));
    assert!(listed.entries.iter().any(|entry| !entry.dir && entry.name == "a.txt" && entry.size == 4));
    assert!(listed.entries.iter().any(|entry| entry.dir));
    assert!(listed.entries.iter().position(|entry| entry.dir).unwrap() < listed.entries.iter().position(|entry| !entry.dir).unwrap());
    let inner = folder::list(&shown, "sub/dir").unwrap();
    assert_eq!(inner.entries.len(), 1);
    assert_eq!(inner.entries[0].name, "c.txt");
    assert!(!inner.entries[0].dir);

    assert!(folder::read(&shown, "").unwrap_err().contains("请选择一个文件"));
    assert_eq!(folder::read(&shown, "missing.txt").unwrap_err(), "文件不存在");
    assert!(folder::read(&shown, "missing/a.txt").unwrap_err().contains("文件夹不存在"));
    assert!(folder::read(&shown, "sub").unwrap_err().contains("这是文件夹"));
    assert!(folder::write(&shown, "", "sub", b"x").unwrap_err().contains("文件夹"));
    assert!(folder::write(&shown, "", "../out.txt", b"nope").unwrap_err().contains("路径"));
    assert!(!tmp.path.join("out.txt").exists());
    assert!(folder::write(&shown, "", "a/b.txt", b"nope").unwrap_err().contains("路径"));
    assert!(folder::write(&shown, "", "", b"nope").unwrap_err().contains("文件名"));
    assert!(folder::write(&shown, "", &"名".repeat(201), b"x").unwrap_err().contains("过长"));
    assert!(folder::read(&shown, "../out.txt").unwrap_err().contains("超出"));
    assert!(folder::read(&shown, "C:\\Windows\\win.ini").unwrap_err().contains("超出"));
    assert!(folder::list(&shown, &"a".repeat(301)).unwrap_err().contains("过长"));
    assert!(folder::write(&shown, "missing-dir", "a.txt", b"x").unwrap_err().contains("文件夹不存在"));

    std::fs::write(sandbox.join("bin.dat"), [0u8, 65, 66]).unwrap();
    assert_eq!(folder::read(&shown, "bin.dat").unwrap(), folder::Preview::Binary);
    std::fs::write(sandbox.join("odd.dat"), [0xff_u8, 0xfe]).unwrap();
    assert_eq!(folder::read(&shown, "odd.dat").unwrap(), folder::Preview::Binary);
    let huge = "字".repeat(100_001);
    std::fs::write(sandbox.join("huge.txt"), &huge).unwrap();
    match folder::read(&shown, "huge.txt").unwrap() {
        folder::Preview::Text { body, truncated } => {
            assert!(truncated);
            assert_eq!(body.chars().count(), 100_000);
            assert!(huge.starts_with(&body));
        }
        folder::Preview::Binary => panic!("长文本不应被当成二进制"),
    }
    let big = sandbox.join("big.bin");
    std::fs::File::create(&big).unwrap().set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(folder::read(&shown, "big.bin").unwrap_err().contains("8MB"));
    let too_big = vec![b'x'; 8 * 1024 * 1024 + 1];
    assert!(folder::write(&shown, "", "too.txt", &too_big).unwrap_err().contains("8MB"));
    assert!(!sandbox.join("too.txt").exists());

    let many = tmp.path.join("many");
    std::fs::create_dir(&many).unwrap();
    for index in 0..201 {
        std::fs::write(many.join(format!("f{index:03}.txt")), "x").unwrap();
    }
    let many_shown = folder::clean_root(&many.display().to_string()).unwrap();
    let crowded = folder::list(&many_shown, "").unwrap();
    assert!(crowded.truncated);
    assert_eq!(crowded.entries.len(), 200);
    assert!(crowded.entries.iter().all(|entry| !entry.dir));

    let gone = tmp.path.join("gone");
    std::fs::create_dir(&gone).unwrap();
    let gone_shown = folder::clean_root(&gone.display().to_string()).unwrap();
    std::fs::remove_dir(&gone).unwrap();
    assert!(folder::list(&gone_shown, "").unwrap_err().contains("文件夹不存在"));

    let outside = tmp.path.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "SECRET_MARK").unwrap();
    if link_file(&outside.join("secret.txt"), &sandbox.join("secret.link")) {
        let msg = folder::read(&shown, "secret.link").unwrap_err();
        assert!(!msg.contains("SECRET_MARK"), "{msg}");
        assert!(msg.contains("超出"), "{msg}");
        assert!(folder::list(&shown, "").unwrap().entries.iter().all(|entry| entry.name != "secret.link"));
    }
    if link_dir(&outside, &sandbox.join("out-link")) {
        let msg = folder::write(&shown, "out-link", "pwned.txt", b"nope").unwrap_err();
        assert!(!msg.contains("已写入"), "{msg}");
        assert!(!outside.join("pwned.txt").exists());
        assert!(folder::list(&shown, "").unwrap().entries.iter().all(|entry| entry.name != "out-link"));
    }

    let legacy_bot = store::new_id();
    std::fs::write(
        store.root().join("bots.json"),
        format!(r#"[{{"id":"{legacy_bot}","name":"旧","prompt":"","providerId":"","model":""}}]"#),
    )
    .unwrap();
    assert_eq!(bots::get(&store, &legacy_bot).unwrap().folder, "");
    let legacy_group = store::new_id();
    std::fs::write(
        store.root().join("groups.json"),
        format!(r#"[{{"id":"{legacy_group}","name":"旧群","botIds":[]}}]"#),
    )
    .unwrap();
    assert_eq!(group::get(&store, &legacy_group).unwrap().folder, "");

    let provider = providers::upsert(&store, Provider {
        name: "本地".into(),
        model: "test-model".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "Ollama").unwrap())
    })
    .unwrap();
    let bot_dir = tmp.path.join("bot-marker");
    std::fs::create_dir(&bot_dir).unwrap();
    let bot_shown = folder::clean_root(&bot_dir.display().to_string()).unwrap();
    assert!(bots::upsert(&store, Bot {
        name: "坏路径".into(),
        folder: "相对".into(),
        ..Bot::draft()
    })
    .unwrap_err()
    .contains("绝对路径"));
    let bot = bots::upsert(&store, Bot {
        name: "读写".into(),
        prompt: "系统提示".into(),
        provider_id: provider.id.clone(),
        model: "override-model".into(),
        folder: format!("  {bot_shown}  "),
        ..Bot::draft()
    })
    .unwrap();
    assert_eq!(bot.folder, bot_shown);
    assert!(std::fs::read_to_string(store.root().join("bots.json")).unwrap().contains("bot-marker"));
    folder::write(&bot_shown, "", "note.txt", b"hello").unwrap();
    let shown_prompt = folder::compose("系统提示", &bot_shown);
    assert!(shown_prompt.contains("note.txt"));
    assert!(shown_prompt.contains("hello"));
    assert_eq!(folder::compose("系统提示", ""), "系统提示");
    let nested = folder::round(&bot_shown, "写入 sub/dir/new.txt\n<<<\nnested\n>>>\n写好了").unwrap();
    assert_eq!(nested, folder::Round::Done("写好了".into()));
    assert_eq!(std::fs::read(bot_dir.join("sub").join("dir").join("new.txt")).unwrap(), b"nested");
    let escaped = folder::round(&bot_shown, "写入 ../out.txt\n<<<\nno\n>>>").unwrap();
    assert!(matches!(escaped, folder::Round::Continue(ref note) if note.contains("超出")));
    assert!(!tmp.path.join("out.txt").exists());
    let hidden = tmp.path.join("skip-root");
    std::fs::create_dir_all(hidden.join("target")).unwrap();
    std::fs::write(hidden.join("target").join("secret.txt"), "SECRET_BUILD").unwrap();
    std::fs::write(hidden.join("keep.txt"), "visible-text").unwrap();
    let hidden_shown = folder::clean_root(&hidden.display().to_string()).unwrap();
    let hidden_prompt = folder::compose("", &hidden_shown);
    assert!(hidden_prompt.contains("visible-text"));
    assert!(hidden_prompt.contains("target/"));
    assert!(!hidden_prompt.contains("SECRET_BUILD"));
    let mut calls = 0;
    let messages = chat::dispatch(&store, &bot.id, "看看文件", |request| {
        calls += 1;
        let body = request.body.clone().unwrap();
        assert!(body.contains("系统提示"));
        assert!(body.contains("看看文件"));
        assert!(body.contains("bot-marker"));
        assert!(body.contains("note.txt"));
        if calls == 1 {
            assert!(body.contains("hello"));
            Ok(reply_for("ollama", "读取 note.txt\n我看看"))
        } else {
            assert!(body.contains("文件 note.txt"));
            Ok(reply_for("ollama", "文件里是 hello"))
        }
    })
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(messages.last().unwrap().content, "文件里是 hello");
    assert!(messages.iter().all(|message| !message.content.contains("<<<")));
    let messages = chat::dispatch(&store, &bot.id, "写下来", |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("看看文件"));
        Ok(reply_for("ollama", "写入 extra.txt\n<<<\n新内容\n>>>\n写好了"))
    })
    .unwrap();
    assert_eq!(std::fs::read(bot_dir.join("extra.txt")).unwrap(), "新内容".as_bytes());
    assert_eq!(messages.last().unwrap().content, "写好了");
    assert_eq!(std::fs::read(bot_dir.join("note.txt")).unwrap(), b"hello");

    let group_dir = tmp.path.join("group-marker");
    std::fs::create_dir(&group_dir).unwrap();
    let group_shown = folder::clean_root(&group_dir.display().to_string()).unwrap();
    let group = group::create(&store, "原来").unwrap();
    assert!(group::save_settings(&store, &group.id, "新名字", "相对").unwrap_err().contains("绝对路径"));
    assert!(group::save_settings(&store, &group.id, "别的", &file.display().to_string()).unwrap_err().contains("已有"));
    assert_eq!(group::get(&store, &group.id).unwrap().name, "原来");
    assert!(group::get(&store, &group.id).unwrap().folder.is_empty());
    let saved = group::save_settings(&store, &group.id, " 讨论 ", &format!("  {group_shown}  ")).unwrap();
    assert_eq!(saved.name, "讨论");
    assert_eq!(saved.folder, group_shown);
    let renamed = group::rename(&store, &group.id, "又改了").unwrap();
    assert_eq!(renamed.folder, group_shown);
    assert_eq!(folder::write(&group_shown, "", "from-group.txt", "群里写下\n".as_bytes()).unwrap(), "from-group.txt");
    assert_eq!(folder::read(&group_shown, "from-group.txt").unwrap(), folder::Preview::Text { body: "群里写下\n".into(), truncated: false });
    group::add_bot(&store, &group.id, &bot.id).unwrap();
    let mut calls = 0;
    drain_group(&store, &group.id, "群里看看", |_, request| {
        calls += 1;
        let body = request.body.clone().unwrap();
        assert!(body.contains("group-marker"));
        assert!(body.contains("from-group.txt"));
        assert!(body.contains("群里写下"));
        assert!(!body.contains("bot-marker"));
        assert!(!body.contains("hello"));
        assert!(body.contains("又改了"));
        Ok(reply_for("ollama", "只说话"))
    })
    .unwrap();
    assert_eq!(calls, 1);
    let written = drain_group(&store, &group.id, "写进群文件夹", |_, _| {
        Ok(reply_for("ollama", "写入 g.txt\n<<<\ngroup-write\n>>>\n写完"))
    })
    .unwrap();
    assert_eq!(std::fs::read(group_dir.join("g.txt")).unwrap(), b"group-write");
    assert_eq!(written.last().unwrap().content, "写完");
    let group_id = group.id.clone();
    group::delete(&store, &group_id).unwrap();
    assert!(group_dir.join("from-group.txt").is_file());
    chat::delete_bot(&store, &bot.id).unwrap();
    assert!(bot_dir.join("note.txt").is_file());
}

#[test]
fn traverse_pick_folder_path() {
    assert!(pick_folder::dialog_layout_ok());
    assert!(pick_folder::path_from_wide(&[]).is_none());
    assert!(pick_folder::path_from_wide(&[0, b'a' as u16]).is_none());
    let mut text: Vec<u16> = "D:\\项目\\资料".encode_utf16().collect();
    text.push(0);
    text.push(b'x' as u16);
    let path = pick_folder::path_from_wide(&text).unwrap();
    assert_eq!(path, std::path::PathBuf::from("D:\\项目\\资料"));
    let mut root: Vec<u16> = "D:\\".encode_utf16().collect();
    root.push(0);
    assert_eq!(pick_folder::path_from_wide(&root).unwrap(), std::path::PathBuf::from("D:\\"));
}


#[test]
fn traverse_image_speech_and_video() {
    assert_eq!(providers::TASKS, ["chat", "image", "speech", "video"]);
    assert_eq!(providers::task_of("").unwrap(), "chat");
    assert_eq!(providers::task_of(" image ").unwrap(), "image");
    assert!(providers::task_of("nope").unwrap_err().contains("不支持的用途"));
    assert_eq!(providers::task_label(""), "对话");
    assert_eq!(providers::task_label("speech"), "语音");
    assert_eq!(providers::task_label("video"), "视频");
    assert_eq!(providers::task_label("nope"), "未知");
    assert!(providers::supports_task("openai", "image"));
    assert!(providers::supports_task("azure", "video"));
    assert!(providers::supports_task("gemini", "speech"));
    assert!(!providers::supports_task("anthropic", "image"));
    assert!(!providers::supports_task("ollama", "speech"));
    assert!(providers::supports_task("ollama", "chat"));

    let old: Provider = serde_json::from_str(
        r#"{"id":"old-provider","name":"旧","kind":"openai","baseUrl":"https://api.openai.com/v1"}"#,
    )
    .unwrap();
    assert!(old.task.is_empty());
    assert!(old.voice.is_empty());

    let mut form = Provider::draft("openai");
    form.task = "image".into();
    providers::change_kind(&mut form, "gemini");
    assert_eq!(form.task, "image");
    providers::change_kind(&mut form, "ollama");
    assert!(form.task.is_empty());

    let preset = providers::presets().iter().find(|item| item.name == "OpenAI 生图").unwrap();
    let mut form = Provider::draft("openai");
    form.api_key = "k".into();
    providers::apply_preset(&mut form, preset);
    assert_eq!(form.task, "image");
    assert_eq!(form.model, "dall-e-3");
    assert_eq!(form.api_key, "k");
    assert_eq!(form.base_url, "https://api.openai.com/v1");
    assert!(providers::presets().iter().any(|item| item.group == "生图、语音、视频" && item.name == "Gemini 语音" && item.voice == "Kore" && item.task == "speech"));
    assert!(providers::presets().iter().any(|item| item.name == "OpenAI 视频" && item.model == "sora-2" && item.task == "video"));
    assert!(providers::presets().iter().any(|item| item.name == "Gemini 生图" && item.model == "imagen-4.0-generate-001"));
    assert!(providers::presets().iter().any(|item| item.name == "Gemini 视频" && item.task == "video"));

    let mut rejected = media_provider("ollama", "image", "llava");
    assert!(providers::validate(rejected.clone()).unwrap_err().contains("不能用于该用途"));
    rejected.kind = "openai".into();
    rejected.base_url = providers::default_base("openai").into();
    rejected.task = "nope".into();
    assert!(providers::validate(rejected.clone()).unwrap_err().contains("不支持的用途"));
    rejected.task = "speech".into();
    rejected.voice = "啊".repeat(41);
    assert!(providers::validate(rejected.clone()).unwrap_err().contains("语音角色过长"));
    rejected.voice = " nova ".into();
    let saved = providers::validate(rejected).unwrap();
    assert_eq!(saved.voice, "nova");
    assert_eq!(saved.task, "speech");
    let chat_task = providers::validate(media_provider("openai", "chat", "gpt-4o-mini")).unwrap();
    assert!(chat_task.task.is_empty());

    let png = tiny_png();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    let openai = media_provider("openai", "image", "dall-e-3");
    let request = image_gen::build(&openai, "一只猫").unwrap();
    assert!(request.url.ends_with("/images/generations"), "{}", request.url);
    assert_eq!(header(&request, "authorization"), Some("Bearer secret"));
    assert_eq!(header(&request, "content-type"), Some("application/json"));
    let body = request.body.unwrap();
    assert!(body.contains("response_format"));
    assert!(body.contains("1024x1024"));
    assert!(body.contains("一只猫"));
    assert!(!image_gen::build(&media_provider("openai", "image", "gpt-image-1"), "猫").unwrap().body.unwrap().contains("response_format"));
    let azure = image_gen::build(&media_provider("azure", "image", "my deploy"), "猫").unwrap();
    assert!(azure.url.contains("/images/generations"), "{}", azure.url);
    assert!(azure.url.contains("my%20deploy"), "{}", azure.url);
    assert!(azure.url.contains("api-version="));
    assert_eq!(header(&azure, "api-key"), Some("secret"));
    assert!(!azure.body.unwrap().contains("\"model\""));
    let imagen = image_gen::build(&media_provider("gemini", "image", "imagen-4.0-generate-001"), "一只猫").unwrap();
    assert!(imagen.url.contains(":predict"), "{}", imagen.url);
    assert!(!imagen.url.contains("predictLongRunning"));
    assert!(imagen.url.contains("key=secret"));
    assert!(!imagen.body.as_deref().unwrap().contains("secret"));
    let flash = image_gen::build(&media_provider("gemini", "image", "gemini-2.5-flash-image"), "一只猫").unwrap();
    assert!(flash.url.contains(":generateContent"));
    assert!(flash.body.unwrap().contains("IMAGE"));
    assert!(image_gen::build(&media_provider("anthropic", "image", "claude"), "猫").unwrap_err().contains("不能生图"));
    assert!(image_gen::build(&media_provider("openai", "image", ""), "猫").unwrap_err().contains("模型名"));
    assert!(image_gen::build(&media_provider("azure", "image", ""), "猫").unwrap_err().contains("部署名"));

    match image_gen::parse("openai", 200, &serde_json::json!({"data":[{"b64_json": encoded, "revised_prompt": "改过的猫"}]}).to_string()).unwrap() {
        Loaded::Bytes(piece) => {
            assert_eq!(piece.note, "改过的猫");
            assert_eq!(piece.ext, "png");
            assert_eq!(piece.bytes, png);
        }
        Loaded::Fetch { .. } => panic!("应该直接得到图片"),
    }
    let messy = format!("{}\n{}", &encoded[..8], &encoded[8..]);
    assert_eq!(
        image_bytes(image_gen::parse("openai", 200, &serde_json::json!({"data":[{"b64_json": messy}]}).to_string()).unwrap()),
        png
    );
    assert!(image_gen::parse("openai", 200, r#"{"data":[{"b64_json":"!!!!"}]}"#).unwrap_err().contains("无法解析"));
    match image_gen::parse("openai", 200, r#"{"data":[{"url":"https://cdn.example/a.png","revised_prompt":"外链"}]}"#).unwrap() {
        Loaded::Fetch { url, note, ext } => {
            assert_eq!(url, "https://cdn.example/a.png");
            assert_eq!(note, "外链");
            assert_eq!(ext, "png");
        }
        Loaded::Bytes(_) => panic!("应该去下载"),
    }
    assert!(image_gen::parse("openai", 401, r#"{"error":{"message":"bad key"}}"#).unwrap_err().contains("bad key"));
    assert!(image_gen::parse("openai", 200, "not-json").unwrap_err().contains("不是 JSON"));
    assert!(image_gen::parse("openai", 200, r#"{"data":[]}"#).unwrap_err().contains("没有返回图片"));
    assert_eq!(
        image_bytes(image_gen::parse("gemini", 200, &serde_json::json!({"predictions":[{"bytesBase64Encoded": encoded, "mimeType":"image/png"}]}).to_string()).unwrap()),
        png
    );
    match image_gen::parse(
        "gemini",
        200,
        &serde_json::json!({"candidates":[{"content":{"parts":[{"text":"画好了"},{"inlineData":{"mimeType":"image/png","data": encoded}}]}}]}).to_string(),
    )
    .unwrap()
    {
        Loaded::Bytes(piece) => {
            assert_eq!(piece.note, "画好了");
            assert_eq!(piece.bytes, png);
        }
        Loaded::Fetch { .. } => panic!("应该直接得到图片"),
    }
    assert!(image_gen::parse("gemini", 200, r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#).unwrap_err().contains("SAFETY"));
    let mut fetched = 0;
    let piece = image_gen::generate(&openai, "一只猫", &mut |request| {
        fetched += 1;
        if fetched == 1 {
            Ok(HttpResponse::text(200, r#"{"data":[{"url":"https://cdn.example/a.png"}]}"#))
        } else {
            assert_eq!(request.method, "GET");
            assert!(request.url.contains("cdn.example"));
            Ok(HttpResponse::binary(200, png.clone()))
        }
    })
    .unwrap();
    assert_eq!(piece.bytes, png);
    assert_eq!(fetched, 2);

    let speech_provider = media_provider("openai", "speech", "tts-1");
    let request = speech::build(&speech_provider, "你好").unwrap();
    assert!(request.url.ends_with("/audio/speech"), "{}", request.url);
    let body = request.body.unwrap();
    assert!(body.contains("\"voice\":\"alloy\""));
    assert!(body.contains("你好"));
    assert!(body.contains("mp3"));
    let mut named_voice = speech_provider.clone();
    named_voice.voice = "nova".into();
    assert!(speech::build(&named_voice, "你好").unwrap().body.unwrap().contains("nova"));
    let gemini_voice = speech::build(&media_provider("gemini", "speech", "gemini-2.5-flash-preview-tts"), "你好").unwrap();
    assert!(gemini_voice.url.contains(":generateContent"));
    let body = gemini_voice.body.unwrap();
    assert!(body.contains("Kore"));
    assert!(body.contains("AUDIO"));
    assert!(!body.contains("系统提示"));
    let azure_voice = speech::build(&media_provider("azure", "speech", "tts"), "你好").unwrap();
    assert!(azure_voice.url.contains("/audio/speech"), "{}", azure_voice.url);
    assert!(!azure_voice.body.unwrap().contains("\"model\""));
    assert!(speech::build(&media_provider("ollama", "speech", "tts"), "你好").unwrap_err().contains("不能生成语音"));
    match speech::parse("openai", 200, "", b"ID3hello").unwrap() {
        Loaded::Bytes(piece) => {
            assert_eq!(piece.ext, "mp3");
            assert_eq!(piece.note, "已生成语音");
            assert_eq!(piece.bytes, b"ID3hello");
        }
        Loaded::Fetch { .. } => panic!("应该是音频字节"),
    }
    let pcm = serde_json::json!({"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"audio/L16;rate=24000","data":"AAECAw=="}}]}}]}).to_string();
    match speech::parse("gemini", 200, &pcm, pcm.as_bytes()).unwrap() {
        Loaded::Bytes(piece) => {
            assert_eq!(piece.ext, "wav");
            assert!(piece.bytes.starts_with(b"RIFF"));
            assert_eq!(&piece.bytes[24..28], &24_000u32.to_le_bytes());
            assert_eq!(&piece.bytes[piece.bytes.len() - 4..], &[0, 1, 2, 3]);
        }
        Loaded::Fetch { .. } => panic!("应该是音频字节"),
    }
    assert!(speech::parse("gemini", 200, r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#, b"{}").unwrap_err().contains("SAFETY"));
    assert!(speech::parse("openai", 200, "", b"").unwrap_err().contains("没有返回语音"));
    let spoken = speech::generate(&speech_provider, "你好", &mut |_| Ok(HttpResponse::binary(200, b"ID3xyz".to_vec()))).unwrap();
    assert_eq!(spoken.ext, "mp3");
    assert_eq!(spoken.note, "已生成语音");

    let video_provider = media_provider("openai", "video", "sora-2");
    let request = video::build(&video_provider, "海浪").unwrap();
    assert!(request.url.ends_with("/videos"), "{}", request.url);
    let body = request.body.unwrap();
    assert!(body.contains("1280x720"));
    assert!(body.contains("\"seconds\":\"4\""));
    assert!(body.contains("海浪"));
    let azure_video = video::build(&media_provider("azure", "video", "sora"), "海浪").unwrap();
    assert!(azure_video.url.contains("/videos?"), "{}", azure_video.url);
    assert!(!azure_video.body.unwrap().contains("\"model\""));
    let gemini_video = video::build(&media_provider("gemini", "video", "veo-3.0-generate-001"), "海浪").unwrap();
    assert!(gemini_video.url.contains(":predictLongRunning"), "{}", gemini_video.url);
    assert!(video::build(&media_provider("anthropic", "video", "claude"), "海浪").unwrap_err().contains("不能生成视频"));
    let mp4 = tiny_mp4();
    let mut calls = Vec::new();
    let piece = video::generate(&video_provider, "海浪", Duration::ZERO, &mut |request| {
        calls.push(request.url.clone());
        match calls.len() {
            1 => Ok(HttpResponse::text(200, r#"{"id":"video_1","status":"queued"}"#)),
            2 => {
                assert!(request.url.ends_with("/videos/video_1"));
                assert!(request.body.is_none());
                assert_eq!(header(request, "authorization"), Some("Bearer secret"));
                Ok(HttpResponse::text(200, r#"{"id":"video_1","status":"completed"}"#))
            }
            _ => {
                assert!(request.url.ends_with("/videos/video_1/content"));
                Ok(HttpResponse::binary(200, mp4.clone()))
            }
        }
    })
    .unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(piece.bytes, mp4);
    assert_eq!(piece.note, "已生成视频");
    assert!(video::generate(&video_provider, "海浪", Duration::ZERO, &mut |_| {
        Ok(HttpResponse::text(200, r#"{"id":"video_1","status":"failed","error":{"message":"被拒绝"}}"#))
    })
    .unwrap_err()
    .contains("被拒绝"));
    let mut polls = 0;
    let err = video::generate(&video_provider, "海浪", Duration::ZERO, &mut |_| {
        polls += 1;
        Ok(HttpResponse::text(200, r#"{"id":"video_1","status":"queued"}"#))
    })
    .unwrap_err();
    assert!(err.contains("超时"), "{err}");
    assert_eq!(polls, media::POLL_LIMIT);
    let gemini = media_provider("gemini", "video", "veo-3.0-generate-001");
    let mut steps = 0;
    let piece = video::generate(&gemini, "海浪", Duration::ZERO, &mut |request| {
        steps += 1;
        match steps {
            1 => Ok(HttpResponse::text(200, r#"{"name":"operations/abc"}"#)),
            2 => {
                assert!(request.url.contains("/operations/abc"), "{}", request.url);
                assert!(request.url.contains("key=secret"));
                Ok(HttpResponse::text(
                    200,
                    r#"{"done":true,"response":{"generateVideoResponse":{"generatedSamples":[{"video":{"uri":"https://generativelanguage.googleapis.com/v1beta/files/xyz"}}]}}}"#,
                ))
            }
            _ => {
                assert!(request.url.contains("files/xyz"));
                assert!(request.url.contains("key=secret"));
                Ok(HttpResponse::binary(200, mp4.clone()))
            }
        }
    })
    .unwrap();
    assert_eq!(piece.ext, "mp4");
    assert_eq!(steps, 3);

    let tmp = Tmp::new();
    let store = tmp.store();
    assert!(media::bot_dir("abc").unwrap_err().contains("非法"));
    assert!(media::group_dir("../secret").is_err());
    assert!(media::save(&store, "../x", b"ID3", "mp3").unwrap_err().contains("非法"));
    assert!(media::save(&store, "media/bots/sample", b"ID3", "exe").unwrap_err().contains("不支持"));
    assert!(media::open_saved(&store, "../x").unwrap_err().contains("非法"));
    assert!(media::open_saved(&store, "media/missing.png").unwrap_err().contains("不存在"));
    assert!(media::decode_rgba(b"not-an-image").is_err());
    let (width, height, rgba) = media::decode_rgba(&png).unwrap();
    assert_eq!((width, height), (1, 1));
    assert_eq!(&rgba[..3], &[9, 8, 7]);

    let err = media::generate(&store, &openai, &"啊".repeat(4000), "猫", "media/bots/sample", Duration::ZERO, |_| unreachable!()).unwrap_err();
    assert!(err.contains("提示过长"), "{err}");
    assert!(media::generate(&store, &openai, "", "", "media/bots/sample", Duration::ZERO, |_| unreachable!()).unwrap_err().contains("没有可发送的用户消息"));
    assert!(media::generate(&store, &media_provider("ollama", "video", "x"), "", "海浪", "media/bots/sample", Duration::ZERO, |_| unreachable!()).unwrap_err().contains("不能生成视频"));
    let outcome = media::generate(&store, &speech_provider, "不要朗读这段", "请读我", "media/bots/sample", Duration::ZERO, |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("请读我"));
        assert!(!body.contains("不要朗读这段"));
        Ok(HttpResponse::binary(200, b"ID3abc".to_vec()))
    })
    .unwrap();
    assert_eq!(outcome.text, "已生成语音");
    assert!(media::saved_path(&store, &outcome.file).unwrap().is_file());

    let sandbox = tmp.path.join("sandbox");
    std::fs::create_dir(&sandbox).unwrap();
    std::fs::write(sandbox.join("UNIQUE_FOLDER_MARKER.txt"), "marker").unwrap();
    let folder = folder::clean_root(&sandbox.display().to_string()).unwrap();
    let provider = providers::upsert(&store, Provider {
        name: "图".into(),
        api_key: "secret".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "OpenAI 生图").unwrap())
    })
    .unwrap();
    let painter = bots::upsert(&store, Bot {
        name: "画师".into(),
        prompt: "扁平插画".into(),
        provider_id: provider.id.clone(),
        model: "gpt-image-1".into(),
        folder: folder.clone(),
        ..Bot::draft()
    })
    .unwrap();
    let messages = chat::dispatch(&store, &painter.id, "一只猫", |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("扁平插画"));
        assert!(body.contains("一只猫"));
        assert!(body.contains("gpt-image-1"));
        assert!(!body.contains("response_format"));
        assert!(!body.contains("UNIQUE_FOLDER_MARKER"));
        assert!(!body.contains("你可以读取和写入文件夹"));
        Ok(HttpResponse::text(200, serde_json::json!({"data":[{"b64_json": encoded}]}).to_string()))
    })
    .unwrap();
    assert_eq!(messages[1].content, "已生成图片");
    let picture = messages[1].file.clone();
    assert!(picture.ends_with(".png"));
    assert!(picture.starts_with(&format!("media/bots/{}/", painter.id)));
    chat::dispatch(&store, &painter.id, "第二张", |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("第二张"));
        assert!(!body.contains("一只猫"));
        assert!(!body.contains("已生成图片"));
        Ok(HttpResponse::text(200, serde_json::json!({"data":[{"b64_json": encoded}]}).to_string()))
    })
    .unwrap();
    chat::delete_chat(&store, &painter.id).unwrap();
    assert!(media::saved_path(&store, &picture).is_err());
    assert!(!tmp.path.join(format!("media/bots/{}", painter.id)).exists());

    let speaker = providers::upsert(&store, Provider {
        name: "读".into(),
        voice: "nova".into(),
        ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "OpenAI 语音").unwrap())
    })
    .unwrap();
    assert_eq!(speaker.voice, "nova");
    let reader = bots::upsert(&store, Bot {
        name: "朗读".into(),
        prompt: "不要朗读这段".into(),
        provider_id: speaker.id,
        ..Bot::draft()
    })
    .unwrap();
    let messages = chat::dispatch(&store, &reader.id, "请读我", |request| {
        let body = request.body.clone().unwrap();
        assert!(body.contains("请读我"));
        assert!(body.contains("nova"));
        assert!(!body.contains("不要朗读这段"));
        Ok(HttpResponse::binary(200, b"ID3chat".to_vec()))
    })
    .unwrap();
    assert_eq!(messages.last().unwrap().content, "已生成语音");
    assert!(messages.last().unwrap().file.ends_with(".mp3"));
    chat::delete_bot(&store, &reader.id).unwrap();
    assert!(!tmp.path.join(format!("media/bots/{}", reader.id)).exists());

    let writer = bots::upsert(&store, Bot {
        name: "作家".into(),
        provider_id: providers::upsert(&store, Provider {
            name: "本地".into(),
            model: "test-model".into(),
            ..Provider::from_preset(providers::presets().iter().find(|item| item.name == "Ollama").unwrap())
        })
        .unwrap()
        .id,
        ..Bot::draft()
    })
    .unwrap();
    let group = group::create(&store, "画室").unwrap();
    group::add_bot(&store, &group.id, &painter.id).unwrap();
    group::add_bot(&store, &group.id, &writer.id).unwrap();
    let messages = drain_group(&store, &group.id, "画一只猫", |bot_id, request| {
        if bot_id == painter.id {
            let body = request.body.clone().unwrap();
            assert!(request.url.contains("/images/generations"), "{}", request.url);
            assert!(body.contains("画一只猫"));
            assert!(body.contains("扁平插画"));
            assert!(!body.contains("你正在群组"));
            Ok(HttpResponse::text(200, serde_json::json!({"data":[{"b64_json": encoded, "revised_prompt": "群里的猫"}]}).to_string()))
        } else {
            let body = request.body.clone().unwrap();
            assert!(body.contains("群里的猫"), "{body}");
            assert!(request.url.contains("/chat/completions"));
            Ok(reply_for("ollama", "看到了"))
        }
    })
    .unwrap();
    let drawn = messages.iter().find(|message| message.speaker_id == painter.id).unwrap();
    assert_eq!(drawn.content, "群里的猫");
    assert!(drawn.file.starts_with(&format!("media/groups/{}/", group.id)));
    assert!(media::saved_path(&store, &drawn.file).unwrap().is_file());
    let group_id = group.id.clone();
    group::delete(&store, &group_id).unwrap();
    assert!(!tmp.path.join(format!("media/groups/{group_id}")).exists());
    assert!(media::saved_path(&store, &drawn.file).is_err());
}

#[test]
fn traverse_reply_usage() {
    assert_eq!(usage::label(0, 0), None);
    assert_eq!(usage::label(3, 0).as_deref(), Some("输入 3 · 输出 0"));
    assert_eq!(usage::label(0, 4).as_deref(), Some("输入 0 · 输出 4"));
    let samples = [
        ("openai", r#"{"usage":{"prompt_tokens":11,"completion_tokens":7,"total_tokens":18}}"#, 11, 7),
        ("ollama", r#"{"usage":{"prompt_tokens":2,"completion_tokens":3}}"#, 2, 3),
        ("azure", r#"{"usage":{"prompt_tokens":4,"completion_tokens":5}}"#, 4, 5),
        ("openai", r#"{"usage":{"input_tokens":8,"output_tokens":9}}"#, 8, 9),
        ("openai", r#"{"usage":{"prompt_tokens":1,"input_tokens":99,"completion_tokens":2,"output_tokens":99}}"#, 1, 2),
        ("anthropic", r#"{"usage":{"input_tokens":12,"output_tokens":6}}"#, 12, 6),
        ("gemini", r#"{"usageMetadata":{"promptTokenCount":15,"candidatesTokenCount":4,"totalTokenCount":19}}"#, 15, 4),
    ];
    for (kind, body, input, output) in samples {
        let spent = usage::from_chat(kind, body);
        assert_eq!((spent.input, spent.output), (input, output), "{kind}");
    }
    assert_eq!(usage::from_chat("openai", r#"{"usage":{"total_tokens":99}}"#), usage::Usage::default());
    assert_eq!(
        usage::from_chat("openai", r#"{"usage":{"prompt_tokens":-3,"completion_tokens":"x"}}"#),
        usage::Usage::default()
    );
    assert_eq!(
        usage::from_chat("nope", r#"{"usage":{"prompt_tokens":1,"completion_tokens":1}}"#),
        usage::Usage::default()
    );
    assert_eq!(usage::from_chat("gemini", "not-json"), usage::Usage::default());

    let old: chat::Message = serde_json::from_str(r#"{"role":"assistant","content":"旧"}"#).unwrap();
    assert_eq!((old.input_tokens, old.output_tokens), (0, 0));
    let old_group: GroupMessage = serde_json::from_str(r#"{"role":"assistant","content":"旧","speakerId":"abc"}"#).unwrap();
    assert_eq!((old_group.input_tokens, old_group.output_tokens), (0, 0));

    let tmp = Tmp::new();
    let store = tmp.store();
    let provider = providers::upsert(&store, Provider {
        name: "用量".into(),
        api_key: "secret".into(),
        model: "m".into(),
        ..Provider::draft("openai")
    })
    .unwrap();
    let bot = bots::upsert(&store, Bot {
        name: "用量 Bot".into(),
        prompt: "提示".into(),
        provider_id: provider.id.clone(),
        model: "m".into(),
        ..Bot::draft()
    })
    .unwrap();
    let messages = chat::dispatch(&store, &bot.id, "你好", |_| {
        Ok(HttpResponse::text(
            200,
            r#"{"choices":[{"message":{"content":"收到"}}],"usage":{"prompt_tokens":21,"completion_tokens":5}}"#,
        ))
    })
    .unwrap();
    assert_eq!(messages[0].input_tokens, 0);
    assert_eq!((messages[1].input_tokens, messages[1].output_tokens), (21, 5));
    assert_eq!(usage::label(21, 5).as_deref(), Some("输入 21 · 输出 5"));
    let raw = std::fs::read_to_string(tmp.path.join(format!("chats/{}.json", bot.id))).unwrap();
    assert!(raw.contains("\"input_tokens\": 21"), "{raw}");
    assert!(raw.contains("\"output_tokens\": 5"), "{raw}");
    let blank = chat::dispatch(&store, &bot.id, "再来", |_| Ok(reply_for("openai", "无用量"))).unwrap();
    assert_eq!((blank.last().unwrap().input_tokens, blank.last().unwrap().output_tokens), (0, 0));
    let raw = std::fs::read_to_string(tmp.path.join(format!("chats/{}.json", bot.id))).unwrap();
    assert_eq!(raw.matches("input_tokens").count(), 1, "{raw}");

    let root = tmp.path.join("box");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("note.txt"), "内容").unwrap();
    let mut calls = 0;
    let (text, spent) = providers::complete_turns(
        &named_model("anthropic", "m"),
        "",
        vec![Turn { role: "user".into(), content: "看一下".into() }],
        root.to_str().unwrap(),
        |_| {
            calls += 1;
            let body = if calls == 1 {
                r#"{"content":[{"type":"text","text":"读取 note.txt"}],"usage":{"input_tokens":10,"output_tokens":4}}"#
            } else {
                r#"{"content":[{"type":"text","text":"看到了"}],"usage":{"input_tokens":30,"output_tokens":2}}"#
            };
            Ok(HttpResponse::text(200, body))
        },
    )
    .unwrap();
    assert_eq!(calls, 2);
    assert_eq!(text, "看到了");
    assert_eq!((spent.input, spent.output), (40, 6));
    let (text, spent) = providers::complete_turns(
        &named_model("openai", "m"),
        "",
        vec![Turn { role: "user".into(), content: "嗨".into() }],
        "",
        |_| Ok(reply_for("openai", "无")),
    )
    .unwrap();
    assert_eq!(text, "无");
    assert_eq!(spent, usage::Usage::default());

    let group = group::create(&store, "用量群").unwrap();
    group::add_bot(&store, &group.id, &bot.id).unwrap();
    let messages = drain_group(&store, &group.id, "大家好", |_, _| {
        Ok(HttpResponse::text(
            200,
            r#"{"choices":[{"message":{"content":"在"}}],"usage":{"prompt_tokens":6,"completion_tokens":1}}"#,
        ))
    })
    .unwrap();
    let reply = messages.iter().find(|item| item.role == "assistant").unwrap();
    assert_eq!((reply.input_tokens, reply.output_tokens), (6, 1));
    let raw = std::fs::read_to_string(tmp.path.join(format!("group-chats/{}.json", group.id))).unwrap();
    assert!(raw.contains("\"inputTokens\": 6"), "{raw}");
    assert!(raw.contains("\"outputTokens\": 1"), "{raw}");
}

fn media_provider(kind: &str, task: &str, model: &str) -> Provider {
    Provider {
        name: "媒体".into(),
        api_key: "secret".into(),
        model: model.into(),
        task: task.into(),
        ..Provider::draft(kind)
    }
}

fn tiny_png() -> Vec<u8> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([9, 8, 7, 255])));
    let mut bytes = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

fn tiny_mp4() -> Vec<u8> {
    b"\0\0\0\x18ftypisom\0\0\0\0".to_vec()
}

fn image_bytes(loaded: Loaded) -> Vec<u8> {
    match loaded {
        Loaded::Bytes(piece) => piece.bytes,
        Loaded::Fetch { .. } => panic!("应该直接得到图片"),
    }
}

fn make_bot(store: &Store, name: &str, provider_id: &str) -> Bot {
    bots::upsert(store, Bot {
        name: name.into(),
        prompt: format!("{name}的提示"),
        provider_id: provider_id.into(),
        ..Bot::draft()
    })
    .unwrap()
}

fn drain_group(
    store: &Store,
    group_id: &str,
    content: &str,
    mut transport: impl FnMut(&str, &HttpRequest) -> Result<HttpResponse, String>,
) -> Result<Vec<GroupMessage>, String> {
    let mut queue = group::post_user(store, group_id, content)?;
    loop {
        match group::take_next(store, group_id, queue)? {
            Some((prepared, rest)) => {
                queue = rest;
                let bot_id = prepared.bot_id.clone();
                let parsed = group::run(prepared, |request| transport(&bot_id, request));
                match parsed {
                    Ok(outcome) => {
                        group::commit_with(
                            store,
                            group_id,
                            &bot_id,
                            &outcome.text,
                            &outcome.file,
                            outcome.input_tokens,
                            outcome.output_tokens,
                        )?;
                    }
                    Err(err) => {
                        group::commit_error(store, group_id, &bot_id, &err)?;
                    }
                }
            }
            None => break,
        }
    }
    group::load(store, group_id)
}

fn link_file(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
}

fn role_text(body: &str, role: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(body).unwrap();
    value["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["role"] == role)
        .map(|item| item["content"].as_str().unwrap().to_string())
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn named(name: &str, kind: &str, base: &str) -> Provider {
    Provider {
        name: name.into(),
        kind: kind.into(),
        base_url: base.into(),
        ..Provider::draft(kind)
    }
}

fn named_model(kind: &str, model: &str) -> Provider {
    Provider {
        model: model.into(),
        ..named("A", kind, providers::default_base(kind))
    }
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn reply_for(kind: &str, text: &str) -> HttpResponse {
    let body = match kind {
        "anthropic" => serde_json::json!({"content":[{"type":"text","text": text}]}),
        "gemini" => serde_json::json!({"candidates":[{"content":{"parts":[{"text": text}]}}]}),
        _ => serde_json::json!({"choices":[{"message":{"content": text}}]}),
    };
    HttpResponse::text(200, body.to_string())
}

fn serve_once(status: u16, body: &str) -> String {
    serve_bytes(status, body.as_bytes())
}

fn serve_bytes(status: u16, body: &[u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_vec();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let mut buf = Vec::new();
        let mut tmp = [0u8; 2048];
        let mut header_end = None;
        while header_end.is_none() {
            match stream.read(&mut tmp) {
                Ok(0) => return,
                Ok(size) => {
                    buf.extend_from_slice(&tmp[..size]);
                    header_end = buf.windows(4).position(|item| item == b"\r\n\r\n").map(|pos| pos + 4);
                }
                Err(_) => return,
            }
        }
        let header_end = header_end.unwrap();
        let header = String::from_utf8_lossy(&buf[..header_end]).to_ascii_lowercase();
        let mut length = 0usize;
        for line in header.lines() {
            if let Some(value) = line.strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        while buf.len() < header_end + length {
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(size) => buf.extend_from_slice(&tmp[..size]),
                Err(_) => break,
            }
        }
        let header = format!(
            "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    format!("http://{addr}")
}
