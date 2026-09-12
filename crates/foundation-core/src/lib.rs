//! foundation-core：领域无关的底座核心。
//!
//! 这里只放"任何本地常驻工具都会用到、且不涉及业务语义"的东西：
//! - 统一的错误层级与错误码（HTTP/IPC 映射由上层决定）
//! - 有界日志环形缓冲 + 落盘（`log` facade 的 `Log` 实现）
//! - 跨平台 trait：子进程守卫、时钟
//!
//! 禁止：文件格式、网络协议、Windows API、任何 provider/领域词汇。

pub mod error;
pub mod logging;
pub mod process;

pub use error::{FoundationError, Result};
pub use logging::{install as install_logger, RingLog};
pub use process::{ChildGuard, Clock, SystemClock};
