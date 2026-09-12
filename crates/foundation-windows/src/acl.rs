//! 数据目录 ACL 收紧。
//!
//! 为什么必须做：`%PROGRAMDATA%` 默认给 `BUILTIN\Users:(OI)(CI)(RX)`，而机器范围 DPAPI
//! 是"同机任意用户都能解密"。不收权等于把凭据摊给所有本地账户。
//!
//! 实现注意（旧版实测）：`icacls` 的 `(OI)(CI)` 只对目录合法，与 `/T` 混用时文件会
//! 处理失败，而 `/inheritance:r` 已经把文件继承的 ACE 摘掉——文件会变成空 DACL，
//! 连属主都读不了。因此分两遍：先显式授权所有子对象，再给目录设置可继承授权。

use std::path::Path;

use foundation_core::{FoundationError, Result};

/// 去掉继承，仅授权 SYSTEM / Administrators / 当前运行账户。
pub fn harden_data_dir(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows_impl(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(FoundationError::Platform(
            "数据目录 ACL 收紧仅在 Windows 上可用".into(),
        ))
    }
}

#[cfg(windows)]
fn windows_impl(path: &Path) -> Result<()> {
    // 用 SID 而不是本地化名称，避免非英文系统上匹配不到账户名
    let mut principals = vec!["*S-1-5-18".to_string(), "*S-1-5-32-544".to_string()];
    if let Some(account) = current_account() {
        principals.push(account);
    }

    let explicit: Vec<String> = principals.iter().map(|p| format!("{p}:F")).collect();
    run_icacls(path, &["/T", "/inheritance:r", "/grant:r"], &explicit)?;

    let inheritable: Vec<String> = principals
        .iter()
        .map(|p| format!("{p}:(OI)(CI)F"))
        .collect();
    run_icacls(path, &["/inheritance:r", "/grant:r"], &inheritable)
}

#[cfg(windows)]
fn run_icacls(path: &Path, flags: &[&str], grants: &[String]) -> Result<()> {
    let output = std::process::Command::new("icacls")
        .arg(path)
        .args(flags)
        .args(grants)
        .args(["/C", "/Q"])
        .output()
        .map_err(|err| FoundationError::Platform(format!("无法执行 icacls：{err}")))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FoundationError::Platform(format!(
            "icacls 失败（{}）：{} {}",
            output.status,
            stdout.trim(),
            stderr.trim()
        )));
    }
    Ok(())
}

/// `DOMAIN\User`；两者缺失时返回 None。
pub fn current_account() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|s| !s.is_empty())?;
    let domain = std::env::var("USERDOMAIN").ok().filter(|s| !s.is_empty());
    Some(match domain {
        Some(domain) => format!("{domain}\\{user}"),
        None => user,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("foundation-acl-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn hardening_keeps_owner_readable_and_removes_users_group() {
        let dir = temp_dir("basic");
        let file = dir.join("agent.log");
        std::fs::write(&file, "hello").unwrap();

        // 先有一个"子对象在收紧之前就存在"的场景——旧版就是在这里踩的坑
        harden_data_dir(&dir).unwrap();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "hello");

        let acl = std::process::Command::new("icacls")
            .arg(&file)
            .output()
            .unwrap();
        let acl_text = String::from_utf8_lossy(&acl.stdout);
        assert!(
            !acl_text.contains(r"BUILTIN\Users"),
            "收紧后不应再有 Users 组：{acl_text}"
        );

        // 收紧之后新建的文件必须继承同样的限制
        let new_file = dir.join("profiles.json");
        std::fs::write(&new_file, "{}").unwrap();
        let acl_new = std::process::Command::new("icacls")
            .arg(&new_file)
            .output()
            .unwrap();
        let acl_new_text = String::from_utf8_lossy(&acl_new.stdout);
        assert!(
            !acl_new_text.contains(r"BUILTIN\Users"),
            "新建文件不应继承 Users：{acl_new_text}"
        );
    }
}
