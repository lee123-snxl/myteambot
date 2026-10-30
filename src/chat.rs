use serde::{Deserialize, Serialize};

use crate::bots;
use crate::folder;
use crate::http_client::{HttpRequest, HttpResponse};
use crate::media::{self, Outcome};
use crate::providers::{self, Provider, Turn};
use crate::store::{self, Store};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(default, skip_serializing_if = "crate::usage::omit")]
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "crate::usage::omit")]
    pub output_tokens: u64,
}

pub struct Prepared {
    store: Store,
    dir: String,
    runtime: Provider,
    prompt: String,
    style: String,
    user_text: String,
    turns: Vec<Turn>,
    root: String,
}

pub fn load(store: &Store, bot_id: &str) -> Result<Vec<Message>, String> {
    store.load_vec(&chat_file(bot_id)?)
}

pub fn prepare(store: &Store, bot_id: &str, content: &str) -> Result<Prepared, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("请输入消息".into());
    }
    if content.chars().count() > 100_000 {
        return Err("消息太长".into());
    }
    let bot = bots::get(store, bot_id)?;
    if bot.provider_id.is_empty() {
        return Err("请先为这个 Bot 选择模型提供方".into());
    }
    let provider = providers::get(store, &bot.provider_id).map_err(|_| "请重新选择模型提供方".to_string())?;
    let mut runtime = provider;
    if !bot.model.trim().is_empty() {
        runtime.model = bot.model.trim().to_string();
    }
    if runtime.model.trim().is_empty() {
        return Err("请填写模型名".into());
    }
    let mut stored = load(store, bot_id)?;
    stored.push(Message {
        role: "user".into(),
        content: content.to_string(),
        file: String::new(),
        input_tokens: 0,
        output_tokens: 0,
    });
    let turns: Vec<Turn> = stored
        .iter()
        .filter(|message| message.role == "user" || message.role == "assistant")
        .map(|message| Turn {
            role: message.role.clone(),
            content: message.content.clone(),
        })
        .collect();
    save(store, bot_id, &stored)?;
    Ok(Prepared {
        store: store.clone(),
        dir: media::bot_dir(bot_id)?,
        runtime,
        style: bot.prompt.clone(),
        prompt: bot.prompt,
        user_text: content.to_string(),
        turns,
        root: bot.folder,
    })
}

pub fn run<F>(prepared: Prepared, transport: F) -> Result<Outcome, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let task = providers::task_of(&prepared.runtime.task)?;
    if task != "chat" {
        return media::generate(
            &prepared.store,
            &prepared.runtime,
            &prepared.style,
            &prepared.user_text,
            &prepared.dir,
            media::POLL_PAUSE,
            transport,
        );
    }
    let system = folder::compose(&prepared.prompt, &prepared.root);
    let (text, spent) = providers::complete_turns(&prepared.runtime, &system, prepared.turns, &prepared.root, transport)?;
    let mut outcome = Outcome::text(text);
    outcome.input_tokens = spent.input;
    outcome.output_tokens = spent.output;
    Ok(outcome)
}

pub fn commit_reply(store: &Store, bot_id: &str, reply: &str) -> Result<Vec<Message>, String> {
    commit_with(store, bot_id, reply, "", 0, 0)
}

pub fn commit_with(
    store: &Store,
    bot_id: &str,
    reply: &str,
    file: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<Vec<Message>, String> {
    let reply = reply.trim();
    let file = file.trim();
    if reply.is_empty() && file.is_empty() {
        return commit_error(store, bot_id, "模型没有返回文本");
    }
    let text = if reply.is_empty() { "已生成文件".to_string() } else { reply.to_string() };
    push(store, bot_id, "assistant", &text, file, input_tokens, output_tokens)
}

pub fn commit_error(store: &Store, bot_id: &str, text: &str) -> Result<Vec<Message>, String> {
    let text = text.trim();
    let text = if text.is_empty() { "请求失败" } else { text };
    push(store, bot_id, "error", text, "", 0, 0)
}

pub fn delete_chat(store: &Store, bot_id: &str) -> Result<(), String> {
    store.remove(&chat_file(bot_id)?)?;
    store.remove_tree(&media::bot_dir(bot_id)?)
}

pub fn delete_bot(store: &Store, id: &str) -> Result<(), String> {
    bots::delete(store, id)?;
    let forgotten = crate::group::forget_bot(store, id);
    let removed = store.remove(&chat_file(id)?);
    let media = store.remove_tree(&media::bot_dir(id)?);
    forgotten.and(removed).and(media)
}

pub fn dispatch<F>(store: &Store, bot_id: &str, content: &str, transport: F) -> Result<Vec<Message>, String>
where
    F: FnMut(&HttpRequest) -> Result<HttpResponse, String>,
{
    let prepared = prepare(store, bot_id, content)?;
    match run(prepared, transport) {
        Ok(outcome) => commit_with(store, bot_id, &outcome.text, &outcome.file, outcome.input_tokens, outcome.output_tokens),
        Err(err) => commit_error(store, bot_id, &err),
    }
}

fn push(
    store: &Store,
    bot_id: &str,
    role: &str,
    content: &str,
    file: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<Vec<Message>, String> {
    let _ = bots::get(store, bot_id)?;
    let mut messages = load(store, bot_id)?;
    messages.push(Message {
        role: role.to_string(),
        content: content.to_string(),
        file: file.to_string(),
        input_tokens,
        output_tokens,
    });
    save(store, bot_id, &messages)?;
    Ok(messages)
}

fn save(store: &Store, bot_id: &str, messages: &[Message]) -> Result<(), String> {
    store.save_vec(&chat_file(bot_id)?, messages)
}

fn chat_file(id: &str) -> Result<String, String> {
    if !store::is_safe_id(id) {
        return Err("非法 Bot 编号".into());
    }
    Ok(format!("chats/{id}.json"))
}
