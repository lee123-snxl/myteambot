use super::shell::{list_item, list_item_x, note, section, side_hint, side_title, Mark};
use super::{join_rel, parent_rel, size_label, App, Panel};

impl App {
    /// 当前对话或群组保存的文件夹。
    pub(super) fn explorer_root(&self) -> Option<String> {
        match self.panel {
            Panel::Chat if !self.chat_id.is_empty() => self.chat_folder(),
            Panel::Group if !self.group_id.is_empty() && !self.group_folder_base.trim().is_empty() => {
                Some(self.group_folder_base.clone())
            }
            _ => None,
        }
    }

    pub(super) fn in_group(&self) -> bool {
        self.panel == Panel::Group && !self.group_id.is_empty()
    }

    pub(super) fn explorer_side(&mut self, ui: &mut egui::Ui) {
        side_title(ui, "资源管理器", None);
        let root = self.explorer_root();
        let in_group = self.in_group();
        if root.is_none() && !in_group {
            side_hint(ui, "打开一个设了文件夹的单聊，或者打开群组，这里会列出文件。");
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(root) = &root {
                self.folder_section(ui, root);
                self.write_section(ui, root);
            }
            if in_group {
                self.attach_section(ui);
            }
        });
    }

    fn folder_section(&mut self, ui: &mut egui::Ui, root: &str) {
        let dir = self.browse_dir.clone();
        let entries = self.browse_entries.clone();
        let truncated = self.browse_truncated;
        let error = self.browse_error.clone();
        let current = self.browse_file.clone();
        let title = folder_title(root);
        let mut up = false;
        let mut open_dir = None;
        let mut open_file = None;
        section(ui, "explorer-folder", &title, None, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 20, right: 10, top: 0, bottom: 2 }).show(ui, |ui| {
                note(ui, root);
                if !dir.is_empty() {
                    note(ui, &format!("当前：{dir}"));
                }
                if !error.is_empty() {
                    ui.add(egui::Label::new(egui::RichText::new(&error).size(12.0).color(super::shell::DANGER)).wrap());
                }
            });
            if !dir.is_empty() && list_item(ui, Mark::Folder, "..", "上级", false).clicked() {
                up = true;
            }
            if entries.is_empty() && error.is_empty() {
                side_hint(ui, "这个文件夹是空的");
            }
            for entry in &entries {
                let path = join_rel(&dir, &entry.name);
                let (mark, detail) = if entry.dir {
                    (Mark::Folder, String::new())
                } else {
                    (Mark::File, size_label(entry.size))
                };
                if list_item(ui, mark, &entry.name, &detail, !entry.dir && current == path).clicked() {
                    if entry.dir {
                        open_dir = Some(entry.name.clone());
                    } else {
                        open_file = Some(path);
                    }
                }
            }
            if truncated {
                side_hint(ui, "还有更多，未列出");
            }
        });
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
    }

    fn write_section(&mut self, ui: &mut egui::Ui, root: &str) {
        let mut write = false;
        let mut import = false;
        section(ui, "explorer-write", "写入文件", None, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin { left: 20, right: 12, top: 2, bottom: 2 }).show(ui, |ui| {
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
                note(ui, "写在当前打开的那一层。对话时 Bot 也会读取和写入这里");
            });
        });
        if write {
            self.write_browse(root);
        }
        if import {
            self.import_browse(root);
        }
    }

    fn attach_section(&mut self, ui: &mut egui::Ui) {
        let files = self.group_files.clone();
        let current = self.preview_id.clone();
        let mut open = None;
        let mut remove = None;
        let add = section(ui, "explorer-attach", "群组文件", Some("放入文件"), |ui| {
            side_hint(ui, "只保存在这台电脑上，不会发给模型");
            if files.is_empty() {
                side_hint(ui, "还没有文件，点标题右边的「+」放入");
            }
            for file in &files {
                let (row, removed) = list_item_x(ui, Mark::File, &file.name, &size_label(file.size), current == file.id, "移除");
                if removed {
                    remove = Some(file.id.clone());
                } else if row.clicked() {
                    open = Some(file.id.clone());
                }
            }
        });
        if add {
            self.import_group_file();
        }
        if let Some(id) = remove {
            self.remove_group_file(&id);
        }
        if let Some(id) = open {
            self.open_group_file(&id);
        }
    }
}

fn folder_title(root: &str) -> String {
    let trimmed = root.trim_end_matches(['\\', '/']);
    trimmed
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}
