//! 开机/登录自启注册（Windows 计划任务封装）。
//!
//! 规则见 `docs/requirements.md` BR-5：参数由 foundation-windows 固化并逐参引号；
//! 未知模式必须拒绝，绝不退化成 boot。Tauri 宿主传入可执行文件与 `--hidden`，
//! 任务拉起后由应用完成自启挂载并隐藏到托盘。

use std::path::Path;

use foundation_core::{FoundationError, Result};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutostartStatus {
    pub installed: bool,
    pub mode: Option<String>,
    pub task_name: Option<String>,
}

impl AutostartStatus {
    fn none() -> Self {
        Self {
            installed: false,
            mode: None,
            task_name: None,
        }
    }
}

/// 查询当前注册的自启任务（boot 优先于 logon）。
pub fn status() -> Result<AutostartStatus> {
    #[cfg(windows)]
    {
        use foundation_windows::task::{self, TASK_NAME_BOOT, TASK_NAME_LOGON};
        if task::status(TASK_NAME_BOOT)? {
            return Ok(AutostartStatus {
                installed: true,
                mode: Some("boot".into()),
                task_name: Some(TASK_NAME_BOOT.into()),
            });
        }
        if task::status(TASK_NAME_LOGON)? {
            return Ok(AutostartStatus {
                installed: true,
                mode: Some("logon".into()),
                task_name: Some(TASK_NAME_LOGON.into()),
            });
        }
        Ok(AutostartStatus::none())
    }
    #[cfg(not(windows))]
    {
        Err(FoundationError::Platform(
            "开机自启仅在 Windows 上可用".into(),
        ))
    }
}

/// 注册自启任务；已注册会被覆盖（同一任务名）。
pub fn install(mode: &str, executable: &Path, extra_args: &[String]) -> Result<AutostartStatus> {
    if mode != "boot" && mode != "logon" {
        // 先校验再触系统：非法模式必须显式报错
        return Err(FoundationError::InvalidInput(format!(
            "未知的自启模式 {mode:?}（只支持 logon / boot）"
        )));
    }

    #[cfg(windows)]
    {
        use foundation_windows::acl::current_account;
        use foundation_windows::task::{self, TaskSpec};

        let working_dir = executable
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let spec = if mode == "boot" {
            TaskSpec::boot(executable, extra_args, &working_dir)
        } else {
            TaskSpec::logon(
                executable,
                extra_args,
                &working_dir,
                current_account().unwrap_or_default(),
            )
        };
        task::install(&spec)?;
        log::info!("已注册自启任务：{}（{mode}）", spec.name);
        status()
    }
    #[cfg(not(windows))]
    {
        let _ = (executable, extra_args);
        Err(FoundationError::Platform(
            "开机自启仅在 Windows 上可用".into(),
        ))
    }
}

/// 移除两种模式的自启任务；不存在的任务视为已移除。
pub fn uninstall() -> Result<AutostartStatus> {
    #[cfg(windows)]
    {
        use foundation_windows::task::{self, TASK_NAME_BOOT, TASK_NAME_LOGON};
        task::uninstall(TASK_NAME_BOOT)?;
        task::uninstall(TASK_NAME_LOGON)?;
        log::info!("已移除自启任务");
        status()
    }
    #[cfg(not(windows))]
    {
        Err(FoundationError::Platform(
            "开机自启仅在 Windows 上可用".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_mode_is_rejected_before_touching_system() {
        let err = install("boot-but-typo", Path::new("drive.exe"), &[]).unwrap_err();
        assert_eq!(err.code(), "invalid_input");
    }

    #[cfg(windows)]
    #[test]
    fn status_query_does_not_error() {
        // 只验证 schtasks 查询链路可用（含中文控制台编码）；不假设机器上有没有历史任务
        let result = status().expect("status 查询不应失败");
        assert!(result.installed == result.task_name.is_some());
    }
}
