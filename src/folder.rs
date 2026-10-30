use std::collections::{HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_LIST: usize = 200;
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CHARS: usize = 100_000;
const MAX_ROOT_CHARS: usize = 1000;
const MAX_REL_CHARS: usize = 300;
const MAX_NAME_CHARS: usize = 200;
const MAX_TREE: usize = 200;
const MAX_DEPTH: u8 = 4;
const SNAPSHOT_FILES: usize = 40;
const SNAPSHOT_CHARS: usize = 60_000;
const SNAPSHOT_FILE_BYTES: u64 = 64 * 1024;
pub const MAX_PASSES: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Text { body: String, truncated: bool },
    Binary,
}

pub fn clean_root(raw: &str) -> Result<String, String> {
    let raw = trim_root(raw);
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw.chars().count() > MAX_ROOT_CHARS {
        return Err("文件夹路径过长".into());
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err("请填写文件夹的绝对路径".into());
    }
    if !path.is_dir() {
        return Err("请选择一个已有的文件夹".into());
    }
    Ok(raw.to_string())
}

pub fn list(root: &str, dir: &str) -> Result<Listing, String> {
    let root_path = open_root(root)?;
    let folder = open_dir(&root_path, dir)?;
    let read = fs::read_dir(&folder).map_err(|err| format!("无法读取文件夹：{err}"))?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.is_empty() || name == "." || name == ".." || name.chars().any(|item| item.is_control()) {
            continue;
        }
        let path = entry.path();
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        if !canon.starts_with(&root_path) {
            continue;
        }
        if canon.is_dir() {
            entries.push(Entry { name, dir: true, size: 0 });
        } else if canon.is_file() {
            let size = fs::metadata(&canon).map(|item| item.len()).unwrap_or(0);
            entries.push(Entry { name, dir: false, size });
        }
    }
    entries.sort_by(|left, right| right.dir.cmp(&left.dir).then_with(|| left.name.cmp(&right.name)));
    let truncated = entries.len() > MAX_LIST;
    entries.truncate(MAX_LIST);
    Ok(Listing { entries, truncated })
}

pub fn read(root: &str, path: &str) -> Result<Preview, String> {
    let root_path = open_root(root)?;
    let parts = parts_of(path)?;
    let Some((name, parents)) = parts.split_last() else {
        return Err("请选择一个文件".into());
    };
    let folder = open_dir_parts(&root_path, parents)?;
    let target = folder.join(name);
    if !target.exists() {
        return Err("文件不存在".into());
    }
    let canon = target.canonicalize().map_err(|err| format!("无法打开：{err}"))?;
    if !canon.starts_with(&root_path) || canon == root_path {
        return Err("路径不能超出文件夹".into());
    }
    if canon.is_dir() {
        return Err("这是文件夹，不能读取".into());
    }
    let meta = fs::metadata(&canon).map_err(|err| format!("无法读取文件：{err}"))?;
    if meta.len() > MAX_BYTES {
        return Err("文件不能超过 8MB".into());
    }
    let bytes = fs::read(&canon).map_err(|err| format!("无法读取文件：{err}"))?;
    preview_of(bytes)
}

pub fn write(root: &str, dir: &str, name: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("文件不能超过 8MB".into());
    }
    let name = file_name(name)?;
    let root_path = open_root(root)?;
    let mut parts = parts_of(dir)?;
    let folder = open_dir_parts(&root_path, &parts)?;
    let target = folder.join(&name);
    if target.exists() {
        let canon = target.canonicalize().map_err(|err| format!("无法打开：{err}"))?;
        if !canon.starts_with(&root_path) || canon == root_path {
            return Err("路径不能超出文件夹".into());
        }
        if canon.is_dir() {
            return Err("这是文件夹，不能写入".into());
        }
    }
    write_bytes(&root_path, &target, bytes)?;
    parts.push(name);
    Ok(parts.join("/"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Round {
    Done(String),
    Continue(String),
}

pub fn compose(prompt: &str, root: &str) -> String {
    let root = root.trim();
    if root.is_empty() {
        return prompt.trim().to_string();
    }
    let body = match snapshot(root) {
        Ok(text) => text,
        Err(err) => format!("无法读取这个文件夹：{err}"),
    };
    let guide = format!(
        "你可以读取和写入文件夹：{root}\n{body}\n需要其中还没给出的文件时，单独一行写：读取 相对路径\n写入或覆盖文件时，标记各占一行：\n写入 相对路径\n<<<\n文件内容\n>>>\n回答用户时不要留下这些标记。名为 target 或 .git 的子文件夹只列出名字，不展开。"
    );
    let prompt = prompt.trim();
    if prompt.is_empty() {
        guide
    } else {
        format!("{prompt}\n\n{guide}")
    }
}

pub fn round(root: &str, reply: &str) -> Result<Round, String> {
    let parsed = parse_reply(reply);
    if parsed.actions.is_empty() {
        return Ok(Round::Done(parsed.visible.trim().to_string()));
    }
    let mut notes = Vec::new();
    let mut wrote = Vec::new();
    let mut need_more = false;
    for action in parsed.actions {
        match action {
            ReplyAction::Read(path) => {
                need_more = true;
                notes.push(read_note(root, &path));
            }
            ReplyAction::Write { path, body } => match write_rel(root, &path, body.as_bytes()) {
                Ok(saved) => wrote.push(saved),
                Err(err) => {
                    need_more = true;
                    notes.push(format!("无法写入 {path}：{err}"));
                }
            },
            ReplyAction::Bad(err) => {
                need_more = true;
                notes.push(err);
            }
        }
    }
    if !wrote.is_empty() {
        notes.insert(0, format!("已写入 {}", wrote.join("、")));
    }
    if need_more {
        return Ok(Round::Continue(notes.join("\n\n")));
    }
    let visible = parsed.visible.trim();
    if !visible.is_empty() {
        return Ok(Round::Done(visible.to_string()));
    }
    Ok(Round::Done(notes.join("\n\n")))
}

pub fn write_rel(root: &str, rel: &str, bytes: &[u8]) -> Result<String, String> {
    let parts = parts_of(rel)?;
    let Some((name, parents)) = parts.split_last() else {
        return Err("请填写文件名".into());
    };
    let root_path = open_root(root)?;
    ensure_dirs(&root_path, parents)?;
    write(root, &parents.join("/"), name, bytes)
}

fn preview_of(bytes: Vec<u8>) -> Result<Preview, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("文件不能超过 8MB".into());
    }
    if bytes.contains(&0) {
        return Ok(Preview::Binary);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Preview::Binary);
    };
    if text.chars().count() > MAX_CHARS {
        let body = text.chars().take(MAX_CHARS).collect();
        return Ok(Preview::Text { body, truncated: true });
    }
    Ok(Preview::Text { body: text, truncated: false })
}

fn open_root(root: &str) -> Result<PathBuf, String> {
    let raw = root.trim();
    let path = Path::new(raw);
    if raw.is_empty() || !path.is_absolute() || !path.is_dir() {
        return Err("文件夹不存在".into());
    }
    path.canonicalize().map_err(|err| format!("无法打开文件夹：{err}"))
}

fn open_dir(root: &Path, dir: &str) -> Result<PathBuf, String> {
    let parts = parts_of(dir)?;
    open_dir_parts(root, &parts)
}

fn open_dir_parts(root: &Path, parts: &[String]) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    for name in parts {
        current.push(name);
        if !current.exists() {
            return Err("文件夹不存在".into());
        }
        let canon = current.canonicalize().map_err(|err| format!("无法打开：{err}"))?;
        if !canon.starts_with(root) {
            return Err("路径不能超出文件夹".into());
        }
        if !canon.is_dir() {
            return Err("这不是文件夹".into());
        }
        current = canon;
    }
    Ok(current)
}

fn parts_of(rel: &str) -> Result<Vec<String>, String> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Ok(Vec::new());
    }
    if rel.chars().count() > MAX_REL_CHARS {
        return Err("路径过长".into());
    }
    if rel.starts_with(['/', '\\']) || rel.contains(':') {
        return Err("路径不能超出文件夹".into());
    }
    let mut parts = Vec::new();
    for part in rel.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.chars().any(|item| item.is_control()) {
            return Err("路径不能超出文件夹".into());
        }
        parts.push(part.to_string());
    }
    Ok(parts)
}

fn file_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请填写文件名".into());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err("文件名过长".into());
    }
    if name == "." || name == ".." || name.contains(['/', '\\', ':']) || name.chars().any(|item| item.is_control()) {
        return Err("文件名不能包含路径".into());
    }
    Ok(name.to_string())
}

fn write_bytes(root: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "路径无效".to_string())?;
    let name = path.file_name().ok_or_else(|| "路径无效".to_string())?;
    let mut tmp_name = OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".localbot.tmp");
    let tmp = parent.join(tmp_name);
    if fs::symlink_metadata(&tmp).is_ok() {
        if let Ok(canon) = tmp.canonicalize() {
            if !canon.starts_with(root) || canon == root {
                return Err("路径不能超出文件夹".into());
            }
        }
        fs::remove_file(&tmp).map_err(|err| format!("写入失败：{err}"))?;
    }
    fs::write(&tmp, bytes).map_err(|err| format!("写入失败：{err}"))?;
    if path.exists() {
        if path.is_dir() {
            let _ = fs::remove_file(&tmp);
            return Err("这是文件夹，不能写入".into());
        }
        if let Ok(canon) = path.canonicalize() {
            if !canon.starts_with(root) || canon == root {
                let _ = fs::remove_file(&tmp);
                return Err("路径不能超出文件夹".into());
            }
        }
        if let Err(err) = fs::remove_file(path) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("写入失败：{err}"));
        }
    }
    fs::rename(&tmp, path).map_err(|err| {
        let _ = fs::remove_file(&tmp);
        format!("写入失败：{err}")
    })
}

enum ReplyAction {
    Read(String),
    Write { path: String, body: String },
    Bad(String),
}

struct ParsedReply {
    visible: String,
    actions: Vec<ReplyAction>,
}

fn snapshot(root: &str) -> Result<String, String> {
    let root_path = open_root(root)?;
    let mut seen = HashSet::new();
    seen.insert(root_path);
    let mut tree = Vec::new();
    let mut files = Vec::new();
    let mut budget = SNAPSHOT_CHARS;
    let mut queue = VecDeque::from([(String::new(), 0u8)]);
    let mut truncated = false;
    while let Some((dir, depth)) = queue.pop_front() {
        if seen.len() > 500 {
            truncated = true;
            break;
        }
        let listing = if dir.is_empty() {
            list(root, &dir)?
        } else {
            match list(root, &dir) {
                Ok(listing) => listing,
                Err(_) => continue,
            }
        };
        if listing.truncated {
            truncated = true;
        }
        for entry in listing.entries {
            if tree.len() >= MAX_TREE {
                truncated = true;
                break;
            }
            let rel = join_rel(&dir, &entry.name);
            if entry.dir {
                tree.push(format!("{rel}/"));
                if depth + 1 < MAX_DEPTH && !hidden_dir(&entry.name) {
                    if let Ok(path) = open_dir(&open_root(root)?, &rel) {
                        if seen.insert(path) {
                            queue.push_back((rel, depth + 1));
                        }
                    }
                }
            } else {
                tree.push(rel.clone());
                take_text(root, &rel, entry.size, &mut files, &mut budget);
            }
        }
    }
    if truncated {
        tree.push("（还有更多，未列出）".into());
    }
    Ok(format_snapshot(&tree, &files))
}

fn hidden_dir(name: &str) -> bool {
    name == "target" || name == ".git"
}

fn take_text(root: &str, rel: &str, size: u64, files: &mut Vec<(String, String)>, budget: &mut usize) {
    if files.len() >= SNAPSHOT_FILES || *budget == 0 || size > SNAPSHOT_FILE_BYTES {
        return;
    }
    let Ok(Preview::Text { body, truncated }) = read(root, rel) else {
        return;
    };
    let take: String = body.chars().take(*budget).collect();
    let cut = truncated || take.chars().count() < body.chars().count();
    *budget -= take.chars().count();
    let body = if cut {
        format!("{take}\n（未完整读取）")
    } else {
        take
    };
    files.push((rel.to_string(), body));
}

fn format_snapshot(tree: &[String], files: &[(String, String)]) -> String {
    let mut out = String::from("目录：\n");
    if tree.is_empty() {
        out.push_str("（空文件夹）\n");
    } else {
        for line in tree {
            out.push_str(line);
            out.push('\n');
        }
    }
    for (path, body) in files {
        out.push_str("\n文件 ");
        out.push_str(path);
        out.push_str("：\n");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn join_rel(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn read_note(root: &str, path: &str) -> String {
    match read(root, path) {
        Ok(Preview::Binary) => format!("{path} 不是文本文件"),
        Ok(Preview::Text { body, truncated }) => {
            if truncated {
                format!("文件 {path}：\n{body}\n（只读取前 10 万字）")
            } else {
                format!("文件 {path}：\n{body}")
            }
        }
        Err(err) => format!("无法读取 {path}：{err}"),
    }
}

fn parse_reply(reply: &str) -> ParsedReply {
    let mut visible = String::new();
    let mut actions = Vec::new();
    let lines: Vec<&str> = reply.split('\n').collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_end_matches('\r');
        if let Some(path) = command(line, "读取") {
            actions.push(ReplyAction::Read(path.to_string()));
            index += 1;
            continue;
        }
        if let Some(path) = command(line, "写入") {
            if index + 1 >= lines.len() || lines[index + 1].trim_end_matches('\r').trim() != "<<<" {
                actions.push(ReplyAction::Bad(format!("写入 {path} 缺少开始标记 <<<")));
                index += 1;
                continue;
            }
            index += 2;
            let mut body = String::new();
            let mut closed = false;
            while index < lines.len() {
                let row = lines[index].trim_end_matches('\r');
                if row.trim() == ">>>" {
                    closed = true;
                    index += 1;
                    break;
                }
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(row);
                index += 1;
            }
            if closed {
                actions.push(ReplyAction::Write { path: path.to_string(), body });
            } else {
                actions.push(ReplyAction::Bad(format!("写入 {path} 缺少结束标记 >>>")));
            }
            continue;
        }
        if !visible.is_empty() {
            visible.push('\n');
        }
        visible.push_str(line);
        index += 1;
    }
    ParsedReply { visible, actions }
}

fn command<'a>(line: &'a str, verb: &str) -> Option<&'a str> {
    let line = line.trim();
    let rest = line.strip_prefix(verb)?;
    let mut chars = rest.chars();
    let first = chars.next()?;
    if first != ' ' && first != '\t' && first != ':' && first != '：' {
        return None;
    }
    let path = rest.trim_start_matches([' ', '\t', ':', '：']).trim();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

fn ensure_dirs(root: &Path, parts: &[String]) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for name in parts {
        current.push(name);
        if !current.exists() {
            fs::create_dir(&current).map_err(|err| format!("无法创建文件夹：{err}"))?;
        }
        let canon = current.canonicalize().map_err(|err| format!("无法打开：{err}"))?;
        if !canon.starts_with(root) || !canon.is_dir() {
            return Err("路径不能超出文件夹".into());
        }
        current = canon;
    }
    Ok(())
}

fn trim_root(raw: &str) -> &str {
    let trimmed = raw.trim();
    let stripped = trimmed.trim_end_matches(['/', '\\']);
    if stripped.is_empty() || stripped.ends_with(':') {
        trimmed
    } else {
        stripped
    }
}
