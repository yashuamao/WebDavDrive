use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// 让子进程不分配控制台窗口（Windows: `CREATE_NO_WINDOW`；其他平台空操作）。
///
/// GUI 宿主需要长期运行；未标记的控制台子进程可能闪出黑框，周期性调用时尤其明显。
pub fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

/// 子进程守卫：宿主死亡时回收子进程（Windows 上由 Job Object 实现）。
///
/// supervisor 只依赖这个 trait，从而不依赖 `foundation-windows`；
/// 平台 crate 为其平台类型实现本 trait。
pub trait ChildGuard: Send + Sync {
    /// 把已启动的进程加入守卫。失败时返回 false 由调用方决定降级策略。
    fn assign(&self, pid: u32) -> bool;
    /// 人类可读的实现名（日志用）。
    fn name(&self) -> &'static str;
}

/// 可注入时钟：便于测试超时/重试逻辑，不直接依赖 SystemTime。
pub trait Clock: Send + Sync {
    /// Unix 毫秒时间戳。
    fn now_millis(&self) -> u64;
}

/// 默认时钟。
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_moves_forward() {
        let clock = SystemClock;
        let a = clock.now_millis();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let b = clock.now_millis();
        assert!(b >= a);
        assert!(a > 1_600_000_000_000, "时钟应是真实时间戳");
    }
}
