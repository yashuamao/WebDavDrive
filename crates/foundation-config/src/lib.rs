//! foundation-config：领域无关的版本化 JSON 文件存储。
//!
//! 只负责"怎么安全地读写一个配置文件"，不知道里面存的是什么：
//! - 信封格式：`{"version": <u32>, "data": <任意 JSON>}`
//! - 保存：先写 `<name>.tmp` 并 fsync，再 rename 覆盖；覆盖前把旧文件复制为 `<name>.bak`
//! - 损坏：把原文件隔离为 `<name>.corrupt-<时间戳>`，返回 `Quarantined` 而不是 Err，
//!   由应用决定记录日志后以空配置继续
//!
//! 禁止把具体配置模型（连接、设置、凭据引用）放进本 crate。

mod store;

pub use store::{Envelope, FileStore, LoadOutcome, SCHEMA_VERSION};
