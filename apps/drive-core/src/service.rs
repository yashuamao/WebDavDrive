//! 应用服务：把 store 与 provider 编排成用例（保存/挂载/卸载/删除/状态/引擎更新）。
//!
//! 删除连接必须防孤儿（AC-10）：remote 删不掉就保留连接并报错，
//! 不能让 rclone.conf 里留下带凭据、永远无人清理的 remote。
//!
//! 引擎更新（rclone 独立升级）的编排也在这里，因为它是唯一同时需要
//! provider（停/起引擎）、engine_update（下载/校验/替换）与界面确认的用例：
//! 无挂载门禁 → 停引擎 → swap_engine → 重启 → 校验 core/version 与
//! options/get 的 vfs.DirCacheTime → 成功清备份，任何失败回滚。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use foundation_core::{FoundationError, Result};
use serde::Serialize;

use crate::app_update::{self, AppUpdateInfo, AppUpdater};
use crate::engine_update::{
    self, cleanup_staging, default_transport, EngineUpdateInfo, EngineUpdater,
};
use crate::model::{Connection, ConnectionInput, ConnectionView};
use crate::params::build_mount_params;
use crate::provider::{EngineStatus, MountProvider, MountRecord, ProbeReport};
use crate::store::ProfileStore;

pub struct AppService {
    store: ProfileStore,
    provider: Arc<dyn MountProvider>,
    updater: EngineUpdater,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppStatus {
    pub engine: EngineStatus,
    pub mounts: Vec<MountRecord>,
    pub profile_count: usize,
    pub secrets: String,
}

impl AppService {
    /// 默认装配：更新状态放在 store 所在的数据目录（%PROGRAMDATA%\WebDavDrive），
    /// 网络走系统 WinHTTP。
    pub fn new(store: ProfileStore, provider: Arc<dyn MountProvider>) -> Self {
        let data_dir = store
            .path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self::with_updater(
            store,
            provider,
            EngineUpdater::new(data_dir, default_transport()),
        )
    }

    /// 宿主/测试定制：换掉数据目录与网络传输层。
    pub fn with_updater(
        store: ProfileStore,
        provider: Arc<dyn MountProvider>,
        updater: EngineUpdater,
    ) -> Self {
        Self {
            store,
            provider,
            updater,
        }
    }

    pub fn updater(&self) -> &EngineUpdater {
        &self.updater
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

// -- 引擎（rclone）独立更新 ---------------------------------------------------------

    /// 设置页「引擎（rclone）」区块的只读信息：不触网，只读本地状态与引擎版本。
    pub fn engine_update_info(&self) -> EngineUpdateInfo {
        let installed = self.provider.installed_version().ok().flatten();
        self.updater.info(installed.as_deref())
    }

    /// 检查更新。force = 设置页的手动按钮（随时可点，绕过每天一次的限制）；
    /// 自动检查传 false，失败只记 last_error，不弹窗。
    pub fn check_engine_update(&self, force: bool) -> EngineUpdateInfo {
        let installed = self.provider.installed_version().ok().flatten();
        self.updater
            .check(installed.as_deref(), force, engine_update::now_unix())
    }

    /// 保存镜像前缀（空 = 官方源）。
    pub fn set_engine_mirror_prefix(&self, prefix: &str) -> Result<EngineUpdateInfo> {
        self.updater.set_mirror_prefix(prefix)?;
        Ok(self.engine_update_info())
    }

    /// 安装指定版本的官方引擎包（只提示、用户确认后调用，安装必然掉挂载）。
    pub fn install_engine_update(&self, version: &str) -> Result<EngineUpdateInfo> {
        let staged = self.updater.stage_release(version)?;
        self.install_staged(&staged, Some(&engine_update::normalize_version(version)))
    }

    /// 用本地文件（rclone.exe 或官方 zip）安装引擎。
    pub fn install_engine_from_file(&self, path: &Path) -> Result<EngineUpdateInfo> {
        let staged = self.updater.stage_local_file(path)?;
        self.install_staged(&staged, None)
    }

    /// 暂存引擎 → 替换 → 返回最新状态。暂存目录无论成败都要清掉。
    fn install_staged(&self, staged: &Path, expected: Option<&str>) -> Result<EngineUpdateInfo> {
        let outcome = self.replace_engine(staged, expected);
        cleanup_staging(staged);
        let version = outcome?;
        self.updater.record_installed(&version)?;
        log::info!("引擎更新完成：v{version}");
        Ok(self.engine_update_info())
    }

    /// 无挂载门禁 → 停引擎 → 替换文件 → 重启校验（失败回滚）。
    ///
    /// 错误文案必须写清"卡在第几步、有没有回滚"，因为用户此刻最关心的是
    /// 引擎还能不能用。
    fn replace_engine(&self, staged: &Path, expected: Option<&str>) -> Result<String> {
        // 第 1 步：门禁。更新必须停引擎，绝不能打断正在使用的虚拟硬盘。
        if self.provider.engine_status().running {
            let mounts = self.provider.list().map_err(|err| {
                FoundationError::Process(format!(
                    "第 1 步（检查挂载）失败：无法确认挂载状态（{err}）；为避免打断正在使用的虚拟硬盘，本次安装已取消"
                ))
            })?;
            if !mounts.is_empty() {
                let points = mounts
                    .iter()
                    .map(|mount| mount.mount_point.as_str())
                    .collect::<Vec<_>>()
                    .join("、");
                return Err(FoundationError::Conflict(format!(
                    "第 1 步（检查挂载）失败：当前还有 {} 个挂载（{points}）。更新引擎会中断这些虚拟硬盘，请先全部卸载再安装",
                    mounts.len()
                )));
            }
        }

        let installed = self.provider.engine_install_path();

        // 第 2 步：停引擎（Windows 会锁住运行中的映像，不停就换不了文件）。
        self.provider.shutdown().map_err(|err| {
            FoundationError::Process(format!(
                "第 2 步（停止 rclone）失败：{err}。引擎文件未被替换，可继续使用"
            ))
        })?;

        // 第 3 步：替换文件（swap_engine 内部失败会自己还原旧文件）。
        let backup = engine_update::swap_engine(&installed, staged).map_err(|err| {
            FoundationError::Process(format!(
                "第 3 步（替换 {}）失败：{err}。原引擎未被破坏，可继续使用（常见原因：目标目录需要管理员权限，或文件被其他程序占用）",
                installed.display()
            ))
        })?;

        // 第 4 步：重启并验收；任何异常都必须回滚到旧引擎。
        match self.verify_engine(expected) {
            Ok(version) => {
                engine_update::discard_backup(&backup);
                log::info!("引擎已替换为 v{version}：{}", installed.display());
                Ok(version)
            }
            Err(err) => {
                let rollback = self.rollback_engine(&installed, &backup);
                Err(FoundationError::Process(format!(
                    "第 4 步（重启并校验引擎）失败：{err}。{rollback}"
                )))
            }
        }
    }

    /// 验收新引擎：core/version 版本号对得上，且 options/get 里 vfs.DirCacheTime 仍在。
    ///
    /// 为什么查 vfs 选项：挂载参数依赖 vfs.DirCacheTime，rclone 若在新版本里改了名字，
    /// 挂载会静默用不上缓存设置——必须在更新成功之前拦住并回滚。
    fn verify_engine(&self, expected: Option<&str>) -> Result<String> {
        self.provider.ensure_started()?;
        let status = self.provider.engine_status();
        if !status.running {
            return Err(FoundationError::Process(
                "引擎进程未就绪（core/version 无响应）".into(),
            ));
        }
        let version = status
            .version
            .ok_or_else(|| FoundationError::Process("引擎未返回版本号".into()))?;
        if let Some(expected) = expected {
            if !engine_update::versions_match(&version, expected) {
                return Err(FoundationError::Process(format!(
                    "版本号不匹配：期望 {expected}，引擎实际报告 {version}"
                )));
            }
        }
        let options = self.provider.options_get()?;
        if !engine_update::has_vfs_dir_cache_time(&options) {
            return Err(FoundationError::Process(
                "引擎不再提供 vfs.DirCacheTime 选项：目录缓存时间会失效".into(),
            ));
        }
        Ok(engine_update::normalize_version(&version))
    }

    /// 回滚：停掉刚起来的新引擎 → 还原旧文件 → 重启旧引擎。
    /// 返回值是要拼进错误信息的结果说明（有没有回滚成功、备份还在不在）。
    fn rollback_engine(&self, installed: &Path, backup: &Path) -> String {
        let stopped = self.provider.shutdown();
        let restored = engine_update::rollback_engine(installed, backup);
        let restarted = self.provider.ensure_started();
        match (stopped, restored, restarted) {
            (_, Ok(()), Ok(())) => "已回滚到原引擎并重新启动。".to_string(),
            (_, Ok(()), Err(err)) => format!(
                "已回滚到原引擎，但原引擎启动失败：{err}。请退出并重新打开应用。"
            ),
            (Err(stop_err), Err(restore_err), _) => format!(
                "回滚失败：停止新引擎时报（{stop_err}），还原原文件时报（{restore_err}）。备份仍在 {}，可手工改回 rclone.exe。",
                backup.display()
            ),
            (_, Err(restore_err), _) => format!(
                "回滚失败：{restore_err}。备份仍在 {}，可手工改回 rclone.exe。",
                backup.display()
            ),
        }
    }

    // -- 应用（安装包）自更新 ---------------------------------------------------
    //
    // 界面上的「应用更新」：检查 GitHub release → 下载并校验 setup.exe →
    // 拉起独立的更新器进程（它等本进程退出后静默安装并重启）。
    // 绿色版（免安装 zip，目录里没有 uninstall.exe）不支持，直接报错让用户去
    // GitHub 下载；镜像前缀沿用引擎更新里的设置，不新增配置项。

    fn app_updater(&self) -> AppUpdater {
        AppUpdater::new(self.updater.dir())
    }

    fn mirror_prefix(&self) -> String {
        self.updater.load_state().mirror_prefix
    }

    /// 当前是不是绿色版：程序目录里没有 uninstall.exe 就是绿色版。
    /// 拿不到自身路径时按绿色版处理（宁可拒绝更新，也不要装错地方）。
    pub fn is_portable(&self) -> bool {
        match std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
            Some(dir) => !app_update::is_installer_directory(&dir),
            None => true,
        }
    }

    /// 不触网的只读信息（设置页打开时用）。
    pub fn app_update_info(&self, installed: &str) -> AppUpdateInfo {
        self.app_updater().info(installed, self.is_portable())
    }

    /// 检查应用更新（force = 用户点了「检查更新」）。失败只记录，不返回 Err。
    pub fn check_app_update(&self, installed: &str, force: bool) -> AppUpdateInfo {
        let mirror = self.mirror_prefix();
        self.app_updater().check(
            installed,
            force,
            engine_update::now_unix(),
            &mirror,
            self.is_portable(),
        )
    }

    /// 下载并校验安装包。此时不退出、不卸载任何东西；下好等用户确认再装。
    pub fn download_app_update(&self, installed: &str, version: &str) -> Result<AppUpdateInfo> {
        if self.is_portable() {
            return Err(FoundationError::Conflict(
                "绿色版不支持自动更新：请到 GitHub Releases 下载新版本".into(),
            ));
        }
        let mirror = self.mirror_prefix();
        let updater = self.app_updater();
        updater.stage_release(version, &mirror)?;
        Ok(updater.info(installed, false))
    }

    /// 拉起更新器：它等本进程退出 → 静默安装 → 重启应用。
    /// 本方法返回后调用方（命令层）必须触发退出，否则更新器只会等到超时。
    pub fn start_app_update(&self, installed: &str) -> Result<AppUpdateInfo> {
        if self.is_portable() {
            return Err(FoundationError::Conflict(
                "绿色版不支持自动更新：请到 GitHub Releases 下载新版本".into(),
            ));
        }
        let updater = self.app_updater();
        let staged = updater.staged_setup().ok_or_else(|| {
            FoundationError::InvalidInput("还没有下载安装包，请先下载更新".into())
        })?;
        let current = std::env::current_exe()
            .map_err(|err| FoundationError::Process(format!("无法定位程序路径：{err}")))?;
        let install_dir = current
            .parent()
            .ok_or_else(|| FoundationError::Process("无法定位程序所在目录".into()))?
            .to_path_buf();
        let copy = app_update::copy_updater(&current)?;
        let plan = app_update::ApplyPlan {
            setup: staged,
            target_dir: install_dir,
            wait_pid: std::process::id(),
            exe: current,
            log_path: updater.log_path(),
            restart_args: if std::env::args().any(|arg| arg == "--hidden") {
                vec!["--hidden".to_string()]
            } else {
                Vec::new()
            },
        };
        app_update::spawn_updater(&copy, &plan)?;
        app_update::log_line(&plan.log_path, "已拉起更新器，等待应用退出后安装");
        log::info!(
            "应用更新器已启动（等本进程退出后安装 {}）",
            plan.setup.display()
        );
        Ok(updater.info(installed, false))
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
