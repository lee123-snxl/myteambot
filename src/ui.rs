use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use crate::bots::{self, Bot};
use crate::chat::{self, Message};
use crate::folder::{self, Entry};
use crate::group::{self, Group, GroupFile, GroupMessage};
use crate::http_client;
use crate::media::{self, Outcome};
use crate::providers::{self, Provider, TestReport};
use crate::store::Store;
use crate::usage;

const USER_BUBBLE: egui::Color32 = egui::Color32::from_rgb(37, 99, 235);
const TEXT: egui::Color32 = egui::Color32::from_rgb(24, 24, 27);
const MUTED: egui::Color32 = egui::Color32::from_rgb(113, 113, 122);
const LINE: egui::Color32 = egui::Color32::from_rgb(228, 228, 231);
const DANGER: egui::Color32 = egui::Color32::from_rgb(153, 27, 27);
const OK: egui::Color32 = egui::Color32::from_rgb(22, 101, 52);
const WARN: egui::Color32 = egui::Color32::from_rgb(161, 98, 7);

pub fn launch() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 740.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title("本机 Bot"),
        ..Default::default()
    };
    eframe::run_native(
        "本机 Bot",
        options,
        Box::new(|cc| {
            install_style(&cc.egui_ctx);
            let font_error = install_cjk(&cc.egui_ctx).err();
            let mut app = App::new();
            if let Some(err) = font_error {
                app.set_err(format!("中文字体加载失败：{err}"));
            }
            Ok(Box::new(app))
        }),
    )
}

fn install_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::light();
    style.visuals.panel_fill = egui::Color32::WHITE;
    style.visuals.window_fill = egui::Color32::from_rgb(250, 250, 250);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);
}

fn install_cjk(ctx: &egui::Context) -> Result<(), String> {
    let bytes = crate::font::load_system_cjk()?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_owned(bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());
    ctx.set_fonts(fonts);
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    Home,
    Provider,
    Bot,
    Chat,
    Group,
}

enum Nav {
    NewProvider,
    NewBot,
    NewGroup,
    Provider(String),
    Bot(String),
    Chat(String),
    Group(String),
}

#[derive(Clone)]
enum Confirm {
    Provider(String),
    Bot(String),
    Group(String),
    Chat(String),
}

#[derive(Clone, Copy)]
enum ProbeKind {
    Test,
    Models,
}

enum Job {
    Chat { bot_id: String, rx: Receiver<Result<Outcome, String>> },
    Group {
        group_id: String,
        bot_id: String,
        rest: Vec<String>,
        rx: Receiver<Result<Outcome, String>>,
    },
    Probe {
        kind: ProbeKind,
        provider_id: Option<String>,
        rx: Receiver<Result<TestReport, String>>,
    },
}

struct App {
    store: Store,
    providers: Vec<Provider>,
    bots: Vec<Bot>,
    panel: Panel,
    provider_form: Provider,
    provider_base: Provider,
    bot_form: Bot,
    bot_base: Bot,
    chat_id: String,
    messages: Vec<Message>,
    groups: Vec<Group>,
    group_id: String,
    group_name: String,
    group_name_base: String,
    group_folder: String,
    group_folder_base: String,
    group_messages: Vec<GroupMessage>,
    group_files: Vec<GroupFile>,
    browse_dir: String,
    browse_entries: Vec<Entry>,
    browse_truncated: bool,
    browse_file: String,
    browse_text: String,
    browse_name: String,
    browse_body: String,
    browse_error: String,
    join_bot: String,
    preview_id: String,
    preview_text: String,
    input: String,
    notice: String,
    notice_ok: bool,
    models: Vec<String>,
    textures: HashMap<String, egui::TextureHandle>,
    job: Option<Job>,
    confirm: Option<Confirm>,
    focus_input: bool,
    data_dir: String,
}

impl App {
    fn new() -> Self {
        let data_dir = crate::store::default_data_dir();
        let (store, mut notice) = match Store::open(&data_dir) {
            Ok(store) => (store, String::new()),
            Err(err) => {
                let fallback = std::env::temp_dir().join("LocalBot");
                let store = Store::open(&fallback).expect("无法创建数据目录");
                (store, err)
            }
        };
        let providers = match providers::list(&store) {
            Ok(items) => items,
            Err(err) => {
                notice = err;
                Vec::new()
            }
        };
        let bots = match bots::list(&store) {
            Ok(items) => items,
            Err(err) => {
                if notice.is_empty() {
                    notice = err;
                }
                Vec::new()
            }
        };
        let groups = match group::list(&store) {
            Ok(items) => items,
            Err(err) => {
                if notice.is_empty() {
                    notice = err;
                }
                Vec::new()
            }
        };
        let provider_form = Provider::draft("openai");
        let bot_form = Bot::draft();
        Self {
            data_dir: store.root().display().to_string(),
            store,
            providers,
            bots,
            panel: Panel::Home,
            provider_base: provider_form.clone(),
            provider_form,
            bot_base: bot_form.clone(),
            bot_form,
            chat_id: String::new(),
            messages: Vec::new(),
            groups,
            group_id: String::new(),
            group_name: String::new(),
            group_name_base: String::new(),
            group_folder: String::new(),
            group_folder_base: String::new(),
            group_messages: Vec::new(),
            group_files: Vec::new(),
            browse_dir: String::new(),
            browse_entries: Vec::new(),
            browse_truncated: false,
            browse_file: String::new(),
            browse_text: String::new(),
            browse_name: String::new(),
            browse_body: String::new(),
            browse_error: String::new(),
            join_bot: String::new(),
            preview_id: String::new(),
            preview_text: String::new(),
            input: String::new(),
            notice,
            notice_ok: false,
            models: Vec::new(),
            textures: HashMap::new(),
            job: None,
            confirm: None,
            focus_input: false,
        }
    }

    fn update_frame(&mut self, ctx: &egui::Context) {
        self.poll(ctx);
        self.sidebar(ctx);
        if self.panel == Panel::Group && !self.group_id.is_empty() {
            self.files_panel(ctx);
        } else if self.panel == Panel::Chat && self.chat_folder().is_some() {
            self.chat_folder_panel(ctx);
        }
        if (self.panel == Panel::Chat && !self.chat_id.is_empty()) || (self.panel == Panel::Group && !self.group_id.is_empty()) {
            self.composer(ctx);
        }
        let frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(250, 250, 250))
            .inner_margin(16.0);
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            self.main_view(ui);
        });
        self.confirm_dialog(ctx);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.take() else {
            return;
        };
        match job {
            Job::Chat { bot_id, rx } => match rx.try_recv() {
                Ok(result) => self.finish_chat(bot_id, result),
                Err(TryRecvError::Empty) => {
                    self.job = Some(Job::Chat { bot_id, rx });
                    ctx.request_repaint_after(Duration::from_millis(80));
                }
                Err(TryRecvError::Disconnected) => self.finish_chat(bot_id, Err("请求中断".into())),
            },
            Job::Group { group_id, bot_id, rest, rx } => match rx.try_recv() {
                Ok(result) => self.finish_group(group_id, bot_id, rest, result),
                Err(TryRecvError::Empty) => {
                    self.job = Some(Job::Group { group_id, bot_id, rest, rx });
                    ctx.request_repaint_after(Duration::from_millis(80));
                }
                Err(TryRecvError::Disconnected) => self.finish_group(group_id, bot_id, rest, Err("请求中断".into())),
            },
            Job::Probe { kind, provider_id, rx } => match rx.try_recv() {
                Ok(result) => self.finish_probe(provider_id, result),
                Err(TryRecvError::Empty) => {
                    self.job = Some(Job::Probe { kind, provider_id, rx });
                    ctx.request_repaint_after(Duration::from_millis(80));
                }
                Err(TryRecvError::Disconnected) => self.set_err("请求中断"),
            },
        }
    }

    fn finish_chat(&mut self, bot_id: String, result: Result<Outcome, String>) {
        let saved = match &result {
            Ok(outcome) => chat::commit_with(
                &self.store,
                &bot_id,
                &outcome.text,
                &outcome.file,
                outcome.input_tokens,
                outcome.output_tokens,
            ),
            Err(err) => chat::commit_error(&self.store, &bot_id, err),
        };
        match saved {
            Ok(messages) => {
                if self.panel == Panel::Chat && self.chat_id == bot_id {
                    self.messages = messages;
                    if let Some(root) = self.chat_folder() {
                        self.reload_browse(&root);
                    }
                }
                match result {
                    Ok(_) => self.clear_notice(),
                    Err(err) => self.set_err(err),
                }
            }
            Err(err) => self.set_err(err),
        }
    }

    fn finish_group(&mut self, group_id: String, bot_id: String, rest: Vec<String>, result: Result<Outcome, String>) {
        let saved = match &result {
            Ok(outcome) => group::commit_with(
                &self.store,
                &group_id,
                &bot_id,
                &outcome.text,
                &outcome.file,
                outcome.input_tokens,
                outcome.output_tokens,
            ),
            Err(err) => group::commit_error(&self.store, &group_id, &bot_id, err),
        };
        if let Err(err) = saved {
            self.set_err(err);
            return;
        }
        self.reload_group_messages(&group_id);
        let folder = self.group_folder_base.clone();
        if self.panel == Panel::Group && self.group_id == group_id && !folder.is_empty() {
            self.reload_browse(&folder);
        }
        match result {
            Ok(_) => self.clear_notice(),
            Err(err) => self.set_err(err),
        }
        self.continue_group(group_id, rest);
    }

    fn continue_group(&mut self, group_id: String, queue: Vec<String>) {
        match group::take_next(&self.store, &group_id, queue) {
            Ok(Some((prepared, rest))) => {
                self.reload_group_messages(&group_id);
                let bot_id = prepared.bot_id.clone();
                let (tx, rx) = mpsc::channel();
                thread::spawn(move || {
                    let result = group::run(prepared, http_client::send);
                    let _ = tx.send(result);
                });
                self.job = Some(Job::Group { group_id, bot_id, rest, rx });
            }
            Ok(None) => self.reload_group_messages(&group_id),
            Err(err) => self.set_err(err),
        }
    }

    fn send_group(&mut self) {
        if self.job.is_some() {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() || self.group_id.is_empty() {
            return;
        }
        let group_id = self.group_id.clone();
        let queue = match group::post_user(&self.store, &group_id, &text) {
            Ok(queue) => queue,
            Err(err) => {
                self.set_err(err);
                return;
            }
        };
        self.input.clear();
        self.reload_group_messages(&group_id);
        self.clear_notice();
        self.continue_group(group_id, queue);
    }

    fn reload_group_messages(&mut self, group_id: &str) {
        if self.panel == Panel::Group && self.group_id == group_id {
            if let Ok(messages) = group::load(&self.store, group_id) {
                self.group_messages = messages;
            }
        }
    }

    fn finish_probe(&mut self, provider_id: Option<String>, result: Result<TestReport, String>) {
        match result {
            Ok(report) => {
                let detail = report.detail;
                if report.models.is_empty() {
                    self.set_ok(detail);
                    return;
                }
                match self.remember_models(provider_id, report.models) {
                    Ok(()) => self.set_ok(detail),
                    Err(err) => self.set_err(err),
                }
            }
            Err(err) => self.set_err(err),
        }
    }

    fn remember_models(&mut self, provider_id: Option<String>, models: Vec<String>) -> Result<(), String> {
        let models = providers::normalize_models(models);
        self.models = models.clone();
        let Some(id) = provider_id.filter(|id| !id.is_empty()) else {
            self.provider_form.models = models;
            return Ok(());
        };
        let saved = providers::save_models(&self.store, &id, models)?;
        if let Some(slot) = self.providers.iter_mut().find(|item| item.id == saved.id) {
            *slot = saved.clone();
        }
        if self.provider_form.id == saved.id {
            self.provider_form.models = saved.models.clone();
            self.provider_base.models = saved.models;
        }
        Ok(())
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let providers = self
            .providers
            .iter()
            .map(|item| (item.id.clone(), item.name.clone()))
            .collect::<Vec<_>>();
        let bots = self
            .bots
            .iter()
            .map(|item| (item.id.clone(), item.name.clone()))
            .collect::<Vec<_>>();
        let current_provider = if self.panel == Panel::Provider {
            self.provider_form.id.clone()
        } else {
            String::new()
        };
        let current_bot = if self.panel == Panel::Bot {
            self.bot_form.id.clone()
        } else {
            String::new()
        };
        let current_chat = if self.panel == Panel::Chat {
            self.chat_id.clone()
        } else {
            String::new()
        };
        let groups = self
            .groups
            .iter()
            .map(|item| (item.id.clone(), item.name.clone()))
            .collect::<Vec<_>>();
        let current_group = if self.panel == Panel::Group {
            self.group_id.clone()
        } else {
            String::new()
        };
        let data_dir = self.data_dir.clone();
        let frame = egui::Frame::NONE.fill(egui::Color32::WHITE).inner_margin(12.0);
        egui::SidePanel::left("sidebar")
            .exact_width(272.0)
            .resizable(false)
            .frame(frame)
            .show(ctx, |ui| {
                ui.heading("本机 Bot");
                ui.label(egui::RichText::new("即开即用，只保存在这台电脑上").size(12.0).color(MUTED));
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(egui::RichText::new(data_dir).size(11.0).color(MUTED));
                    ui.label(egui::RichText::new("数据目录").size(11.0).color(MUTED));
                    ui.separator();
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            section_title(ui, "模型提供方", "添加", || Nav::NewProvider, self);
                            for (id, name) in providers {
                                if ui.selectable_label(current_provider == id, name).clicked() {
                                    self.navigate(Nav::Provider(id));
                                }
                            }
                            ui.add_space(10.0);
                            section_title(ui, "Bot", "新建", || Nav::NewBot, self);
                            for (id, name) in &bots {
                                if ui.selectable_label(current_bot == *id, name).clicked() {
                                    self.navigate(Nav::Bot(id.clone()));
                                }
                            }
                            ui.add_space(10.0);
                            ui.strong("聊天");
                            if bots.is_empty() {
                                ui.label(egui::RichText::new("保存一个 Bot 后可以在这里聊天").size(12.0).color(MUTED));
                            }
                            for (id, name) in bots {
                                if ui.selectable_label(current_chat == id, name).clicked() {
                                    self.navigate(Nav::Chat(id));
                                }
                            }
                            ui.add_space(10.0);
                            section_title(ui, "群组", "新建", || Nav::NewGroup, self);
                            if groups.is_empty() {
                                ui.label(egui::RichText::new("点「新建」开始一个群组").size(12.0).color(MUTED));
                            }
                            for (id, name) in groups {
                                if ui.selectable_label(current_group == id, name).clicked() {
                                    self.navigate(Nav::Group(id));
                                }
                            }
                        });
                    });
                });
            });
    }

    fn main_view(&mut self, ui: &mut egui::Ui) {
        self.show_notice(ui);
        if self.dirty() {
            ui.label(egui::RichText::new("有未保存的修改").color(WARN));
        }
        match self.panel {
            Panel::Home => self.show_home(ui),
            Panel::Provider => self.show_provider(ui),
            Panel::Bot => self.show_bot(ui),
            Panel::Chat => self.show_chat(ui),
            Panel::Group => self.show_group(ui),
        }
    }

    fn show_home(&self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("开始");
        ui.add_space(6.0);
        ui.label("1. 在左侧添加模型提供方。云端接口填写密钥，本地模型选 Ollama 或兼容地址。用途可以是对话、生图、语音或视频。");
        ui.label("2. 新建 Bot，写下名字和提示词。");
        ui.label("3. 在左侧「聊天」里选择一个 Bot，开始对话。");
        ui.label("4. 在左侧「群组」里新建群组，加入 Bot，然后在群里说话。右侧可以放入文件并浏览。");
    }

    fn show_provider(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("provider-form")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.provider_form_body(ui);
            });
    }

    fn provider_form_body(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.provider_form.id.is_empty() { "添加模型提供方" } else { "编辑模型提供方" });
        field(ui, "名称", &mut self.provider_form.name, false, "例如：我的 DeepSeek");
        let previous = self.provider_form.kind.clone();
        let mut kind = previous.clone();
        ui.label("接口类型");
        egui::ComboBox::from_id_salt("provider-kind")
            .selected_text(providers::kind_label(&kind))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for item in providers::KINDS {
                    ui.selectable_value(&mut kind, (*item).to_string(), providers::kind_label(item));
                }
            });
        if kind != previous {
            providers::change_kind(&mut self.provider_form, &kind);
            self.models = self.provider_form.models.clone();
        }
        let mut picked_task = if self.provider_form.task.is_empty() {
            "chat".to_string()
        } else {
            self.provider_form.task.clone()
        };
        let task_before = picked_task.clone();
        ui.label("用途");
        egui::ComboBox::from_id_salt("provider-task")
            .selected_text(providers::task_label(&picked_task))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for item in providers::TASKS {
                    ui.selectable_value(&mut picked_task, (*item).to_string(), providers::task_label(item));
                }
            });
        if picked_task != task_before {
            self.provider_form.task = if picked_task == "chat" { String::new() } else { picked_task };
        }
        if !providers::supports_task(&self.provider_form.kind, if self.provider_form.task.is_empty() { "chat" } else { &self.provider_form.task }) {
            ui.label(egui::RichText::new("这个接口只能对话").size(12.0).color(WARN));
        }
        self.preset_buttons(ui);
        field(ui, "接口地址", &mut self.provider_form.base_url, false, "https://");
        if self.provider_form.base_url.contains("example.openai.azure.com") {
            ui.label(egui::RichText::new("把地址改成你的资源，例如 https://my-name.openai.azure.com").size(12.0).color(MUTED));
        }
        field(ui, "API 密钥", &mut self.provider_form.api_key, true, "本地服务通常不用填");
        let model_label = if self.provider_form.kind == "azure" { "部署名" } else { "默认模型" };
        field(ui, model_label, &mut self.provider_form.model, false, "可稍后从模型列表选择");
        let task = providers::task_of(&self.provider_form.task).unwrap_or("chat");
        let model_hint = match task {
            "image" => "默认模型填生图模型的名字",
            "speech" => "默认模型填语音模型的名字",
            "video" => "默认模型填视频模型的名字",
            _ => "",
        };
        if !model_hint.is_empty() {
            ui.label(egui::RichText::new(model_hint).size(12.0).color(MUTED));
        }
        if task == "speech" {
            field(ui, "语音角色", &mut self.provider_form.voice, false, "留空时 Gemini 用 Kore，其他用 alloy");
        }
        if self.provider_form.kind == "azure" {
            field(ui, "API 版本", &mut self.provider_form.api_version, false, "2024-10-21");
        }
        self.form_buttons(ui, true);
        self.model_chips(ui);
    }

    fn show_bot(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.bot_form.id.is_empty() { "新建 Bot" } else { "Bot" });
        field(ui, "名字", &mut self.bot_form.name, false, "例如：日程助手");
        let options = self
            .providers
            .iter()
            .map(|item| (item.id.clone(), item.name.clone(), item.model.clone()))
            .collect::<Vec<_>>();
        let current = options
            .iter()
            .find(|item| item.0 == self.bot_form.provider_id)
            .map(|item| item.1.clone())
            .unwrap_or_else(|| {
                if self.bot_form.provider_id.is_empty() {
                    "未选择".to_string()
                } else {
                    "提供方已删除".to_string()
                }
            });
        ui.label("模型提供方");
        let mut selected = self.bot_form.provider_id.clone();
        egui::ComboBox::from_id_salt("bot-provider")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, String::new(), "未选择");
                for (id, name, _) in &options {
                    ui.selectable_value(&mut selected, id.clone(), name);
                }
            });
        self.bot_form.provider_id = selected;
        let usage = self.providers.iter().find(|item| item.id == self.bot_form.provider_id).map(|provider| {
            let task = providers::task_label(&provider.task).to_string();
            let voice = if providers::task_of(&provider.task).ok() == Some("speech") {
                Some(if provider.voice.trim().is_empty() {
                    "默认".to_string()
                } else {
                    provider.voice.clone()
                })
            } else {
                None
            };
            (task, voice)
        });
        if let Some((task, voice)) = usage {
            ui.label(egui::RichText::new(format!("用途：{task}")).size(12.0).color(MUTED));
            if let Some(voice) = voice {
                ui.label(egui::RichText::new(format!("语音角色：{voice}")).size(12.0).color(MUTED));
            }
        }
        let hint = options
            .iter()
            .find(|item| item.0 == self.bot_form.provider_id)
            .and_then(|item| {
                if item.2.is_empty() {
                    None
                } else {
                    Some(format!("使用提供方默认（{}）", item.2))
                }
            })
            .unwrap_or_else(|| "使用提供方默认".to_string());
        let known = self
            .providers
            .iter()
            .find(|item| item.id == self.bot_form.provider_id)
            .map(|item| item.models.clone())
            .unwrap_or_default();
        let choices = providers::model_choices(&known, &self.bot_form.model);
        if choices.is_empty() {
            field(ui, "模型", &mut self.bot_form.model, false, "留空则使用提供方的默认模型");
            if !self.bot_form.provider_id.is_empty() {
                ui.label(egui::RichText::new("先在这个提供方里点「读取模型」，这里就能选择").size(12.0).color(MUTED));
            }
        } else {
            ui.label("模型");
            let mut picked = self.bot_form.model.clone();
            let shown = if picked.is_empty() { hint.clone() } else { picked.clone() };
            egui::ComboBox::from_id_salt("bot-model")
                .selected_text(shown)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut picked, String::new(), hint);
                    for name in &choices {
                        ui.selectable_value(&mut picked, name.clone(), name);
                    }
                });
            self.bot_form.model = picked;
        }
        ui.label("系统提示词");
        ui.add(
            egui::TextEdit::multiline(&mut self.bot_form.prompt)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("这个 Bot 的职责、语气和限制"),
        );
        folder_field(ui, &mut self.bot_form.folder, "选择 Bot 的文件夹");
        ui.label(egui::RichText::new("保存后，单聊时这个 Bot 会读取这个文件夹，也可以把内容写进去。读到的文本会发给模型").size(12.0).color(MUTED));
        self.form_buttons(ui, false);
        ui.add_space(8.0);
        if self.bot_form.id.is_empty() {
            ui.label("保存后，到左侧「聊天」里选择这个 Bot。");
            return;
        }
        let chat_id = self.bot_form.id.clone();
        if ui.button("和这个 Bot 聊天").clicked() {
            self.navigate(Nav::Chat(chat_id));
        }
    }

    fn show_chat(&mut self, ui: &mut egui::Ui) {
        let name = self
            .bots
            .iter()
            .find(|item| item.id == self.chat_id)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| "Bot".to_string());
        let mut remove = false;
        ui.horizontal(|ui| {
            ui.heading(format!("与 {name} 对话"));
            if ui.button("删除对话").clicked() {
                remove = true;
            }
        });
        ui.label(egui::RichText::new("Bot 的名字和提示词在左侧单独配置").size(12.0).color(MUTED));
        if remove {
            if self.chat_job(&self.chat_id) {
                self.set_err("这个对话还在回复");
            } else {
                self.confirm = Some(Confirm::Chat(self.chat_id.clone()));
            }
        }
        ui.add_space(6.0);
        let frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(244, 244, 245))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(10.0);
        frame.show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.show_messages(ui, &name);
                });
        });
    }

    fn show_messages(&mut self, ui: &mut egui::Ui, bot_name: &str) {
        if self.messages.is_empty() && self.job.is_none() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("从下面的输入框开始对话").color(MUTED));
            });
        }
        let rows: Vec<(String, String, String, u64, u64)> = self
            .messages
            .iter()
            .map(|message| {
                (
                    message.role.clone(),
                    message.content.clone(),
                    message.file.clone(),
                    message.input_tokens,
                    message.output_tokens,
                )
            })
            .collect();
        for (role, content, file, input_tokens, output_tokens) in rows {
            match role.as_str() {
                "user" => self.show_record(ui, Some("你"), &content, &file, true, input_tokens, output_tokens),
                "assistant" => self.show_record(ui, Some(bot_name), &content, &file, false, input_tokens, output_tokens),
                _ => error_line(ui, &content),
            }
            ui.add_space(8.0);
        }
        if self.busy_for_bot() {
            let label = working_label(self.provider_task(&self.chat_id), "");
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(label);
            });
        }
    }

    fn show_group(&mut self, ui: &mut egui::Ui) {
        ui.heading("群组");
        ui.label(egui::RichText::new("你的话会先记下来，Bot 再按加入顺序一个一个回复").size(12.0).color(MUTED));
        field(ui, "名字", &mut self.group_name, false, "例如：项目讨论");
        folder_field(ui, &mut self.group_folder, "选择群组的文件夹");
        ui.label(egui::RichText::new("保存后，群里的 Bot 会读取这个文件夹，也可以把内容写进去。读到的文本会发给模型").size(12.0).color(MUTED));
        let mut save = false;
        let mut cancel = false;
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("保存").clicked() {
                save = true;
            }
            if ui.button("取消").clicked() {
                cancel = true;
            }
            if ui.button("删除").clicked() {
                remove = true;
            }
        });
        if save {
            match self.save_group() {
                Ok(()) => self.set_ok("已保存"),
                Err(err) => self.set_err(err),
            }
        }
        if cancel {
            self.group_name = self.group_name_base.clone();
            self.group_folder = self.group_folder_base.clone();
            self.clear_notice();
        }
        if remove {
            if self.group_job(&self.group_id) {
                self.set_err("这个群组还在回复");
            } else {
                self.confirm = Some(Confirm::Group(self.group_id.clone()));
            }
        }

        let member_ids = self
            .groups
            .iter()
            .find(|group| group.id == self.group_id)
            .map(|group| group.bot_ids.clone())
            .unwrap_or_default();
        let choices = self
            .bots
            .iter()
            .filter(|bot| !member_ids.contains(&bot.id))
            .map(|bot| (bot.id.clone(), bot.name.clone()))
            .collect::<Vec<_>>();
        if !choices.iter().any(|(id, _)| id == &self.join_bot) {
            self.join_bot.clear();
        }
        ui.add_space(4.0);
        ui.label("成员");
        if member_ids.is_empty() {
            ui.label(egui::RichText::new("还没有 Bot").size(12.0).color(MUTED));
        }
        let mut kick = None;
        egui::ScrollArea::vertical()
            .id_salt("group-members")
            .max_height(132.0)
            .show(ui, |ui| {
                for id in &member_ids {
                    let name = self.bot_label(id);
                    ui.horizontal(|ui| {
                        ui.label(&name);
                        if ui.small_button("踢出").clicked() {
                            kick = Some(id.clone());
                        }
                    });
                }
            });
        if self.bots.is_empty() {
            ui.label(egui::RichText::new("先保存一个 Bot，才能加入群组").size(12.0).color(MUTED));
        } else if choices.is_empty() {
            ui.label(egui::RichText::new("保存的 Bot 都已经在群里").size(12.0).color(MUTED));
        } else {
            let mut selected = self.join_bot.clone();
            let shown = choices
                .iter()
                .find(|(id, _)| id == &selected)
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| "选择 Bot".to_string());
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("group-join")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut selected, String::new(), "选择 Bot");
                        for (id, name) in &choices {
                            ui.selectable_value(&mut selected, id.clone(), name);
                        }
                    });
                self.join_bot = selected;
                let can_join = !self.join_bot.is_empty();
                if ui.add_enabled(can_join, egui::Button::new("加入")).clicked() {
                    self.join_selected_bot();
                }
            });
        }
        if let Some(bot_id) = kick {
            self.kick_bot(&bot_id);
        }

        ui.add_space(6.0);
        let frame = egui::Frame::NONE
            .fill(egui::Color32::from_rgb(244, 244, 245))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(10.0);
        frame.show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.show_group_messages(ui);
                });
        });
    }

    fn show_group_messages(&mut self, ui: &mut egui::Ui) {
        if self.group_messages.is_empty() && self.group_speaker().is_none() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("从下面的输入框开始说话").color(MUTED));
            });
        }
        let rows: Vec<(String, String, String, String, u64, u64)> = self
            .group_messages
            .iter()
            .map(|message| {
                (
                    message.role.clone(),
                    message.content.clone(),
                    message.file.clone(),
                    message.speaker_id.clone(),
                    message.input_tokens,
                    message.output_tokens,
                )
            })
            .collect();
        for (role, content, file, speaker, input_tokens, output_tokens) in rows {
            match role.as_str() {
                "user" => self.show_record(ui, Some("你"), &content, &file, true, input_tokens, output_tokens),
                "assistant" => {
                    let name = self.bot_label(&speaker);
                    self.show_record(ui, Some(&name), &content, &file, false, input_tokens, output_tokens);
                }
                _ => {
                    let text = if speaker.is_empty() {
                        content
                    } else {
                        format!("{}：{content}", self.bot_label(&speaker))
                    };
                    error_line(ui, &text);
                }
            }
            ui.add_space(8.0);
        }
        if let Some(bot_id) = self.group_speaker() {
            let name = self.bot_label(&bot_id);
            let label = working_label(self.provider_task(&bot_id), &name);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(label);
            });
        }
    }

    fn show_record(
        &mut self,
        ui: &mut egui::Ui,
        caption: Option<&str>,
        text: &str,
        file: &str,
        mine: bool,
        input_tokens: u64,
        output_tokens: u64,
    ) {
        let image = if is_picture(file) { self.texture_for(ui.ctx(), file).ok() } else { None };
        let file_owned = file.to_string();
        let store = self.store.clone();
        let mut open_error = None;
        bubble(ui, caption, text, mine, |ui| {
            if let Some((id, size)) = image {
                ui.add_space(4.0);
                ui.add(egui::Image::new(egui::load::SizedTexture::new(id, fit_media(size))));
            }
            if !file_owned.is_empty() {
                ui.add_space(4.0);
                if ui.small_button(open_label(&file_owned)).clicked() {
                    if let Err(err) = media::open_saved(&store, &file_owned) {
                        open_error = Some(err);
                    }
                }
            }
        });
        if let Some(err) = open_error {
            self.set_err(err);
        }
        if !mine {
            if let Some(line) = usage::label(input_tokens, output_tokens) {
                ui.label(egui::RichText::new(line).size(11.0).color(MUTED));
            }
        }
    }

    fn texture_for(&mut self, ctx: &egui::Context, rel: &str) -> Result<(egui::TextureId, egui::Vec2), String> {
        if let Some(handle) = self.textures.get(rel) {
            return Ok((handle.id(), handle.size_vec2()));
        }
        let bytes = self.store.read_file(rel)?;
        let (width, height, rgba) = media::decode_rgba(&bytes)?;
        let image = egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba);
        let handle = ctx.load_texture(rel, image, egui::TextureOptions::LINEAR);
        let shown = (handle.id(), handle.size_vec2());
        self.textures.insert(rel.to_string(), handle);
        Ok(shown)
    }

    fn provider_task(&self, bot_id: &str) -> &'static str {
        let task = self
            .bots
            .iter()
            .find(|bot| bot.id == bot_id)
            .and_then(|bot| self.providers.iter().find(|item| item.id == bot.provider_id))
            .map(|item| item.task.as_str())
            .unwrap_or("");
        match task {
            "image" => "image",
            "speech" => "speech",
            "video" => "video",
            _ => "chat",
        }
    }

    fn files_panel(&mut self, ctx: &egui::Context) {
        let files = self.group_files.clone();
        let preview_id = self.preview_id.clone();
        let preview_text = self.preview_text.clone();
        let folder = self.group_folder_base.clone();
        let frame = egui::Frame::NONE.fill(egui::Color32::WHITE).inner_margin(12.0);
        egui::SidePanel::right("group-files")
            .exact_width(300.0)
            .resizable(false)
            .frame(frame)
            .show(ctx, |ui| {
                if !folder.is_empty() {
                    self.folder_browser(ui, &folder);
                    ui.separator();
                }
                ui.strong("文件");
                if ui.button("放入文件").clicked() {
                    self.import_group_file();
                }
                ui.label(egui::RichText::new("只保存在这台电脑上，不会发给模型").size(11.0).color(MUTED));
                egui::ScrollArea::vertical()
                    .id_salt("group-file-list")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        if files.is_empty() {
                            ui.label(egui::RichText::new("还没有文件").size(12.0).color(MUTED));
                        }
                        for file in &files {
                            ui.horizontal(|ui| {
                                if ui.small_button("移除").clicked() {
                                    self.remove_group_file(&file.id);
                                }
                                let label = format!("{}  {}", file.name, size_label(file.size));
                                if ui.selectable_label(preview_id == file.id, label).clicked() {
                                    self.open_group_file(&file.id);
                                }
                            });
                        }
                    });
                ui.separator();
                ui.strong("浏览");
                if preview_id.is_empty() {
                    ui.label(egui::RichText::new("点文件名查看内容").size(12.0).color(MUTED));
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("group-file-preview")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label(egui::RichText::new(&preview_text).size(13.0).color(TEXT));
                    });
            });
    }

    fn composer(&mut self, ctx: &egui::Context) {
        let mut send = false;
        let busy = self.job.is_some();
        let focus = self.focus_input;
        let in_group = self.panel == Panel::Group;
        let hint = if in_group {
            "对群组说一句，Enter 发送".to_string()
        } else {
            match self.provider_task(&self.chat_id) {
                "image" => "描述要生成的图片，Enter 发送",
                "speech" => "输入要朗读的文字，Enter 发送",
                "video" => "描述要生成的视频，Enter 发送",
                _ => "输入消息，Enter 发送",
            }
            .to_string()
        };
        let frame = egui::Frame::NONE.fill(egui::Color32::WHITE).inner_margin(12.0);
        egui::TopBottomPanel::bottom("composer").frame(frame).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 88.0).max(120.0);
                let edit = egui::TextEdit::singleline(&mut self.input)
                    .id(egui::Id::new("composer-input"))
                    .desired_width(width)
                    .hint_text(hint);
                let response = ui.add_enabled(!busy, edit);
                if focus {
                    response.request_focus();
                }
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    send = true;
                }
                let can_send = !busy && !self.input.trim().is_empty();
                let button = egui::Button::new(egui::RichText::new("发送").color(egui::Color32::WHITE))
                    .fill(USER_BUBBLE);
                if ui.add_enabled(can_send, button).clicked() {
                    send = true;
                }
            });
        });
        if focus {
            self.focus_input = false;
        }
        if send {
            if in_group {
                self.send_group();
            } else {
                self.send_chat();
            }
        }
    }

    fn preset_buttons(&mut self, ui: &mut egui::Ui) {
        let mut chosen: Option<&'static str> = None;
        ui.label("预设");
        for group in ["云端 API", "本地", "生图、语音、视频"] {
            ui.label(egui::RichText::new(group).size(12.0).color(MUTED));
            ui.horizontal_wrapped(|ui| {
                for preset in providers::presets().iter().filter(|item| item.group == group) {
                    if ui.small_button(preset.name).clicked() {
                        chosen = Some(preset.name);
                    }
                }
            });
        }
        if let Some(name) = chosen {
            if let Some(preset) = providers::presets().iter().find(|item| item.name == name) {
                providers::apply_preset(&mut self.provider_form, preset);
                self.models = self.provider_form.models.clone();
            }
        }
    }

    fn form_buttons(&mut self, ui: &mut egui::Ui, provider: bool) {
        ui.add_space(4.0);
        let busy = self.job.is_some();
        let mut save = false;
        let mut cancel = false;
        let mut remove = false;
        let mut test = false;
        let mut models = false;
        let saved = if provider {
            !self.provider_form.id.is_empty()
        } else {
            !self.bot_form.id.is_empty()
        };
        ui.horizontal_wrapped(|ui| {
            if ui.button("保存").clicked() {
                save = true;
            }
            if provider && ui.add_enabled(!busy, egui::Button::new("测试连接")).clicked() {
                test = true;
            }
            if ui.add_enabled(!busy, egui::Button::new("读取模型")).clicked() {
                models = true;
            }
            if ui.button("取消").clicked() {
                cancel = true;
            }
            if saved && ui.button("删除").clicked() {
                remove = true;
            }
        });
        if save {
            match if provider { self.save_provider() } else { self.save_bot() } {
                Ok(()) => self.set_ok("已保存"),
                Err(err) => self.set_err(err),
            }
        }
        if test {
            self.start_probe(ProbeKind::Test, true);
        }
        if models {
            self.start_probe(ProbeKind::Models, provider);
        }
        if cancel {
            self.abandon();
        }
        if remove {
            self.confirm = Some(if provider {
                Confirm::Provider(self.provider_form.id.clone())
            } else {
                Confirm::Bot(self.bot_form.id.clone())
            });
        }
    }

    fn model_chips(&mut self, ui: &mut egui::Ui) {
        if self.models.is_empty() {
            return;
        }
        ui.label(egui::RichText::new("模型列表").size(12.0).color(MUTED));
        let models = self.models.iter().take(40).cloned().collect::<Vec<_>>();
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            for model in &models {
                if ui.small_button(model).clicked() {
                    picked = Some(model.clone());
                }
            }
        });
        if self.models.len() > models.len() {
            ui.label(egui::RichText::new(format!("还有 {} 个未显示", self.models.len() - models.len())).size(12.0).color(MUTED));
        }
        if let Some(model) = picked {
            self.provider_form.model = model;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("确认删除")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(match &confirm {
                    Confirm::Provider(_) => "删除这个模型提供方？已保存的密钥会一起去掉。",
                    Confirm::Bot(_) => "删除这个 Bot 和它的对话？它也会离开所有群组。指定的文件夹不会删除。",
                    Confirm::Group(_) => "删除这个群组、里面的对话和放入的文件？指定的文件夹不会删除。",
                    Confirm::Chat(_) => "删除和这个 Bot 的全部对话？记录会从本机去掉，Bot 还在。",
                });
                ui.horizontal(|ui| {
                    if ui.button("删除").clicked() {
                        self.perform_delete(&confirm);
                    }
                    if ui.button("取消").clicked() {
                        self.confirm = None;
                    }
                });
            });
        if !open {
            self.confirm = None;
        }
    }

    fn send_chat(&mut self) {
        if self.job.is_some() {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let bot_id = self.chat_id.clone();
        if bot_id.is_empty() {
            return;
        }
        let prepared = match chat::prepare(&self.store, &bot_id, &text) {
            Ok(prepared) => prepared,
            Err(err) => {
                self.set_err(err);
                return;
            }
        };
        self.input.clear();
        self.messages = chat::load(&self.store, &bot_id).unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = chat::run(prepared, http_client::send);
            let _ = tx.send(result);
        });
        self.job = Some(Job::Chat { bot_id, rx });
        self.clear_notice();
    }

    fn start_probe(&mut self, kind: ProbeKind, from_provider_form: bool) {
        if self.job.is_some() {
            self.set_err("上一个请求还在进行");
            return;
        }
        let (provider, provider_id) = if from_provider_form {
            match providers::validate(self.provider_form.clone()) {
                Ok(provider) => {
                    let id = if provider.id.is_empty() { None } else { Some(provider.id.clone()) };
                    (provider, id)
                }
                Err(err) => {
                    self.set_err(err);
                    return;
                }
            }
        } else if let Some(provider) = self.providers.iter().find(|item| item.id == self.bot_form.provider_id).cloned() {
            let id = Some(provider.id.clone());
            (provider, id)
        } else {
            self.set_err("请先选择模型提供方");
            return;
        };
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = providers::test_provider_with(&provider, http_client::send);
            let _ = tx.send(result);
        });
        self.job = Some(Job::Probe { kind, provider_id, rx });
        self.set_ok(match kind {
            ProbeKind::Test => "正在测试连接…",
            ProbeKind::Models => "正在读取模型…",
        });
    }

    fn navigate(&mut self, nav: Nav) {
        if self.same_target(&nav) && !self.dirty() {
            return;
        }
        if self.dirty() {
            if let Err(err) = self.save_current() {
                self.set_err(err);
                return;
            }
        }
        self.clear_notice();
        self.models.clear();
        match nav {
            Nav::NewProvider => {
                let draft = Provider::draft("openai");
                self.provider_form = draft.clone();
                self.provider_base = draft;
                self.panel = Panel::Provider;
            }
            Nav::NewBot => {
                let draft = Bot::draft();
                self.bot_form = draft.clone();
                self.bot_base = draft;
                self.panel = Panel::Bot;
            }
            Nav::Provider(id) => {
                if let Some(provider) = self.providers.iter().find(|item| item.id == id).cloned() {
                    self.models = provider.models.clone();
                    self.provider_form = provider.clone();
                    self.provider_base = provider;
                    self.panel = Panel::Provider;
                }
            }
            Nav::Bot(id) => {
                if let Some(bot) = self.bots.iter().find(|item| item.id == id).cloned() {
                    self.bot_form = bot.clone();
                    self.bot_base = bot;
                    self.panel = Panel::Bot;
                }
            }
            Nav::Chat(id) => {
                if self.bots.iter().any(|item| item.id == id) {
                    self.chat_id = id.clone();
                    self.messages = chat::load(&self.store, &id).unwrap_or_default();
                    self.input.clear();
                    self.focus_input = true;
                    self.panel = Panel::Chat;
                    let folder = self.bots.iter().find(|item| item.id == id).map(|item| item.folder.clone()).unwrap_or_default();
                    self.reset_browse();
                    self.reload_browse(&folder);
                }
            }
            Nav::NewGroup => match group::unused_name(&self.store).and_then(|name| group::create(&self.store, &name)) {
                Ok(group) => match group::list(&self.store) {
                    Ok(groups) => {
                        self.groups = groups;
                        self.enter_group(&group.id);
                        if self.notice.is_empty() {
                            self.set_ok("已创建群组");
                        }
                    }
                    Err(err) => self.set_err(err),
                },
                Err(err) => self.set_err(err),
            },
            Nav::Group(id) => self.enter_group(&id),
        }
    }

    fn same_target(&self, nav: &Nav) -> bool {
        match nav {
            Nav::Provider(id) => self.panel == Panel::Provider && self.provider_form.id == *id,
            Nav::Bot(id) => self.panel == Panel::Bot && self.bot_form.id == *id,
            Nav::Chat(id) => self.panel == Panel::Chat && self.chat_id == *id,
            Nav::Group(id) => self.panel == Panel::Group && self.group_id == *id,
            Nav::NewProvider | Nav::NewBot | Nav::NewGroup => false,
        }
    }

    fn save_current(&mut self) -> Result<(), String> {
        match self.panel {
            Panel::Provider => self.save_provider(),
            Panel::Bot => self.save_bot(),
            Panel::Group => self.save_group(),
            Panel::Home | Panel::Chat => Ok(()),
        }
    }

    fn save_provider(&mut self) -> Result<(), String> {
        let saved = providers::upsert(&self.store, self.provider_form.clone())?;
        self.providers = providers::list(&self.store)?;
        self.provider_form = saved.clone();
        self.provider_base = saved;
        Ok(())
    }

    fn save_bot(&mut self) -> Result<(), String> {
        let saved = bots::upsert(&self.store, self.bot_form.clone())?;
        self.bots = bots::list(&self.store)?;
        self.bot_form = saved.clone();
        self.bot_base = saved;
        Ok(())
    }

    fn abandon(&mut self) {
        match self.panel {
            Panel::Provider if self.provider_form.id.is_empty() => self.panel = Panel::Home,
            Panel::Provider => self.provider_form = self.provider_base.clone(),
            Panel::Bot if self.bot_form.id.is_empty() => {
                self.panel = Panel::Home;
                self.messages.clear();
            }
            Panel::Bot => self.bot_form = self.bot_base.clone(),
            Panel::Group => {
                self.group_name = self.group_name_base.clone();
                self.group_folder = self.group_folder_base.clone();
            }
            Panel::Home | Panel::Chat => {}
        }
        self.clear_notice();
    }

    fn perform_delete(&mut self, confirm: &Confirm) {
        let deleted = match confirm {
            Confirm::Provider(id) => providers::delete(&self.store, id),
            Confirm::Bot(id) if self.group_speaker_is(id) => Err("这个 Bot 还在回复".into()),
            Confirm::Bot(id) => chat::delete_bot(&self.store, id),
            Confirm::Group(id) if self.group_job(id) => Err("这个群组还在回复".into()),
            Confirm::Group(id) => group::delete(&self.store, id),
            Confirm::Chat(id) if self.chat_job(id) => Err("这个对话还在回复".into()),
            Confirm::Chat(id) => chat::delete_chat(&self.store, id),
        };
        if deleted.is_ok() {
            if let Confirm::Chat(id) = confirm {
                if self.chat_id == *id {
                    self.messages.clear();
                }
                self.confirm = None;
                self.set_ok("已删除");
                return;
            }
        }
        match deleted {
            Ok(()) => match (providers::list(&self.store), bots::list(&self.store), group::list(&self.store)) {
                (Ok(providers), Ok(bots), Ok(groups)) => {
                    self.providers = providers;
                    self.bots = bots;
                    self.groups = groups;
                    self.panel = Panel::Home;
                    self.chat_id.clear();
                    self.messages.clear();
                    self.clear_group_view();
                    self.confirm = None;
                    self.set_ok("已删除");
                }
                _ => {
                    self.confirm = None;
                    self.set_err("已删除，但重新加载失败");
                }
            },
            Err(err) => {
                self.confirm = None;
                self.set_err(err);
            }
        }
    }

    fn dirty(&self) -> bool {
        match self.panel {
            Panel::Home | Panel::Chat => false,
            Panel::Provider => self.provider_form != self.provider_base,
            Panel::Bot => self.bot_form != self.bot_base,
            Panel::Group => self.group_name != self.group_name_base || self.group_folder != self.group_folder_base,
        }
    }

    fn busy_for_bot(&self) -> bool {
        matches!(&self.job, Some(Job::Chat { bot_id, .. }) if self.panel == Panel::Chat && bot_id == &self.chat_id)
    }

    fn chat_job(&self, id: &str) -> bool {
        matches!(&self.job, Some(Job::Chat { bot_id, .. }) if bot_id == id)
    }

    fn show_notice(&self, ui: &mut egui::Ui) {
        if self.notice.is_empty() {
            return;
        }
        let color = if self.notice_ok { OK } else { DANGER };
        ui.label(egui::RichText::new(&self.notice).color(color));
    }

    fn set_err(&mut self, text: impl Into<String>) {
        self.notice = text.into();
        self.notice_ok = false;
    }

    fn set_ok(&mut self, text: impl Into<String>) {
        self.notice = text.into();
        self.notice_ok = true;
    }

    fn clear_notice(&mut self) {
        self.notice.clear();
        self.notice_ok = false;
    }

    fn save_group(&mut self) -> Result<(), String> {
        if self.group_id.is_empty() {
            return Ok(());
        }
        let saved = group::save_settings(&self.store, &self.group_id, &self.group_name, &self.group_folder)?;
        self.groups = group::list(&self.store)?;
        self.group_name = saved.name.clone();
        self.group_name_base = saved.name;
        self.group_folder = saved.folder.clone();
        self.group_folder_base = saved.folder.clone();
        self.reset_browse();
        self.reload_browse(&saved.folder);
        Ok(())
    }

    fn enter_group(&mut self, id: &str) {
        let Some(group) = self.groups.iter().find(|item| item.id == id).cloned() else {
            return;
        };
        self.group_id = group.id.clone();
        self.group_name = group.name.clone();
        self.group_name_base = group.name;
        self.group_folder = group.folder.clone();
        self.group_folder_base = group.folder.clone();
        self.reset_browse();
        self.reload_browse(&group.folder);
        let mut error = None;
        match group::load(&self.store, &group.id) {
            Ok(messages) => self.group_messages = messages,
            Err(err) => {
                self.group_messages.clear();
                error = Some(err);
            }
        }
        match group::list_files(&self.store, &group.id) {
            Ok(files) => self.group_files = files,
            Err(err) => {
                self.group_files.clear();
                if error.is_none() {
                    error = Some(err);
                }
            }
        }
        self.join_bot.clear();
        self.preview_id.clear();
        self.preview_text.clear();
        self.input.clear();
        self.focus_input = true;
        self.panel = Panel::Group;
        if let Some(err) = error {
            self.set_err(err);
        }
    }

    fn clear_group_view(&mut self) {
        self.group_id.clear();
        self.group_name.clear();
        self.group_name_base.clear();
        self.group_folder.clear();
        self.group_folder_base.clear();
        self.reset_browse();
        self.group_messages.clear();
        self.group_files.clear();
        self.join_bot.clear();
        self.preview_id.clear();
        self.preview_text.clear();
    }

    fn join_selected_bot(&mut self) {
        let bot_id = self.join_bot.clone();
        if bot_id.is_empty() {
            return;
        }
        match group::add_bot(&self.store, &self.group_id, &bot_id) {
            Ok(_) => match group::list(&self.store) {
                Ok(groups) => {
                    self.groups = groups;
                    self.join_bot.clear();
                    self.set_ok("已加入");
                }
                Err(err) => self.set_err(err),
            },
            Err(err) => self.set_err(err),
        }
    }

    fn kick_bot(&mut self, bot_id: &str) {
        match group::remove_bot(&self.store, &self.group_id, bot_id) {
            Ok(_) => match group::list(&self.store) {
                Ok(groups) => {
                    self.groups = groups;
                    self.set_ok("已踢出");
                }
                Err(err) => self.set_err(err),
            },
            Err(err) => self.set_err(err),
        }
    }

    fn bot_label(&self, id: &str) -> String {
        self.bots
            .iter()
            .find(|bot| bot.id == id)
            .map(|bot| bot.name.clone())
            .unwrap_or_else(|| "已删除的 Bot".to_string())
    }

    fn group_job(&self, id: &str) -> bool {
        matches!(&self.job, Some(Job::Group { group_id, .. }) if group_id == id)
    }

    fn group_speaker(&self) -> Option<String> {
        match &self.job {
            Some(Job::Group { group_id, bot_id, .. }) if self.panel == Panel::Group && group_id == &self.group_id => {
                Some(bot_id.clone())
            }
            _ => None,
        }
    }

    fn group_speaker_is(&self, id: &str) -> bool {
        matches!(&self.job, Some(Job::Group { bot_id, .. }) if bot_id == id)
    }

    fn import_group_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().set_title("放入群组").pick_file() else {
            return;
        };
        match group::import_path(&self.store, &self.group_id, &path) {
            Ok(file) => {
                if let Err(err) = self.reload_group_files() {
                    self.set_err(err);
                    return;
                }
                self.open_group_file(&file.id);
                if self.notice.is_empty() || self.notice_ok {
                    self.set_ok("已放入文件");
                }
            }
            Err(err) => self.set_err(err),
        }
    }

    fn open_group_file(&mut self, file_id: &str) {
        match group::preview(&self.store, &self.group_id, file_id) {
            Ok(group::Preview::Binary) => {
                self.preview_id = file_id.to_string();
                self.preview_text = "这个文件不是文本，不能在窗口里显示。".into();
            }
            Ok(group::Preview::Text { body, truncated }) => {
                self.preview_id = file_id.to_string();
                self.preview_text = if body.is_empty() {
                    "（空文件）".into()
                } else if truncated {
                    format!("{body}\n\n（只显示前 10 万字）")
                } else {
                    body
                };
            }
            Err(err) => self.set_err(err),
        }
    }

    fn remove_group_file(&mut self, file_id: &str) {
        match group::remove_file(&self.store, &self.group_id, file_id) {
            Ok(()) => {
                if self.preview_id == file_id {
                    self.preview_id.clear();
                    self.preview_text.clear();
                }
                if let Err(err) = self.reload_group_files() {
                    self.set_err(err);
                    return;
                }
                self.set_ok("已移除文件");
            }
            Err(err) => self.set_err(err),
        }
    }

    fn chat_folder(&self) -> Option<String> {
        self.bots
            .iter()
            .find(|item| item.id == self.chat_id)
            .map(|item| item.folder.clone())
            .filter(|folder| !folder.trim().is_empty())
    }

    fn chat_folder_panel(&mut self, ctx: &egui::Context) {
        let Some(root) = self.chat_folder() else {
            return;
        };
        let frame = egui::Frame::NONE.fill(egui::Color32::WHITE).inner_margin(12.0);
        egui::SidePanel::right("chat-folder")
            .exact_width(300.0)
            .resizable(false)
            .frame(frame)
            .show(ctx, |ui| {
                self.folder_browser(ui, &root);
            });
    }

    fn folder_browser(&mut self, ui: &mut egui::Ui, root: &str) {
        let dir = self.browse_dir.clone();
        let entries = self.browse_entries.clone();
        let truncated = self.browse_truncated;
        let error = self.browse_error.clone();
        let current = self.browse_file.clone();
        let preview = self.browse_text.clone();
        let mut up = false;
        let mut open_dir = None;
        let mut open_file = None;
        let mut write = false;
        let mut import = false;
        ui.strong("文件夹");
        ui.label(egui::RichText::new(root).size(11.0).color(MUTED));
        ui.horizontal(|ui| {
            if !dir.is_empty() && ui.button("上级").clicked() {
                up = true;
            }
            let here = if dir.is_empty() { "当前文件夹".to_string() } else { dir.clone() };
            ui.label(egui::RichText::new(here).size(12.0).color(MUTED));
        });
        if !error.is_empty() {
            ui.label(egui::RichText::new(&error).size(12.0).color(DANGER));
        }
        egui::ScrollArea::vertical()
            .id_salt("folder-list")
            .max_height(180.0)
            .show(ui, |ui| {
                if entries.is_empty() && error.is_empty() {
                    ui.label(egui::RichText::new("这个文件夹是空的").size(12.0).color(MUTED));
                }
                for entry in &entries {
                    let label = if entry.dir {
                        format!("{}/", entry.name)
                    } else {
                        format!("{}  {}", entry.name, size_label(entry.size))
                    };
                    let path = join_rel(&dir, &entry.name);
                    let selected = !entry.dir && current == path;
                    if ui.selectable_label(selected, label).clicked() {
                        if entry.dir {
                            open_dir = Some(entry.name.clone());
                        } else {
                            open_file = Some(path);
                        }
                    }
                }
                if truncated {
                    ui.label(egui::RichText::new("还有更多，未列出").size(12.0).color(MUTED));
                }
            });
        ui.separator();
        ui.strong("读取");
        if current.is_empty() {
            ui.label(egui::RichText::new("点文件名查看内容，点文件夹名进入").size(12.0).color(MUTED));
        } else {
            ui.label(egui::RichText::new(&current).size(11.0).color(MUTED));
            egui::ScrollArea::vertical()
                .id_salt("folder-preview")
                .max_height(140.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(&preview).size(13.0).color(TEXT));
                });
        }
        ui.separator();
        ui.strong("写入");
        ui.add(
            egui::TextEdit::singleline(&mut self.browse_name)
                .desired_width(f32::INFINITY)
                .hint_text("文件名，例如 笔记.txt"),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.browse_body)
                .desired_rows(4)
                .desired_width(f32::INFINITY)
                .hint_text("文本内容"),
        );
        ui.horizontal(|ui| {
            if ui.button("写入").clicked() {
                write = true;
            }
            if ui.button("放入文件").clicked() {
                import = true;
            }
        });
        ui.label(egui::RichText::new("写在当前打开的文件夹里。对话时 Bot 也会读取和写入这里").size(11.0).color(MUTED));
        if up {
            self.browse_dir = parent_rel(&dir);
            self.browse_file.clear();
            self.browse_text.clear();
            self.reload_browse(root);
        }
        if let Some(name) = open_dir {
            self.browse_dir = join_rel(&dir, &name);
            self.browse_file.clear();
            self.browse_text.clear();
            self.reload_browse(root);
        }
        if let Some(path) = open_file {
            self.open_browse(root, &path);
        }
        if write {
            self.write_browse(root);
        }
        if import {
            self.import_browse(root);
        }
    }

    fn reset_browse(&mut self) {
        self.browse_dir.clear();
        self.browse_entries.clear();
        self.browse_truncated = false;
        self.browse_file.clear();
        self.browse_text.clear();
        self.browse_name.clear();
        self.browse_body.clear();
        self.browse_error.clear();
    }

    fn reload_browse(&mut self, root: &str) {
        if root.trim().is_empty() {
            self.browse_entries.clear();
            self.browse_truncated = false;
            self.browse_error.clear();
            return;
        }
        match folder::list(root, &self.browse_dir) {
            Ok(listing) => {
                self.browse_entries = listing.entries;
                self.browse_truncated = listing.truncated;
                self.browse_error.clear();
            }
            Err(err) => {
                self.browse_entries.clear();
                self.browse_truncated = false;
                self.browse_error = err;
            }
        }
    }

    fn open_browse(&mut self, root: &str, path: &str) {
        match folder::read(root, path) {
            Ok(folder::Preview::Binary) => {
                self.browse_file = path.to_string();
                self.browse_text = "这个文件不是文本，不能在窗口里显示。".into();
            }
            Ok(folder::Preview::Text { body, truncated }) => {
                self.browse_file = path.to_string();
                self.browse_text = if body.is_empty() {
                    "（空文件）".into()
                } else if truncated {
                    format!("{body}\n\n（只显示前 10 万字）")
                } else {
                    body
                };
            }
            Err(err) => self.set_err(err),
        }
    }

    fn write_browse(&mut self, root: &str) {
        let name = self.browse_name.clone();
        let body = self.browse_body.clone();
        match folder::write(root, &self.browse_dir, &name, body.as_bytes()) {
            Ok(path) => {
                self.reload_browse(root);
                self.set_ok("已写入");
                self.open_browse(root, &path);
            }
            Err(err) => self.set_err(err),
        }
    }

    fn import_browse(&mut self, root: &str) {
        let Some(path) = rfd::FileDialog::new().set_title("写入文件夹").pick_file() else {
            return;
        };
        let Some(name) = path.file_name().and_then(|item| item.to_str()) else {
            self.set_err("文件名无效");
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) => {
                self.set_err(format!("读取文件失败：{err}"));
                return;
            }
        };
        match folder::write(root, &self.browse_dir, name, &bytes) {
            Ok(saved) => {
                self.reload_browse(root);
                self.set_ok("已写入");
                self.open_browse(root, &saved);
            }
            Err(err) => self.set_err(err),
        }
    }

    fn reload_group_files(&mut self) -> Result<(), String> {
        self.group_files = group::list_files(&self.store, &self.group_id)?;
        Ok(())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_frame(ctx);
    }
}

fn section_title(ui: &mut egui::Ui, title: &str, action: &str, nav: impl FnOnce() -> Nav, app: &mut App) {
    ui.horizontal(|ui| {
        ui.strong(title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(action).clicked() {
                app.navigate(nav());
            }
        });
    });
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn parent_rel(dir: &str) -> String {
    match dir.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

fn folder_field(ui: &mut egui::Ui, text: &mut String, title: &str) {
    ui.label("文件夹");
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 72.0).max(120.0);
        ui.add(
            egui::TextEdit::singleline(text)
                .desired_width(width)
                .hint_text("绝对路径，留空则不浏览"),
        );
        if ui.button("选择").clicked() {
            if let Some(path) = crate::pick_folder::pick(title, text) {
                *text = path.display().to_string();
            }
        }
    });
    ui.label(egui::RichText::new("点「选择」后，在树里点一下文件夹，再点确定。").size(12.0).color(MUTED));
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, password: bool, hint: &str) {
    ui.label(label);
    let mut edit = egui::TextEdit::singleline(text)
        .desired_width(f32::INFINITY)
        .hint_text(hint);
    if password {
        edit = edit.password(true);
    }
    ui.add(edit);
}

fn bubble(ui: &mut egui::Ui, caption: Option<&str>, text: &str, mine: bool, extra: impl FnOnce(&mut egui::Ui)) {
    let max_width = (ui.available_width() * 0.72).clamp(200.0, 560.0);
    let layout = if mine {
        egui::Layout::right_to_left(egui::Align::Min)
    } else {
        egui::Layout::left_to_right(egui::Align::Min)
    };
    if let Some(caption) = caption {
        ui.with_layout(layout, |ui| {
            ui.label(egui::RichText::new(caption).size(11.0).color(MUTED));
        });
    }
    ui.with_layout(layout, |ui| {
        let frame = egui::Frame::NONE
            .fill(if mine { USER_BUBBLE } else { egui::Color32::WHITE })
            .stroke(if mine {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(1.0_f32, LINE)
            })
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(egui::Margin::symmetric(12, 8));
        frame.show(ui, |ui| {
            ui.set_max_width(max_width);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(text)
                        .size(15.0)
                        .color(if mine { egui::Color32::WHITE } else { TEXT }),
                );
                extra(ui);
            });
        });
    });
    ui.add_space(2.0);
}

fn working_label(task: &str, name: &str) -> String {
    let action = match task {
        "image" => "正在生图…",
        "speech" => "正在生成语音…",
        "video" => "正在生成视频…",
        _ => "正在回复…",
    };
    if name.is_empty() {
        action.to_string()
    } else {
        format!("{name} {action}")
    }
}

fn fit_media(size: egui::Vec2) -> egui::Vec2 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return size;
    }
    let scale = (320.0 / size.x).min(320.0 / size.y).min(1.0);
    egui::vec2(size.x * scale, size.y * scale)
}

fn is_picture(file: &str) -> bool {
    file.ends_with(".png") || file.ends_with(".jpg")
}

fn open_label(file: &str) -> &'static str {
    if is_picture(file) {
        "打开图片"
    } else if file.ends_with(".mp3") || file.ends_with(".wav") {
        "打开语音"
    } else if file.ends_with(".mp4") {
        "打开视频"
    } else {
        "打开文件"
    }
}

fn size_label(size: u64) -> String {
    if size < 1024 {
        format!("{size} B")
    } else if size < 1024 * 1024 {
        format!("{:.1} KB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
    }
}

fn error_line(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(254, 242, 242))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(DANGER));
        });
}
