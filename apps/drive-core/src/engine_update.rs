//! rclone 引擎独立更新的逻辑与接线（可单测）。
//!
//! - 纯判定：择版、SHA256SUMS 解析、每日限流、版本比较、镜像前缀拼接、GitHub 响应解析；
//! - 状态：%PROGRAMDATA%\WebDavDrive\engine-update.json（last_check_at / latest_version /
//!   etag / installed_version / last_error / mirror_prefix）的读写；
//! - 网络：EngineUpdater 只依赖可注入的 UpdateTransport，真实实现是系统 WinHTTP；
//! - 安装编排（无挂载门禁 → 停引擎 → swap → 重启 → 校验 → 回滚）在 service。
//! 单独做引擎更新是因为 RC/vfs 语义与 rclone 版本强耦合。

/// 自动检查间隔：每天最多一次（手动检查不走这个判定）。
pub const AUTO_CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;

/// 现在是否应该自动检查。
pub fn should_auto_check(last_check_unix: Option<i64>, now_unix: i64) -> bool {
    match last_check_unix {
        Some(last) => now_unix.saturating_sub(last) >= AUTO_CHECK_INTERVAL_SECS,
        None => true,
    }
}

/// 从 release 资产名里挑 Windows x64 引擎包。
pub fn pick_windows_amd64_asset<'a, I>(names: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    names
        .into_iter()
        .find(|name| name.starts_with("rclone-") && name.ends_with("-windows-amd64.zip"))
        .map(str::to_string)
}

/// 从 SHA256SUMS 文本里取出某个文件名的 sha256（小写）。
pub fn sha256_for(text: &str, file_name: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        if name == file_name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// 去掉 `v` 前缀。
pub fn normalize_version(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

/// 按数字段比较（1.10 > 1.9）。
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn parse(value: &str) -> Vec<u64> {
        normalize_version(value)
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parse(latest), parse(installed));
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}


// ---------------------------------------------------------------------------
// 更新源：官方 GitHub release（可用镜像前缀改写）
// ---------------------------------------------------------------------------

/// 官方 release 查询接口（跟随 GitHub API，tag_name 形如 v1.75.1）。
pub const RELEASE_API_URL: &str = "https://api.github.com/repos/rclone/rclone/releases/latest";

/// 官方资产下载根：<根>/v<版本>/rclone-v<版本>-windows-amd64.zip。
pub const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/rclone/rclone/releases/download";

/// 更新状态文件名（放在数据目录下，体积只有几百字节）。
pub const STATE_FILE_NAME: &str = "engine-update.json";

/// 更新说明最多保留的字符数：release body 可能很长，持久化要克制。
pub const RELEASE_NOTES_LIMIT: usize = 4000;

/// Windows x64 官方资产名。
pub fn asset_name_for(version: &str) -> String {
    format!("rclone-v{}-windows-amd64.zip", normalize_version(version))
}

/// 规范化镜像前缀：去空白与尾部斜杠；不是 http(s) 绝对地址就当成"官方"（空串）。
///
/// 为什么要校验：前缀会被直接拼进 URL，用户填错时必须安静地退回官方源，
/// 而不是拼出一个语法错误的地址去发请求。
pub fn normalize_mirror_prefix(prefix: &str) -> String {
    let prefix = prefix.trim().trim_end_matches('/');
    if prefix.starts_with("http://") || prefix.starts_with("https://") {
        prefix.to_string()
    } else {
        String::new()
    }
}

/// 把镜像前缀拼到官方地址前面（GitHub 加速代理的通用形式：前缀 + 完整官方 URL）。空 = 官方。
pub fn mirror_join(prefix: &str, url: &str) -> String {
    let prefix = normalize_mirror_prefix(prefix);
    if prefix.is_empty() {
        url.to_string()
    } else {
        format!("{prefix}/{url}")
    }
}

/// 实际使用的 release 查询地址。
pub fn release_url(prefix: &str) -> String {
    mirror_join(prefix, RELEASE_API_URL)
}

/// 实际使用的资产下载地址。
pub fn asset_url(prefix: &str, version: &str, asset_name: &str) -> String {
    let version = normalize_version(version);
    mirror_join(
        prefix,
        &format!("{RELEASE_DOWNLOAD_BASE}/v{version}/{asset_name}"),
    )
}

/// 同一 release 的 SHA256SUMS 地址。
pub fn sums_url(prefix: &str, version: &str) -> String {
    let version = normalize_version(version);
    mirror_join(
        prefix,
        &format!("{RELEASE_DOWNLOAD_BASE}/v{version}/SHA256SUMS"),
    )
}

/// 界面上的更新源描述。
pub fn source_label(prefix: &str) -> String {
    let prefix = normalize_mirror_prefix(prefix);
    if prefix.is_empty() {
        "rclone 官方（api.github.com）".to_string()
    } else {
        format!("镜像前缀 {prefix}")
    }
}

/// 解析 GitHub latest release 响应，挑出 Windows x64 包与 SHA256SUMS。
#[derive(Debug, Clone, PartialEq)]
pub struct LatestRelease {
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sums_url: String,
    pub notes: String,
}

/// 解析 release JSON。纯函数，便于用真实响应片段做单测。
pub fn parse_latest_release(body: &str) -> std::result::Result<LatestRelease, String> {
    let value: Value = serde_json::from_str(body).map_err(|err| format!("不是合法 JSON：{err}"))?;
    // GitHub 限流/出错时会返回 200 + {"message": "API rate limit exceeded..."}
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Err(format!("GitHub API 报错：{message}"));
    }
    let tag = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "响应缺少 tag_name".to_string())?;
    let version = normalize_version(tag);
    if version.is_empty() {
        return Err("响应里的 tag_name 为空".to_string());
    }

    let assets: Vec<&Value> = value
        .get("assets")
        .and_then(Value::as_array)
        .map(|list| list.iter().collect())
        .unwrap_or_default();
    // 用具名嵌套函数而不是闭包：闭包的生命周期省略规则在这里推不出来。
    fn name_of(asset: &Value) -> Option<&str> {
        asset.get("name").and_then(Value::as_str)
    }
    fn url_of(asset: &Value) -> Option<String> {
        asset
            .get("browser_download_url")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    let asset_name = pick_windows_amd64_asset(assets.iter().filter_map(|asset| name_of(asset)))
        .ok_or_else(|| format!("release v{version} 里没有 windows-amd64 资产"))?;
    let download_base = RELEASE_DOWNLOAD_BASE;
    let asset_url = assets
        .iter()
        .find(|asset| name_of(asset) == Some(asset_name.as_str()))
        .and_then(|asset| url_of(asset))
        .unwrap_or_else(|| {
            format!("{download_base}/v{version}/{asset_name}")
        });
    let sums_url = assets
        .iter()
        .find(|asset| name_of(asset) == Some("SHA256SUMS"))
        .and_then(|asset| url_of(asset))
        .unwrap_or_else(|| format!("{download_base}/v{version}/SHA256SUMS"));

    Ok(LatestRelease {
        version,
        asset_name,
        asset_url,
        sums_url,
        notes: truncate_notes(
            value
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
    })
}

/// 截断更新说明（按字符，避免切坏多字节）。
pub fn truncate_notes(notes: &str) -> String {
    let notes = notes.trim();
    if notes.chars().count() <= RELEASE_NOTES_LIMIT {
        return notes.to_string();
    }
    let mut text: String = notes.chars().take(RELEASE_NOTES_LIMIT).collect();
    text.push_str("…（已截断）");
    text
}

/// 版本号是否等价（官方 tag 带 v，引擎 core/version 有的带、有的不带）。
pub fn versions_match(left: &str, right: &str) -> bool {
    normalize_version(left) == normalize_version(right) && !normalize_version(left).is_empty()
}

/// 升级后引擎是否仍然认识 vfs.DirCacheTime。
///
/// 为什么必须查：挂载参数里用了 vfs.DirCacheTime，rclone 若在新版本里改名/移除，
/// 挂载会静默失效——必须在更新成功之前拦住并回滚。
pub fn has_vfs_dir_cache_time(options: &Value) -> bool {
    options
        .get("vfs")
        .and_then(|vfs| vfs.get("DirCacheTime"))
        .map(|value| !value.is_null())
        .unwrap_or(false)
}

/// 现在（Unix 秒）。
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_windows_amd64_asset_only() {
        let names = [
            "rclone-v1.75.1-windows-386.zip",
            "rclone-v1.75.1-windows-amd64.zip",
            "SHA256SUMS",
        ];
        assert_eq!(
            pick_windows_amd64_asset(names).as_deref(),
            Some("rclone-v1.75.1-windows-amd64.zip")
        );
        assert!(pick_windows_amd64_asset(["SHA256SUMS"]).is_none());
    }

    #[test]
    fn parses_sha256sums() {
        let hash = "a".repeat(64);
        let text = format!("# c\n{hash}  rclone-v1.75.1-windows-amd64.zip\nbbb  other.zip\n");
        assert_eq!(
            sha256_for(&text, "rclone-v1.75.1-windows-amd64.zip").as_deref(),
            Some(hash.as_str())
        );
        assert!(sha256_for(&text, "missing.zip").is_none());
    }

    #[test]
    fn daily_auto_check_window() {
        let now = 1_800_000_000;
        assert!(should_auto_check(None, now));
        assert!(!should_auto_check(Some(now - 60), now));
        assert!(!should_auto_check(Some(now - AUTO_CHECK_INTERVAL_SECS + 1), now));
        assert!(should_auto_check(Some(now - AUTO_CHECK_INTERVAL_SECS), now));
        assert!(should_auto_check(Some(now - AUTO_CHECK_INTERVAL_SECS * 3), now));
    }

    #[test]
    fn version_compare() {
        assert!(is_newer("v1.76.0", "1.75.1"));
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(!is_newer("v1.75.1", "1.75.1"));
        assert!(!is_newer("v1.75.0", "1.75.1"));
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use foundation_core::{FoundationError, Result};

/// 引擎备份后缀：替换失败时用它回滚。
pub const ENGINE_BACKUP_SUFFIX: &str = ".old";

fn io_error(action: &str, path: &Path, err: std::io::Error) -> FoundationError {
    FoundationError::Process(format!("{action} {} 失败：{err}", path.display()))
}

/// 把暂存的引擎装到运行位置，并返回旧文件的备份路径。
///
/// 前提：调用方必须已经停掉 rclone —— Windows 会锁住运行中的映像，没停就换不了。
/// 任一步失败都会把已改名的旧文件还原，保证"要么换成，要么保持原样"。
pub fn swap_engine(installed: &Path, staged: &Path) -> Result<PathBuf> {
    if !staged.exists() {
        return Err(FoundationError::Process(format!(
            "暂存的引擎不存在：{}",
            staged.display()
        )));
    }
    let backup = PathBuf::from(format!("{}{ENGINE_BACKUP_SUFFIX}", installed.display()));
    if backup.exists() {
        fs::remove_file(&backup).map_err(|err| io_error("清理旧备份", &backup, err))?;
    }
    let had_previous = installed.exists();
    if had_previous {
        fs::rename(installed, &backup).map_err(|err| io_error("备份现有引擎", installed, err))?;
    }
    if let Err(err) = fs::copy(staged, installed) {
        let failure = io_error("写入新引擎", installed, err);
        if had_previous {
            let _ = fs::rename(&backup, installed);
        }
        return Err(failure);
    }
    Ok(backup)
}

/// 回滚：把备份换回原位并清掉备份。
pub fn rollback_engine(installed: &Path, backup: &Path) -> Result<()> {
    if !backup.exists() {
        return Err(FoundationError::Process(format!(
            "备份不存在，无法回滚：{}",
            backup.display()
        )));
    }
    if installed.exists() {
        fs::remove_file(installed).map_err(|err| io_error("删除新引擎", installed, err))?;
    }
    fs::rename(backup, installed).map_err(|err| io_error("回滚引擎", installed, err))
}

/// 替换成功后清理备份。
pub fn discard_backup(backup: &Path) {
    let _ = fs::remove_file(backup);
}

#[cfg(test)]
mod swap_tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("engine-swap-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn swap_replaces_engine_and_keeps_backup() {
        let dir = temp("swap");
        let installed = dir.join("rclone.exe");
        let staged = dir.join("staged.exe");
        fs::write(&installed, b"old").unwrap();
        fs::write(&staged, b"new").unwrap();

        let backup = swap_engine(&installed, &staged).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"new");
        assert_eq!(fs::read(&backup).unwrap(), b"old");
        discard_backup(&backup);
        assert!(!backup.exists());
        assert_eq!(fs::read(&installed).unwrap(), b"new", "清理备份不能动新引擎");
    }

    #[test]
    fn rollback_restores_previous_engine() {
        let dir = temp("rollback");
        let installed = dir.join("rclone.exe");
        let staged = dir.join("staged.exe");
        fs::write(&installed, b"old").unwrap();
        fs::write(&staged, b"new").unwrap();

        let backup = swap_engine(&installed, &staged).unwrap();
        rollback_engine(&installed, &backup).unwrap();
        assert_eq!(fs::read(&installed).unwrap(), b"old", "回滚后应恢复旧引擎");
        assert!(!backup.exists());
    }

    #[test]
    fn missing_staged_file_is_rejected_without_touching_installed() {
        let dir = temp("missing");
        let installed = dir.join("rclone.exe");
        fs::write(&installed, b"old").unwrap();

        let err = swap_engine(&installed, &dir.join("nope.exe")).unwrap_err();
        assert!(err.to_string().contains("暂存的引擎不存在"));
        assert_eq!(fs::read(&installed).unwrap(), b"old", "失败不得破坏现有引擎");
    }
}

// ---------------------------------------------------------------------------
// 更新状态（engine-update.json）与网络接线
// ---------------------------------------------------------------------------

use std::io::Read;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::zip_extract;

/// 持久化的更新状态（%PROGRAMDATA%\WebDavDrive\engine-update.json）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EngineUpdateState {
    /// 上次检查时间（Unix 秒）。成功与失败都会记，保证自动检查「每天最多一次」。
    #[serde(default)]
    pub last_check_at: Option<i64>,
    /// 上次看到的官方最新版本（不含 v）。
    #[serde(default)]
    pub latest_version: Option<String>,
    /// release 接口返回的 ETag，用于 If-None-Match 节流。
    #[serde(default)]
    pub etag: Option<String>,
    /// 本地引擎版本（最近一次探测或安装结果）。
    #[serde(default)]
    pub installed_version: Option<String>,
    /// 上次检查失败原因。界面据此显示「上次检查失败/可重试」，后台失败绝不弹窗。
    #[serde(default)]
    pub last_error: Option<String>,
    /// 最新版本的更新说明（已截断）。
    #[serde(default)]
    pub notes: Option<String>,
    /// 用户填写的镜像前缀；空串 = 官方源。
    #[serde(default)]
    pub mirror_prefix: String,
}

/// 返回给界面的引擎更新信息（引擎路径/运行态在 AppStatus.engine 里，不在这里重复）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineUpdateInfo {
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub last_check_at: Option<i64>,
    pub last_error: Option<String>,
    pub notes: Option<String>,
    pub mirror_prefix: String,
    pub source: String,
    /// 本次调用是否真的发起了网络检查（被每日限流跳过时为 false）。
    pub checked: bool,
    /// 服务端返回 304：沿用缓存里的最新版本。
    pub not_modified: bool,
}

fn info_from(state: &EngineUpdateState, checked: bool, not_modified: bool) -> EngineUpdateInfo {
    EngineUpdateInfo {
        installed_version: state.installed_version.clone(),
        latest_version: state.latest_version.clone(),
        update_available: update_available(state),
        last_check_at: state.last_check_at,
        last_error: state.last_error.clone(),
        notes: state.notes.clone(),
        mirror_prefix: state.mirror_prefix.clone(),
        source: source_label(&state.mirror_prefix),
        checked,
        not_modified,
    }
}

/// 本地版本与最新版本比较（缺任一版本时按「未知」处理，不提示更新）。
pub fn update_available(state: &EngineUpdateState) -> bool {
    match (&state.latest_version, &state.installed_version) {
        (Some(latest), Some(installed)) => !installed.trim().is_empty() && is_newer(latest, installed),
        _ => false,
    }
}

/// 一次文本 GET 的结果（只用于小文件：release JSON / SHA256SUMS）。
#[derive(Debug, Clone, PartialEq)]
pub struct HttpText {
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
}

/// 引擎更新的网络抽象：生产实现是系统 WinHTTP，测试用假实现。
pub trait UpdateTransport: Send + Sync {
    /// 带可选 If-None-Match 的 GET。304 也必须正常返回（不是错误）。
    fn get_text(&self, url: &str, etag: Option<&str>) -> Result<HttpText>;
    /// 下载到本地文件：大文件必须流式落盘，不能整个读进内存。
    fn download(&self, url: &str, dest: &Path) -> Result<()>;
}

/// 生产传输层：Windows 上走系统 WinHTTP（自带 TLS 与系统代理设置）。
pub fn default_transport() -> Arc<dyn UpdateTransport> {
    #[cfg(windows)]
    {
        Arc::new(crate::http::WinHttpTransport::new())
    }
    #[cfg(not(windows))]
    {
        Arc::new(crate::http::UnsupportedTransport)
    }
}

/// 新建临时暂存目录（%TEMP%\webdav-drive-engine-<uuid>）。
///
/// 引擎包有 20MB 以上，绝不能写进 ACL 收紧过的数据目录。
pub fn new_staging_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("webdav-drive-engine-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 删除暂存目录（安装成功或失败都必须调用）。只删我们自己建的目录。
pub fn cleanup_staging(staged: &Path) {
    let Some(dir) = staged.parent() else { return };
    let ours = dir
        .file_name()
        .map(|name| name.to_string_lossy().starts_with("webdav-drive-engine-"))
        .unwrap_or(false);
    if ours {
        let _ = fs::remove_dir_all(dir);
    }
}

/// 字节串的 sha256（小写十六进制）。
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 流式计算文件的 sha256（避免把引擎包整个读进内存）。
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 更新状态文件 + 检查/下载的接线。
pub struct EngineUpdater {
    dir: PathBuf,
    transport: Arc<dyn UpdateTransport>,
}

impl EngineUpdater {
    pub fn new(dir: impl Into<PathBuf>, transport: Arc<dyn UpdateTransport>) -> Self {
        Self {
            dir: dir.into(),
            transport,
        }
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join(STATE_FILE_NAME)
    }

    /// 读取状态。文件缺失/损坏都退回默认值并记日志：更新状态坏掉不能拖垮整个应用。
    pub fn load_state(&self) -> EngineUpdateState {
        match fs::read_to_string(self.state_path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|err| {
                log::warn!("engine-update.json 解析失败（{err}），按默认状态处理");
                EngineUpdateState::default()
            }),
            Err(_) => EngineUpdateState::default(),
        }
    }

    /// 原子写入：先写 .tmp 再 rename，避免断电留下半个 JSON。
    pub fn save_state(&self, state: &EngineUpdateState) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(state)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 保存镜像前缀（空/非法 = 官方）。
    pub fn set_mirror_prefix(&self, prefix: &str) -> Result<EngineUpdateState> {
        let mut state = self.load_state();
        state.mirror_prefix = normalize_mirror_prefix(prefix);
        self.save_state(&state)?;
        Ok(state)
    }

    /// 安装成功后记录本地版本。
    pub fn record_installed(&self, version: &str) -> Result<()> {
        let mut state = self.load_state();
        state.installed_version = Some(normalize_version(version));
        self.save_state(&state)
    }

    /// 只读展示（不发网络请求）。
    pub fn info(&self, installed: Option<&str>) -> EngineUpdateInfo {
        let mut state = self.load_state();
        if let Some(installed) = installed.filter(|value| !value.trim().is_empty()) {
            state.installed_version = Some(normalize_version(installed));
        }
        info_from(&state, false, false)
    }

    /// 检查更新。force = 手动点「检查更新」，绕过每日限流。
    ///
    /// 网络失败不返回 Err，而是写进 last_error 并持久化：后台自动检查失败必须静默，
    /// 设置页显示「上次检查失败/可重试」即可。
    pub fn check(&self, installed: Option<&str>, force: bool, now: i64) -> EngineUpdateInfo {
        let mut state = self.load_state();
        if let Some(installed) = installed.filter(|value| !value.trim().is_empty()) {
            state.installed_version = Some(normalize_version(installed));
        }
        if !force && !should_auto_check(state.last_check_at, now) {
            // 命中「每天最多一次」：不发请求，直接用缓存状态。
            return info_from(&state, false, false);
        }

        state.last_check_at = Some(now);
        let etag = state.etag.clone();
        let url = release_url(&state.mirror_prefix);
        match self.transport.get_text(&url, etag.as_deref()) {
            Ok(response) if response.status == 304 => {
                state.last_error = None;
                log::info!("引擎更新检查：release 未变化（304），沿用缓存版本");
                if let Err(err) = self.save_state(&state) {
                    log::warn!("写入 engine-update.json 失败：{err}");
                }
                return info_from(&state, true, true);
            }
            Ok(response) if (200..300).contains(&response.status) => {
                match parse_latest_release(&response.body) {
                    Ok(release) => {
                        log::info!("引擎更新检查：最新版本 v{}", release.version);
                        state.latest_version = Some(release.version);
                        state.notes = Some(release.notes);
                        state.etag = response.etag.clone().or(etag);
                        state.last_error = None;
                    }
                    Err(err) => {
                        state.last_error = Some(format!("更新源响应无法解析：{err}"));
                    }
                }
            }
            Ok(response) => {
                state.last_error = Some(format!(
                    "更新源返回 HTTP {}（{url}）；可稍后在设置页重试",
                    response.status
                ));
            }
            Err(err) => {
                state.last_error = Some(format!("检查更新失败：{err}；可稍后重试"));
                log::warn!("引擎更新检查失败（静默降级）：{err}");
            }
        }
        if let Err(err) = self.save_state(&state) {
            log::warn!("写入 engine-update.json 失败：{err}");
        }
        info_from(&state, true, false)
    }

    /// 官方包：下载 → SHA256SUMS 校验 → 解出 rclone.exe，返回暂存 exe 路径。
    ///
    /// 每一步的错误都写明「第几步失败」，用户看得懂卡在哪，也不会留下半个安装。
    pub fn stage_release(&self, version: &str) -> Result<PathBuf> {
        let staging = new_staging_dir()?;
        let staged = staging.join("rclone-staged.exe");
        match self.fetch_release(&staging, &staged, version) {
            Ok(()) => Ok(staged),
            Err(err) => {
                // 失败就不能留下 20MB 的半个包。
                let _ = fs::remove_dir_all(&staging);
                Err(err)
            }
        }
    }

    /// 下载并解出引擎到给定暂存目录（失败由调用方清理目录）。
    fn fetch_release(&self, staging: &Path, staged: &Path, version: &str) -> Result<()> {
        let version = normalize_version(version);
        if version.is_empty() {
            return Err(FoundationError::InvalidInput("缺少要安装的版本号".into()));
        }
        let prefix = self.load_state().mirror_prefix;
        let asset_name = asset_name_for(&version);
        let archive = staging.join(&asset_name);

        let download = asset_url(&prefix, &version, &asset_name);
        log::info!("下载 rclone v{version}：{download}");
        self.transport.download(&download, &archive).map_err(|err| {
            FoundationError::Process(format!("第 1 步（下载 rclone v{version} 引擎包）失败：{err}"))
        })?;

        let sums_location = sums_url(&prefix, &version);
        let sums = self
            .transport
            .get_text(&sums_location, None)
            .map_err(|err| FoundationError::Process(format!("第 2 步（下载 SHA256SUMS）失败：{err}")))?;
        if !(200..300).contains(&sums.status) {
            return Err(FoundationError::Process(format!(
                "第 2 步（下载 SHA256SUMS）失败：HTTP {}（{sums_location}）",
                sums.status
            )));
        }
        let expected = sha256_for(&sums.body, &asset_name).ok_or_else(|| {
            FoundationError::DataCorrupted(format!(
                "第 3 步（校验）失败：SHA256SUMS 里没有 {asset_name}"
            ))
        })?;
        let actual = sha256_file(&archive)?;
        if actual != expected {
            return Err(FoundationError::DataCorrupted(format!(
                "第 3 步（校验）失败：{asset_name} 的 SHA256 与官方 SHA256SUMS 不一致（期望 {expected}，实际 {actual}）；已放弃安装，引擎保持原状"
            )));
        }
        log::info!("{asset_name} SHA256 校验通过");

        zip_extract::extract_rclone_exe(&archive, staged).map_err(|err| {
            FoundationError::Process(format!("第 4 步（解压 {asset_name}）失败：{err}"))
        })?;
        let _ = fs::remove_file(&archive);
        Ok(())
    }

    /// 本地文件：rclone.exe 直接复制，zip 解出 rclone.exe。
    pub fn stage_local_file(&self, path: &Path) -> Result<PathBuf> {
        let staging = new_staging_dir()?;
        let staged = staging.join("rclone-staged.exe");
        match self.fetch_local_file(path, &staged) {
            Ok(()) => Ok(staged),
            Err(err) => {
                let _ = fs::remove_dir_all(&staging);
                Err(err)
            }
        }
    }

    /// 从本地文件准备暂存引擎（失败由调用方清理目录）。
    fn fetch_local_file(&self, path: &Path, staged: &Path) -> Result<()> {
        if !path.is_file() {
            return Err(FoundationError::NotFound(format!(
                "文件不存在：{}",
                path.display()
            )));
        }
        let is_zip = path
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if is_zip {
            zip_extract::extract_rclone_exe(path, staged).map_err(|err| {
                FoundationError::Process(format!("解压 {} 失败：{err}", path.display()))
            })?;
            return Ok(());
        }

        let mut head = [0u8; 2];
        let mut file = fs::File::open(path)?;
        file.read_exact(&mut head).map_err(|err| {
            FoundationError::InvalidInput(format!("无法读取 {}：{err}", path.display()))
        })?;
        if &head != b"MZ" {
            return Err(FoundationError::InvalidInput(format!(
                "{} 不是 Windows 可执行文件（缺少 MZ 头）；请选择 rclone.exe 或官方 zip 包",
                path.display()
            )));
        }
        fs::copy(path, staged)?;
        Ok(())
    }
}

#[cfg(test)]
mod update_tests {
    use super::*;
    use std::sync::Mutex;

    const RELEASE_JSON: &str = r#"{
        "tag_name": "v1.99.3",
        "body": "变更\n- 修复了 VFS 目录缓存\n",
        "assets": [
            {"name": "rclone-v1.99.3-windows-386.zip", "browser_download_url": "https://github.com/rclone/rclone/releases/download/v1.99.3/rclone-v1.99.3-windows-386.zip"},
            {"name": "rclone-v1.99.3-windows-amd64.zip", "browser_download_url": "https://github.com/rclone/rclone/releases/download/v1.99.3/rclone-v1.99.3-windows-amd64.zip"},
            {"name": "SHA256SUMS", "browser_download_url": "https://github.com/rclone/rclone/releases/download/v1.99.3/SHA256SUMS"}
        ]
    }"#;

    /// 假响应：FoundationError 不是 Clone，所以失败只存文案。
    #[derive(Clone)]
    enum FakeResponse {
        Text(HttpText),
        Failure(String),
    }

    #[derive(Default)]
    struct FakeTransport {
        urls: Mutex<Vec<String>>,
        etags: Mutex<Vec<Option<String>>>,
        response: Mutex<Option<FakeResponse>>,
    }

    impl FakeTransport {
        fn ok(body: &str, etag: Option<&str>) -> Self {
            let transport = Self::default();
            *transport.response.lock().unwrap() = Some(FakeResponse::Text(HttpText {
                status: 200,
                body: body.to_string(),
                etag: etag.map(str::to_string),
            }));
            transport
        }

        fn failing(message: &str) -> Self {
            let transport = Self::default();
            *transport.response.lock().unwrap() = Some(FakeResponse::Failure(message.to_string()));
            transport
        }

        fn status(status: u16) -> Self {
            let transport = Self::default();
            *transport.response.lock().unwrap() = Some(FakeResponse::Text(HttpText {
                status,
                body: String::new(),
                etag: None,
            }));
            transport
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }

        fn etags(&self) -> Vec<Option<String>> {
            self.etags.lock().unwrap().clone()
        }
    }

    impl UpdateTransport for FakeTransport {
        fn get_text(&self, url: &str, etag: Option<&str>) -> Result<HttpText> {
            self.urls.lock().unwrap().push(url.to_string());
            self.etags.lock().unwrap().push(etag.map(str::to_string));
            match self.response.lock().unwrap().clone() {
                Some(FakeResponse::Text(text)) => Ok(text),
                Some(FakeResponse::Failure(message)) => Err(FoundationError::Process(message)),
                None => Err(FoundationError::Process("没有预置响应".into())),
            }
        }

        fn download(&self, _url: &str, _dest: &Path) -> Result<()> {
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("engine-update-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_release_and_picks_windows_asset() {
        let release = parse_latest_release(RELEASE_JSON).unwrap();
        assert_eq!(release.version, "1.99.3");
        assert_eq!(release.asset_name, "rclone-v1.99.3-windows-amd64.zip");
        assert!(release
            .asset_url
            .ends_with("/v1.99.3/rclone-v1.99.3-windows-amd64.zip"));
        assert!(release.sums_url.ends_with("/v1.99.3/SHA256SUMS"));
        assert!(release.notes.contains("VFS 目录缓存"));

        // GitHub 限流也会返回 200：必须当成错误，不能拿 message 当 release。
        let limited = r#"{"message":"API rate limit exceeded for 1.2.3.4."}"#;
        assert!(parse_latest_release(limited).unwrap_err().contains("rate limit"));
        assert!(parse_latest_release("not json").is_err());
    }

    #[test]
    fn mirror_prefix_rewrites_every_url() {
        assert_eq!(
            normalize_mirror_prefix("  https://ghfast.top/  "),
            "https://ghfast.top"
        );
        assert_eq!(normalize_mirror_prefix("ghfast.top"), "");
        assert_eq!(
            release_url("https://ghfast.top"),
            "https://ghfast.top/https://api.github.com/repos/rclone/rclone/releases/latest"
        );
        assert_eq!(release_url(""), RELEASE_API_URL);
        assert_eq!(
            asset_url(
                "https://ghfast.top/",
                "v1.75.1",
                "rclone-v1.75.1-windows-amd64.zip"
            ),
            "https://ghfast.top/https://github.com/rclone/rclone/releases/download/v1.75.1/rclone-v1.75.1-windows-amd64.zip"
        );
        assert!(sums_url("", "1.75.1").ends_with("/v1.75.1/SHA256SUMS"));
        assert_eq!(source_label(""), "rclone 官方（api.github.com）");
        assert!(source_label("https://mirror.example").contains("mirror.example"));
    }

    #[test]
    fn checks_vfs_and_version_helpers() {
        assert!(versions_match("v1.75.1", "1.75.1"));
        assert!(!versions_match("v1.75.1", "1.75.0"));
        assert!(!versions_match("", ""), "空版本号不算匹配");
        assert!(has_vfs_dir_cache_time(
            &serde_json::json!({"vfs": {"DirCacheTime": "5m0s"}})
        ));
        assert!(!has_vfs_dir_cache_time(&serde_json::json!({"vfs": {}})));
        assert!(!has_vfs_dir_cache_time(
            &serde_json::json!({"vfs": {"DirCacheTime": null}})
        ));
        assert!(!has_vfs_dir_cache_time(&serde_json::json!({})));
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let long = "字".repeat(RELEASE_NOTES_LIMIT + 10);
        assert!(truncate_notes(&long).ends_with("…（已截断）"));
    }

    #[test]
    fn check_respects_daily_limit_but_force_bypasses_it() {
        let dir = temp_dir("check-limit");
        let transport = Arc::new(FakeTransport::ok(RELEASE_JSON, Some("etag-1")));
        let updater = EngineUpdater::new(&dir, transport.clone());
        let now = 1_800_000_000;

        let first = updater.check(Some("1.75.1"), false, now);
        assert!(first.checked);
        assert_eq!(first.latest_version.as_deref(), Some("1.99.3"));
        assert!(first.update_available);
        assert_eq!(first.last_error, None);
        assert_eq!(transport.urls().len(), 1, "首次自动检查应发请求");

        // 同一天内再自动检查：跳过网络，但仍返回缓存的最新版本。
        let second = updater.check(Some("1.75.1"), false, now + 60);
        assert!(!second.checked, "每天最多一次：不应重复请求");
        assert_eq!(transport.urls().len(), 1);
        assert_eq!(second.latest_version.as_deref(), Some("1.99.3"));

        // 手动按钮随时可点，绕过限流。
        let forced = updater.check(Some("1.75.1"), true, now + 120);
        assert!(forced.checked);
        assert_eq!(transport.urls().len(), 2);
        assert_eq!(
            transport.etags().pop().flatten().as_deref(),
            Some("etag-1"),
            "第二次请求应带 If-None-Match"
        );

        // 状态已落盘：换一个实例也读得到。
        let reopened = EngineUpdater::new(&dir, transport).load_state();
        assert_eq!(reopened.latest_version.as_deref(), Some("1.99.3"));
        assert_eq!(reopened.etag.as_deref(), Some("etag-1"));
        assert_eq!(reopened.installed_version.as_deref(), Some("1.75.1"));
        assert_eq!(reopened.last_check_at, Some(now + 120));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_survives_304_and_network_failure_silently() {
        let dir = temp_dir("check-fail");

        // 304：沿用缓存版本，清掉上次错误。
        let seeded = EngineUpdater::new(&dir, Arc::new(FakeTransport::ok(RELEASE_JSON, Some("e"))));
        seeded.check(Some("1.75.1"), false, 1_000);
        let not_modified = EngineUpdater::new(&dir, Arc::new(FakeTransport::status(304)));
        let info = not_modified.check(Some("1.75.1"), true, 2_000);
        assert!(info.not_modified);
        assert!(info.checked);
        assert_eq!(info.latest_version.as_deref(), Some("1.99.3"));
        assert_eq!(info.last_error, None);

        // 网络失败：不 panic、不返回 Err，只记 last_error 并照常记 last_check_at。
        let offline = EngineUpdater::new(&dir, Arc::new(FakeTransport::failing("无法连接 winhttp")));
        let info = offline.check(Some("1.75.1"), true, 3_000);
        assert!(info.checked);
        assert!(info
            .last_error
            .as_deref()
            .unwrap()
            .contains("无法连接 winhttp"));
        let state = EngineUpdater::new(&dir, Arc::new(FakeTransport::default())).load_state();
        assert_eq!(state.last_check_at, Some(3_000));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mirror_prefix_is_persisted_and_used_by_check() {
        let dir = temp_dir("mirror");
        let transport = Arc::new(FakeTransport::ok(RELEASE_JSON, None));
        let updater = EngineUpdater::new(&dir, transport.clone());
        updater.set_mirror_prefix("https://ghfast.top/").unwrap();
        let info = updater.check(Some("1.75.1"), true, 42);
        assert_eq!(info.mirror_prefix, "https://ghfast.top");
        assert_eq!(
            transport.urls()[0],
            "https://ghfast.top/https://api.github.com/repos/rclone/rclone/releases/latest"
        );
        // 非法前缀安静退回官方源。
        updater.set_mirror_prefix("随便写的").unwrap();
        assert_eq!(updater.load_state().mirror_prefix, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_state_file_falls_back_to_default() {
        let dir = temp_dir("corrupt");
        fs::write(dir.join(STATE_FILE_NAME), b"{ not json").unwrap();
        let updater = EngineUpdater::new(&dir, Arc::new(FakeTransport::default()));
        assert_eq!(updater.load_state(), EngineUpdateState::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_file_matches_in_memory_hash() {
        let dir = temp_dir("sha");
        let file = dir.join("blob.bin");
        let payload = vec![7u8; 300 * 1024];
        fs::write(&file, &payload).unwrap();
        assert_eq!(sha256_file(&file).unwrap(), sha256_hex(&payload));
        let _ = fs::remove_dir_all(&dir);
    }


    /// 官方下载路径：download 写一个真 zip，SHA256SUMS 用它的真实 sha256。
    struct ReleaseTransport {
        asset_name: String,
        archive: Vec<u8>,
        corrupt_sums: bool,
        downloads: Mutex<Vec<String>>,
    }

    impl ReleaseTransport {
        fn new(version: &str, engine: &[u8], corrupt_sums: bool) -> Self {
            let folder = format!("rclone-v{version}-windows-amd64");
            let archive = crate::zip_extract::test_zip::build_zip(&[
                (&format!("{folder}/README.txt"), b"docs".as_slice(), true),
                (&format!("{folder}/rclone.exe"), engine, true),
            ]);
            Self {
                asset_name: asset_name_for(version),
                archive,
                corrupt_sums,
                downloads: Mutex::new(Vec::new()),
            }
        }
    }

    impl UpdateTransport for ReleaseTransport {
        fn get_text(&self, url: &str, _etag: Option<&str>) -> Result<HttpText> {
            if !url.ends_with("/SHA256SUMS") {
                return Err(FoundationError::Process(format!("不该请求 {url}")));
            }
            let hash = if self.corrupt_sums {
                "0".repeat(64)
            } else {
                sha256_hex(&self.archive)
            };
            Ok(HttpText {
                status: 200,
                body: format!("{hash}  {}\n", self.asset_name),
                etag: None,
            })
        }

        fn download(&self, url: &str, dest: &Path) -> Result<()> {
            self.downloads.lock().unwrap().push(url.to_string());
            fs::write(dest, &self.archive)?;
            Ok(())
        }
    }

    #[test]
    fn stage_release_downloads_verifies_and_extracts_engine() {
        let dir = temp_dir("stage-release");
        let transport = Arc::new(ReleaseTransport::new("v1.99.3", b"MZ fake-engine", false));
        let updater = EngineUpdater::new(&dir, transport.clone());

        let staged = updater.stage_release("v1.99.3").unwrap();
        assert_eq!(fs::read(&staged).unwrap(), b"MZ fake-engine");
        assert_eq!(
            transport.downloads.lock().unwrap()[0],
            asset_url("", "1.99.3", "rclone-v1.99.3-windows-amd64.zip"),
            "下载地址必须是官方资产地址"
        );
        cleanup_staging(&staged);
        assert!(!staged.exists(), "安装收尾必须清掉暂存目录");

        // SHA256 对不上：第 3 步失败，且不能留下暂存引擎。
        let updater = EngineUpdater::new(&dir, Arc::new(ReleaseTransport::new("v1.99.3", b"MZ fake-engine", true)));
        let err = updater.stage_release("1.99.3").unwrap_err();
        assert!(err.to_string().contains("第 3 步"), "{err}");
        assert!(err.to_string().contains("SHA256"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stage_local_file_rejects_non_executable() {
        let dir = temp_dir("stage-local");
        let readme = dir.join("notes.txt");
        fs::write(&readme, b"hello").unwrap();
        let updater = EngineUpdater::new(&dir, Arc::new(FakeTransport::default()));
        let err = updater.stage_local_file(&readme).unwrap_err();
        assert!(err.to_string().contains("MZ"), "{err}");
        assert!(updater
            .stage_local_file(&dir.join("missing.exe"))
            .unwrap_err()
            .to_string()
            .contains("文件不存在"));
        let _ = fs::remove_dir_all(&dir);
    }
}
