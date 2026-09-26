//! rclone provider：托管 `rclone rcd`，经 RC API 管 remote 与挂载。
//!
//! 行为规格（docs/requirements.md BR-4/BR-6）：
//! - 配置文件静态加密，密钥由 `SecretStore` 保护，经 `--password-command` 提供给 rclone；
//! - 密钥缺失/损坏必须拒绝启动，绝不静默重建；
//! - RC 口令不进 argv（用 RCLONE_RC_USER/RCLONE_RC_PASS 环境变量）；
//! - 引擎放入 Job Object，宿主被强杀时回收（Windows）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use foundation_core::{ChildGuard, FoundationError, Result};
use foundation_secrets::{KeyRing, SecretStore};
use foundation_supervisor::{ManagedChild, Readiness, Spec};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{EngineStatus, MountProvider, MountRecord, ProbeReport, RcloneRc};
use crate::model::Connection;
use crate::params::build_mount_params;

const ENCRYPTED_MARKER: &str = "# Encrypted rclone configuration File";

/// 引擎运行配置（由宿主注入，便于打包与测试）。
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// 显式 rclone 路径；None 时按 exe 同目录/bin、PATH、RCLONE_EXE 查找。
    pub rclone_path: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub rc_addr: String,
    pub rc_user: String,
    pub rc_pass: String,
    /// 提供给 rclone `--password-command` 的完整命令行（通常是本程序的 pwcmd 工具）。
    pub password_command: String,
    pub ready_timeout: Duration,
}

impl EngineConfig {
    pub fn new(data_dir: impl Into<PathBuf>, rc_addr: impl Into<String>) -> Self {
        Self {
            rclone_path: None,
            data_dir: data_dir.into(),
            rc_addr: rc_addr.into(),
            rc_user: "webdav-drive".into(),
            rc_pass: format!("{}", Uuid::new_v4().simple()),
            password_command: String::new(),
            ready_timeout: Duration::from_secs(25),
        }
    }

    pub fn config_path(&self) -> PathBuf {
        self.data_dir.join("rclone.conf")
    }

    pub fn key_path(&self) -> PathBuf {
        self.data_dir.join("config_key.enc")
    }
}

struct Engine {
    child: ManagedChild,
    rc: RcloneRc,
}

pub struct RcloneProvider {
    config: EngineConfig,
    key_store: Arc<dyn SecretStore>,
    engine: Mutex<Option<Engine>>,
}

impl RcloneProvider {
    pub fn new(config: EngineConfig, key_store: Arc<dyn SecretStore>) -> Self {
        Self {
            config,
            key_store,
            engine: Mutex::new(None),
        }
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// 查找 rclone.exe：显式路径 → RCLONE_EXE → exe 同目录/bin → PATH。
    pub fn find_engine(&self) -> Option<PathBuf> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(path) = &self.config.rclone_path {
            candidates.push(path.clone());
        }
        if let Ok(hint) = std::env::var("RCLONE_EXE") {
            if !hint.trim().is_empty() {
                candidates.push(PathBuf::from(hint));
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("rclone.exe"));
                candidates.push(dir.join("bin").join("rclone.exe"));
            }
        }
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                candidates.push(dir.join("rclone.exe"));
            }
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn config_is_encrypted(&self) -> Result<bool> {
        use std::io::Read;
        let config_path = self.config.config_path();
        if !config_path.exists() {
            return Ok(false);
        }
        let mut file = std::fs::File::open(config_path)?;
        let mut head = [0u8; 96];
        let read = file.read(&mut head)?;
        Ok(String::from_utf8_lossy(&head[..read]).starts_with(ENCRYPTED_MARKER))
    }

    /// 保证密钥可用并返回它。缺失而配置已加密 → `key_unavailable`（AC-26）。
    pub fn ensure_key(&self, encrypted_payload_exists: bool) -> Result<String> {
        let keyring = KeyRing::new(self.key_store.clone(), self.config.key_path());
        Ok(keyring
            .ensure_key(encrypted_payload_exists)?
            .secret()
            .to_string())
    }

    /// 在宿主窗口启动前验证配置密钥状态，避免把加密配置缺钥匙的问题拖到首次操作。
    pub fn validate_startup(&self) -> Result<()> {
        std::fs::create_dir_all(&self.config.data_dir)?;
        let encrypted = self.config_is_encrypted()?;
        if encrypted || self.config.key_path().exists() {
            self.ensure_key(encrypted)?;
        }
        Ok(())
    }

    /// 确保配置存在且已静态加密（AC-37）。必须在启动 rclone 之前完成。
    fn prepare_config(&self, engine: &Path) -> Result<()> {
        std::fs::create_dir_all(&self.config.data_dir)?;
        let config_path = self.config.config_path();
        if !config_path.exists() {
            std::fs::write(&config_path, b"")?;
        }

        let encrypted = self.config_is_encrypted()?;
        self.ensure_key(encrypted)?;
        if encrypted {
            return Ok(());
        }
        if self.config.password_command.trim().is_empty() {
            return Err(FoundationError::InvalidInput(
                "未配置 --password-command，无法启用 rclone 配置加密".into(),
            ));
        }

        let mut command = Command::new(engine);
        command
            .args([
                "config",
                "encryption",
                "set",
                "--config",
                config_path.to_string_lossy().as_ref(),
                "--password-command",
                self.config.password_command.as_str(),
            ])
            .stdin(Stdio::null());
        foundation_core::process::hide_console(&mut command);
        let output = command
            .output()
            .map_err(|err| FoundationError::Process(format!("执行 rclone 配置加密失败：{err}")))?;
        if !output.status.success() {
            return Err(FoundationError::Process(format!(
                "rclone 配置加密失败：{} {}",
                String::from_utf8_lossy(&output.stdout).trim(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        log::info!("rclone.conf 已启用静态加密：{}", config_path.display());
        Ok(())
    }

    fn build_engine(&self) -> Result<Engine> {
        let path = self.find_engine().ok_or_else(|| {
            FoundationError::Process(
                "找不到 rclone.exe：请放到程序同目录 bin/ 下，或用 RCLONE_EXE 指定".into(),
            )
        })?;
        self.prepare_config(&path)?;

        let mut spec = Spec::new(&path);
        spec.args = vec![
            "rcd".into(),
            "--rc-addr".into(),
            self.config.rc_addr.clone(),
            "--config".into(),
            self.config.config_path().to_string_lossy().to_string(),
            "--password-command".into(),
            self.config.password_command.clone(),
            "--log-level".into(),
            "ERROR".into(),
        ];
        // 口令走环境变量，不进 argv
        spec.env = vec![
            ("RCLONE_RC_USER".into(), self.config.rc_user.clone()),
            ("RCLONE_RC_PASS".into(), self.config.rc_pass.clone()),
        ];
        // WinFsp 装在当前会话 PATH 更新之前时，rclone 会找不到 winfsp-x64.dll；
        // 这里显式把安装目录注入子进程 PATH（有则加，无则不动）。
        #[cfg(windows)]
        if let Some(dir) = winfsp_bin_dir() {
            let mut path = dir.to_string_lossy().to_string();
            if let Ok(existing) = std::env::var("PATH") {
                let already = existing
                    .to_ascii_lowercase()
                    .contains(&dir.to_string_lossy().to_ascii_lowercase());
                if !already && !existing.is_empty() {
                    path.push(';');
                    path.push_str(&existing);
                }
            }
            spec.env.push(("PATH".into(), path));
        }
        spec.readiness = Readiness::Http {
            url: format!("http://{}/core/version", self.config.rc_addr),
        };
        spec.ready_timeout = self.config.ready_timeout;
        spec.guard = build_guard()?;

        let child = ManagedChild::spawn_ready(&spec)?;
        let rc = RcloneRc::new(
            self.config.rc_addr.clone(),
            self.config.rc_user.clone(),
            self.config.rc_pass.clone(),
        );
        log::info!(
            "rclone rcd 就绪：{}（{}）",
            self.config.rc_addr,
            path.display()
        );
        Ok(Engine { child, rc })
    }
}

/// 探测 WinFsp 安装目录（目录下带 winfsp-x64.dll 才算数；System32 与 WinFsp\bin 都兼容）。
#[cfg(windows)]
fn winfsp_bin_dir() -> Option<PathBuf> {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| String::from("C:\\Windows"));
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(Path::new(&system_root).join("System32"));
    candidates.push(PathBuf::from("C:\\Program Files (x86)\\WinFsp\\bin"));
    candidates.push(PathBuf::from("C:\\Program Files\\WinFsp\\bin"));
    candidates.into_iter().find(|dir| {
        ["winfsp-x64.dll", "winfsp-a64.dll"]
            .iter()
            .any(|dll| dir.join(dll).exists())
    })
}

#[cfg(windows)]
fn build_guard() -> Result<Option<Arc<dyn ChildGuard>>> {
    use foundation_windows::JobObject;
    Ok(Some(Arc::new(JobObject::kill_on_close()?)))
}

#[cfg(not(windows))]
fn build_guard() -> Result<Option<Arc<dyn ChildGuard>>> {
    Ok(None)
}

impl RcloneProvider {
    fn with_rc<T>(&self, f: impl FnOnce(&RcloneRc) -> Result<T>) -> Result<T> {
        self.ensure_started()?;
        let guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        let engine = guard
            .as_ref()
            .ok_or_else(|| FoundationError::Process("rclone 尚未启动".into()))?;
        f(&engine.rc)
    }

    fn sync_remote_with(
        &self,
        rc: &RcloneRc,
        connection: &Connection,
        password: &str,
    ) -> Result<()> {
        let mut parameters = Map::new();
        parameters.insert("url".into(), Value::String(connection.url.clone()));
        parameters.insert(
            "vendor".into(),
            Value::String(if connection.vendor.is_empty() {
                "other".into()
            } else {
                connection.vendor.clone()
            }),
        );
        parameters.insert("user".into(), Value::String(connection.user.clone()));
        parameters.insert("pass".into(), Value::String(password.to_string()));

        // rclone 的 config/get 对不存在的 remote 返回 200 + `{}`（不是错误），
        // 所以必须按响应内容判断，不能按调用是否成功判断。
        let exists = rc
            .config_get(&connection.remote)
            .ok()
            .and_then(|value| value.get("type").cloned())
            .is_some();

        if exists {
            rc.config_update(&connection.remote, parameters)?;
            log::info!("更新 rclone remote：{}", connection.remote);
            return Ok(());
        }

        match rc.config_create(&connection.remote, "webdav", parameters.clone()) {
            Ok(_) => {
                log::info!("新建 rclone remote：{}", connection.remote);
                Ok(())
            }
            Err(create_err) => {
                // 并发创建/残留状态：回退到 update，仍失败则返回最初的错误
                if rc
                    .config_get(&connection.remote)
                    .map(|value| value.get("type").is_some())
                    .unwrap_or(false)
                {
                    rc.config_update(&connection.remote, parameters)?;
                    log::info!("remote 已存在，改为更新：{}", connection.remote);
                    Ok(())
                } else {
                    Err(create_err)
                }
            }
        }
    }
}

impl RcloneProvider {
    /// 当前引擎进程 pid（未启动为 None）；诊断与回收测试用。
    pub fn engine_pid(&self) -> Option<u32> {
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        let running = guard
            .as_mut()
            .map(|engine| engine.child.is_running())
            .unwrap_or(false);
        if running {
            guard.as_ref().map(|engine| engine.child.pid())
        } else {
            guard.take();
            None
        }
    }

    /// 读取 remote 的实际配置（诊断/测试用；pass 是 rclone obscure 后的值）。
    pub fn remote_config(&self, name: &str) -> Result<Value> {
        self.with_rc(|rc| rc.config_get(name))
    }
}

/// 从 rclone version 的输出里取版本号（第一行形如 `rclone v1.75.1`）。
pub fn parse_engine_version(output: &str) -> Option<String> {
    let first = output.lines().next()?.trim();
    let token = first.split_whitespace().nth(1)?;
    let version = token.trim_start_matches('v');
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

impl MountProvider for RcloneProvider {
    fn id(&self) -> &'static str {
        "rclone"
    }

    fn engine_status(&self) -> EngineStatus {
        let path = self.find_engine();
        let mut status = EngineStatus {
            installed: path.is_some(),
            path: path.as_ref().map(|p| p.to_string_lossy().to_string()),
            version: None,
            running: false,
            rc_addr: Some(self.config.rc_addr.clone()),
        };
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        let mut exited = false;
        if let Some(engine) = guard.as_mut() {
            if !engine.child.is_running() {
                exited = true;
            } else {
                match engine.rc.version() {
                    Ok(version) => {
                        status.running = true;
                        status.version = version
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                    Err(err) => {
                        // 进程终止与 RC 端口关闭之间存在短暂竞态；探测失败后再回收一次状态。
                        if !engine.child.is_running() {
                            exited = true;
                        } else {
                            log::warn!("rclone 进程仍在，但 RC 健康检查失败：{err}");
                        }
                    }
                }
            }
        }
        if exited {
            guard.take();
            log::warn!("检测到 rclone 进程已退出；下次操作将自动重启引擎");
        }
        status
    }

    /// 替换目标：优先用正在使用的那个 rclone.exe；没有现成的就装到 exe 同目录
    /// （与打包布局一致：dist\WebDavDrive-<版本>\drive.exe + rclone.exe）。
    fn engine_install_path(&self) -> PathBuf {
        if let Some(path) = self.find_engine() {
            return path;
        }
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("rclone.exe")))
            .unwrap_or_else(|| PathBuf::from("rclone.exe"))
    }

    /// 引擎版本：进程在跑就问 RC（零额外进程），否则直接执行 rclone version。
    fn installed_version(&self) -> Result<Option<String>> {
        {
            let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(engine) = guard.as_mut() {
                if engine.child.is_running() {
                    if let Ok(payload) = engine.rc.version() {
                        let version = payload
                            .get("version")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        return Ok(version);
                    }
                }
            }
        }
        let Some(path) = self.find_engine() else {
            return Ok(None);
        };
        let mut command = Command::new(&path);
        command.arg("version").stdin(Stdio::null());
        foundation_core::process::hide_console(&mut command);
        let output = command.output().map_err(|err| {
            FoundationError::Process(format!("无法执行 {} version：{err}", path.display()))
        })?;
        Ok(parse_engine_version(&String::from_utf8_lossy(&output.stdout)))
    }

    fn options_get(&self) -> Result<Value> {
        self.with_rc(|rc| rc.options_get())
    }

    fn ensure_started(&self) -> Result<()> {
        let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(engine) = guard.as_mut() {
            if engine.child.is_running() {
                return Ok(());
            }
            log::warn!("rclone 进程已意外退出，正在重新启动");
            guard.take();
        }
        let engine = self.build_engine()?;
        *guard = Some(engine);
        Ok(())
    }

    fn ensure_remote(&self, connection: &Connection, password: &str) -> Result<()> {
        self.with_rc(|rc| self.sync_remote_with(rc, connection, password))
    }

    fn probe(&self, connection: &Connection, password: &str) -> Result<ProbeReport> {
        let fs = format!("{}:", connection.remote);
        self.with_rc(|rc| {
            self.sync_remote_with(rc, connection, password)?;
            let listing = rc.list_root(&fs)?;
            let entries = listing
                .get("list")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let sample = entries
                .iter()
                .take(10)
                .filter_map(|entry| entry.get("Name").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            Ok(ProbeReport {
                entries: entries.len(),
                sample,
            })
        })
    }

    fn mount(&self, connection: &Connection, password: &str) -> Result<MountRecord> {
        let params = build_mount_params(connection)?;
        self.with_rc(|rc| {
            self.sync_remote_with(rc, connection, password)?;
            let result = rc.mount(
                &params.fs,
                &params.mount_point,
                &params.vfs,
                &params.mount_opts,
                &params.flat,
            )?;
            let mount_point = result
                .get("mountPoint")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| params.mount_point.clone());
            Ok(MountRecord {
                fs: params.fs.clone(),
                mount_point,
            })
        })
    }

    fn unmount(&self, mount_point: &str) -> Result<()> {
        self.with_rc(|rc| rc.unmount(mount_point).map(|_| ()))
    }

    fn list(&self) -> Result<Vec<MountRecord>> {
        self.with_rc(|rc| rc.list_mounts())
    }

    fn delete_remote(&self, remote: &str) -> Result<()> {
        self.with_rc(|rc| match rc.config_delete(remote) {
            Ok(_) => Ok(()),
            Err(err) => {
                let text = err.to_string().to_lowercase();
                if text.contains("couldn't find") || text.contains("not found") {
                    log::warn!("remote {remote} 已不存在，视为删除成功");
                    Ok(())
                } else {
                    Err(err)
                }
            }
        })
    }

    fn shutdown(&self) -> Result<()> {
        let engine = {
            let mut guard = self.engine.lock().unwrap_or_else(|p| p.into_inner());
            guard.take()
        };
        if let Some(mut engine) = engine {
            let mut cleanup_error: Option<String> = None;

            match engine.rc.list_mounts() {
                Ok(mounts) => {
                    for mount in mounts {
                        if let Err(err) = engine.rc.unmount(&mount.mount_point) {
                            log::error!("退出时卸载 {} 失败：{err}", mount.mount_point);
                            cleanup_error.get_or_insert_with(|| err.to_string());
                        }
                    }
                }
                Err(err) => {
                    log::error!("退出时无法读取挂载列表：{err}");
                    cleanup_error.get_or_insert_with(|| err.to_string());
                }
            }

            let quit_error = engine.rc.quit().err();
            let exited = match engine.child.wait_for_exit() {
                Ok(exited) => exited,
                Err(err) => {
                    cleanup_error.get_or_insert_with(|| err.to_string());
                    false
                }
            };
            if !exited {
                if let Some(err) = quit_error {
                    log::warn!("rclone 未响应 core/quit：{err}；改为强制停止");
                }
                engine.child.stop()?;
            }
            log::info!("rclone rcd 已停止");

            if let Some(err) = cleanup_error {
                return Err(FoundationError::Process(format!(
                    "引擎已停止，但退出前清理挂载失败：{err}"
                )));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_engine_version_from_cli_output() {
        // rclone version 的真实输出（第一行是版本，后面是 os/version 等）。
        let output = "rclone v1.75.1\n- os/version: Microsoft Windows 11 Pro (64 bit)\n";
        assert_eq!(parse_engine_version(output).as_deref(), Some("1.75.1"));
        assert_eq!(parse_engine_version("rclone v1.60.0").as_deref(), Some("1.60.0"));
        assert_eq!(parse_engine_version(""), None);
        assert_eq!(parse_engine_version("rclone"), None);
        assert_eq!(parse_engine_version("rclone v"), None);
    }
}
