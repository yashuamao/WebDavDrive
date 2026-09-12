//! foundation-windows：Windows 平台能力。
//!
//! 只放"任何 Windows 本地工具都会用、且与业务无关"的能力：
//! - 命令行引号（Task Scheduler / cmd 参数序列化）
//! - Job Object（宿主死亡时回收子进程）
//! - 数据目录 ACL 收紧（去继承，仅 SYSTEM/Administrators/当前用户）
//! - 计划任务（XML 生成 + schtasks 注册/查询/删除）
//! - 单实例命名互斥体
//!
//! 非 Windows 平台上这些函数返回 `FoundationError::Platform`，便于跨平台编译与测试。

pub mod acl;
pub mod job;
pub mod quote;
pub mod single_instance;
pub mod task;

pub use job::JobObject;
pub use quote::{command_line, quote_arg};
pub use single_instance::SingleInstance;
pub use task::{TaskMode, TaskSpec};
