//! Job Object：宿主进程死亡（含被强杀）时由内核回收子进程。
//!
//! Windows 不会自动带走子进程；没有这层保护，被任务管理器强杀 / 崩溃的宿主会留下
//! 孤儿引擎进程，占着端口或挂载点，导致下次启动失败。

use foundation_core::{ChildGuard, FoundationError, Result};

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// 持有 Job 句柄；句柄关闭时内核杀掉仍在 Job 内的进程。
    pub struct JobObject {
        handle: HANDLE,
    }

    // HANDLE 是内核对象句柄，跨线程使用安全；JobObject 只做封装。
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn kill_on_close() -> Result<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(FoundationError::Platform(format!(
                    "CreateJobObjectW 失败：{}",
                    std::io::Error::last_os_error()
                )));
            }

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const core::ffi::c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                let err = std::io::Error::last_os_error();
                unsafe { CloseHandle(handle) };
                return Err(FoundationError::Platform(format!(
                    "SetInformationJobObject 失败：{err}"
                )));
            }
            Ok(Self { handle })
        }
    }

    impl ChildGuard for JobObject {
        fn assign(&self, pid: u32) -> bool {
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return false;
            }
            let ok = unsafe { AssignProcessToJobObject(self.handle, process) };
            unsafe { CloseHandle(process) };
            ok != 0
        }

        fn name(&self) -> &'static str {
            "win32-job-object"
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub struct JobObject;

    impl JobObject {
        pub fn kill_on_close() -> Result<Self> {
            Err(FoundationError::Platform(
                "Job Object 仅在 Windows 上可用".into(),
            ))
        }
    }

    impl ChildGuard for JobObject {
        fn assign(&self, _pid: u32) -> bool {
            false
        }

        fn name(&self) -> &'static str {
            "unsupported"
        }
    }
}

pub use imp::JobObject;

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn creates_kill_on_close_job_and_reports_name() {
        let job = JobObject::kill_on_close().unwrap();
        assert_eq!(job.name(), "win32-job-object");
        // 不存在的 pid：分配必须返回 false 而不是 panic
        assert!(!job.assign(0x7fff_ffff));
    }
}
