use serde::{Deserialize, Serialize};

use crate::store::{self, Store};

const FILE: &str = "bots.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

impl Bot {
    pub fn draft() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            prompt: String::new(),
            provider_id: String::new(),
            model: String::new(),
            folder: String::new(),
        }
    }
}

pub fn list(store: &Store) -> Result<Vec<Bot>, String> {
    store.load_vec(FILE)
}

pub fn get(store: &Store, id: &str) -> Result<Bot, String> {
    list(store)?
        .into_iter()
        .find(|bot| bot.id == id)
        .ok_or_else(|| "Bot 不存在".to_string())
}

pub fn upsert(store: &Store, mut bot: Bot) -> Result<Bot, String> {
    bot = validate(bot)?;
    if bot.id.is_empty() {
        bot.id = store::new_id();
    }
    let mut all = list(store)?;
    if let Some(pos) = all.iter().position(|item| item.id == bot.id) {
        all[pos] = bot.clone();
    } else {
        all.push(bot.clone());
    }
    store.save_vec(FILE, &all)?;
    Ok(bot)
}

pub fn delete(store: &Store, id: &str) -> Result<(), String> {
    if !store::is_safe_id(id) {
        return Err("非法 Bot 编号".into());
    }
    let mut all = list(store)?;
    let before = all.len();
    all.retain(|bot| bot.id != id);
    if all.len() == before {
        return Err("Bot 不存在".into());
    }
    store.save_vec(FILE, &all)
}

pub fn validate(mut bot: Bot) -> Result<Bot, String> {
    bot.name = bot.name.trim().to_string();
    if bot.name.is_empty() {
        return Err("请填写 Bot 名字".into());
    }
    if bot.name.chars().count() > 80 {
        return Err("Bot 名字过长".into());
    }
    if bot.prompt.chars().count() > 100_000 {
        return Err("提示词过长".into());
    }
    bot.model = bot.model.trim().to_string();
    if bot.model.chars().count() > 200 {
        return Err("模型名过长".into());
    }
    bot.provider_id = bot.provider_id.trim().to_string();
    if !bot.provider_id.is_empty() && !store::is_safe_id(&bot.provider_id) {
        return Err("模型提供方无效".into());
    }
    if !bot.id.is_empty() && !store::is_safe_id(&bot.id) {
        return Err("非法 Bot 编号".into());
    }
    bot.folder = crate::folder::clean_root(&bot.folder)?;
    Ok(bot)
}
