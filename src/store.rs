use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|err| format!("无法创建数据目录：{err}"))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn load_vec<T: DeserializeOwned>(&self, file: &str) -> Result<Vec<T>, String> {
        let path = self.rel(file)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&path).map_err(|err| format!("读取 {file} 失败：{err}"))?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&text).map_err(|err| format!("{file} 已损坏：{err}"))
    }

    pub fn save_vec<T: Serialize>(&self, file: &str, items: &[T]) -> Result<(), String> {
        let path = self.rel(file)?;
        let mut body = serde_json::to_vec_pretty(items).map_err(|err| format!("保存失败：{err}"))?;
        body.push(b'\n');
        write_atomic(&path, &body)
    }

    pub fn remove(&self, file: &str) -> Result<(), String> {
        let path = self.rel(file)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|err| format!("删除 {file} 失败：{err}"))?;
        }
        Ok(())
    }

    pub fn write_file(&self, file: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.rel(file)?;
        write_atomic(&path, bytes)
    }

    pub fn read_file(&self, file: &str) -> Result<Vec<u8>, String> {
        let path = self.rel(file)?;
        fs::read(&path).map_err(|err| format!("读取 {file} 失败：{err}"))
    }

    pub fn remove_tree(&self, dir: &str) -> Result<(), String> {
        let path = self.rel(dir)?;
        if path == self.root {
            return Err("非法路径".into());
        }
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|err| format!("删除目录失败：{err}"))?;
        }
        Ok(())
    }

    fn rel(&self, file: &str) -> Result<PathBuf, String> {
        if file.is_empty() || file.contains("..") || Path::new(file).is_absolute() {
            return Err("非法路径".into());
        }
        Ok(self.root.join(file))
    }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn is_safe_id(id: &str) -> bool {
    let len = id.len();
    (8..=64).contains(&len) && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("LOCALBOT_DATA") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let in_target = dir.components().any(|part| part.as_os_str() == "target");
            if !in_target {
                return dir.join("data");
            }
        }
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("LocalBot")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("无法创建目录：{err}"))?;
    }
    let mut tmp_name = path.as_os_str().to_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    fs::write(&tmp, bytes).map_err(|err| format!("写入失败：{err}"))?;
    if path.exists() {
        fs::remove_file(path).map_err(|err| format!("替换失败：{err}"))?;
    }
    fs::rename(&tmp, path).map_err(|err| format!("替换失败：{err}"))
}
