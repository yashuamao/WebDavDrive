//! 应用服务：把 store 与 provider 编排成用例（保存/挂载/卸载/删除/状态）。
//!
//! 删除连接必须防孤儿（AC-10）：remote 删不掉就保留连接并报错，
//! 不能让 rclone.conf 里留下带凭据、永远无人清理的 remote。

use std::sync::Arc;

use foundation_core::{FoundationError, Result};
use serde::Serialize;

use crate::model::{Connection, ConnectionInput, ConnectionView};
use crate::params::build_mount_params;
use crate::provider::{EngineStatus, MountProvider, MountRecord, ProbeReport};
use crate::store::ProfileStore;

pub struct AppService {
    store: ProfileStore,
    provider: Arc<dyn MountProvider>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub engine: EngineStatus,
    pub mounts: Vec<MountRecord>,
    pub profile_count: usize,
    pub secrets: String,
}

impl AppService {
    pub fn new(store: ProfileStore, provider: Arc<dyn MountProvider>) -> Self {
        Self { store, provider }
    }

    pub fn store(&self) -> &ProfileStore {
        &self.store
    }

    pub fn provider(&self) -> &Arc<dyn MountProvider> {
        &self.provider
    }

    pub fn list(&self) -> Vec<ConnectionView> {
        self.store.list().iter().map(|c| c.view()).collect()
    }

    pub fn get_view(&self, id: &str) -> Option<ConnectionView> {
        self.store.get(id).map(|c| c.view())
    }

    /// 保存连接并同步 remote；remote 同步失败时连接已保存，向上报错由 UI 提示重试。
    ///
    /// 若这条连接正处于挂载状态、且本次改动会影响挂载行为（盘符、卷标、VFS 缓存模式、
    /// 目录缓存时间、只读、附加参数、地址/账号…），保存后会自动卸载并按新设置重新挂载。
    ///
    /// 为什么必须在这里做：rclone 的 RC API 只能创建/销毁挂载，没有「修改活动挂载的
    /// vfs 选项」的接口（`options/set` 只改全局默认值），所以不重新挂载的话，用户改完
    /// 目录缓存时间再保存，已经在用的盘上不会有任何变化——看起来就像这个设置无效。
    pub fn upsert(&self, input: ConnectionInput) -> Result<ConnectionView> {
        let existing = input.id.as_deref().and_then(|id| self.store.get(id));
        let connection = self.store.upsert(input)?;
        let password = self.store.reveal_password(&connection)?;
        self.provider.ensure_remote(&connection, &password)?;
        if let Some(existing) = existing {
            self.reapply_if_mounted(&existing, &connection, &password)?;
        }
        Ok(connection.view())
    }

    /// 设置变了且这条连接正在挂载中 → 卸载再挂载，让新设置立刻生效。
    ///
    /// 失败时连接已经保存（与上面 remote 同步失败的约定一致），错误消息会说明当前
    /// 处于「旧挂载还在」还是「已卸载但没挂上」，用户据此决定手动重挂还是直接重试。
    fn reapply_if_mounted(
        &self,
        previous: &Connection,
        connection: &Connection,
        password: &str,
    ) -> Result<()> {
        if !mount_settings_changed(previous, connection) {
            return Ok(());
        }
        let mounted = self
            .provider
            .list()
            .unwrap_or_default()
            .into_iter()
            .find(|mount| mount_matches(previous, mount) || mount_matches(connection, mount));
        let Some(mount) = mounted else {
            return Ok(());
        };
        // 先把新参数算一遍：挂载点写错这类错误必须在卸载之前暴露，
        // 否则会把一个本来能用的盘卸掉却挂不回来。
        build_mount_params(connection)?;
        if let Err(err) = self.provider.unmount(&mount.mount_point) {
            return Err(FoundationError::Process(format!(
                "设置已保存，但无法卸载现有挂载 {}：{err}。新设置尚未生效，请手动卸载后重新挂载。",
                mount.mount_point
            )));
        }
        if let Err(err) = self.provider.mount(connection, password) {
            return Err(FoundationError::Process(format!(
                "设置已保存，旧挂载 {} 已卸载，但用新设置重新挂载失败：{err}。请修正后手动挂载。",
                mount.mount_point
            )));
        }
        log::info!(
            "设置变更：已重新挂载 {}（{}）以应用新的缓存/挂载参数",
            connection.name,
            connection.drive
        );
        Ok(())
    }

    pub fn probe(&self, id: &str) -> Result<ProbeReport> {
        let connection = self
            .store
            .get(id)
            .ok_or_else(|| FoundationError::NotFound(format!("连接 {id} 不存在")))?;
        let password = self.store.reveal_password(&connection)?;
        self.provider.probe(&connection, &password)
    }

    pub fn mount(&self, id: &str) -> Result<MountRecord> {
        let connection = self
            .store
            .get(id)
            .ok_or_else(|| FoundationError::NotFound(format!("连接 {id} 不存在")))?;
        let password = self.store.reveal_password(&connection)?;
        self.provider.mount(&connection, &password)
    }

    pub fn unmount(&self, id_or_point: &str) -> Result<()> {
        let mount_point = if let Some(connection) = self.store.get(id_or_point) {
            self.provider
                .list()?
                .into_iter()
                .find(|mount| mount_matches(&connection, mount))
                .map(|mount| mount.mount_point)
                .unwrap_or(connection.drive)
        } else {
            id_or_point.to_string()
        };
        self.provider.unmount(&mount_point)
    }

    /// 删除连接：先卸载 → 删 remote → 删配置；remote 删除失败则保留配置（AC-10）。
    pub fn delete(&self, id: &str) -> Result<()> {
        let connection = self
            .store
            .get(id)
            .ok_or_else(|| FoundationError::NotFound(format!("连接 {id} 不存在")))?;

        if let Ok(mounts) = self.provider.list() {
            if let Some(mount) = mounts
                .iter()
                .find(|mount| mount_matches(&connection, mount))
            {
                self.provider.unmount(&mount.mount_point)?;
            }
        } else {
            log::warn!("引擎不可达，跳过卸载检查");
        }

        self.provider.delete_remote(&connection.remote)?;
        self.store.delete(id)?;
        Ok(())
    }

    pub fn status(&self) -> AppStatus {
        let engine = self.provider.engine_status();
        let mounts = if engine.running {
            self.provider.list().unwrap_or_default()
        } else {
            Vec::new()
        };
        AppStatus {
            engine,
            mounts,
            profile_count: self.store.list().len(),
            secrets: self.store.secrets_name().to_string(),
        }
    }

    /// 启动时挂载所有勾选自启的连接；单个失败不影响其余（返回失败清单）。
    pub fn mount_all_autostart(&self) -> Vec<(String, String)> {
        let mut failures = Vec::new();
        for connection in self.store.list() {
            if !connection.autostart {
                continue;
            }
            let result = self
                .store
                .reveal_password(&connection)
                .and_then(|password| self.provider.mount(&connection, &password));
            match result {
                Ok(record) => log::info!("开机挂载 {} → {}", connection.name, record.mount_point),
                Err(err) => {
                    log::error!("开机挂载 {} 失败：{err}", connection.name);
                    failures.push((connection.id, err.to_string()));
                }
            }
        }
        failures
    }

    /// 正常退出前卸载全部虚拟硬盘，并等待 rclone 确认挂载列表已经清空。
    ///
    /// 与 `shutdown` 的兜底清理不同，本方法失败时保留引擎进程，让宿主可以取消退出、
    /// 恢复窗口并允许用户重试或选择强制退出。
    pub fn unmount_all_and_confirm(&self) -> Result<()> {
        if !self.provider.engine_status().running {
            return Ok(());
        }

        let mounts = self.provider.list()?;
        let mut failures = Vec::new();
        for mount in mounts {
            if let Err(err) = self.provider.unmount(&mount.mount_point) {
                log::error!("退出前卸载 {} 失败：{err}", mount.mount_point);
                failures.push(format!("{}：{err}", mount.mount_point));
            }
        }
        if !failures.is_empty() {
            return Err(FoundationError::Process(format!(
                "以下挂载无法卸载：{}",
                failures.join("；")
            )));
        }

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let remaining = self.provider.list()?;
            if remaining.is_empty() {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                let points = remaining
                    .iter()
                    .map(|mount| mount.mount_point.as_str())
                    .collect::<Vec<_>>()
                    .join("、");
                return Err(FoundationError::Process(format!(
                    "等待虚拟硬盘从系统中移除超时：{points}"
                )));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn shutdown(&self) -> Result<()> {
        self.provider.shutdown()
    }
}

/// 这次保存是否改变了「挂载行为」（决定要不要重新挂载）。
///
/// 比较凭据/地址（会换掉 rclone remote 的内容）与最终挂载参数（盘符、卷标、VFS
/// 缓存模式、目录缓存时间、只读、附加参数…都用 build_mount_params 比一遍）。
fn mount_settings_changed(previous: &Connection, connection: &Connection) -> bool {
    if previous.url != connection.url
        || previous.user != connection.user
        || previous.vendor != connection.vendor
        || previous.password_enc != connection.password_enc
    {
        return true;
    }
    match (build_mount_params(previous), build_mount_params(connection)) {
        (Ok(before), Ok(after)) => before != after,
        // 新设置本身算不出参数（例如挂载点非法）：当成有变化，
        // 交给重新挂载流程报错，而不是静默忽略。
        _ => true,
    }
}

fn mount_matches(connection: &crate::model::Connection, mount: &MountRecord) -> bool {
    let remote = format!("{}:", connection.remote);
    mount.fs.eq_ignore_ascii_case(&remote)
        || (connection.drive != "*" && mount.mount_point.eq_ignore_ascii_case(&connection.drive))
}
