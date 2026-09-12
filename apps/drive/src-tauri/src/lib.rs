//! Tauri 2 托盘宿主：装配 drive-core、托盘菜单、单实例与 IPC 命令。

mod commands;

use std::path::PathBuf;
use std::sync::Arc;

use drive_core::provider::{EngineConfig, RcloneProvider};
use drive_core::{AppService, ProfileStore};
use foundation_core::{install_logger, RingLog};
use log::LevelFilter;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};

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
        .and_then(|exe| exe.parent().map(|dir| dir.join("drive-pwcmd.exe")))
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

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let engine = MenuItem::with_id(app, "engine-start", "启动引擎", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
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
            "quit" => app.exit(0),
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
        Err(err) => log::error!("数据目录 ACL 收紧失败：{err}"),
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
    let service = Arc::new(AppService::new(store, provider));
    let state = AppState {
        service: service.clone(),
        log: log.clone(),
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
            commands::list_mounts,
            commands::ensure_engine,
            commands::shutdown_engine,
            commands::logs,
            commands::autostart_status,
            commands::install_autostart,
            commands::uninstall_autostart,
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
                    let _ = state.service.shutdown();
                }
            }
        });
}
