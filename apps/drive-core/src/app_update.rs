//! 应用（WebDAV Drive 自身）自更新：检查 → 下载 → 校验 → 退出 → 静默安装 → 重启。
//!
//! 为什么和引擎更新（rclone）分开：引擎更新只替换 rclone.exe，进程不用退出；
//! 应用更新要替换掉正在运行的 drive.exe，中途必然结束当前进程，所以「装」这一步
//! 只能交给另一个进程完成——也就是本模块里的更新器（drive-updater.exe，当前程序的
//! 临时副本）：它等原进程退出 → 静默安装 → 重新启动应用。
//!
//! 关键约束：
//! - 安装包只认本仓库 GitHub Release 里的 Windows 安装包资产，并校验 SHA256
//!   （优先用 API 响应里的 digest，其次下载同名 .sha256 旁文件）；
//! - 更新器副本必须改名（不能叫 drive.exe），否则安装包的「应用正在运行」检测
//!   会命中更新器自己，静默安装会被打断；
//! - 绿色版（免安装 zip）没有 uninstall.exe：不做自更新，只提示去 GitHub 下载；
//! - 网络/状态/镜像前缀复用 crate::engine_update（UpdateTransport、镜像拼接）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use foundation_core::{FoundationError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine_update::{
    cleanup_staging, default_transport, is_newer, mirror_join, new_staging_dir,
    normalize_mirror_prefix, normalize_version, sha256_file, sha256_for, should_auto_check,
    source_label, truncate_notes, UpdateTransport,
};

/// 本项目的 GitHub 仓库：更新只认这里发布的安装包。
pub const REPO: &str = "yashuamao/WebDavDrive";

/// Release API（latest）。
pub const RELEASE_API_URL: &str =
    "https://api.github.com/repos/yashuamao/WebDavDrive/releases/latest";

/// Release 资产下载前缀（镜像前缀会拼在它前面）。
pub const RELEASE_DOWNLOAD_BASE: &str =
    "https://github.com/yashuamao/WebDavDrive/releases/download";

/// 状态文件名（放在 %PROGRAMDATA%\WebDavDrive）。
pub const STATE_FILE_NAME: &str = "app-update.json";

/// 更新日志文件名（同目录）。
pub const LOG_FILE_NAME: &str = "app-update.log";

/// 安装包资产名后缀：GitHub 会把资产名里的空格换成点。
pub const SETUP_ASSET_SUFFIX: &str = "_x64-setup.exe";

/// 下载后暂存的安装包文件名。
pub const STAGED_SETUP_NAME: &str = "WebDAVDrive-setup.exe";

/// 更新器副本的目录前缀（留在 %TEMP%，下次启动清理）。
pub const UPDATER_PREFIX: &str = "webdav-drive-updater-";

/// 引擎/应用共用的暂存目录前缀（engine_update::new_staging_dir）。
pub const STAGING_PREFIX: &str = "webdav-drive-engine-";

/// 更新器副本名，必须是这个值：与主程序不同名，安装包才不会把更新器当成主程序。
pub const UPDATER_EXE_NAME: &str = "drive-updater.exe";

/// 等待原应用退出的上限：超时宁可放弃更新，也不能在应用还活着时装文件。
pub const WAIT_EXIT_TIMEOUT: Duration = Duration::from_secs(120);

/// 更新器内部开关（不面向用户）。
pub const APPLY_FLAG: &str = "--apply-update";

/// 安装位置记忆的注册表键：与 NSIS 模板的 MANUPRODUCTKEY 一致（currentUser 模式用 HKCU）。
pub const INSTALL_LOCATION_KEY: &str = r"HKCU\Software\foundation\WebDAV Drive";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 安装包资产名的规范形式（发布脚本按它命名；GitHub 把空格换成点）。
pub fn setup_asset_name(version: &str) -> String {
    format!("WebDAV.Drive_{version}_x64-setup.exe")
}

/// 一个 Release 里跟应用更新有关的字段。
#[derive(Debug, Clone)]
pub struct LatestRelease {
    /// 原始 tag（形如 v0.1.9），下载路径要用它。
    pub tag_name: String,
    /// 规范化版本号（去掉 v）。
    pub version: String,
    pub asset_name: String,
    pub asset_url: String,
    pub sums_url: Option<String>,
    /// API 的资产 digest（sha256:<hex>）。
    pub digest: Option<String>,
    pub notes: String,
}

/// 应用更新的本地状态（%PROGRAMDATA%\WebDavDrive\app-update.json）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppUpdateState {
    pub last_check_at: Option<i64>,
    pub latest_version: Option<String>,
    pub tag_name: Option<String>,
    pub etag: Option<String>,
    pub installed_version: Option<String>,
    pub last_error: Option<String>,
    pub notes: Option<String>,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub sums_url: Option<String>,
    pub setup_sha256: Option<String>,
    /// 已下载并通过校验的安装包路径（重启后仍可继续安装）。
    pub staged_setup: Option<String>,
    pub staged_version: Option<String>,
    /// 只用于界面展示「更新源」：实际用的是引擎更新里的镜像前缀。
    pub mirror_prefix: Option<String>,
}

/// 设置页「应用更新」区块需要的信息（序列化给前端）。
#[derive(Debug, Clone, Serialize)]
pub struct AppUpdateInfo {
    pub installed_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub last_check_at: Option<i64>,
    pub last_error: Option<String>,
    pub notes: Option<String>,
    pub source: String,
    /// 本次是否真的发起了网络检查（被每天一次的限制跳过时为 false）。
    pub checked: bool,
    pub not_modified: bool,
    /// 绿色版（免安装 zip）：不做自更新，只提示去 GitHub 下载。
    pub portable: bool,
    /// 安装包已下载并通过校验，可以直接安装。
    pub setup_downloaded: bool,
}

/// 只要 Release 里的 Windows 安装包资产（.sha256 旁文件不算）。
pub fn pick_setup_asset<'a, I>(names: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    names
        .into_iter()
        .find(|name| name.ends_with(SETUP_ASSET_SUFFIX))
        .map(str::to_string)
}

/// 找安装包对应的 .sha256 旁文件（没有 digest 时的兜底）。
pub fn pick_sums_asset<'a, I>(names: I, setup: &str) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let names: Vec<&str> = names.into_iter().collect();
    let expected = format!("{setup}.sha256");
    names
        .iter()
        .find(|name| **name == expected)
        .or_else(|| names.iter().find(|name| name.ends_with(".sha256")))
        .map(|name| name.to_string())
}

/// 解析 GitHub latest release 响应。
pub fn parse_latest_release(body: &str) -> std::result::Result<LatestRelease, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|err| format!("GitHub 响应不是有效 JSON：{err}"))?;
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return Err(format!("GitHub API 报错：{message}"));
    }
    let tag_name = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "响应缺少 tag_name".to_string())?
        .to_string();
    let notes = value.get("body").and_then(Value::as_str).unwrap_or_default();
    let assets = value
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| "响应缺少 assets".to_string())?;

    let names: Vec<&str> = assets
        .iter()
        .filter_map(|asset| asset.get("name").and_then(Value::as_str))
        .collect();
    let asset_name = pick_setup_asset(names.iter().copied()).ok_or_else(|| {
        format!("release {tag_name} 里没有 Windows 安装包资产（形如 {SETUP_ASSET_SUFFIX}）")
    })?;
    let asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(Value::as_str) == Some(asset_name.as_str()))
        .ok_or_else(|| format!("release {tag_name} 里找不到 {asset_name} 的下载地址"))?;
    let asset_url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("{RELEASE_DOWNLOAD_BASE}/{tag_name}/{asset_name}"));
    let sums_url = pick_sums_asset(names.iter().copied(), &asset_name)
        .map(|name| format!("{RELEASE_DOWNLOAD_BASE}/{tag_name}/{name}"));
    let digest = asset
        .get("digest")
        .and_then(Value::as_str)
        .and_then(|raw| raw.strip_prefix("sha256:"))
        .map(|hex| hex.trim().to_ascii_lowercase())
        .filter(|hex| hex.len() == 64);

    Ok(LatestRelease {
        version: normalize_version(&tag_name),
        tag_name,
        asset_name,
        asset_url,
        sums_url,
        digest,
        notes: truncate_notes(notes),
    })
}

/// 是否发现了比本机更新的版本。
pub fn update_available(state: &AppUpdateState) -> bool {
    match (
        state.latest_version.as_deref(),
        state.installed_version.as_deref(),
    ) {
        (Some(latest), Some(installed))
            if !latest.trim().is_empty() && !installed.trim().is_empty() =>
        {
            is_newer(latest, installed)
        }
        _ => false,
    }
}

fn info_from(
    state: &AppUpdateState,
    checked: bool,
    not_modified: bool,
    portable: bool,
) -> AppUpdateInfo {
    let staged = state
        .staged_setup
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    AppUpdateInfo {
        installed_version: state.installed_version.clone().unwrap_or_default(),
        latest_version: state.latest_version.clone(),
        update_available: update_available(state),
        last_check_at: state.last_check_at,
        last_error: state.last_error.clone(),
        notes: state.notes.clone(),
        source: source_label(state.mirror_prefix.as_deref().unwrap_or_default()),
        checked,
        not_modified,
        portable,
        setup_downloaded: staged.is_some(),
    }
}

/// 目录里是不是「安装版」：安装包会在程序目录放 uninstall.exe，绿色版 zip 里没有。
pub fn is_installer_directory(dir: &Path) -> bool {
    dir.join("uninstall.exe").is_file()
}

/// 应用更新的状态、检查与下载（网络传输可注入，便于单测）。
pub struct AppUpdater {
    dir: PathBuf,
    transport: Arc<dyn UpdateTransport>,
}

impl AppUpdater {
    /// 默认走系统 WinHTTP。
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_transport(dir, default_transport())
    }

    pub fn with_transport(dir: impl Into<PathBuf>, transport: Arc<dyn UpdateTransport>) -> Self {
        Self {
            dir: dir.into(),
            transport,
        }
    }

    pub fn state_path(&self) -> PathBuf {
        self.dir.join(STATE_FILE_NAME)
    }

    pub fn log_path(&self) -> PathBuf {
        self.dir.join(LOG_FILE_NAME)
    }

    /// 读取状态；文件缺失/损坏都退回默认值（更新状态坏掉不能拖垮应用）。
    pub fn load_state(&self) -> AppUpdateState {
        match fs::read_to_string(self.state_path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|err| {
                log::warn!("app-update.json 解析失败（{err}），按默认状态处理");
                AppUpdateState::default()
            }),
            Err(_) => AppUpdateState::default(),
        }
    }

    /// 原子写入：先写 .tmp 再 rename。
    pub fn save_state(&self, state: &AppUpdateState) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.state_path();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(state)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn persist(&self, state: &AppUpdateState) {
        if let Err(err) = self.save_state(state) {
            log::warn!("app-update.json 保存失败：{err}");
        }
    }

    /// 只读展示（不触网）。
    pub fn info(&self, installed: &str, portable: bool) -> AppUpdateInfo {
        let mut state = self.load_state();
        state.installed_version = Some(normalize_version(installed));
        info_from(&state, false, false, portable)
    }

    /// 检查更新。force = 设置页手动按钮（绕过每天一次的限制）。
    ///
    /// 网络失败不返回 Err：写进 last_error，由设置页显示「上次检查失败/可重试」。
    /// mirror = 引擎更新里设置的镜像前缀（一个设置同时管两件事）。
    pub fn check(
        &self,
        installed: &str,
        force: bool,
        now: i64,
        mirror: &str,
        portable: bool,
    ) -> AppUpdateInfo {
        let mut state = self.load_state();
        state.installed_version = Some(normalize_version(installed));
        state.mirror_prefix = Some(normalize_mirror_prefix(mirror));

        if !force && !should_auto_check(state.last_check_at, now) {
            return info_from(&state, false, false, portable);
        }

        state.last_check_at = Some(now);
        let url = mirror_join(mirror, RELEASE_API_URL);
        match self.transport.get_text(&url, state.etag.as_deref()) {
            Ok(text) if text.status == 304 => {
                state.last_error = None;
                let info = info_from(&state, true, true, portable);
                self.persist(&state);
                return info;
            }
            Ok(text) if (200..300).contains(&text.status) => match parse_latest_release(&text.body) {
                Ok(release) => {
                    state.latest_version = Some(release.version.clone());
                    state.tag_name = Some(release.tag_name.clone());
                    state.asset_name = Some(release.asset_name.clone());
                    state.asset_url = Some(release.asset_url.clone());
                    state.sums_url = release.sums_url.clone();
                    state.notes = Some(release.notes.clone());
                    state.setup_sha256 = release.digest.clone();
                    if let Some(etag) = text.etag.clone() {
                        state.etag = Some(etag);
                    }
                    state.last_error = None;
                    // 暂存包不是这次检查到的版本时作废，避免装上旧版本。
                    if state.staged_version.as_deref() != Some(release.version.as_str()) {
                        state.staged_setup = None;
                        state.staged_version = None;
                    }
                }
                Err(message) => state.last_error = Some(message),
            },
            Ok(text) => state.last_error = Some(format!("GitHub 返回 HTTP {}", text.status)),
            Err(err) => state.last_error = Some(format!("检查更新失败：{err}")),
        }

        let info = info_from(&state, true, false, portable);
        self.persist(&state);
        info
    }

    /// 已下载并通过校验的安装包路径（文件不在就返回 None）。
    pub fn staged_setup(&self) -> Option<PathBuf> {
        let state = self.load_state();
        let path = PathBuf::from(state.staged_setup?);
        path.is_file().then_some(path)
    }

    /// 下载并校验安装包，返回暂存路径；失败会删掉整个暂存目录并报错。
    pub fn stage_release(&self, version: &str, mirror: &str) -> Result<PathBuf> {
        let mut state = self.load_state();
        let version = normalize_version(version);
        if version.trim().is_empty() {
            return Err(FoundationError::InvalidInput("要安装的版本号为空".into()));
        }
        let checked_same = state
            .latest_version
            .as_deref()
            .map(|latest| latest == version)
            .unwrap_or(false);
        let asset_name = if checked_same {
            state
                .asset_name
                .clone()
                .unwrap_or_else(|| setup_asset_name(&version))
        } else {
            setup_asset_name(&version)
        };
        let asset_url = if checked_same {
            state
                .asset_url
                .clone()
                .map(|url| mirror_join(mirror, &url))
                .unwrap_or_else(|| {
                    mirror_join(
                        mirror,
                        &format!("{RELEASE_DOWNLOAD_BASE}/v{version}/{asset_name}"),
                    )
                })
        } else {
            mirror_join(
                mirror,
                &format!("{RELEASE_DOWNLOAD_BASE}/v{version}/{asset_name}"),
            )
        };

        let staging = new_staging_dir()?;
        let setup = staging.join(STAGED_SETUP_NAME);
        let outcome = self.download_and_verify(&asset_url, &asset_name, &setup, mirror, &mut state, &version);
        if let Err(err) = outcome {
            cleanup_staging(&staging);
            return Err(err);
        }

        state.staged_setup = Some(setup.display().to_string());
        state.staged_version = Some(version);
        self.persist(&state);
        Ok(setup)
    }

    fn download_and_verify(
        &self,
        asset_url: &str,
        asset_name: &str,
        setup: &Path,
        mirror: &str,
        state: &mut AppUpdateState,
        version: &str,
    ) -> Result<()> {
        self.transport.download(asset_url, setup).map_err(|err| {
            FoundationError::Process(format!(
                "第 1 步（下载 WebDAV Drive v{version} 安装包）失败：{err}"
            ))
        })?;

        let expected = match state.setup_sha256.clone() {
            Some(hex) if hex.len() == 64 => Some(hex),
            _ => self.fetch_expected_sha(state, asset_name, mirror)?,
        };
        let expected = expected.ok_or_else(|| {
            FoundationError::Process(
                "第 2 步（获取安装包校验值）失败：GitHub 既没有提供 digest，也没有 .sha256 旁文件"
                    .into(),
            )
        })?;
        let actual = sha256_file(setup)?;
        if !actual.eq_ignore_ascii_case(&expected) {
            return Err(FoundationError::Process(format!(
                "第 3 步（校验安装包）失败：SHA256 不匹配（期望 {expected}，实际 {actual}），已放弃本次更新"
            )));
        }
        Ok(())
    }

    /// digest 缺失时下载同名 .sha256 旁文件（内容形如「<hex>  <资产名>」）。
    fn fetch_expected_sha(
        &self,
        state: &mut AppUpdateState,
        asset_name: &str,
        mirror: &str,
    ) -> Result<Option<String>> {
        let Some(url) = state.sums_url.clone() else {
            return Ok(None);
        };
        let text = self
            .transport
            .get_text(&mirror_join(mirror, &url), None)
            .map_err(|err| {
                FoundationError::Process(format!("第 2 步（下载 SHA256 校验文件）失败：{err}"))
            })?;
        if !(200..300).contains(&text.status) {
            return Err(FoundationError::Process(format!(
                "第 2 步（下载 SHA256 校验文件）失败：HTTP {}",
                text.status
            )));
        }
        Ok(sha256_for(&text.body, asset_name))
    }
}

/// 更新器要执行的一次安装（由主程序写入命令行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPlan {
    /// 已下载并校验过的安装包。
    pub setup: PathBuf,
    /// 静默安装的目标目录（当前程序所在目录）。
    pub target_dir: PathBuf,
    /// 需要等待退出的主程序进程号。
    pub wait_pid: u32,
    /// 安装完成后要启动的程序。
    pub exe: PathBuf,
    /// 进度日志。
    pub log_path: PathBuf,
    /// 重启时带的参数（例如 --hidden）。
    pub restart_args: Vec<String>,
}

/// 解析更新器命令行；没有 APPLY_FLAG 时返回 Ok(None)（正常启动）。
///
/// 传进来的应当是和 `std::env::args()` 一样的完整命令行：**第一个元素是程序自身路径**，
/// 解析时会被跳过（否则会被当成未知参数，更新器一启动就退出）。
pub fn apply_plan_from_args<I>(args: I) -> std::result::Result<Option<ApplyPlan>, String>
where
    I: IntoIterator<Item = String>,
{
    let mut raw = args.into_iter();
    let _program = raw.next();
    let args: Vec<String> = raw.collect();

    // 正常启动时应用自己的参数（例如 --hidden）不归这里管：只有出现 APPLY_FLAG
    // 才按更新器命令行解析，否则原样交给主流程。
    if !args.iter().any(|arg| arg == APPLY_FLAG) {
        return Ok(None);
    }

    let mut setup = None;
    let mut target = None;
    let mut pid = None;
    let mut exe = None;
    let mut log = None;
    let mut restart_args = Vec::new();

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            APPLY_FLAG => {}
            "--setup" => setup = iter.next(),
            "--target" => target = iter.next(),
            "--pid" => {
                let value = iter.next().ok_or_else(|| "--pid 缺少取值".to_string())?;
                pid = Some(
                    value
                        .parse::<u32>()
                        .map_err(|err| format!("--pid 不是有效的进程号：{err}"))?,
                );
            }
            "--exe" => exe = iter.next(),
            "--log" => log = iter.next(),
            "--restart-arg" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "--restart-arg 缺少取值".to_string())?;
                restart_args.push(value);
            }
            "" => {}
            other => return Err(format!("未知参数：{other}")),
        }
    }

    let missing = |name: &str| format!("{APPLY_FLAG} 缺少 {name}");
    Ok(Some(ApplyPlan {
        setup: PathBuf::from(setup.ok_or_else(|| missing("--setup"))?),
        target_dir: PathBuf::from(target.ok_or_else(|| missing("--target"))?),
        wait_pid: pid.ok_or_else(|| missing("--pid"))?,
        exe: PathBuf::from(exe.ok_or_else(|| missing("--exe"))?),
        log_path: PathBuf::from(log.ok_or_else(|| missing("--log"))?),
        restart_args,
    }))
}

/// 追加一行更新日志（best-effort：更新器没有日志框架，只能写文件）。
pub fn log_line(path: &Path, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let line = format!("[{}] {message}\n", crate::engine_update::now_unix());
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

/// 把当前程序复制成临时更新器（改名 drive-updater.exe），返回副本路径。
pub fn copy_updater(current_exe: &Path) -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("{UPDATER_PREFIX}{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir)?;
    let dest = dir.join(UPDATER_EXE_NAME);
    fs::copy(current_exe, &dest)
        .map_err(|err| FoundationError::Process(format!("无法准备更新程序副本：{err}")))?;
    Ok(dest)
}

/// 拉起更新器（分离进程）：它等本进程退出后静默安装并重启应用。
pub fn spawn_updater(updater: &Path, plan: &ApplyPlan) -> Result<()> {
    let mut command = std::process::Command::new(updater);
    command
        .arg(APPLY_FLAG)
        .arg("--setup")
        .arg(&plan.setup)
        .arg("--target")
        .arg(&plan.target_dir)
        .arg("--pid")
        .arg(plan.wait_pid.to_string())
        .arg("--exe")
        .arg(&plan.exe)
        .arg("--log")
        .arg(&plan.log_path);
    for arg in &plan.restart_args {
        command.arg("--restart-arg").arg(arg);
    }
    if let Some(dir) = updater.parent() {
        command.current_dir(dir);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|err| FoundationError::Process(format!("无法启动更新程序：{err}")))?;
    Ok(())
}

/// 更新器主流程：等主程序退出 → 静默安装 → 重启 → 清理暂存。
pub fn run_apply_update(plan: &ApplyPlan) -> Result<()> {
    log_line(
        &plan.log_path,
        &format!(
            "更新器启动：等待应用（PID {}）退出后安装 {}",
            plan.wait_pid,
            plan.setup.display()
        ),
    );

    if !plan.setup.is_file() {
        let message = format!("安装包不存在：{}", plan.setup.display());
        log_line(&plan.log_path, &message);
        return Err(FoundationError::Process(message));
    }
    if !wait_for_exit(plan.wait_pid, WAIT_EXIT_TIMEOUT) {
        let message = "等待应用退出超时（120 秒），已放弃本次更新（可重新点击安装）".to_string();
        log_line(&plan.log_path, &message);
        return Err(FoundationError::Process(message));
    }
    log_line(&plan.log_path, "应用已退出，开始静默安装");
    std::thread::sleep(Duration::from_millis(500));

    let mut command = std::process::Command::new(&plan.setup);
    command.arg("/S");
    let target_arg = format!("/D={}", plan.target_dir.display());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // NSIS 规定 /D= 必须是原样参数：带引号会被当成目录名的一部分，必须放在最后。
        command.raw_arg(target_arg);
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    command.arg(target_arg);

    let status = command
        .spawn()
        .map_err(|err| {
            let message = format!("无法运行安装包 {}：{err}", plan.setup.display());
            log_line(&plan.log_path, &message);
            FoundationError::Process(message)
        })?
        .wait()
        .map_err(|err| {
            let message = format!("等待安装包结束时出错：{err}");
            log_line(&plan.log_path, &message);
            FoundationError::Process(message)
        })?;

    if !status.success() {
        let code = status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "未知".to_string());
        let message = format!("安装包运行失败（退出码 {code}），请手动重新安装");
        log_line(&plan.log_path, &message);
        return Err(FoundationError::Process(message));
    }

    log_line(&plan.log_path, "安装完成，正在重启应用");
    let mut restart = std::process::Command::new(&plan.exe);
    restart.args(&plan.restart_args);
    if let Some(dir) = plan.exe.parent() {
        restart.current_dir(dir);
    }
    restart.spawn().map_err(|err| {
        let message = format!("安装完成但无法重启应用 {}：{err}", plan.exe.display());
        log_line(&plan.log_path, &message);
        FoundationError::Process(message)
    })?;

    if let Some(staging) = plan.setup.parent() {
        // 安装包已经跑完，暂存目录可以直接删（删不掉也不影响使用）。
        let _ = fs::remove_dir_all(staging);
    }
    log_line(&plan.log_path, "更新流程结束");
    Ok(())
}

/// 等到进程退出（或超时）。
pub fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !process_running(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 进程是否还在运行。
///
/// 用 tasklist 的 CSV 输出判定：PID 是独立字段，不受系统语言影响；
/// 查不到时 tasklist 打印一句本地化提示（不含 PID），因此不会误判。
/// tasklist 本身不可用时按「还在运行」处理——绝不能以为应用退出了就动手安装。
fn process_running(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut command = std::process::Command::new("tasklist");
    command.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.output() {
        Ok(output) => {
            let expected = pid.to_string();
            let text = String::from_utf8_lossy(&output.stdout);
            text.lines()
                .any(|line| line.split(',').any(|field| field.trim_matches('"') == expected))
        }
        Err(err) => {
            log::warn!("无法查询进程 {pid} 状态（{err}），按仍在运行处理");
            true
        }
    }
}

/// 清理上次更新留下的临时目录（本次运行中正在使用的删不掉，忽略失败）。
pub fn cleanup_stale_updaters() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(UPDATER_PREFIX) || name.starts_with(STAGING_PREFIX) {
            let path = entry.path();
            if fs::remove_dir_all(&path).is_ok() {
                log::info!("已清理更新临时目录：{}", path.display());
            }
        }
    }
}

/// 把程序所在目录记进注册表，让安装包把「上次用的位置」当默认安装目录。
///
/// 走 reg.exe：仓库其它地方没有引入注册表 API，而 REG_SZ 由系统按 Unicode 写入，
/// 中文路径不会乱码。位置在临时目录（开发/测试副本）时跳过，避免污染注册表。
pub fn remember_install_location(dir: &Path) -> Result<()> {
    if dir.starts_with(std::env::temp_dir()) {
        log::info!("程序位于临时目录（{}），跳过安装位置记忆", dir.display());
        return Ok(());
    }
    let mut command = std::process::Command::new("reg");
    command
        .args([
            "add",
            INSTALL_LOCATION_KEY,
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
        ])
        .arg(dir)
        .arg("/f");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|err| FoundationError::Process(format!("无法记录安装位置：{err}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(FoundationError::Process(format!(
            "记录安装位置失败（reg 退出码 {:?}）：{}",
            output.status.code(),
            detail.trim()
        )));
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::engine_update::HttpText;

    /// 64 位 hex 的假 digest（真实 API 里是 sha256:<64hex>）。
    const DIGEST: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn release_json() -> String {
        format!(
            r#"{{
            "tag_name": "v0.1.9",
            "body": "变更\n- 支持应用自更新\n",
            "assets": [
                {{"name": "WebDAV.Drive_0.1.9_x64-setup.exe", "browser_download_url": "https://github.com/yashuamao/WebDavDrive/releases/download/v0.1.9/WebDAV.Drive_0.1.9_x64-setup.exe", "digest": "sha256:{DIGEST}"}},
                {{"name": "WebDAV.Drive_0.1.9_x64-setup.exe.sha256", "browser_download_url": "https://github.com/yashuamao/WebDavDrive/releases/download/v0.1.9/WebDAV.Drive_0.1.9_x64-setup.exe.sha256"}},
                {{"name": "WebDavDrive-0.1.9.zip", "browser_download_url": "https://github.com/yashuamao/WebDavDrive/releases/download/v0.1.9/WebDavDrive-0.1.9.zip"}}
            ]
        }}"#
        )
    }

    #[derive(Default)]
    struct FakeTransport {
        responses: Mutex<std::collections::VecDeque<std::result::Result<HttpText, String>>>,
        urls: Mutex<Vec<String>>,
        body: Mutex<Vec<u8>>,
    }

    impl FakeTransport {
        fn push(self: &Arc<Self>, status: u16, body: &str, etag: Option<&str>) -> Arc<Self> {
            self.responses.lock().unwrap().push_back(Ok(HttpText {
                status,
                body: body.to_string(),
                etag: etag.map(str::to_string),
            }));
            self.clone()
        }

        fn push_failure(self: &Arc<Self>, message: &str) -> Arc<Self> {
            self.responses
                .lock()
                .unwrap()
                .push_back(Err(message.to_string()));
            self.clone()
        }

        /// 下载（stage_release）会写入的字节。
        fn with_body(self: &Arc<Self>, bytes: &[u8]) -> Arc<Self> {
            *self.body.lock().unwrap() = bytes.to_vec();
            self.clone()
        }

        fn urls(&self) -> Vec<String> {
            self.urls.lock().unwrap().clone()
        }
    }

    impl UpdateTransport for FakeTransport {
        fn get_text(&self, url: &str, _etag: Option<&str>) -> Result<HttpText> {
            self.urls.lock().unwrap().push(url.to_string());
            match self.responses.lock().unwrap().pop_front() {
                Some(Ok(text)) => Ok(text),
                Some(Err(message)) => Err(FoundationError::Process(message)),
                None => Err(FoundationError::Process("没有预置响应".into())),
            }
        }

        fn download(&self, _url: &str, dest: &Path) -> Result<()> {
            fs::write(dest, self.body.lock().unwrap().clone())?;
            Ok(())
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "app-update-test-{tag}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ok_transport(body: &str) -> Arc<FakeTransport> {
        Arc::new(FakeTransport::default()).push(200, body, Some("\"etag-1\""))
    }

    // -- 解析 Release ------------------------------------------------------------

    #[test]
    fn parses_release_and_picks_setup_asset() {
        let release = parse_latest_release(&release_json()).unwrap();
        assert_eq!(release.version, "0.1.9");
        assert_eq!(release.tag_name, "v0.1.9");
        assert_eq!(release.asset_name, "WebDAV.Drive_0.1.9_x64-setup.exe");
        assert_eq!(release.digest.as_deref(), Some(DIGEST));
        assert!(release
            .asset_url
            .ends_with("/v0.1.9/WebDAV.Drive_0.1.9_x64-setup.exe"));
        assert!(release
            .sums_url
            .as_deref()
            .unwrap()
            .ends_with("WebDAV.Drive_0.1.9_x64-setup.exe.sha256"));
        assert!(release.notes.contains("应用自更新"));
    }

    #[test]
    fn setup_asset_name_is_the_github_dot_form() {
        assert_eq!(
            setup_asset_name("0.1.9"),
            "WebDAV.Drive_0.1.9_x64-setup.exe"
        );
    }

    #[test]
    fn release_without_setup_asset_is_rejected() {
        let body = r#"{"tag_name": "v0.1.9", "assets": [{"name": "WebDavDrive-0.1.9.zip", "browser_download_url": "https://example.com/z.zip"}]}"#;
        let err = parse_latest_release(body).unwrap_err();
        assert!(err.contains("没有 Windows 安装包资产"), "{err}");
    }

    #[test]
    fn release_api_message_is_reported() {
        let err = parse_latest_release(r#"{"message":"API rate limit exceeded"}"#).unwrap_err();
        assert!(err.contains("GitHub API 报错"), "{err}");
    }

    #[test]
    fn short_digest_is_ignored_so_sums_file_is_used() {
        let body = release_json().replace(DIGEST, "abc");
        let release = parse_latest_release(&body).unwrap();
        assert!(release.digest.is_none());
        assert!(release.sums_url.is_some());
    }

    // -- 检查更新 ----------------------------------------------------------------

    #[test]
    fn check_reports_available_update_from_official_source() {
        let dir = temp_dir("check");
        let transport = ok_transport(&release_json());
        let updater = AppUpdater::with_transport(&dir, transport.clone());

        let info = updater.check("0.1.8", true, 1000, "", false);
        assert!(info.checked && !info.not_modified);
        assert!(info.update_available);
        assert_eq!(info.latest_version.as_deref(), Some("0.1.9"));
        assert_eq!(info.installed_version, "0.1.8");
        assert!(info.last_error.is_none());
        assert_eq!(transport.urls(), vec![RELEASE_API_URL.to_string()]);
    }

    #[test]
    fn check_uses_mirror_prefix_when_set() {
        let dir = temp_dir("check-mirror");
        let transport = ok_transport(&release_json());
        let updater = AppUpdater::with_transport(&dir, transport.clone());

        let info = updater.check("0.1.8", true, 1000, "https://ghfast.top", false);
        assert!(info.update_available);
        let urls = transport.urls();
        assert_eq!(urls.len(), 1);
        assert!(urls[0].starts_with("https://ghfast.top/"), "{}", urls[0]);
        assert!(urls[0].ends_with(RELEASE_API_URL), "{}", urls[0]);
    }

    #[test]
    fn auto_check_is_skipped_within_a_day() {
        let dir = temp_dir("check-skip");
        let transport = ok_transport(&release_json());
        let updater = AppUpdater::with_transport(&dir, transport.clone());
        updater
            .save_state(&AppUpdateState {
                last_check_at: Some(1000),
                installed_version: Some("0.1.8".into()),
                ..Default::default()
            })
            .unwrap();

        let info = updater.check("0.1.8", false, 1500, "", false);
        assert!(!info.checked);
        assert!(transport.urls().is_empty());
    }

    #[test]
    fn not_modified_keeps_previous_result() {
        let dir = temp_dir("check-304");
        let transport = Arc::new(FakeTransport::default()).push(304, "", None);
        let updater = AppUpdater::with_transport(&dir, transport.clone());
        updater
            .save_state(&AppUpdateState {
                latest_version: Some("0.1.9".into()),
                installed_version: Some("0.1.8".into()),
                ..Default::default()
            })
            .unwrap();

        let info = updater.check("0.1.8", true, 2000, "", false);
        assert!(info.checked && info.not_modified);
        assert!(info.update_available);
        assert!(info.last_error.is_none());
    }

    #[test]
    fn http_failure_is_recorded_not_returned_as_error() {
        let dir = temp_dir("check-500");
        let transport = Arc::new(FakeTransport::default()).push(500, "", None);
        let updater = AppUpdater::with_transport(&dir, transport.clone());

        let info = updater.check("0.1.8", true, 1000, "", false);
        assert!(info.last_error.as_deref().unwrap().contains("HTTP 500"));
        assert!(!info.update_available);
        updater
            .save_state(&AppUpdateState {
                last_check_at: Some(1000),
                ..Default::default()
            })
            .unwrap();
    }

    #[test]
    fn transport_failure_is_recorded_not_returned_as_error() {
        let dir = temp_dir("check-fail");
        let transport = Arc::new(FakeTransport::default()).push_failure("连接被重置");
        let updater = AppUpdater::with_transport(&dir, transport.clone());

        let info = updater.check("0.1.8", true, 1000, "", false);
        let error = info.last_error.unwrap();
        assert!(error.contains("检查更新失败"), "{error}");
        assert!(error.contains("连接被重置"), "{error}");
    }

    // -- 下载与校验 --------------------------------------------------------------

    #[test]
    fn stage_release_downloads_verifies_digest_and_records_state() {
        let dir = temp_dir("stage");
        let bytes = b"setup-bytes".to_vec();
        let actual = crate::engine_update::sha256_hex(&bytes);
        let json = release_json().replace(DIGEST, &actual);
        let transport = ok_transport(&json).with_body(&bytes);
        let updater = AppUpdater::with_transport(&dir, transport.clone());
        updater.check("0.1.8", true, 1000, "", false);

        let staged = updater.stage_release("0.1.9", "").unwrap();
        assert_eq!(fs::read(&staged).unwrap(), bytes);
        assert_eq!(
            staged.file_name().unwrap().to_string_lossy(),
            STAGED_SETUP_NAME
        );
        assert_eq!(updater.staged_setup().as_deref(), Some(staged.as_path()));

        let info = updater.info("0.1.8", false);
        assert!(info.setup_downloaded);
        assert!(info.update_available);
    }

    #[test]
    fn stage_release_rejects_wrong_digest_and_cleans_staging() {
        let dir = temp_dir("stage-bad");
        let bytes = b"tampered".to_vec();
        let transport = ok_transport(&release_json()).with_body(&bytes);
        let updater = AppUpdater::with_transport(&dir, transport.clone());
        updater.check("0.1.8", true, 1000, "", false);

        let err = updater.stage_release("0.1.9", "").unwrap_err().to_string();
        assert!(err.contains("第 3 步（校验安装包）失败"), "{err}");
        assert!(updater.staged_setup().is_none());
    }

    #[test]
    fn stage_release_falls_back_to_sums_file_when_digest_missing() {
        let dir = temp_dir("stage-sums");
        let bytes = b"setup-bytes".to_vec();
        let actual = crate::engine_update::sha256_hex(&bytes);
        let json = release_json().replace(DIGEST, "too-short");
        let sums = format!("{actual}  WebDAV.Drive_0.1.9_x64-setup.exe\n");
        let transport = Arc::new(FakeTransport::default())
            .push(200, &json, Some("\"etag-1\""))
            .push(200, &sums, None)
            .with_body(&bytes);
        let updater = AppUpdater::with_transport(&dir, transport.clone());
        updater.check("0.1.8", true, 1000, "", false);

        let staged = updater.stage_release("0.1.9", "").unwrap();
        assert!(staged.is_file());
        assert_eq!(transport.urls().len(), 2);
        assert!(transport.urls()[1].ends_with(".exe.sha256"));
    }

    #[test]
    fn stage_release_requires_a_version() {
        let dir = temp_dir("stage-empty");
        let updater = AppUpdater::with_transport(&dir, Arc::new(FakeTransport::default()));
        assert!(updater.stage_release("", "").is_err());
    }

    // -- 安装版 / 绿色版 ----------------------------------------------------------

    #[test]
    fn installer_directory_is_detected_by_uninstaller() {
        let dir = temp_dir("portable");
        assert!(!is_installer_directory(&dir));
        fs::write(dir.join("uninstall.exe"), b"stub").unwrap();
        assert!(is_installer_directory(&dir));
    }

    // -- 更新器命令行与等待 ------------------------------------------------------

    /// 模拟真实命令行：第一项是程序自身路径（std::env::args() 的行为）。
    fn args(items: &[&str]) -> Vec<String> {
        let mut all = vec![r"C:\Temp\drive-updater.exe".to_string()];
        all.extend(items.iter().map(|item| item.to_string()));
        all
    }

    #[test]
    fn apply_plan_parses_internal_cli() {
        let parsed = apply_plan_from_args(args(&[
            "--apply-update",
            "--setup",
            r"C:\tmp\setup.exe",
            "--target",
            r"C:\Program Files\WebDAV Drive",
            "--pid",
            "4242",
            "--exe",
            r"C:\Program Files\WebDAV Drive\drive.exe",
            "--log",
            r"C:\ProgramData\WebDavDrive\app-update.log",
            "--restart-arg",
            "--hidden",
        ]))
        .unwrap()
        .unwrap();

        assert_eq!(parsed.wait_pid, 4242);
        assert_eq!(parsed.setup, PathBuf::from(r"C:\tmp\setup.exe"));
        assert_eq!(parsed.restart_args, vec!["--hidden".to_string()]);
    }

    #[test]
    fn normal_startup_has_no_apply_plan() {
        assert!(apply_plan_from_args(args(&["--hidden"])).unwrap().is_none());
        assert!(apply_plan_from_args(Vec::<String>::new()).unwrap().is_none());
    }

    #[test]
    fn apply_plan_reports_missing_or_unknown_arguments() {
        let err = apply_plan_from_args(args(&["--apply-update", "--setup", "s.exe"]))
            .unwrap_err();
        assert!(err.contains("--target"), "{err}");
        let err = apply_plan_from_args(args(&["--apply-update", "--setup"])).unwrap_err();
        assert!(err.contains("--setup"), "{err}");
        let err = apply_plan_from_args(args(&["--apply-update", "--pid", "abc"]))
            .unwrap_err();
        assert!(err.contains("--pid"), "{err}");
        let err = apply_plan_from_args(args(&["--apply-update", "--nope"])).unwrap_err();
        assert!(err.contains("未知参数"), "{err}");
    }

    #[test]
    fn apply_plan_skips_the_program_path_argument() {
        // 回归：std::env::args() 的第一个元素是程序路径，曾被当成未知参数，
        // 导致更新器一启动就报“未知参数：...drive-updater.exe”并退出。
        let parsed = apply_plan_from_args(args(&[
            "--apply-update",
            "--setup",
            "s.exe",
            "--target",
            "t",
            "--pid",
            "7",
            "--exe",
            "e.exe",
            "--log",
            "l.log",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(parsed.setup, PathBuf::from("s.exe"));
        assert_eq!(parsed.wait_pid, 7);
        // 只有程序路径（正常启动）时不算更新器命令行。
        assert!(apply_plan_from_args(args(&[])).unwrap().is_none());
    }

    #[test]
    fn process_running_knows_itself_and_unused_pids() {
        assert!(process_running(std::process::id()));
        assert!(!process_running(0));
    }

    #[test]
    fn wait_for_exit_returns_true_once_the_child_is_gone() {
        let mut child = std::process::Command::new("ping")
            .args(["-n", "2", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("需要能用 ping 做等待测试");
        let pid = child.id();
        assert!(wait_for_exit(pid, Duration::from_secs(20)));
        let _ = child.wait();
    }

    #[test]
    fn wait_for_exit_times_out_for_a_live_process() {
        assert!(!wait_for_exit(std::process::id(), Duration::from_millis(600)));
    }

    // -- 临时目录与安装位置 ------------------------------------------------------

    #[test]
    fn cleanup_stale_updaters_only_removes_own_directories() {
        let temp = std::env::temp_dir();
        let updater_dir = temp.join(format!("{UPDATER_PREFIX}{}", uuid::Uuid::new_v4()));
        let staging_dir = temp.join(format!("{STAGING_PREFIX}{}", uuid::Uuid::new_v4()));
        let other_dir = temp.join(format!("webdav-drive-keep-{}", uuid::Uuid::new_v4()));
        for dir in [&updater_dir, &staging_dir, &other_dir] {
            fs::create_dir_all(dir).unwrap();
        }

        cleanup_stale_updaters();
        assert!(!updater_dir.exists());
        assert!(!staging_dir.exists());
        assert!(other_dir.exists());
        let _ = fs::remove_dir_all(&other_dir);
    }

    #[test]
    fn remembering_temp_locations_is_skipped() {
        let dir = temp_dir("location");
        // 临时目录直接返回 Ok，不写注册表（开发/测试副本不污染安装位置记忆）。
        assert!(remember_install_location(&dir).is_ok());
    }

    #[test]
    fn log_line_appends_one_line_per_call() {
        let dir = temp_dir("log");
        let path = dir.join(LOG_FILE_NAME);
        log_line(&path, "第一行");
        log_line(&path, "第二行");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("第一行") && text.contains("第二行"));
        assert!(text.starts_with('['));
    }
}
