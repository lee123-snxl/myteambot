use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bots::{self, Bot};
use crate::folder;
use crate::http_client::{HttpRequest, HttpResponse};
use crate::media::{self, Outcome};
use crate::providers::{self, Provider, Turn};
use crate::store::{self, Store};

const GROUPS: &str = "groups.json";
const MAX_BOTS: usize = 12;
const MAX_FILES: usize = 100;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PREVIEW_CHARS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub bot_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub speaker_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(default, skip_serializing_if = "crate::usage::omit")]
    pub input_tokens: u64,
    #[serde(default, skip_serializing_if = "crate::usage::omit")]
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupFile {
    pub id: String,
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Text { body: String, truncated: bool },
    Binary,
}

#[derive(Debug)]
pub struct Prepared {
    pub bot_id: String,
    store: Store,
    dir: String,
    runtime: Provider,
    prompt: String,
    style: String,
    user_text: String,
    turns: Vec<Turn>,
    root: String,
}

pub fn list(store: &Store) -> Result<Vec<Group>, String> {
    store.load_vec(GROUPS)
}

pub fn get(store: &Store, id: &str) -> Result<Group, String> {
    if !store::is_safe_id(id) {
        return Err("非法群组编号".into());
    }
    list(store)?
        .into_iter()
        .find(|group| group.id == id)
        .ok_or_else(|| "群组不存在".to_string())
}

pub fn unused_name(store: &Store) -> Result<String, String> {
    let all = list(store)?;
    let mut name = "新群组".to_string();
    let mut number = 2u32;
    while all.iter().any(|group| group.name == name) {
        if number > 1000 {
            return Err("无法命名新群组".into());
        }
        name = format!("新群组 {number}");
        number += 1;
    }
    Ok(name)
}

pub fn create(store: &Store, name: &str) -> Result<Group, String> {
    let group = Group {
        id: store::new_id(),
        name: clean_name(name)?,
        bot_ids: Vec::new(),
        folder: String::new(),
    };
    let mut all = list(store)?;
    all.push(group.clone());
    store.save_vec(GROUPS, &all)?;
    Ok(group)
}

pub fn rename(store: &Store, id: &str, name: &str) -> Result<Group, String> {
    let mut group = get(store, id)?;
    group.name = clean_name(name)?;
    write_group(store, &group)?;
    Ok(group)
}

pub fn save_settings(store: &Store, id: &str, name: &str, folder: &str) -> Result<Group, String> {
    let name = clean_name(name)?;
    let folder = folder::clean_root(folder)?;
    let mut group = get(store, id)?;
    group.name = name;
    group.folder = folder;
    write_group(store, &group)?;
    Ok(group)
}

pub fn delete(store: &Store, id: &str) -> Result<(), String> {
    if !store::is_safe_id(id) {
        return Err("非法群组编号".into());
    }
    let mut all = list(store)?;
    let before = all.len();
    all.retain(|group| group.id != id);
    if all.len() == before {
        return Err("群组不存在".into());
    }
    store.remove(&chat_path(id)?)?;
    store.remove_tree(&files_dir(id)?)?;
    store.remove_tree(&media::group_dir(id)?)?;
    store.save_vec(GROUPS, &all)
}

pub fn add_bot(store: &Store, group_id: &str, bot_id: &str) -> Result<Group, String> {
    if !store::is_safe_id(bot_id) {
        return Err("非法 Bot 编号".into());
    }
    let _ = bots::get(store, bot_id)?;
    let mut group = get(store, group_id)?;
    if group.bot_ids.iter().any(|id| id == bot_id) {
        return Err("这个 Bot 已经在群组里".into());
    }
    if group.bot_ids.len() >= MAX_BOTS {
        return Err("一个群组最多 12 个 Bot".into());
    }
    group.bot_ids.push(bot_id.to_string());
    write_group(store, &group)?;
    Ok(group)
}

pub fn remove_bot(store: &Store, group_id: &str, bot_id: &str) -> Result<Group, String> {
    let mut group = get(store, group_id)?;
    let before = group.bot_ids.len();
    group.bot_ids.retain(|id| id != bot_id);
    if group.bot_ids.len() == before {
        return Err("Bot 不在群组里".into());
    }
    write_group(store, &group)?;
    Ok(group)
}

pub fn forget_bot(store: &Store, bot_id: &str) -> Result<(), String> {
    if !store::is_safe_id(bot_id) {
        return Err("非法 Bot 编号".into());
    }
    let mut all = list(store)?;
    let mut changed = false;
    for group in &mut all {
        let before = group.bot_ids.len();
        group.bot_ids.retain(|id| id != bot_id);
        if group.bot_ids.len() != before {
            changed = true;
        }
    }
    if changed {
        store.save_vec(GROUPS, &all)?;
    }
    Ok(())
}

pub fn load(store: &Store, group_id: &str) -> Result<Vec<GroupMessage>, String> {
    let _ = get(store, group_id)?;
    store.load_vec(&chat_path(group_id)?)
}

pub fn post_user(store: &Store, group_id: &str, content: &str) -> Result<Vec<String>, String> {
    let content = content.trim();
    if content.is_empty() {
        return Err("请输入消息".into());
    }
    if content.chars().count() > 100_000 {
        return Err("消息太长".into());
    }
    let group = get(store, group_id)?;
    let mut messages = load(store, group_id)?;
    messages.push(GroupMessage {
        role: "user".into(),
        content: content.to_string(),
        speaker_id: String::new(),
        file: String::new(),
        input_tokens: 0,
        output_tokens: 0,
    });
    save_messages(store, group_id, &messages)?;
    let bots = bots::list(store)?;
    Ok(group
        .bot_ids
        .into_iter()
        .filter(|id| bots.iter().any(|bot| &bot.id == id))
        .collect())
}

pub fn take_next(store: &Store, group_id: &str, mut queue: Vec<String>) -> Result<Option<(Prepared, Vec<String>)>, String> {
    while !queue.is_empty() {
        let Some(group) = present(get(store, group_id))? else {
            return Ok(None);
        };
        let bot_id = queue.remove(0);
        if !group.bot_ids.iter().any(|id| id == &bot_id) || bots::get(store, &bot_id).is_err() {
            continue;
        }
        match prepare(store, group_id, &bot_id) {
            Ok(prepared) => return Ok(Some((prepared, queue))),
            Err(err) => {
                commit_error(store, group_id, &bot_id, &err)?;
            }
        }
    }
    Ok(None)
}

pub fn commit_reply(store: &Store, group_id: &str, bot_id: &str, reply: &str) -> Result<Vec<GroupMessage>, String> {
    commit_with(store, group_id, bot_id, reply, "", 0, 0)
}

pub fn commit_with(
    store: &Store,
    group_id: &str,
    bot_id: &str,
    reply: &str,
    file: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<Vec<GroupMessage>, String> {
    let reply = reply.trim();
    let file = file.trim();
    if reply.is_empty() && file.is_empty() {
        return commit_error(store, group_id, bot_id, "模型没有返回文本");
    }
    if present(get(store, group_id))?.is_none() {
        return Ok(Vec::new());
    }
    let text = if reply.is_empty() { "已生成文件".to_string() } else { reply.to_string() };
    push(store, group_id, "assistant", &text, bot_id, file, input_tokens, output_tokens)
}

pub fn commit_error(store: &Store, group_id: &str, bot_id: &str, text: &str) -> Result<Vec<GroupMessage>, String> {
    if present(get(store, group_id))?.is_none() {
        return Ok(Vec::new());
    }
    let text = text.trim();
    let text = if text.is_empty() { "请求失败" } else { text };
    push(store, group_id, "error", text, bot_id, "", 0, 0)
}

pub fn list_files(store: &Store, group_id: &str) -> Result<Vec<GroupFile>, String> {
    let _ = get(store, group_id)?;
    store.load_vec(&index_path(group_id)?)
}

pub fn import_path(store: &Store, group_id: &str, source: &Path) -> Result<GroupFile, String> {
    let _ = get(store, group_id)?;
    if !source.is_file() {
        return Err("请选择一个文件".into());
    }
    let size = fs::metadata(source)
        .map_err(|err| format!("读取文件失败：{err}"))?
        .len();
    if size > MAX_FILE_BYTES {
        return Err("文件不能超过 8MB".into());
    }
    let bytes = fs::read(source).map_err(|err| format!("读取文件失败：{err}"))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("文件不能超过 8MB".into());
    }
    let mut files = list_files(store, group_id)?;
    if files.len() >= MAX_FILES {
        return Err("这个群组最多存放 100 个文件".into());
    }
    let file = GroupFile {
        id: store::new_id(),
        name: file_label(source),
        size: bytes.len() as u64,
    };
    let blob = blob_path(group_id, &file.id)?;
    store.write_file(&blob, &bytes)?;
    files.push(file.clone());
    if let Err(err) = store.save_vec(&index_path(group_id)?, &files) {
        let _ = store.remove(&blob);
        return Err(err);
    }
    Ok(file)
}

pub fn preview(store: &Store, group_id: &str, file_id: &str) -> Result<Preview, String> {
    let files = list_files(store, group_id)?;
    if !files.iter().any(|file| file.id == file_id) {
        return Err("文件不存在".into());
    }
    let bytes = store
        .read_file(&blob_path(group_id, file_id)?)
        .map_err(|err| format!("读取文件失败：{err}"))?;
    if bytes.contains(&0) {
        return Ok(Preview::Binary);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Preview::Binary);
    };
    let count = text.chars().count();
    if count > MAX_PREVIEW_CHARS {
        let body = text.chars().take(MAX_PREVIEW_CHARS).collect();
        return Ok(Preview::Text { body, truncated: true });
    }
    Ok(Preview::Text { body: text, truncated: false })
}

pub fn remove_file(store: &Store, group_id: &str, file_id: &str) -> Result<(), String> {
    let _ = get(store, group_id)?;
    if !store::is_safe_id(file_id) {
        return Err("非法文件编号".into());
    }
    let mut files = list_files(store, group_id)?;
    let before = files.len();
    files.retain(|file| file.id != file_id);
    if files.len() == before {
        return Err("文件不存在".into());
    }
    store.save_vec(&index_path(group_id)?, &files)?;
    store.remove(&blob_path(group_id, file_id)?)
}

fn prepare(store: &Store, group_id: &str, bot_id: &str) -> Result<Prepared, String> {
    let group = get(store, group_id)?;
    if !group.bot_ids.iter().any(|id| id == bot_id) {
        return Err("Bot 不在群组里".into());
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
    let bots = bots::list(store)?;
    let members = live_members(&bots, &group);
    let messages = load(store, group_id)?;
    let user_text = messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| message.content.trim().to_string())
        .unwrap_or_default();
    let prompt = group_system(&bot, &group, &members);
    let turns = perspective(&bot, &bots, &messages);
    Ok(Prepared {
        bot_id: bot.id.clone(),
        store: store.clone(),
        dir: media::group_dir(group_id)?,
        runtime,
        prompt,
        style: bot.prompt,
        user_text,
        turns,
        root: group.folder,
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

fn live_members(bots: &[Bot], group: &Group) -> Vec<Bot> {
    group
        .bot_ids
        .iter()
        .filter_map(|id| bots.iter().find(|bot| &bot.id == id).cloned())
        .collect()
}

fn group_system(bot: &Bot, group: &Group, members: &[Bot]) -> String {
    let others = members
        .iter()
        .filter(|item| item.id != bot.id)
        .map(|item| item.name.as_str())
        .collect::<Vec<_>>();
    let roster = if others.is_empty() {
        "群里没有其他 Bot。".to_string()
    } else {
        format!("其他 Bot：{}。", others.join("、"))
    };
    let note = format!(
        "你正在群组「{}」里，你的名字是「{}」。{}发言里，用户以「用户：」开头，其他 Bot 以自己的名字开头。只以你自己的身份回复，不要代替别人说话。",
        group.name, bot.name, roster
    );
    if bot.prompt.trim().is_empty() {
        note
    } else {
        format!("{}\n\n{note}", bot.prompt.trim())
    }
}

fn perspective(bot: &Bot, bots: &[Bot], messages: &[GroupMessage]) -> Vec<Turn> {
    let mut turns = Vec::new();
    for message in messages {
        if message.role == "error" {
            continue;
        }
        let body = message.content.trim();
        if body.is_empty() {
            continue;
        }
        if message.role == "assistant" && message.speaker_id == bot.id {
            push_line(&mut turns, "assistant", body);
            continue;
        }
        if message.role != "user" && message.role != "assistant" {
            continue;
        }
        let speaker = speaker_name(bots, &message.speaker_id, &message.role);
        push_line(&mut turns, "user", &format!("{speaker}：{body}"));
    }
    turns
}

fn speaker_name(bots: &[Bot], speaker_id: &str, role: &str) -> String {
    if role == "user" || speaker_id.is_empty() {
        return "用户".to_string();
    }
    bots.iter()
        .find(|bot| bot.id == speaker_id)
        .map(|bot| bot.name.clone())
        .unwrap_or_else(|| "成员".to_string())
}

fn push_line(turns: &mut Vec<Turn>, role: &str, line: &str) {
    if let Some(last) = turns.last_mut() {
        if last.role == role {
            last.content.push('\n');
            last.content.push_str(line);
            return;
        }
    }
    turns.push(Turn {
        role: role.to_string(),
        content: line.to_string(),
    });
}

fn push(
    store: &Store,
    group_id: &str,
    role: &str,
    content: &str,
    speaker_id: &str,
    file: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Result<Vec<GroupMessage>, String> {
    let mut messages = load(store, group_id)?;
    messages.push(GroupMessage {
        role: role.to_string(),
        content: content.to_string(),
        speaker_id: speaker_id.to_string(),
        file: file.to_string(),
        input_tokens,
        output_tokens,
    });
    save_messages(store, group_id, &messages)?;
    Ok(messages)
}

fn save_messages(store: &Store, group_id: &str, messages: &[GroupMessage]) -> Result<(), String> {
    store.save_vec(&chat_path(group_id)?, messages)
}

fn write_group(store: &Store, group: &Group) -> Result<(), String> {
    let mut all = list(store)?;
    let Some(pos) = all.iter().position(|item| item.id == group.id) else {
        return Err("群组不存在".into());
    };
    all[pos] = group.clone();
    store.save_vec(GROUPS, &all)
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("请填写群组名字".into());
    }
    if name.chars().count() > 80 {
        return Err("群组名字过长".into());
    }
    Ok(name)
}

fn present(result: Result<Group, String>) -> Result<Option<Group>, String> {
    match result {
        Ok(group) => Ok(Some(group)),
        Err(err) if err == "群组不存在" || err == "非法群组编号" => Ok(None),
        Err(err) => Err(err),
    }
}

fn file_label(path: &Path) -> String {
    let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
        return "未命名".into();
    };
    let name: String = name.trim().chars().filter(|item| !item.is_control()).collect();
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." {
        return "未命名".into();
    }
    name.chars().take(80).collect()
}

fn chat_path(id: &str) -> Result<String, String> {
    if !store::is_safe_id(id) {
        return Err("非法群组编号".into());
    }
    Ok(format!("group-chats/{id}.json"))
}

fn files_dir(id: &str) -> Result<String, String> {
    if !store::is_safe_id(id) {
        return Err("非法群组编号".into());
    }
    Ok(format!("group-files/{id}"))
}

fn index_path(id: &str) -> Result<String, String> {
    Ok(format!("{}/index.json", files_dir(id)?))
}

fn blob_path(group_id: &str, file_id: &str) -> Result<String, String> {
    if !store::is_safe_id(file_id) {
        return Err("非法文件编号".into());
    }
    Ok(format!("{}/{}", files_dir(group_id)?, file_id))
}
