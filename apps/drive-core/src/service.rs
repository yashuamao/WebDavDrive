//! 应用服务：把 store 与 provider 编排成用例（保存/挂载/卸载/删除/状态）。
//!
//! 删除连接必须防孤儿（AC-10）：remote 删不掉就保留连接并报错，
//! 不能让 rclone.conf 里留下带凭据、永远无人清理的 remote。

use std::sync::Arc;

use foundation_core::{FoundationError, Result};
use serde::Serialize;

use crate::model::{ConnectionInput, ConnectionView};
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
    pub fn upsert(&self, input: ConnectionInput) -> Result<ConnectionView> {
        let connection = self.store.upsert(input)?;
        let password = self.store.reveal_password(&connection)?;
        self.provider.ensure_remote(&connection, &password)?;
        Ok(connection.view())
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
        let mount_point = self
            .store
            .get(id_or_point)
            .map(|c| c.drive)
            .unwrap_or_else(|| id_or_point.to_string());
        self.provider.unmount(&mount_point)
    }

    /// 删除连接：先卸载 → 删 remote → 删配置；remote 删除失败则保留配置（AC-10）。
    pub fn delete(&self, id: &str) -> Result<()> {
        let connection = self
            .store
            .get(id)
            .ok_or_else(|| FoundationError::NotFound(format!("连接 {id} 不存在")))?;

        if let Ok(mounts) = self.provider.list() {
            let mounted = mounts
                .iter()
                .any(|m| m.mount_point.to_lowercase() == connection.drive.to_lowercase());
            if mounted {
                self.provider.unmount(&connection.drive)?;
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

    pub fn shutdown(&self) -> Result<()> {
        self.provider.shutdown()
    }
}
