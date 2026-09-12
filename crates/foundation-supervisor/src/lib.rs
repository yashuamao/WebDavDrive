//! foundation-supervisor：外部进程托管。
//!
//! 负责"把一个外部引擎进程安全地拉起来，等到它真的可用，再干净地停掉"：
//! - spawn + 环境变量注入（口令等敏感值不走 argv，Windows 命令行可能被其他用户读到）
//! - 就绪探测（TCP/HTTP，见 `probe`）
//! - 宿主死亡时回收：把进程交给 `ChildGuard`（Windows 实现为 Job Object）
//! - 停止：先给宽限期，再强杀；调用方应先通过引擎自身 API 优雅停止
//!
//! 本 crate 不知道引擎是什么，也不解析任何业务参数。

mod probe;

pub use probe::{port_open, probe, Readiness};

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use foundation_core::{ChildGuard, FoundationError, Result};

/// 启动规格。
pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// 环境变量（口令/密钥走这里，不进 argv）。
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
    pub readiness: Readiness,
    pub ready_timeout: Duration,
    pub stop_grace: Duration,
    /// 宿主死亡回收守卫（Windows：Job Object）。
    pub guard: Option<Arc<dyn ChildGuard>>,
}

impl Spec {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            readiness: Readiness::None,
            ready_timeout: Duration::from_secs(25),
            stop_grace: Duration::from_secs(8),
            guard: None,
        }
    }
}

/// 被托管的子进程。
pub struct ManagedChild {
    child: Child,
    /// 必须持有：guard 一旦被 drop，Job 句柄关闭会立刻杀掉子进程。
    guard: Option<Arc<dyn ChildGuard>>,
    readiness: Readiness,
    stop_grace: Duration,
}

impl ManagedChild {
    pub fn spawn(spec: &Spec) -> Result<Self> {
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .envs(spec.env.iter().cloned())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }

        let child = command.spawn().map_err(|err| {
            FoundationError::Process(format!("无法启动 {}：{err}", spec.program.display()))
        })?;

        if let Some(guard) = &spec.guard {
            if !guard.assign(child.id()) {
                log::warn!(
                    "无法把进程 {}（pid {}）加入 {}；宿主被强杀时可能留下孤儿",
                    spec.program.display(),
                    child.id(),
                    guard.name()
                );
            }
        }

        Ok(Self {
            child,
            guard: spec.guard.clone(),
            readiness: spec.readiness.clone(),
            stop_grace: spec.stop_grace,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// 守卫实现名（日志/诊断用；同时保证 guard 句柄确实被持有）。
    pub fn guard_name(&self) -> Option<&'static str> {
        self.guard.as_ref().map(|guard| guard.name())
    }

    /// 进程是否仍在运行（顺带回收僵尸状态）。
    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// 轮询直到就绪；进程提前退出或超时都返回明确错误。
    pub fn wait_ready(&mut self, spec: &Spec) -> Result<()> {
        let deadline = Instant::now() + spec.ready_timeout;
        let mut last = "尚未开始探测".to_string();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                let hint = readiness_addr(&self.readiness)
                    .filter(|addr| port_open(addr, Duration::from_millis(300)))
                    .map(|addr| format!("；{addr} 已有进程监听，可能已有实例在运行"))
                    .unwrap_or_default();
                return Err(FoundationError::Process(format!(
                    "进程启动后立即退出（{status}）{hint}"
                )));
            }

            match probe(&self.readiness, Duration::from_millis(800)) {
                Ok(()) => return Ok(()),
                Err(err) => last = err,
            }

            if Instant::now() >= deadline {
                return Err(FoundationError::Process(format!(
                    "等待就绪超时（{:?}）：{last}",
                    spec.ready_timeout
                )));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// spawn + wait_ready 的组合。
    pub fn spawn_ready(spec: &Spec) -> Result<Self> {
        let mut child = Self::spawn(spec)?;
        if let Err(err) = child.wait_ready(spec) {
            let _ = child.stop();
            return Err(err);
        }
        Ok(child)
    }

    /// 停止：先宽限，再强杀。调用方应先通过引擎 API 优雅停止。
    pub fn stop(&mut self) -> Result<()> {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return Ok(());
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + self.stop_grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                Ok(None) => {
                    let _ = self.child.kill();
                    return self.child.wait().map(|_| ()).map_err(Into::into);
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
}

impl std::fmt::Debug for ManagedChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedChild")
            .field("pid", &self.child.id())
            .field("guard", &self.guard_name())
            .finish_non_exhaustive()
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        // 不让子进程跟着孤儿化；guard 仍在时内核也会兜底
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
        }
    }
}

fn readiness_addr(readiness: &Readiness) -> Option<&str> {
    match readiness {
        Readiness::Tcp { addr } => Some(addr),
        Readiness::Http { url } => url
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next()),
        Readiness::None => None,
    }
}
