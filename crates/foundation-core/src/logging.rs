use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record, SetLoggerError};

/// 有界日志：内存环形缓冲（给 UI 看）+ 可选落盘文件。
///
/// 设计约束（来自旧版实测）：
/// - `close()` 之后的写入不得 panic（关机竞态里后台线程还会记日志）；
/// - 磁盘写失败不能拖垮调用方（只丢日志）；
/// - `tail()` 随时可读，不要求已安装为全局 logger。
pub struct RingLog {
    capacity: usize,
    lines: Mutex<VecDeque<String>>,
    file: Mutex<Option<File>>,
    level: LevelFilter,
    closed: AtomicBool,
}

impl RingLog {
    pub fn new(capacity: usize) -> Arc<Self> {
        Self::with_level(capacity, LevelFilter::Info)
    }

    pub fn with_level(capacity: usize, level: LevelFilter) -> Arc<Self> {
        let capacity = capacity.max(1);
        Arc::new(Self {
            capacity,
            lines: Mutex::new(VecDeque::with_capacity(capacity.min(4096))),
            file: Mutex::new(None),
            level,
            closed: AtomicBool::new(false),
        })
    }

    /// 打开日志文件（追加）。父目录必须已存在，由调用方保证。
    pub fn attach_file(&self, path: &Path) -> std::io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        if let Ok(mut guard) = self.file.lock() {
            *guard = Some(file);
        }
        Ok(())
    }

    pub fn tail(&self, count: usize) -> Vec<String> {
        let guard = match self.lines.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let skip = guard.len().saturating_sub(count);
        guard.iter().skip(skip).cloned().collect()
    }

    /// 关闭文件句柄；之后写入只进内存。
    /// Windows 上不关文件会导致日志无法清理/轮转。
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.file.lock() {
            if let Some(mut file) = guard.take() {
                let _ = file.flush();
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn write_line(&self, level: Level, message: &str) {
        let stamp = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!("{stamp} [{level}] {message}");

        if let Ok(mut lines) = self.lines.lock() {
            if lines.len() == self.capacity {
                lines.pop_front();
            }
            lines.push_back(line.clone());
        }

        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                // 日志失败绝不能拖垮进程
                let _ = file.write_all(line.as_bytes());
                let _ = file.write_all(b"\n");
                let _ = file.flush();
            }
        }
    }
}

struct SharedLogger(Arc<RingLog>);

impl Log for SharedLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.0.level
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            self.0
                .write_line(record.level(), &record.args().to_string());
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.0.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

impl Drop for RingLog {
    fn drop(&mut self) {
        self.close();
    }
}

/// 安装为全局 logger；调用方保留 `Arc<RingLog>` 以便 `tail()`。
pub fn install(ring: &Arc<RingLog>) -> std::result::Result<(), SetLoggerError> {
    log::set_max_level(ring.level);
    log::set_boxed_logger(Box::new(SharedLogger(ring.clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::info;

    fn temp_log_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("foundation-log-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.log")
    }

    #[test]
    fn ring_is_bounded_and_ordered() {
        let log = RingLog::new(3);
        for i in 0..5 {
            log.write_line(Level::Info, &format!("line-{i}"));
        }
        let tail = log.tail(10);
        assert_eq!(tail.len(), 3);
        assert!(tail[0].ends_with("line-2"));
        assert!(tail[2].ends_with("line-4"));
    }

    #[test]
    fn write_after_close_does_not_panic_and_still_lands_in_ring() {
        let log = RingLog::new(8);
        log.attach_file(&temp_log_path("close")).unwrap();
        log.write_line(Level::Error, "before");
        log.close();
        assert!(log.is_closed());
        log.write_line(Level::Info, "after"); // 不得 panic
        assert!(log.tail(8).iter().any(|l| l.ends_with("after")));
    }

    #[test]
    fn file_receives_lines() {
        let path = temp_log_path("file");
        let _ = std::fs::remove_file(&path);
        let log = RingLog::new(16);
        log.attach_file(&path).unwrap();
        log.write_line(Level::Info, "hello-file");
        log.close();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello-file"));
    }

    #[test]
    fn logger_can_be_installed_and_used() {
        let log = RingLog::new(16);
        // 全局 logger 只能装一次；测试进程内可能已有，忽略重复安装错误
        let _ = install(&log);
        info!("through-facade");
        assert!(log.tail(16).iter().any(|l| l.ends_with("through-facade")));
    }
}
