//! 连接配置存储。
//!
//! 在 `foundation-config`（原子写/备份/损坏隔离）之上叠加连接语义：
//! - 旧版 Python `profiles.json`（`{"version":1,"profiles":[...]}`）自动迁移到新信封；
//! - 保存密码经 `SecretStore` 保护，明文不落盘（AC-04）；
//! - 解密失败必须报错（AC-06），绝不用空值覆盖。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use foundation_config::{FileStore, LoadOutcome, SCHEMA_VERSION};
use foundation_core::{FoundationError, Result};
use foundation_secrets::SecretStore;
use serde_json::{json, Value};

use crate::model::{Connection, ConnectionInput};

pub struct ProfileStore {
    file: FileStore,
    secrets: Arc<dyn SecretStore>,
    profiles: Mutex<Vec<Connection>>,
}

impl ProfileStore {
    pub fn open(data_dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let path = data_dir.join("profiles.json");
        let file = FileStore::new(&path);

        let profiles = if let Some(legacy) = read_legacy(&path)? {
            // 旧格式：先按新信封写回（save 会自动生成 .bak 保留原文件）
            log::info!("检测到旧版 profiles.json，迁移到新格式（原文件保留为 .bak）");
            file.save(&json!({ "profiles": legacy }))?;
            legacy
        } else {
            match file.load()? {
                LoadOutcome::Missing => Vec::new(),
                LoadOutcome::Loaded(envelope) => {
                    if envelope.version > SCHEMA_VERSION {
                        return Err(FoundationError::UnsupportedVersion {
                            found: envelope.version,
                            expected: SCHEMA_VERSION,
                        });
                    }
                    decode_profiles(&envelope.data)?
                }
                LoadOutcome::Quarantined { backup, error } => {
                    log::error!(
                        "profiles.json 解析失败（{error}），已隔离到 {}；本次以空配置启动",
                        backup.display()
                    );
                    Vec::new()
                }
            }
        };

        Ok(Self {
            file,
            secrets,
            profiles: Mutex::new(profiles),
        })
    }

    pub fn path(&self) -> &Path {
        self.file.path()
    }

    pub fn secrets_name(&self) -> &'static str {
        self.secrets.name()
    }

    pub fn list(&self) -> Vec<Connection> {
        self.with_profiles(|profiles| profiles.to_vec())
    }

    pub fn get(&self, id: &str) -> Option<Connection> {
        self.with_profiles(|profiles| profiles.iter().find(|c| c.id == id).cloned())
    }

    pub fn by_remote(&self, remote: &str) -> Option<Connection> {
        self.with_profiles(|profiles| profiles.iter().find(|c| c.remote == remote).cloned())
    }

    /// 新建或更新；空密码保留原值（AC-05）。
    pub fn upsert(&self, input: ConnectionInput) -> Result<Connection> {
        let existing = input.id.as_deref().and_then(|id| self.get(id.trim()));
        let connection = Connection::from_input(input, existing.as_ref(), self.secrets.as_ref())?;

        self.with_profiles_mut(|profiles| {
            profiles.retain(|c| c.id != connection.id);
            profiles.push(connection.clone());
            profiles.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.persist(profiles)?;
            Ok(connection.clone())
        })
    }

    pub fn delete(&self, id: &str) -> Result<Option<Connection>> {
        self.with_profiles_mut(|profiles| {
            let removed = profiles
                .iter()
                .position(|c| c.id == id)
                .map(|i| profiles.remove(i));
            if removed.is_some() {
                self.persist(profiles)?;
            }
            Ok(removed)
        })
    }

    /// 解密连接密码。空令牌返回空串；令牌存在但解不开必须报错（AC-06）。
    pub fn reveal_password(&self, connection: &Connection) -> Result<String> {
        match &connection.password_enc {
            None => Ok(String::new()),
            Some(token) => self.secrets.unprotect(token).map_err(|err| {
                FoundationError::SecretDecrypt(format!(
                    "无法解密「{}」保存的密码（{err}）；请重新编辑该连接并输入密码",
                    connection.name
                ))
            }),
        }
    }

    fn persist(&self, profiles: &[Connection]) -> Result<()> {
        self.file.save(&json!({ "profiles": profiles }))
    }

    fn with_profiles<T>(&self, f: impl FnOnce(&[Connection]) -> T) -> T {
        let guard = self.profiles.lock().unwrap_or_else(|p| p.into_inner());
        f(&guard)
    }

    fn with_profiles_mut<T>(&self, f: impl FnOnce(&mut Vec<Connection>) -> Result<T>) -> Result<T> {
        let mut guard = self.profiles.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }
}

fn decode_profiles(data: &Value) -> Result<Vec<Connection>> {
    let list = data.get("profiles").cloned().unwrap_or_else(|| json!([]));
    serde_json::from_value(list).map_err(|err| {
        FoundationError::DataCorrupted(format!("profiles 字段不是合法的连接列表：{err}"))
    })
}

/// 读取旧版 Python 格式：对象含 `profiles` 且不含新信封的 `data`。
fn read_legacy(path: &PathBuf) -> Result<Option<Vec<Connection>>> {
    if !path.exists() {
        return Ok(None);
    }
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return Ok(None); // 交给 FileStore 走损坏隔离
    };
    let is_legacy = value.get("data").is_none() && value.get("profiles").is_some();
    if !is_legacy {
        return Ok(None);
    }
    let list = value.get("profiles").cloned().unwrap_or_else(|| json!([]));
    let mut profiles: Vec<Connection> = serde_json::from_value(list).map_err(|err| {
        FoundationError::DataCorrupted(format!("旧版 profiles.json 结构无法识别：{err}"))
    })?;
    for profile in &mut profiles {
        if profile.remote.is_empty() {
            profile.remote = crate::model::slugify_remote(&profile.name, &profile.id);
        }
    }
    Ok(Some(profiles))
}
