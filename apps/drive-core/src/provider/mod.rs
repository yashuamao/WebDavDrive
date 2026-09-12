//! 挂载引擎抽象与首个实现（rclone）。
//!
//! 应用层只依赖 `MountProvider`，后续可加入 Windows 原生 WebClient provider
//! （见 `docs/adr/0002-mount-engine.md`）。

mod rc;
mod rclone;

pub use rc::RcloneRc;
pub use rclone::{EngineConfig, RcloneProvider};

use foundation_core::Result;
use serde::Serialize;

use crate::model::Connection;

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct EngineStatus {
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub running: bool,
    pub rc_addr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProbeReport {
    pub entries: usize,
    pub sample: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MountRecord {
    pub fs: String,
    pub mount_point: String,
}

pub trait MountProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// 引擎安装/运行状态（不触发启动）。
    fn engine_status(&self) -> EngineStatus;

    /// 幂等启动引擎；失败返回明确错误。
    fn ensure_started(&self) -> Result<()>;

    /// 建/更新 remote（不挂载）。
    fn ensure_remote(&self, connection: &Connection, password: &str) -> Result<()>;

    /// 探测连接可用性（列目标根目录）。
    fn probe(&self, connection: &Connection, password: &str) -> Result<ProbeReport>;

    /// 挂载（内部会先同步 remote）。
    fn mount(&self, connection: &Connection, password: &str) -> Result<MountRecord>;

    fn unmount(&self, mount_point: &str) -> Result<()>;

    fn list(&self) -> Result<Vec<MountRecord>>;

    /// 删除 remote（连接删除流程用）。
    fn delete_remote(&self, remote: &str) -> Result<()>;

    /// 停止引擎并清理（进程退出时调用）。
    fn shutdown(&self) -> Result<()>;
}
