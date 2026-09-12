use std::fmt;

/// 底座统一错误。
///
/// `code()` 是稳定的机器可读标识：HTTP 层映射状态码、IPC 层映射错误弹窗，
/// 文案（`Display`）只给人看。新增变体必须同时补 `code()` 与测试。
#[derive(Debug, thiserror::Error)]
pub enum FoundationError {
    /// 输入不合法（用户可修复），对应 HTTP 400 / IPC invalid-argument。
    #[error("输入非法：{0}")]
    InvalidInput(String),

    /// 配置或数据文件损坏（已隔离或可隔离），对应 HTTP 500 / IPC data-corrupted。
    #[error("数据损坏：{0}")]
    DataCorrupted(String),

    /// 配置版本比当前程序认识的新，拒绝写入，避免降级损坏。
    #[error("配置版本不支持：发现 {found}，本程序支持到 {expected}")]
    UnsupportedVersion { found: u32, expected: u32 },

    /// 密钥缺失/损坏：必须显式失败，绝不静默重建。
    #[error("密钥不可用：{0}")]
    KeyUnavailable(String),

    /// 凭据解密失败：上层不得用空值覆盖既有凭据。
    #[error("凭据解密失败：{0}")]
    SecretDecrypt(String),

    /// 外部进程/引擎错误（启动失败、就绪超时、意外退出）。
    #[error("外部进程错误：{0}")]
    Process(String),

    /// 平台能力失败（ACL、计划任务、Job Object、单实例……）。
    #[error("平台错误：{0}")]
    Platform(String),

    /// 找不到资源（连接、挂载点、remote……由上层补充语境）。
    #[error("未找到：{0}")]
    NotFound(String),

    /// 状态冲突（重复挂载、版本冲突）。
    #[error("状态冲突：{0}")]
    Conflict(String),

    /// 底层 I/O。
    #[error("I/O 错误：{0}")]
    Io(#[from] std::io::Error),

    /// JSON/序列化。
    #[error("序列化错误：{0}")]
    Serde(#[from] serde_json::Error),
}

impl FoundationError {
    /// 稳定错误码（供上层映射，不随文案变化）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::DataCorrupted(_) => "data_corrupted",
            Self::UnsupportedVersion { .. } => "unsupported_version",
            Self::KeyUnavailable(_) => "key_unavailable",
            Self::SecretDecrypt(_) => "secret_decrypt",
            Self::Process(_) => "process",
            Self::Platform(_) => "platform",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Io(_) => "io",
            Self::Serde(_) => "serde",
        }
    }
}

/// 底座统一 Result。
pub type Result<T> = std::result::Result<T, FoundationError>;

/// 便于把任意 Display 错误折成 InvalidInput。
pub fn invalid<T, E: fmt::Display>(context: &str, err: E) -> Result<T> {
    Err(FoundationError::InvalidInput(format!("{context}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        let cases: Vec<FoundationError> = vec![
            FoundationError::InvalidInput("x".into()),
            FoundationError::DataCorrupted("x".into()),
            FoundationError::UnsupportedVersion {
                found: 2,
                expected: 1,
            },
            FoundationError::KeyUnavailable("x".into()),
            FoundationError::SecretDecrypt("x".into()),
            FoundationError::Process("x".into()),
            FoundationError::Platform("x".into()),
            FoundationError::NotFound("x".into()),
            FoundationError::Conflict("x".into()),
            FoundationError::Io(std::io::Error::other("x")),
            FoundationError::Serde(serde_json::from_str::<i32>("nope").unwrap_err()),
        ];
        let mut codes: Vec<&str> = cases.iter().map(|e| e.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), cases.len(), "错误码必须唯一");
        assert!(codes.contains(&"key_unavailable"));
    }
}
