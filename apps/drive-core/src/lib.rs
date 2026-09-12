//! drive-core：webdav-drive 应用核心（不含 Tauri）。
//!
//! 分层：`model`（连接语义）→ `params`（rclone 参数映射）→ `store`（配置持久化）
//! → `provider`（挂载引擎适配）→ `service`（应用服务编排）。
//! Tauri 宿主只负责把这些能力暴露成命令与界面。

pub mod autostart;
pub mod model;
pub mod params;
pub mod provider;
pub mod service;
pub mod store;

pub use autostart::AutostartStatus;
pub use model::{Connection, ConnectionInput, ConnectionView};
pub use provider::{
    EngineConfig, EngineStatus, MountProvider, MountRecord, ProbeReport, RcloneProvider, RcloneRc,
};
pub use service::{AppService, AppStatus};
pub use store::ProfileStore;
