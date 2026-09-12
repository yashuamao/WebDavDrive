//! 单实例守卫：基于命名互斥体。
//!
//! 为什么需要：计划任务、手动启动和自动恢复可能同时拉起进程；两个实例会争抢
//! 引擎端口、配置文件和自启任务。第二个实例应聚焦已有窗口（Tauri 单实例插件）
//! 或明确退出，而不是继续抢资源。

use foundation_core::{FoundationError, Result};

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    pub struct SingleInstance {
        handle: HANDLE,
    }

    unsafe impl Send for SingleInstance {}
    unsafe impl Sync for SingleInstance {}

    impl SingleInstance {
        /// 拿到互斥体返回 Some；已有实例返回 None。
        pub fn acquire(name: &str) -> Result<Option<Self>> {
            let mut wide: Vec<u16> = name.encode_utf16().collect();
            wide.push(0);
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
            if handle.is_null() {
                return Err(FoundationError::Platform(format!(
                    "CreateMutexW 失败：{}",
                    std::io::Error::last_os_error()
                )));
            }
            let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            if already_exists {
                unsafe { CloseHandle(handle) };
                return Ok(None);
            }
            Ok(Some(Self { handle }))
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub struct SingleInstance;

    impl SingleInstance {
        pub fn acquire(_name: &str) -> Result<Option<Self>> {
            Err(FoundationError::Platform(
                "单实例互斥体仅在 Windows 上可用".into(),
            ))
        }
    }
}

pub use imp::SingleInstance;

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_reports_existing_instance() {
        let name = format!("foundation-single-instance-test-{}", std::process::id());
        let first = SingleInstance::acquire(&name).unwrap();
        assert!(first.is_some(), "首个实例应拿到互斥体");
        let second = SingleInstance::acquire(&name).unwrap();
        assert!(second.is_none(), "第二个实例必须被识别");
        drop(first);
    }
}
