use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use foundation_core::{FoundationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 当前信封版本。新增不兼容字段时递增，并在应用层写迁移。
pub const SCHEMA_VERSION: u32 = 1;

/// 磁盘上的信封。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub data: Value,
}

/// 读取结果。损坏不是 Err：旧版行为是"隔离后以空配置继续"。
#[derive(Debug, Clone, PartialEq)]
pub enum LoadOutcome {
    /// 文件不存在（首次运行）。
    Missing,
    /// 读到了合法信封；`version` 可能比当前程序新，调用方必须自行拒绝写入。
    Loaded(Envelope),
    /// 文件损坏，已隔离到 `backup`，本次按空配置继续。
    Quarantined { backup: PathBuf, error: String },
}

/// 单个 JSON 文件的读写器；线程安全由调用方保证（配置写入频率低）。
#[derive(Debug, Clone)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn backup_path(&self) -> PathBuf {
        with_suffix(&self.path, ".bak")
    }

    pub fn tmp_path(&self) -> PathBuf {
        with_suffix(&self.path, ".tmp")
    }

    /// 读取并校验信封；损坏时隔离原文件。
    pub fn load(&self) -> Result<LoadOutcome> {
        if !self.path.exists() {
            return Ok(LoadOutcome::Missing);
        }
        let raw = fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            // 空文件按缺失处理（旧版新建 rclone.conf 也是 0 字节）
            return Ok(LoadOutcome::Missing);
        }

        let parsed: std::result::Result<Value, _> = serde_json::from_str(&raw);
        let value = match parsed {
            Ok(v) => v,
            Err(err) => return self.quarantine(&err.to_string()),
        };

        let Some(obj) = value.as_object() else {
            return self.quarantine("顶层不是 JSON 对象");
        };
        let Some(version) = obj.get("version").and_then(Value::as_u64) else {
            return self.quarantine("缺少 version 字段");
        };
        let Some(data) = obj.get("data") else {
            return self.quarantine("缺少 data 字段");
        };

        Ok(LoadOutcome::Loaded(Envelope {
            version: version as u32,
            data: data.clone(),
        }))
    }

    /// 写入数据并维护备份。版本固定为 SCHEMA_VERSION。
    pub fn save(&self, data: &Value) -> Result<()> {
        self.save_with_version(SCHEMA_VERSION, data)
    }

    /// 仅在明确做迁移时使用：把旧数据升级后按目标版本写回。
    pub fn save_with_version(&self, version: u32, data: &Value) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        // 备份上一版（best-effort：备份失败只告警，不阻塞保存）
        if self.path.exists() {
            if let Err(err) = fs::copy(&self.path, self.backup_path()) {
                log::warn!("配置文件备份失败 {}: {err}", self.backup_path().display());
            }
        }

        let envelope = Envelope {
            version,
            data: data.clone(),
        };
        let tmp = self.tmp_path();
        {
            let mut file = fs::File::create(&tmp)?;
            serde_json::to_writer_pretty(&mut file, &envelope)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        // Windows 上 fs::rename 使用 MoveFileEx(REPLACE_EXISTING)，可覆盖已有文件
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 把损坏文件改名隔离，返回备份路径。
    pub fn quarantine_path(&self) -> PathBuf {
        let stamp = chrono_stamp();
        with_suffix(&self.path, &format!(".corrupt-{stamp}"))
    }

    fn quarantine(&self, reason: &str) -> Result<LoadOutcome> {
        let backup = self.quarantine_path();
        match fs::rename(&self.path, &backup) {
            Ok(()) => {
                log::error!(
                    "{} 解析失败（{reason}），已隔离到 {}；本次以空配置启动",
                    self.path.display(),
                    backup.display()
                );
                Ok(LoadOutcome::Quarantined {
                    backup,
                    error: reason.to_string(),
                })
            }
            Err(err) => Err(FoundationError::DataCorrupted(format!(
                "{} 解析失败（{reason}）且隔离失败：{err}",
                self.path.display()
            ))),
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

fn chrono_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}
