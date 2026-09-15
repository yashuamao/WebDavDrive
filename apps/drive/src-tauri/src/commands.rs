//! Tauri 命令：只做参数/错误转换，业务全部在 `drive_core::AppService`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use drive_core::model::ConnectionInput;
use drive_core::provider::MountRecord;
use drive_core::{AppService, AppStatus, AutostartStatus, ConnectionView, ProbeReport};
use foundation_core::RingLog;
use tauri::{AppHandle, State};

pub struct AppState {
    pub service: Arc<AppService>,
    pub log: Arc<RingLog>,
    pub exit_in_progress: AtomicBool,
    pub force_exit: AtomicBool,
}

fn message(err: impl std::fmt::Display) -> String {
    err.to_string()
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppStatus {
    state.service.status()
}

#[tauri::command]
pub fn list_connections(state: State<'_, AppState>) -> Vec<ConnectionView> {
    state.service.list()
}

#[tauri::command]
pub fn save_connection(
    state: State<'_, AppState>,
    input: ConnectionInput,
) -> Result<ConnectionView, String> {
    state.service.upsert(input).map_err(message)
}

#[tauri::command]
pub fn delete_connection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.service.delete(&id).map_err(message)
}

#[tauri::command]
pub fn probe_connection(state: State<'_, AppState>, id: String) -> Result<ProbeReport, String> {
    state.service.probe(&id).map_err(message)
}

#[tauri::command]
pub fn mount_connection(state: State<'_, AppState>, id: String) -> Result<MountRecord, String> {
    if state.exit_in_progress.load(Ordering::Acquire) {
        return Err("应用正在退出，暂不能创建新挂载".into());
    }
    state.service.mount(&id).map_err(message)
}

#[tauri::command]
pub fn unmount_connection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.service.unmount(&id).map_err(message)
}

#[tauri::command]
pub fn open_connection(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let connection = state
        .service
        .get_view(&id)
        .ok_or_else(|| format!("连接 {id} 不存在"))?;
    let remote = format!("{}:", connection.remote);
    let mount = state
        .service
        .status()
        .mounts
        .into_iter()
        .find(|mount| {
            mount.fs.eq_ignore_ascii_case(&remote)
                || (connection.drive != "*"
                    && mount.mount_point.eq_ignore_ascii_case(&connection.drive))
        })
        .ok_or_else(|| format!("连接「{}」尚未挂载", connection.name))?;

    std::process::Command::new("explorer.exe")
        .arg(&mount.mount_point)
        .spawn()
        .map_err(|err| format!("无法打开 {}：{err}", mount.mount_point))?;
    Ok(())
}

#[tauri::command]
pub fn list_mounts(state: State<'_, AppState>) -> Result<Vec<MountRecord>, String> {
    state.service.status();
    state.service.provider().list().map_err(message)
}

#[tauri::command]
pub fn ensure_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.service.provider().ensure_started().map_err(message)
}

#[tauri::command]
pub fn shutdown_engine(state: State<'_, AppState>) -> Result<(), String> {
    state.service.shutdown().map_err(message)
}

#[tauri::command]
pub fn exit_application(app: AppHandle) {
    crate::begin_graceful_exit(&app);
}

#[tauri::command]
pub fn force_exit(app: AppHandle, state: State<'_, AppState>) {
    state.force_exit.store(true, Ordering::Release);
    app.exit(1);
}

#[tauri::command]
pub fn logs(state: State<'_, AppState>, limit: Option<usize>) -> Vec<String> {
    state.log.tail(limit.unwrap_or(200))
}

#[tauri::command]
pub fn autostart_status() -> Result<AutostartStatus, String> {
    drive_core::autostart::status().map_err(message)
}

#[tauri::command]
pub fn install_autostart(mode: String) -> Result<AutostartStatus, String> {
    let exe = std::env::current_exe().map_err(|err| format!("无法定位程序路径：{err}"))?;
    drive_core::autostart::install(&mode, &exe, &["--hidden".to_string()]).map_err(message)
}

#[tauri::command]
pub fn uninstall_autostart() -> Result<AutostartStatus, String> {
    drive_core::autostart::uninstall().map_err(message)
}
