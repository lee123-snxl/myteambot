use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke};

use super::{working_label, App, Job, Nav, Panel, ProbeKind, View};
use crate::providers;

pub(super) const ACTIVITY_BG: Color32 = Color32::from_rgb(24, 24, 24);
pub(super) const SIDE_BG: Color32 = Color32::from_rgb(24, 24, 24);
pub(super) const EDITOR_BG: Color32 = Color32::from_rgb(31, 31, 31);
pub(super) const BORDER: Color32 = Color32::from_rgb(43, 43, 43);
pub(super) const ACCENT: Color32 = Color32::from_rgb(0, 120, 212);
pub(super) const STATUS_BG: Color32 = Color32::from_rgb(0, 122, 204);
pub(super) const TEXT: Color32 = Color32::from_rgb(204, 204, 204);
pub(super) const BRIGHT: Color32 = Color32::from_rgb(240, 240, 240);
pub(super) const MUTED: Color32 = Color32::from_rgb(157, 157, 157);
pub(super) const INPUT_BG: Color32 = Color32::from_rgb(49, 49, 49);
pub(super) const INPUT_BORDER: Color32 = Color32::from_rgb(60, 60, 60);
pub(super) const HOVER: Color32 = Color32::from_rgb(42, 45, 46);
pub(super) const SELECTED: Color32 = Color32::from_rgb(4, 57, 94);
pub(super) const CARD: Color32 = Color32::from_rgb(37, 37, 38);
pub(super) const USER_BUBBLE: Color32 = Color32::from_rgb(38, 79, 120);
pub(super) const ERROR_BG: Color32 = Color32::from_rgb(90, 29, 29);
pub(super) const DANGER: Color32 = Color32::from_rgb(241, 76, 76);
pub(super) const OK: Color32 = Color32::from_rgb(137, 209, 133);
pub(super) const WARN: Color32 = Color32::from_rgb(204, 167, 0);

const TAB_HEIGHT: f32 = 35.0;
const NOTICE_SECS: u64 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Activity {
    Talk,
    Explorer,
    Bots,
    Providers,
}

const ACTIVITIES: [(Activity, &str); 4] = [
    (Activity::Talk, "会话"),
    (Activity::Explorer, "资源管理器"),
    (Activity::Bots, "Bot"),
    (Activity::Providers, "模型提供方"),
];

/// 侧栏行前面的小图标。
#[derive(Clone, Copy)]
pub(super) enum Mark {
    None,
    Folder,
    File,
}

pub(super) struct Tab {
    pub(super) label: String,
    pub(super) view: View,
    pub(super) dirty: bool,
    pub(super) closable: bool,
}

pub(super) enum TabEvent {
    Select(View),
    Close(View),
}

pub(super) fn install_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = EDITOR_BG;
    visuals.window_fill = CARD;
    visuals.window_stroke = Stroke::new(1.0_f32, INPUT_BORDER);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(4);
    visuals.window_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(120) };
    visuals.popup_shadow = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(100) };
    visuals.extreme_bg_color = INPUT_BG;
    visuals.faint_bg_color = HOVER;
    visuals.code_bg_color = INPUT_BG;
    visuals.hyperlink_color = Color32::from_rgb(77, 170, 252);
    visuals.selection.bg_fill = SELECTED;
    visuals.selection.stroke = Stroke::new(1.0_f32, BRIGHT);
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = DANGER;
    let widgets = &mut visuals.widgets;
    for item in [
        &mut widgets.noninteractive,
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        item.corner_radius = CornerRadius::same(3);
        item.expansion = 0.0;
    }
    widgets.noninteractive.bg_fill = EDITOR_BG;
    widgets.noninteractive.weak_bg_fill = EDITOR_BG;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.inactive.bg_fill = INPUT_BG;
    widgets.inactive.weak_bg_fill = INPUT_BG;
    widgets.inactive.bg_stroke = Stroke::new(1.0_f32, INPUT_BORDER);
    widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    widgets.hovered.bg_fill = Color32::from_rgb(60, 60, 60);
    widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 60, 60);
    widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(82, 82, 82));
    widgets.hovered.fg_stroke = Stroke::new(1.0_f32, BRIGHT);
    widgets.active.bg_fill = Color32::from_rgb(70, 70, 70);
    widgets.active.weak_bg_fill = Color32::from_rgb(70, 70, 70);
    widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    widgets.active.fg_stroke = Stroke::new(1.0_f32, BRIGHT);
    widgets.open.bg_fill = INPUT_BG;
    widgets.open.weak_bg_fill = INPUT_BG;
    widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    widgets.open.fg_stroke = Stroke::new(1.0_f32, BRIGHT);
    style.visuals = visuals;
    style.spacing.item_spacing = vec2(8.0, 6.0);
    style.spacing.button_padding = vec2(10.0, 4.0);
    style.spacing.interact_size.y = 24.0;
    style.text_styles = [
        (egui::TextStyle::Heading, FontId::proportional(20.0)),
        (egui::TextStyle::Body, FontId::proportional(13.5)),
        (egui::TextStyle::Button, FontId::proportional(13.0)),
        (egui::TextStyle::Small, FontId::proportional(11.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();
    ctx.set_style(style);
}

impl App {
    pub(super) fn activity_bar(&mut self, ctx: &egui::Context) {
        let has_files = self.explorer_root().is_some() || self.in_group();
        egui::SidePanel::left("activity")
            .exact_width(48.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(ACTIVITY_BG))
            .show(ctx, |ui| {
                right_border(ui);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (activity, name) in ACTIVITIES {
                    let active = self.side_open && self.activity == activity;
                    let badge = activity == Activity::Explorer && has_files;
                    if activity_button(ui, activity, name, active, badge).clicked() {
                        if self.activity == activity {
                            self.side_open = !self.side_open;
                        } else {
                            self.activity = activity;
                            self.side_open = true;
                        }
                    }
                }
            });
    }

    pub(super) fn side_bar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .resizable(true)
            .default_width(250.0)
            .width_range(180.0..=440.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(SIDE_BG))
            .show(ctx, |ui| {
                right_border(ui);
                match self.activity {
                    Activity::Talk => self.talk_side(ui),
                    Activity::Explorer => self.explorer_side(ui),
                    Activity::Bots => self.bot_side(ui),
                    Activity::Providers => self.provider_side(ui),
                }
            });
    }

    fn talk_side(&mut self, ui: &mut egui::Ui) {
        side_title(ui, "会话", None);
        let bots = self.bots.iter().map(|item| (item.id.clone(), item.name.clone())).collect::<Vec<_>>();
        let groups = self
            .groups
            .iter()
            .map(|item| (item.id.clone(), item.name.clone(), item.bot_ids.len()))
            .collect::<Vec<_>>();
        let current_chat = if self.panel == Panel::Chat { self.chat_id.clone() } else { String::new() };
        let current_group = if self.panel == Panel::Group { self.group_id.clone() } else { String::new() };
        let mut nav = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            section(ui, "side-chats", "单聊", None, |ui| {
                if bots.is_empty() {
                    side_hint(ui, "保存一个 Bot 后可以在这里聊天");
                }
                for (id, name) in &bots {
                    if list_item(ui, Mark::None, name, "", current_chat == *id).clicked() {
                        nav = Some(Nav::Chat(id.clone()));
                    }
                }
            });
            let add = section(ui, "side-groups", "群组", Some("新建群组"), |ui| {
                if groups.is_empty() {
                    side_hint(ui, "点标题右边的「+」新建群组");
                }
                for (id, name, count) in &groups {
                    if list_item(ui, Mark::None, name, &format!("{count} 个 Bot"), current_group == *id).clicked() {
                        nav = Some(Nav::Group(id.clone()));
                    }
                }
            });
            if add {
                nav = Some(Nav::NewGroup);
            }
        });
        if let Some(nav) = nav {
            self.navigate(nav);
        }
    }

    fn bot_side(&mut self, ui: &mut egui::Ui) {
        let add = side_title(ui, "Bot", Some("新建 Bot"));
        let current = if self.panel == Panel::Bot { self.bot_form.id.clone() } else { String::new() };
        let rows = self
            .bots
            .iter()
            .map(|bot| {
                let detail = if bot.model.is_empty() {
                    self.providers
                        .iter()
                        .find(|item| item.id == bot.provider_id)
                        .map(|item| item.name.clone())
                        .unwrap_or_default()
                } else {
                    bot.model.clone()
                };
                (bot.id.clone(), bot.name.clone(), detail)
            })
            .collect::<Vec<_>>();
        let mut nav = if add { Some(Nav::NewBot) } else { None };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if rows.is_empty() {
                side_hint(ui, "点标题右边的「+」新建 Bot");
            }
            for (id, name, detail) in &rows {
                if list_item(ui, Mark::None, name, detail, current == *id).clicked() {
                    nav = Some(Nav::Bot(id.clone()));
                }
            }
        });
        if let Some(nav) = nav {
            self.navigate(nav);
        }
    }

    fn provider_side(&mut self, ui: &mut egui::Ui) {
        let add = side_title(ui, "模型提供方", Some("添加模型提供方"));
        let current = if self.panel == Panel::Provider { self.provider_form.id.clone() } else { String::new() };
        let rows = self
            .providers
            .iter()
            .map(|item| {
                let detail = format!("{} · {}", providers::kind_label(&item.kind), providers::task_label(&item.task));
                (item.id.clone(), item.name.clone(), detail)
            })
            .collect::<Vec<_>>();
        let mut nav = if add { Some(Nav::NewProvider) } else { None };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if rows.is_empty() {
                side_hint(ui, "点标题右边的「+」添加提供方");
            }
            for (id, name, detail) in &rows {
                if list_item(ui, Mark::None, name, detail, current == *id).clicked() {
                    nav = Some(Nav::Provider(id.clone()));
                }
            }
        });
        if let Some(nav) = nav {
            self.navigate(nav);
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        let busy = self.busy_label();
        let dirty = self.dirty();
        let data_dir = self.data_dir.clone();
        egui::TopBottomPanel::bottom("status")
            .exact_height(22.0)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(STATUS_BG).inner_margin(egui::Margin::symmetric(10, 0)))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    ui.label(egui::RichText::new(format!("数据目录  {data_dir}")).size(12.0).color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(label) = busy {
                            ui.label(egui::RichText::new(label).size(12.0).color(Color32::WHITE));
                            ui.add(egui::Spinner::new().size(12.0).color(Color32::WHITE));
                        }
                        if dirty {
                            ui.label(egui::RichText::new("● 未保存").size(12.0).color(Color32::WHITE));
                        }
                    });
                });
            });
    }

    fn busy_label(&self) -> Option<String> {
        match self.job.as_ref()? {
            Job::Chat { bot_id, .. } | Job::Group { bot_id, .. } => {
                Some(working_label(self.provider_task(bot_id), &self.bot_label(bot_id)))
            }
            Job::Probe { kind: ProbeKind::Test, .. } => Some("正在测试连接…".into()),
            Job::Probe { kind: ProbeKind::Models, .. } => Some("正在读取模型…".into()),
        }
    }

    pub(super) fn notification(&mut self, ctx: &egui::Context, lifted: bool) {
        if self.notice.is_empty() {
            return;
        }
        if self.notice_ok {
            let shown = self.notice_at.elapsed();
            let limit = Duration::from_secs(NOTICE_SECS);
            if shown >= limit {
                self.clear_notice();
                return;
            }
            ctx.request_repaint_after(limit - shown);
        }
        let color = if self.notice_ok { OK } else { DANGER };
        let text = self.notice.clone();
        let mut close = false;
        let lift = if lifted { 96.0 } else { 34.0 };
        egui::Area::new(egui::Id::new("notice"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-14.0, -lift))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(CARD)
                    .stroke(Stroke::new(1.0_f32, INPUT_BORDER))
                    .corner_radius(CornerRadius::same(4))
                    .shadow(egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(140) })
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(400.0);
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 4.0, color);
                            ui.add(egui::Label::new(egui::RichText::new(&text).color(TEXT)).wrap());
                            if ui.add(egui::Button::new(egui::RichText::new("×").size(15.0).color(MUTED)).frame(false)).clicked() {
                                close = true;
                            }
                        });
                    });
            });
        if close {
            self.clear_notice();
        }
    }

    pub(super) fn mark_notice(&mut self) {
        self.notice_at = Instant::now();
    }
}

fn right_border(ui: &egui::Ui) {
    let rect = ui.max_rect();
    ui.painter().vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0_f32, BORDER));
}

fn activity_button(ui: &mut egui::Ui, activity: Activity, name: &str, active: bool, badge: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::click());
    let painter = ui.painter();
    let color = if active || response.hovered() { BRIGHT } else { Color32::from_rgb(133, 133, 133) };
    if active {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, ACCENT);
    }
    activity_icon(painter, rect.center(), activity, color);
    if badge {
        painter.circle_filled(pos2(rect.right() - 13.0, rect.bottom() - 13.0), 3.5, ACCENT);
    }
    response.on_hover_text(name)
}

fn activity_icon(painter: &egui::Painter, c: egui::Pos2, activity: Activity, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    let outline = |min: egui::Vec2, max: egui::Vec2, radius: u8| {
        painter.rect_stroke(Rect::from_min_max(c + min, c + max), radius, stroke, egui::StrokeKind::Middle);
    };
    match activity {
        Activity::Talk => {
            outline(vec2(-10.0, -8.0), vec2(10.0, 5.0), 3);
            painter.add(egui::Shape::line(
                vec![c + vec2(-5.0, 5.0), c + vec2(-7.0, 10.0), c + vec2(0.0, 5.0)],
                stroke,
            ));
        }
        Activity::Explorer => {
            outline(vec2(-8.0, -5.0), vec2(4.0, 10.0), 1);
            painter.add(egui::Shape::line(
                vec![c + vec2(-4.0, -5.0), c + vec2(-4.0, -10.0), c + vec2(8.0, -10.0), c + vec2(8.0, 6.0), c + vec2(4.0, 6.0)],
                stroke,
            ));
        }
        Activity::Bots => {
            outline(vec2(-9.0, -4.0), vec2(9.0, 9.0), 3);
            painter.circle_filled(c + vec2(-4.0, 2.5), 1.8, color);
            painter.circle_filled(c + vec2(4.0, 2.5), 1.8, color);
            painter.line_segment([c + vec2(0.0, -4.0), c + vec2(0.0, -8.0)], stroke);
            painter.circle_filled(c + vec2(0.0, -9.5), 1.8, color);
        }
        Activity::Providers => {
            outline(vec2(-7.0, -3.0), vec2(7.0, 5.0), 2);
            painter.line_segment([c + vec2(-3.5, -3.0), c + vec2(-3.5, -9.0)], stroke);
            painter.line_segment([c + vec2(3.5, -3.0), c + vec2(3.5, -9.0)], stroke);
            painter.line_segment([c + vec2(0.0, 5.0), c + vec2(0.0, 10.0)], stroke);
        }
    }
}

fn row_mark(painter: &egui::Painter, c: egui::Pos2, mark: Mark) {
    let stroke = Stroke::new(1.2_f32, MUTED);
    match mark {
        Mark::None => {}
        Mark::Folder => {
            painter.add(egui::Shape::line(
                vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)],
                stroke,
            ));
        }
        Mark::File => {
            painter.add(egui::Shape::line(
                vec![
                    c + vec2(-4.0, -5.5),
                    c + vec2(1.5, -5.5),
                    c + vec2(4.0, -3.0),
                    c + vec2(4.0, 5.5),
                    c + vec2(-4.0, 5.5),
                    c + vec2(-4.0, -5.5),
                ],
                stroke,
            ));
        }
    }
}

/// 侧栏顶部标题。有动作时右边显示「+」，返回是否点了它。
pub(super) fn side_title(ui: &mut egui::Ui, title: &str, action: Option<&str>) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.0),
        MUTED,
    );
    let Some(tip) = action else {
        return false;
    };
    let button = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(22.0, 22.0));
    plus_button(ui, button, ui.id().with(("side-title", title)), tip)
}

/// 可折叠的侧栏分区。有动作时标题右边显示「+」，返回是否点了它。
pub(super) fn section(
    ui: &mut egui::Ui,
    id: &str,
    title: &str,
    action: Option<&str>,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let id = ui.make_persistent_id(id);
    let mut open = ui.data_mut(|data| *data.get_persisted_mut_or(id, true));
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 0.0, HOVER);
    }
    let arrow = pos2(rect.left() + 12.0, rect.center().y);
    let points = if open {
        vec![arrow + vec2(-4.0, -2.0), arrow + vec2(4.0, -2.0), arrow + vec2(0.0, 3.0)]
    } else {
        vec![arrow + vec2(-2.0, -4.0), arrow + vec2(3.0, 0.0), arrow + vec2(-2.0, 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(points, TEXT, Stroke::NONE));
    ui.painter().text(
        pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(12.0),
        BRIGHT,
    );
    let mut clicked = false;
    if let Some(tip) = action {
        let button = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(20.0, 20.0));
        clicked = plus_button(ui, button, id.with("action"), tip);
    }
    if response.clicked() && !clicked {
        open = !open;
        ui.data_mut(|data| data.insert_persisted(id, open));
    }
    if open {
        add(ui);
        ui.add_space(6.0);
    }
    clicked
}

fn plus_button(ui: &mut egui::Ui, rect: Rect, id: egui::Id, tip: &str) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, INPUT_BORDER);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, "+", FontId::proportional(17.0), TEXT);
    response.on_hover_text(tip).clicked()
}

/// 侧栏里的一行：名字在前，说明用浅色跟在后面。
pub(super) fn list_item(ui: &mut egui::Ui, mark: Mark, label: &str, detail: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    row_body(ui, rect, &response, mark, label, detail, selected, 0.0);
    response
}

/// 带「×」的侧栏行。返回这一行和是否点了「×」。
pub(super) fn list_item_x(
    ui: &mut egui::Ui,
    mark: Mark,
    label: &str,
    detail: &str,
    selected: bool,
    tip: &str,
) -> (egui::Response, bool) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let close_rect = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(18.0, 18.0));
    let close = ui.interact(close_rect, response.id.with("x"), Sense::click());
    row_body(ui, rect, &response, mark, label, detail, selected || close.hovered(), 22.0);
    if response.hovered() || close.hovered() || selected {
        if close.hovered() {
            ui.painter().rect_filled(close_rect, 3.0, INPUT_BORDER);
        }
        ui.painter().text(close_rect.center(), Align2::CENTER_CENTER, "×", FontId::proportional(15.0), TEXT);
    }
    let removed = close.on_hover_text(tip).clicked();
    (response, removed)
}

#[allow(clippy::too_many_arguments)]
fn row_body(
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    mark: Mark,
    label: &str,
    detail: &str,
    selected: bool,
    reserve: f32,
) {
    let fill = if selected {
        SELECTED
    } else if response.hovered() {
        HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    let clip = Rect::from_min_max(rect.min, pos2(rect.right() - reserve - 6.0, rect.bottom()));
    let painter = ui.painter_at(clip);
    let left = match mark {
        Mark::None => rect.left() + 20.0,
        _ => {
            row_mark(&painter, pos2(rect.left() + 22.0, rect.center().y), mark);
            rect.left() + 34.0
        }
    };
    let name = painter.layout_no_wrap(label.to_string(), FontId::proportional(13.0), if selected { BRIGHT } else { TEXT });
    let name_width = name.size().x;
    let top = rect.center().y - name.size().y / 2.0;
    painter.galley(pos2(left, top), name, TEXT);
    if !detail.is_empty() {
        painter.text(
            pos2(left + 8.0 + name_width, rect.center().y),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(11.5),
            MUTED,
        );
    }
}

pub(super) fn side_hint(ui: &mut egui::Ui, text: &str) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: 20, right: 12, top: 2, bottom: 4 }).show(ui, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(MUTED)).wrap());
    });
}

/// 编辑区顶部的标签条。右边放当前页的操作。
pub(super) fn tab_strip(ui: &mut egui::Ui, tabs: &[Tab], current: View, actions: impl FnOnce(&mut egui::Ui)) -> Option<TabEvent> {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_HEIGHT), Sense::hover());
    ui.painter().rect_filled(bar, 0.0, SIDE_BG);
    ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0_f32, BORDER));
    let mut event = None;
    let builder = egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for item in tabs {
            if let Some(found) = tab(ui, item, item.view == current) {
                event = Some(found);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            actions(ui);
        });
    });
    event
}

fn tab(ui: &mut egui::Ui, item: &Tab, active: bool) -> Option<TabEvent> {
    let color = if active { BRIGHT } else { MUTED };
    let galley = ui.painter().layout_no_wrap(item.label.clone(), FontId::proportional(13.0), color);
    let marked = item.dirty || item.closable;
    let width = (galley.size().x + 24.0 + if marked { 22.0 } else { 0.0 }).min(260.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, if active { EDITOR_BG } else { SIDE_BG });
    if active {
        painter.hline(rect.x_range(), rect.top() + 1.0, Stroke::new(2.0_f32, ACCENT));
    }
    painter.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0_f32, BORDER));
    let text_clip = Rect::from_min_max(rect.min, pos2(rect.right() - if marked { 30.0 } else { 8.0 }, rect.bottom()));
    ui.painter_at(text_clip)
        .galley(pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0), galley, color);
    let mut closed = false;
    if marked {
        let mark = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(18.0, 18.0));
        let mut show_x = false;
        if item.closable {
            let close = ui.interact(mark, response.id.with("close"), Sense::click());
            show_x = !item.dirty || close.hovered() || response.hovered();
            if close.hovered() {
                painter.rect_filled(mark, 3.0, INPUT_BORDER);
            }
            closed = close.on_hover_text("关闭").clicked();
        }
        let (glyph, size) = if show_x { ("×", 15.0) } else { ("●", 10.0) };
        painter.text(mark.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size), color);
    }
    if closed {
        Some(TabEvent::Close(item.view))
    } else if response.clicked() {
        Some(TabEvent::Select(item.view))
    } else {
        None
    }
}

/// 标签条右边的文字操作。
pub(super) fn bar_action(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(text).size(12.5).color(TEXT)).frame(false)).clicked()
}

pub(super) fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let button = egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(ACCENT);
    ui.add_enabled(enabled, button).clicked()
}

pub(super) fn danger_button(ui: &mut egui::Ui, text: &str) -> bool {
    let button = egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(Color32::from_rgb(161, 38, 38));
    ui.add(button).clicked()
}

/// 编辑区里的表单页：可以滚动，内容靠左，限制最大宽度。
pub(super) fn page(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 32, right: 24, top: 20, bottom: 24 }).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(760.0));
                add(ui);
            });
        });
}

/// 表单里的小节标题，下面画一条分隔线。
pub(super) fn group_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(text).size(15.0).color(BRIGHT));
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, BORDER));
    ui.add_space(2.0);
}

pub(super) fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(MUTED)).wrap());
}

pub(super) fn tab_label(name: &str, fallback: &str) -> String {
    if name.trim().is_empty() {
        fallback.to_string()
    } else {
        name.to_string()
    }
}
