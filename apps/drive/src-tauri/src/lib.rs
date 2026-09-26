//! Tauri 2 托盘宿主：装配 drive-core、托盘菜单、单实例与 IPC 命令。

mod commands;
mod file_dialog;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use drive_core::provider::{EngineConfig, RcloneProvider};
use drive_core::{AppService, ProfileStore};
use foundation_core::{install_logger, RingLog};
use log::LevelFilter;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

use commands::AppState;

/// 数据目录：默认 `%PROGRAMDATA%\WebDavDrive`（与旧版一致，便于用户迁移）。
fn data_dir() -> PathBuf {
    let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(base).join("WebDavDrive")
}

/// `rclone --password-command` 指向随程序分发的 pwcmd 工具。
fn password_command(key_path: &std::path::Path) -> String {
    let pwcmd = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .and_then(|dir| {
            // 同目录优先（绿色版与修正后的安装包），再兜底 resources\（旧安装布局）。
            let direct = dir.join("drive-pwcmd.exe");
            if direct.is_file() {
                return Some(direct);
            }
            let nested = dir.join("resources").join("drive-pwcmd.exe");
            if nested.is_file() {
                return Some(nested);
            }
            Some(direct)
        })
        .unwrap_or_else(|| PathBuf::from("drive-pwcmd.exe"));
    format!(
        "\"{}\" --key-file \"{}\"",
        pwcmd.display(),
        key_path.display()
    )
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub(crate) fn begin_graceful_exit(app: &AppHandle) {
    let service = {
        let state = app.state::<AppState>();
        if state.exit_in_progress.swap(true, Ordering::AcqRel) {
            return;
        }
        state.service.clone()
    };

    let handle = app.clone();
    std::thread::spawn(move || {
        log::info!("收到退出请求：开始取消全部挂载");
        let result = service
            .unmount_all_and_confirm()
            .and_then(|_| service.shutdown());
        match result {
            Ok(()) => {
                log::info!("全部虚拟硬盘已移除，正在退出应用");
                handle.exit(0);
            }
            Err(err) => {
                handle
                    .state::<AppState>()
                    .exit_in_progress
                    .store(false, Ordering::Release);
                log::error!("退出前取消挂载失败，已中止退出：{err}");
                show_main(&handle);
                if let Err(emit_err) = handle.emit("exit-cleanup-failed", err.to_string()) {
                    log::error!("发送退出失败提示失败：{emit_err}");
                }
            }
        }
    });
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    // 托盘图标只在这里创建，且必须只创建一次：`tauri.conf.json` 里若声明 `app.trayIcon`，
    // Tauri 会在 `build()` 阶段（setup 之前）自动再建一个 id 为 "main"、没有菜单的托盘图标，
    // 托盘区就会出现两个图标，且配置生成的那个点了没反应。
    if app.config().app.tray_icon.is_some() {
        log::warn!(
            "tauri.conf.json 声明了 app.trayIcon，会与 setup_tray 重复创建托盘图标，应移除该配置"
        );
    }

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let engine = MenuItem::with_id(app, "engine-start", "启动引擎", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出并卸载所有驱动器", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &engine, &separator, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("WebDAV Drive")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "engine-start" => {
                let service = app.state::<AppState>().service.clone();
                std::thread::spawn(move || {
                    if let Err(err) = service.provider().ensure_started() {
                        log::error!("手动启动引擎失败：{err}");
                    }
                });
            }
            "quit" => begin_graceful_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    // 计划任务拉起时带 --hidden：启动即隐藏到托盘
    let hidden = std::env::args().any(|arg| arg == "--hidden");
    let data_dir = data_dir();
    let _ = std::fs::create_dir_all(&data_dir);

    let log = RingLog::with_level(600, LevelFilter::Info);
    let _ = log.attach_file(&data_dir.join("agent.log"));
    let _ = install_logger(&log);

    #[cfg(windows)]
    match foundation_windows::acl::harden_data_dir(&data_dir) {
        Ok(()) => log::info!("数据目录权限已收紧：{}", data_dir.display()),
        Err(err) => {
            log::error!("数据目录 ACL 收紧失败，已拒绝启动：{err}");
            log.close();
            panic!("无法安全启动：数据目录 ACL 收紧失败：{err}");
        }
    }

    let secrets = foundation_secrets::default_store(data_dir.join("keystore.bin"));
    log::info!("密钥后端：{}", secrets.name());
    let store = match ProfileStore::open(&data_dir, secrets.clone()) {
        Ok(store) => store,
        Err(err) => {
            log::error!("加载 profiles.json 失败：{err}");
            panic!("无法启动：{err}");
        }
    };

    let mut engine = EngineConfig::new(&data_dir, "127.0.0.1:5572");
    engine.password_command = password_command(&engine.key_path());
    let provider = Arc::new(RcloneProvider::new(engine, secrets));
    if let Err(err) = provider.validate_startup() {
        log::error!("验证 rclone 配置密钥失败，已拒绝启动：{err}");
        log.close();
        panic!("无法启动：{err}");
    }
    let service = Arc::new(AppService::new(store, provider));
    let state = AppState {
        service: service.clone(),
        log: log.clone(),
        exit_in_progress: AtomicBool::new(false),
        force_exit: AtomicBool::new(false),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main(app);
        }))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::probe_connection,
            commands::mount_connection,
            commands::unmount_connection,
            commands::open_connection,
            commands::list_mounts,
            commands::ensure_engine,
            commands::shutdown_engine,
            commands::exit_application,
            commands::force_exit,
            commands::logs,
            commands::autostart_status,
            commands::install_autostart,
            commands::uninstall_autostart,
            commands::engine_status,
            commands::check_engine_update,
            commands::set_engine_mirror_prefix,
            commands::install_engine_update,
            commands::install_engine_from_file,
            commands::pick_engine_file,
        ])
        .setup(move |app| {
            setup_tray(app)?;
            if hidden {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            // 启动时挂载自启项：后台执行，不阻塞首屏
            let service = app.state::<AppState>().service.clone();
            std::thread::spawn(move || {
                let failures = service.mount_all_autostart();
                for (id, err) in failures {
                    log::error!("自启挂载失败：{id}: {err}");
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 关闭窗口 = 隐藏到托盘；退出走托盘菜单或应用退出
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    if state.force_exit.load(Ordering::Acquire) {
                        log::warn!("用户选择强制退出，跳过等待卸载确认");
                    } else if let Err(err) = state.service.shutdown() {
                        log::error!("退出清理失败：{err}");
                    }
                    state.log.close();
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    /// 托盘图标只能由 `setup_tray()` 创建一次。
    ///
    /// `tauri.conf.json` 的 `app.trayIcon` 会让 Tauri 在 `build()` 阶段（setup 之前）
    /// 再自动创建一个 id 为 `main`、没有菜单的托盘图标，托盘区就会出现两个图标，
    /// 且配置生成的那个点击无反应。此测试锁死「配置里不声明托盘图标」这一约束。
    #[test]
    fn tray_icon_is_not_declared_in_config() {
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        assert!(
            context.config().app.tray_icon.is_none(),
            "tauri.conf.json 不应声明 app.trayIcon：它会与 setup_tray() 重复创建托盘图标（见 CHANGELOG 0.1.1）"
        );
    }

    /// 前端通过 Tauri event API 接收退出清理失败通知；发布版必须显式授权主窗口监听。
    #[test]
    fn main_capability_allows_event_listening() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities/default.json");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("缺少主窗口 capability {}：{err}", path.display()));
        let capability: serde_json::Value =
            serde_json::from_str(&source).expect("capabilities/default.json 必须是有效 JSON");

        let windows = capability["windows"]
            .as_array()
            .expect("主窗口 capability 必须声明 windows");
        assert!(
            windows.iter().any(|window| window.as_str() == Some("main")),
            "capability 必须绑定 Tauri 主窗口 main"
        );

        let permissions = capability["permissions"]
            .as_array()
            .expect("主窗口 capability 必须声明 permissions");
        assert!(
            permissions.iter().any(|permission| {
                matches!(
                    permission.as_str(),
                    Some("core:event:allow-listen" | "core:event:default")
                )
            }),
            "前端调用 event.listen，主窗口必须具备 core:event:allow-listen 权限"
        );
    }

    /// 可选事件订阅即使失败，也不能阻止 React 界面完成挂载。
    #[test]
    fn optional_ui_event_subscription_handles_rejection() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/hooks/useDriveManager.ts");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("无法读取前端脚本 {}：{err}", path.display()));
        let event_subscription = source
            .find("listen<string>(\"exit-cleanup-failed\"")
            .expect("前端必须订阅退出清理失败事件");
        let rejection_handler = source[event_subscription..]
            .find(".catch(")
            .expect("可选事件订阅必须处理权限或运行时拒绝");

        assert!(rejection_handler > 0, "可选事件订阅失败不得拖垮整个界面");
    }

    /// 引擎更新命令必须全部注册进 invoke_handler，漏一个前端按钮就会报「命令不存在」。
    #[test]
    fn engine_update_commands_are_registered() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("无法读取 {}：{err}", path.display()));
        for command in [
            "engine_status",
            "check_engine_update",
            "set_engine_mirror_prefix",
            "install_engine_update",
            "install_engine_from_file",
            "pick_engine_file",
        ] {
            assert!(
                source.contains(&format!("commands::{command},")),
                "引擎更新命令 {command} 未注册到 invoke_handler"
            );
        }
    }

    /// 打包脚本直接调用 Cargo，必须显式启用 Tauri 的 production protocol。
    /// 否则只要 tauri.conf.json 声明了 devUrl，发布版就会继续访问开发服务器。
    #[test]
    fn windows_package_enables_tauri_custom_protocol() {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let manifest = fs::read_to_string(&manifest_path)
            .unwrap_or_else(|err| panic!("无法读取 {}：{err}", manifest_path.display()));
        assert!(
            manifest.contains("custom-protocol = [\"tauri/custom-protocol\"]"),
            "drive crate 必须把 custom-protocol 转发给 tauri"
        );

        let package_script_path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/package-windows.ps1");
        let package_script = fs::read_to_string(&package_script_path)
            .unwrap_or_else(|err| panic!("无法读取 {}：{err}", package_script_path.display()));
        assert!(
            package_script.contains("--features drive/custom-protocol"),
            "Windows 正式打包必须启用 drive/custom-protocol"
        );
    }
}
