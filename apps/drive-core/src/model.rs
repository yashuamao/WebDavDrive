//! 连接模型与输入校验。
//!
//! 规则来自 `docs/requirements.md`：BR-2（remote 命名）、BR-3（id/URL 校验）、
//! AC-01..AC-03；字段与旧版 `profiles.json` 保持兼容（新增字段都用默认值）。

use foundation_core::{FoundationError, Result};
use foundation_secrets::SecretStore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const VFS_CACHE_MODES: [&str; 4] = ["off", "minimal", "writes", "full"];
pub const WEBDAV_VENDORS: [&str; 5] = ["other", "nextcloud", "owncloud", "sharepoint", "rclone"];

/// 一条连接（磁盘格式；`password_enc` 是可解密令牌，绝不出现在 UI DTO 里）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub name: String,
    pub remote: String,
    pub url: String,
    pub vendor: String,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_enc: Option<String>,
    pub drive: String,
    pub volname: String,
    pub network_mode: bool,
    pub vfs_cache_mode: String,
    pub dir_cache_time: String,
    pub read_only: bool,
    pub autostart: bool,
    pub extra_opts: String,
}

/// 面向 UI 的脱敏视图。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionView {
    pub id: String,
    pub name: String,
    pub remote: String,
    pub url: String,
    pub vendor: String,
    pub user: String,
    pub password_set: bool,
    pub drive: String,
    pub volname: String,
    pub network_mode: bool,
    pub vfs_cache_mode: String,
    pub dir_cache_time: String,
    pub read_only: bool,
    pub autostart: bool,
    pub extra_opts: String,
}

impl Connection {
    pub fn view(&self) -> ConnectionView {
        ConnectionView {
            id: self.id.clone(),
            name: self.name.clone(),
            remote: self.remote.clone(),
            url: self.url.clone(),
            vendor: self.vendor.clone(),
            user: self.user.clone(),
            password_set: self.password_enc.is_some(),
            drive: self.drive.clone(),
            volname: self.volname.clone(),
            network_mode: self.network_mode,
            vfs_cache_mode: self.vfs_cache_mode.clone(),
            dir_cache_time: self.dir_cache_time.clone(),
            read_only: self.read_only,
            autostart: self.autostart,
            extra_opts: self.extra_opts.clone(),
        }
    }
}

/// 新建/更新输入。缺省字段表示"保持原值"（`None` 与空串语义不同）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ConnectionInput {
    pub id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub vendor: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub clear_password: bool,
    pub drive: Option<String>,
    pub volname: Option<String>,
    pub network_mode: Option<bool>,
    pub vfs_cache_mode: Option<String>,
    pub dir_cache_time: Option<String>,
    pub read_only: Option<bool>,
    pub autostart: Option<bool>,
    pub extra_opts: Option<String>,
}

impl Connection {
    /// 由输入与现有连接合成一条新记录。
    pub fn from_input(
        input: ConnectionInput,
        existing: Option<&Connection>,
        secrets: &dyn SecretStore,
    ) -> Result<Self> {
        if input.clear_password
            && input
                .password
                .as_deref()
                .is_some_and(|password| !password.is_empty())
        {
            return Err(FoundationError::InvalidInput(
                "不能同时设置新密码和清除已保存密码".into(),
            ));
        }
        let id = input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(generate_id);
        if !is_valid_id(&id) {
            return Err(FoundationError::InvalidInput(format!(
                "非法的配置 id：{id:?}（只允许字母、数字、下划线和连字符）"
            )));
        }

        let name = pick(input.name.clone(), existing.map(|c| &c.name), "WebDAV")
            .trim()
            .to_string();
        let url = pick(input.url.clone(), existing.map(|c| &c.url), "")
            .trim()
            .to_string();
        if url.is_empty() {
            return Err(FoundationError::InvalidInput("WebDAV 地址不能为空".into()));
        }
        let lower = url.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(FoundationError::InvalidInput(
                "WebDAV 地址必须以 http:// 或 https:// 开头".into(),
            ));
        }

        let password_enc = match input.password.as_deref() {
            Some(password) if !password.is_empty() => Some(secrets.protect(password)?),
            _ if input.clear_password => None,
            _ => existing.and_then(|c| c.password_enc.clone()),
        };

        let drive = pick(input.drive.clone(), existing.map(|c| &c.drive), "X:");
        let drive = if is_drive_letter(&drive) {
            drive.trim().to_ascii_uppercase()
        } else {
            drive.trim().to_string()
        };

        let remote = existing
            .map(|c| c.remote.clone())
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| slugify_remote(&name, &id));

        Ok(Self {
            id: id.clone(),
            name: name.clone(),
            remote,
            url,
            vendor: pick(input.vendor.clone(), existing.map(|c| &c.vendor), "other")
                .trim()
                .to_string(),
            user: pick(input.user.clone(), existing.map(|c| &c.user), ""),
            password_enc,
            drive,
            volname: pick(input.volname.clone(), existing.map(|c| &c.volname), &name),
            network_mode: input
                .network_mode
                .or_else(|| existing.map(|c| c.network_mode))
                .unwrap_or(false),
            vfs_cache_mode: pick(
                input.vfs_cache_mode.clone(),
                existing.map(|c| &c.vfs_cache_mode),
                "writes",
            ),
            dir_cache_time: pick(
                input.dir_cache_time.clone(),
                existing.map(|c| &c.dir_cache_time),
                "5m",
            ),
            read_only: input
                .read_only
                .or_else(|| existing.map(|c| c.read_only))
                .unwrap_or(false),
            autostart: input
                .autostart
                .or_else(|| existing.map(|c| c.autostart))
                .unwrap_or(false),
            extra_opts: pick(
                input.extra_opts.clone(),
                existing.map(|c| &c.extra_opts),
                "",
            )
            .trim()
            .to_string(),
        })
    }
}

fn pick(input: Option<String>, existing: Option<&String>, fallback: &str) -> String {
    input
        .filter(|s| !s.is_empty())
        .or_else(|| existing.cloned())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn generate_id() -> String {
    let hex = Uuid::new_v4().simple().to_string();
    format!("webdav-{}", &hex[..8])
}

/// BR-3：id 会进 URL 与 HTML 属性，只允许安全字符。
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_drive_letter(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// BR-2：remote 名由名称 slug + id 后缀组成，只含 `[A-Za-z0-9_]`。
pub fn slugify_remote(name: &str, id: &str) -> String {
    let mut base: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    base.truncate(24);
    let base = base.trim_matches('_').to_string();
    let suffix: String = id
        .rsplit('-')
        .next()
        .unwrap_or(id)
        .chars()
        .take(8)
        .collect();
    format!(
        "{}_{}",
        if base.is_empty() { "webdav" } else { &base },
        suffix
    )
}
